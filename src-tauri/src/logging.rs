// Panic-safe file logger.
//
// Background: macOS LaunchServices does not guarantee stderr is connected
// to a valid fd when it spawns the app to handle a URL scheme, file
// association, or login item. Rust's `eprintln!`/`println!` macros panic
// on write failure, and a panic that crosses an ObjC -> Rust callback
// (e.g. the deep-link handler) aborts the whole process.
//
// This logger writes to a file under the platform's log directory and
// forwards Warn/Error records to Sentry. All write failures are swallowed
// so a logging failure can never crash the app.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use log::{Level, Log, Metadata, Record, SetLoggerError};

const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
const SENTRY_MESSAGE_CHAR_CAP: usize = 400;

struct FileLogger {
    file: Mutex<Option<File>>,
    path: PathBuf,
    records_since_rotate_check: std::sync::atomic::AtomicU64,
}

impl FileLogger {
    fn write_record(&self, record: &Record, display_level: Level) {
        let Ok(mut guard) = self.file.lock() else {
            return;
        };
        let Some(file) = guard.as_mut() else {
            return;
        };
        let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let _ = writeln!(
            file,
            "{ts} {level:<5} {target}: {msg}",
            level = display_level,
            target = record.target(),
            msg = record.args(),
        );
        let _ = file.flush();
    }

    fn rotate_if_needed(&self) {
        let metadata = match fs::metadata(&self.path) {
            Ok(m) => m,
            Err(_) => return,
        };
        if metadata.len() < MAX_LOG_BYTES {
            return;
        }
        let Ok(mut guard) = self.file.lock() else {
            return;
        };
        // Drop the current handle before renaming so Windows can't hold it open;
        // also necessary on macOS for log inspection while the app runs.
        *guard = None;
        let backup = self.path.with_extension("log.old");
        let _ = fs::remove_file(&backup);
        let _ = fs::rename(&self.path, &backup);
        if let Ok(f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            *guard = Some(f);
        }
    }
}

fn is_transient_transport_error(msg: &str) -> bool {
    msg.contains("error sending request")
        || msg.contains("dns error")
        || msg.contains("connection refused")
        || msg.contains("connection reset")
        || msg.contains("operation timed out")
        || msg.contains("network is unreachable")
        || msg.contains("os error 50") // macOS: Network is down
        || msg.contains("os error 51") // macOS: Network is unreachable
        || msg.contains("os error 65") // macOS: No route to host
}

// Non-2xx response from the update endpoint. Most commonly a transient 5xx
// from GitHub releases or a 404 during a tag-publish race — not actionable.
fn is_updater_endpoint_error(msg: &str) -> bool {
    msg.contains("update endpoint did not respond with a successful status code")
}

// Drop transient transport errors (offline laptop, flaky wifi, upstream blip)
// from Sentry. They still hit the local log file via write_record.
fn skip_sentry(target: &str, msg: &str) -> bool {
    if target.starts_with("tauri_plugin_updater") {
        return is_transient_transport_error(msg) || is_updater_endpoint_error(msg);
    }
    // proxy_intercept bypass forwarders (plain + websocket-upgrade variant):
    // when CC is bypassing the local Python proxy and we re-issue directly to
    // the upstream API, transient network failures aren't actionable — client
    // already gets a 502 and CC retries. The upgrade variant was missed by the
    // original prefix and accumulated as RUST-2R (393 events, all transport).
    if target.starts_with("headroom_desktop_lib::proxy_intercept")
        && (msg.starts_with("proxy_intercept bypass forward failed")
            || msg.starts_with("proxy_intercept bypass upgrade forward failed"))
    {
        return is_transient_transport_error(msg);
    }
    // The accept loop self-heals: it backs off and keeps accepting. A transient
    // EMFILE (or similar) under load isn't actionable as a Sentry event.
    if target.starts_with("headroom_desktop_lib::proxy_intercept")
        && msg.starts_with("[proxy_intercept] accept error")
    {
        return true;
    }
    // A held intercept port reaches Sentry via the explicit once-per-error
    // capture at the emit site (RUST-62); this warn repeats on every 15s bind
    // retry and only duplicated it (RUST-5R). Local log only.
    //
    // Coupled to the emit site's wording in `proxy_intercept::spawn` -- if that
    // message changes and this substring is not changed with it, the retry warn
    // silently starts flooding Sentry again. `skips_foreign_port_bind_retry_warns`
    // is the guard; keep its fixture a copy of the real message.
    if target.starts_with("headroom_desktop_lib::proxy_intercept")
        && msg.starts_with("[proxy_intercept] port")
        && msg.contains("held but not answering")
    {
        return true;
    }
    // report_codex_upstream_error logs the RAW upstream body locally and then
    // captures a separate, status-fingerprinted event with only the structural
    // summary. The bridge defeated both halves (RUST-5Q): the raw body — which
    // quotes the user's request fields — left the machine, and because this
    // line carries no fingerprint Sentry parameterized it into one grab-bag
    // mixing 400/403/503/507. The capture at the emit site is the Sentry path.
    if target.starts_with("headroom_desktop_lib::proxy_intercept")
        && msg.starts_with("codex upstream error ")
    {
        return true;
    }
    // Kompress prefetch is best-effort; the proxy lazy-loads the model on first
    // request if this fails. These two variants carry no actionable detail (the
    // spawn error is rare and the restart self-heals on next request), so they
    // are pure noise. The "download error" variant is NOT suppressed — it
    // carries a classified cause and is the systemic signal worth tracking.
    if target.starts_with("headroom_desktop_lib::state")
        && (msg.starts_with("kompress prefetch failed")
            || msg.starts_with("kompress prefetch: restart after download failed"))
    {
        return true;
    }
    // The download-error variant reaches Sentry via an explicit
    // category-fingerprinted capture_message at the emit site (RUST-3C
    // grab-bag split); the accompanying log::warn is local-only.
    if target.starts_with("headroom_desktop_lib::state")
        && msg.starts_with("kompress prefetch download error")
    {
        return true;
    }
    // Same split for the /stats probe: the reason is in the message, so a
    // 15s timeout and an HTTP 404 grouped as one issue (RUST-6V) that no fix
    // could ever resolve. The category-fingerprinted capture at the emit site
    // is the Sentry path.
    if target.starts_with("headroom_desktop_lib::state")
        && msg.starts_with("headroom /stats fetch failed")
    {
        return true;
    }
    // Boot-validation failure reaches Sentry via the fully-tagged Level::Error
    // capture at the same emit site (capture_runtime_upgrade_failure, RUST-4A:
    // versions, boot diagnostics, pip tail); this bridged warn double-reports
    // the same incident as RUST-2N with none of that context.
    if target.starts_with("headroom_desktop_lib::state")
        && msg.starts_with("run_upgrade_with_ui: boot validation failed")
    {
        return true;
    }
    // The pip final-failure warn embeds pip's stderr tail, so message-based
    // grouping opened a fresh issue per tail for one underlying failure
    // (RUST-6M/6N/6P, all the same half-built venv). It reaches Sentry via the
    // per-category fingerprinted capture at the emit site instead.
    if target.starts_with("headroom_desktop_lib::tool_manager")
        && msg.starts_with("pip install attempt ")
        && msg.contains("failed (final)")
    {
        return true;
    }
    // Same split as the pip line above: one partial-plugin-install message
    // covered five unrelated causes under one fingerprint (RUST-6K), so it was
    // untriageable AND unresolvable -- any resolve regressed on the next
    // sibling shape. It reaches Sentry via the per-category fingerprinted
    // capture at the emit site instead.
    if target.starts_with("headroom_desktop_lib::tool_manager")
        && msg.contains("installed for some hosts but not all")
    {
        return true;
    }
    // Ad-hoc codesign of venv native extensions is best-effort (EDR nicety):
    // codesign exits non-zero when a single .so can't be re-signed, but the
    // rest are signed and the smoke test is the real gate. A per-file failure
    // isn't actionable, so keep the log line but drop the Sentry event.
    if target.starts_with("headroom_desktop_lib::tool_manager")
        && msg.starts_with("ad-hoc codesign exited")
    {
        return true;
    }
    // Uninstall/cleanup teardown is best-effort by construction. It races a
    // still-exiting backend that re-creates a file mid-walk ("Directory not
    // empty"), a venv Windows still holds open ("Access is denied", RUST-6T),
    // and settings files we deliberately leave alone when they don't parse
    // ("refusing to overwrite potentially valid user settings", RUST-6X --
    // that branch is the correct one, not a failure). The app is being removed
    // either way, so none of it is actionable in a release. Matched by prefix
    // across modules: the same teardown runs from client_adapters (files,
    // settings) and lib (plugins, MCP servers), and every sibling shape landed
    // as its own un-fixable issue.
    if msg.starts_with("cleanup: ") || msg.starts_with("uninstall: removing ") {
        return true;
    }
    // Codex thread retag is best-effort over every *.sqlite in the Codex dirs;
    // a Codex-owned DB corrupted on the user's disk ("database disk image is
    // malformed") is environmental and unfixable by a release. The retag
    // already skips the file; keep the local log, drop the Sentry event.
    if target.starts_with("headroom_desktop_lib::client_adapters")
        && msg.starts_with("codex retag")
        && msg.contains("database disk image is malformed")
    {
        return true;
    }
    // The backend-port fallback reaches Sentry via the explicit capture at the
    // emit site (tool_manager), which carries occupant_cmd/occupant_pid tags and
    // both port numbers. This warn fires at the same instant with none of that
    // context, so one fallback landed as two issues (RUST-7E and RUST-7F, same
    // millisecond). Same split as the intercept-port line above.
    if target.starts_with("headroom_desktop_lib::tool_manager")
        && msg.starts_with("[backend_port] ")
    {
        return true;
    }
    // A host with no usable Secret Service (headless VM, xrdp session with no
    // login keyring) is the case this fallback exists FOR: the 0600 file is the
    // designed path, sign-in works, nothing is broken. It fired once per process
    // as a fresh error-level issue on every Linux box without a desktop keyring
    // (RUST-7G). Keep the local log so a support thread can see which store was
    // used; drop the Sentry event.
    if target.starts_with("headroom_desktop_lib::keychain")
        && msg.starts_with("OS credential store unusable")
    {
        return true;
    }
    // The machine-id digest is a deterministic value (sha256 of the hardware
    // UUID); the keychain write is a best-effort cache whose failure changes
    // nothing (next launch recomputes the same value). Dominant cause is a ghost
    // keychain entry from another app signature — environmental, unfixable here,
    // identical every launch. Keep the local log, drop the Sentry event (RUST-3P
    // / RUST-51: the earlier demote to log::warn still reached Sentry via this
    // logger, so it needs the explicit skip here).
    if target.starts_with("headroom_desktop_lib::device")
        && msg.starts_with("Could not persist machine id digest")
    {
        return true;
    }
    false
}

/// Replace the user's home directory with `~` wherever it appears.
pub(crate) fn scrub_home(msg: &str) -> String {
    match dirs::home_dir() {
        Some(home) => {
            let home = home.to_string_lossy();
            let home = home.trim_end_matches('/');
            if home.is_empty() {
                msg.to_string()
            } else {
                msg.replace(home, "~")
            }
        }
        None => msg.to_string(),
    }
}

impl Log for FileLogger {
    fn enabled(&self, _meta: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        let msg = format!("{}", record.args());
        let demote = record.level() <= Level::Warn && skip_sentry(record.target(), &msg);
        let display_level = if demote && record.level() == Level::Error {
            Level::Warn
        } else {
            record.level()
        };

        // Rotation must not depend on level: an info-heavy session can blow
        // past MAX_LOG_BYTES without ever logging a warning. Warn+ checks
        // every record; info/debug check every 64th to keep the stat off the
        // hot path.
        if display_level <= Level::Warn
            || self
                .records_since_rotate_check
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                % 64
                == 0
        {
            self.rotate_if_needed();
        }
        self.write_record(record, display_level);

        if record.level() <= Level::Warn {
            if demote {
                return;
            }
            let level = match record.level() {
                Level::Error => sentry::Level::Error,
                _ => sentry::Level::Warning,
            };
            // Home paths embed the local username; replace with ~ so it
            // never leaves the machine.
            let scrubbed = scrub_home(&msg);
            let truncated: String = scrubbed.chars().take(SENTRY_MESSAGE_CHAR_CAP).collect();
            sentry::capture_message(&truncated, level);
        }
    }

    fn flush(&self) {
        if let Ok(mut g) = self.file.lock() {
            if let Some(f) = g.as_mut() {
                let _ = f.flush();
            }
        }
    }
}

/// Initialize the global logger. Safe to call once at startup. Subsequent
/// calls return Err but do not panic.
pub fn init() -> Result<PathBuf, SetLoggerError> {
    let path = log_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok();
    let logger = FileLogger {
        file: Mutex::new(file),
        path: path.clone(),
        records_since_rotate_check: std::sync::atomic::AtomicU64::new(0),
    };
    log::set_boxed_logger(Box::new(logger))?;
    log::set_max_level(log::LevelFilter::Debug);
    Ok(path)
}

#[cfg(target_os = "macos")]
pub(crate) fn log_path() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join("Library/Logs/Headroom/headroom-desktop.log"))
        .unwrap_or_else(|| PathBuf::from("/tmp/headroom-desktop.log"))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn log_path() -> PathBuf {
    dirs::data_local_dir()
        .map(|d| d.join("headroom/headroom-desktop.log"))
        .unwrap_or_else(|| std::env::temp_dir().join("headroom-desktop.log"))
}

#[cfg(test)]
mod tests {
    use super::skip_sentry;

    #[test]
    fn skips_updater_transport_errors() {
        assert!(skip_sentry(
            "tauri_plugin_updater::updater",
            "failed to check for updates: error sending request for url (https://github.com/...)"
        ));
        assert!(skip_sentry(
            "tauri_plugin_updater",
            "dns error: failed to lookup address"
        ));
        assert!(skip_sentry(
            "tauri_plugin_updater::updater",
            "operation timed out"
        ));
    }

    #[test]
    fn skips_updater_endpoint_status_errors() {
        assert!(skip_sentry(
            "tauri_plugin_updater::updater",
            "update endpoint did not respond with a successful status code"
        ));
    }

    #[test]
    fn keeps_updater_non_transport_errors() {
        assert!(!skip_sentry(
            "tauri_plugin_updater::updater",
            "signature verification failed"
        ));
        assert!(!skip_sentry(
            "tauri_plugin_updater",
            "invalid release manifest"
        ));
    }

    #[test]
    fn skips_foreign_port_bind_retry_warns() {
        assert!(skip_sentry(
            "headroom_desktop_lib::proxy_intercept",
            "[proxy_intercept] port 6767 is held but not answering /health (leftover Headroom, another app, or a reserved range); retrying in 15s (Address already in use (os error 48))"
        ));
        // Other bind/loop errors from proxy_intercept stay in Sentry.
        assert!(!skip_sentry(
            "headroom_desktop_lib::proxy_intercept",
            "[proxy_intercept] error: some other failure; retrying in 15s"
        ));
    }

    #[test]
    fn skips_machine_id_digest_persist_failures() {
        assert!(skip_sentry(
            "headroom_desktop_lib::device",
            "Could not persist machine id digest (non-fatal, using computed value): duplicate item"
        ));
        // A different device.rs warning is not blanket-skipped.
        assert!(!skip_sentry(
            "headroom_desktop_lib::device",
            "hardware UUID unavailable"
        ));
    }

    #[test]
    fn skips_codex_retag_malformed_db() {
        assert!(skip_sentry(
            "headroom_desktop_lib::client_adapters",
            "codex retag openai->headroom skipped for ~/.codex/logs_2.sqlite: database disk image is malformed"
        ));
        // Other retag skip causes (locked DB, schema drift) stay in Sentry.
        assert!(!skip_sentry(
            "headroom_desktop_lib::client_adapters",
            "codex retag openai->headroom skipped for ~/.codex/state_5.sqlite: database is locked"
        ));
    }

    #[test]
    fn keeps_other_targets() {
        assert!(!skip_sentry(
            "headroom_desktop_lib::pricing",
            "error sending request: timeout"
        ));
        assert!(!skip_sentry("reqwest", "error sending request"));
    }

    #[test]
    fn skips_proxy_intercept_bypass_transport_errors() {
        assert!(skip_sentry(
            "headroom_desktop_lib::proxy_intercept",
            "proxy_intercept bypass forward failed: error sending request for url (https://api.anthropic.com/v1/messages?beta=true)"
        ));
        assert!(skip_sentry(
            "headroom_desktop_lib::proxy_intercept",
            "proxy_intercept bypass forward failed: dns error: failed to lookup address"
        ));
    }

    #[test]
    fn keeps_proxy_intercept_non_transport_errors() {
        assert!(!skip_sentry(
            "headroom_desktop_lib::proxy_intercept",
            "proxy_intercept bypass forward failed: invalid header value"
        ));
        assert!(!skip_sentry(
            "headroom_desktop_lib::proxy_intercept",
            "some other proxy_intercept warning"
        ));
    }

    #[test]
    fn skips_kompress_prefetch_best_effort_warnings() {
        assert!(skip_sentry(
            "headroom_desktop_lib::state",
            "kompress prefetch failed: some error"
        ));
        assert!(skip_sentry(
            "headroom_desktop_lib::state",
            "kompress prefetch: restart after download failed: boom"
        ));
    }

    #[test]
    fn skips_uninstall_cleanup_removal_warnings() {
        assert!(skip_sentry(
            "headroom_desktop_lib::client_adapters",
            "cleanup: removing /Users/x/Library/Application Support/Headroom failed: Directory not empty (os error 66)"
        ));
        // RUST-6X: the parse failed, so we left the file alone -- the safe
        // branch, reported as if it were a defect.
        assert!(skip_sentry(
            "headroom_desktop_lib::client_adapters",
            "cleanup: stripping hook from ~/.claude/settings.local.json failed: parsing \
             ~/.claude/settings.local.json failed (JSON/JSON5); refusing to overwrite \
             potentially valid user settings"
        ));
        // RUST-6T: same teardown, different module -- the venv is still open on
        // Windows when uninstall_and_quit deletes it.
        assert!(skip_sentry(
            "headroom_desktop_lib",
            "uninstall: removing serena failed: removing ~\\AppData\\Local\\Headroom\\headroom\\serena-venv: Access is denied. (os error 5)"
        ));
    }

    #[test]
    fn skips_codex_upstream_error_raw_body_warning() {
        // RUST-5Q: this line carries the raw upstream body (user request fields)
        // and no fingerprint, so Sentry grab-bagged 400/403/503/507 together.
        // The status-fingerprinted capture at the emit site is the Sentry path.
        assert!(skip_sentry(
            "headroom_desktop_lib::proxy_intercept",
            "codex upstream error 400 on /v1/responses: {\"error\":{\"message\":\"Unsupported value\"}}"
        ));
        assert!(skip_sentry(
            "headroom_desktop_lib::proxy_intercept",
            "codex upstream error 503 on /v1/responses: upstream connect error"
        ));
        assert!(!skip_sentry(
            "headroom_desktop_lib::proxy_intercept",
            "some other proxy_intercept warning"
        ));
    }

    #[test]
    fn skips_bridged_pip_final_failure_warning() {
        // RUST-6M/6N/6P: the stderr tail is in the message, so message-based
        // grouping opened a new issue per tail. The per-category fingerprinted
        // capture at the emit site is the Sentry path.
        assert!(skip_sentry(
            "headroom_desktop_lib::tool_manager",
            "pip install attempt 3/3 failed (final): exit=1; stderr tail: Check the permissions."
        ));
        assert!(skip_sentry(
            "headroom_desktop_lib::tool_manager",
            "pip install attempt 3/3 failed (final): exit=1; stderr tail: No module named pip"
        ));
        // The retry line is log::info (never bridged) and any other pip warn
        // still reports.
        assert!(!skip_sentry(
            "headroom_desktop_lib::tool_manager",
            "pip install produced no usable venv"
        ));
    }

    #[test]
    fn partial_plugin_install_warn_is_local_only() {
        // RUST-6K: the bridged warn grouped five causes under one fingerprint.
        // It now reaches Sentry only via the fingerprinted capture at the emit
        // site, so the warn itself must be demoted to local-only.
        assert!(skip_sentry(
            "headroom_desktop_lib::tool_manager",
            "ponytail installed for some hosts but not all: Codex: command failed (exit 1): \
             codex plugin add ponytail@ponytail"
        ));
        // Unrelated plugin warns still report.
        assert!(!skip_sentry(
            "headroom_desktop_lib::tool_manager",
            "caveman smoke test failed after upgrade: stale receipt removed"
        ));
    }

    #[test]
    fn skips_adhoc_codesign_best_effort_warning() {
        assert!(skip_sentry(
            "headroom_desktop_lib::tool_manager",
            "ad-hoc codesign exited Some(1) for 633 files: /path/_http_writer.so: replacing existing signature"
        ));
        // A genuine signing regression surfaces via the smoke-test gate, not
        // this best-effort line; an unrelated tool_manager warn still reports.
        assert!(!skip_sentry(
            "headroom_desktop_lib::tool_manager",
            "some other tool_manager warning"
        ));
    }

    #[test]
    fn skips_kompress_prefetch_download_error_warn() {
        // Sentry now gets this via the explicit category-fingerprinted
        // capture_message at the emit site (RUST-3C grab-bag split); the
        // bridged warn would double-report.
        assert!(skip_sentry(
            "headroom_desktop_lib::state",
            "kompress prefetch download error: [network] Max retries exceeded"
        ));
    }

    #[test]
    fn skips_stats_fetch_failed_warn() {
        // RUST-6V: timeout and HTTP 404 shared one message shape, so Sentry
        // grouped two different bugs together. The fingerprinted capture at
        // the emit site is the Sentry path now.
        assert!(skip_sentry(
            "headroom_desktop_lib::state",
            "headroom /stats fetch failed (HTTP 404 Not Found); dashboard loses the layers"
        ));
        assert!(skip_sentry(
            "headroom_desktop_lib::state",
            "headroom /stats fetch failed (timed out after 15s); dashboard loses the layers"
        ));
        assert!(!skip_sentry(
            "headroom_desktop_lib::state",
            "some other state warning"
        ));
    }

    #[test]
    fn skips_bypass_upgrade_forward_transport_errors() {
        // The websocket-upgrade forwarder variant (RUST-2R) gets the same
        // transient-transport treatment as the plain bypass forwarder.
        assert!(skip_sentry(
            "headroom_desktop_lib::proxy_intercept",
            "proxy_intercept bypass upgrade forward failed: error sending request for url (https://api.openai.com/v1/responses)"
        ));
        // Non-transport failures on the same path still report.
        assert!(!skip_sentry(
            "headroom_desktop_lib::proxy_intercept",
            "proxy_intercept bypass upgrade forward failed: builder error"
        ));
    }

    #[test]
    fn skips_boot_validation_failed_rollback_warn() {
        // capture_runtime_upgrade_failure at the same site carries the
        // fully-tagged event (RUST-4A); the bridged warn duplicated it as
        // RUST-2N.
        assert!(skip_sentry(
            "headroom_desktop_lib::state",
            "run_upgrade_with_ui: boot validation failed (timed_out); rolling back to Some(\"0.30.0\")"
        ));
    }

    #[test]
    fn keeps_other_state_warnings() {
        assert!(!skip_sentry(
            "headroom_desktop_lib::state",
            "some other state warning"
        ));
    }

    #[test]
    fn skips_backend_port_fallback_warn_but_not_siblings() {
        // The emit-site capture_message is the Sentry path for this event.
        assert!(skip_sentry(
            "headroom_desktop_lib::tool_manager",
            "[backend_port] 6768 held by unknown process; falling back to 6770"
        ));
        // Other tool_manager warnings still report.
        assert!(!skip_sentry(
            "headroom_desktop_lib::tool_manager",
            "managed headroom exited unexpectedly"
        ));
    }

    #[test]
    fn skips_keyring_fallback_but_not_other_keychain_failures() {
        assert!(skip_sentry(
            "headroom_desktop_lib::keychain",
            "OS credential store unusable (Couldn't access platform secure storage: \
             Secret Service: no result found); storing Headroom secrets in a 0600 file \
             under the app data dir instead"
        ));
        // A real keychain failure that is NOT the designed fallback still reports.
        assert!(!skip_sentry(
            "headroom_desktop_lib::keychain",
            "failed to write Headroom secret to the file store"
        ));
    }

    #[test]
    fn scrub_home_replaces_home_dir_with_tilde() {
        let home = dirs::home_dir().unwrap();
        let msg = format!(
            "cleanup: removing {}/Library/Application Support/x",
            home.display()
        );
        let scrubbed = super::scrub_home(&msg);
        assert_eq!(
            scrubbed,
            "cleanup: removing ~/Library/Application Support/x"
        );
        assert_eq!(super::scrub_home("no paths here"), "no paths here");
    }
}
