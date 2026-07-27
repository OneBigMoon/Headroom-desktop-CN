use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::models::{
    ClientConnectorStatus, ClientHealth, ClientSetupResult, ClientSetupVerification, ClientStatus,
};
use crate::storage::{app_data_dir, config_file};

// Raw proxy base — use provider-specific constants below when configuring client endpoints.
const HEADROOM_PROXY_URL: &str = "http://127.0.0.1:6767";
const HEADROOM_ANTHROPIC_BASE_URL: &str = "http://127.0.0.1:6767";
const HEADROOM_OPENAI_BASE_URL: &str = "http://127.0.0.1:6767/v1";
const HEADROOM_GROK_PROXY_BASE_URL: &str = "http://127.0.0.1:6767/v1";
const ZSH_PROFILE_FILE: &str = ".zprofile";
const ZSH_RC_FILE: &str = ".zshrc";
const BASH_PROFILE_FILE: &str = ".bash_profile";
const BASH_LOGIN_FILE: &str = ".bash_login";
const POSIX_PROFILE_FILE: &str = ".profile";
const BASH_RC_FILE: &str = ".bashrc";
const ALL_SHELL_FILES: [&str; 6] = [
    ZSH_PROFILE_FILE,
    ZSH_RC_FILE,
    BASH_PROFILE_FILE,
    BASH_LOGIN_FILE,
    POSIX_PROFILE_FILE,
    BASH_RC_FILE,
];

#[derive(Debug, Clone, Copy)]
struct ManagedClientSpec {
    id: &'static str,
    name: &'static str,
}

const MANAGED_CLIENT_SPECS: [ManagedClientSpec; 3] = [
    ManagedClientSpec {
        id: "claude_code",
        name: "Claude Code",
    },
    ManagedClientSpec {
        id: "codex",
        name: "Codex",
    },
    ManagedClientSpec {
        id: "grok_build",
        name: "Grok Build",
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShellFamily {
    Zsh,
    Bash,
    Posix,
}

pub fn detect_clients() -> Vec<ClientStatus> {
    let setup_state = load_setup_state();

    vec![
        detect_claude_code_client(is_configured(&setup_state, "claude_code")),
        detect_codex_client(is_configured(&setup_state, "codex")),
        detect_grok_build_client(is_configured(&setup_state, "grok_build")),
    ]
}

pub fn ensure_rtk_integrations(
    managed_rtk_path: &Path,
    managed_python_path: &Path,
) -> Result<(Vec<String>, Vec<String>)> {
    ensure_rtk_integrations_for_targets(
        managed_rtk_path,
        managed_python_path,
        &resolve_default_shell_targets(),
    )
}

fn ensure_rtk_integrations_for_targets(
    managed_rtk_path: &Path,
    managed_python_path: &Path,
    shell_targets: &[PathBuf],
) -> Result<(Vec<String>, Vec<String>)> {
    // Respect the user's opt-out so bootstrap, restore, and client setup don't
    // silently re-add the PATH export and Claude Code hook after they've been
    // turned off via the tool status toggle. Also skip when the binary is absent
    // (not installed / uninstalled) so we never write integrations pointing at a
    // missing rtk.
    if is_rtk_disabled() || !managed_rtk_path.exists() {
        return Ok((Vec::new(), Vec::new()));
    }

    let mut changed_files = Vec::new();
    let mut backup_files = Vec::new();

    let mut path_updates = ensure_managed_rtk_on_path(managed_rtk_path, shell_targets)?;
    let mut hook_updates = ensure_claude_code_rtk_hook(managed_rtk_path, managed_python_path)?;
    changed_files.append(&mut path_updates.0);
    backup_files.append(&mut path_updates.1);
    changed_files.append(&mut hook_updates.0);
    backup_files.append(&mut hook_updates.1);

    // Codex has no PreToolUse-style hook, so the auto-rewrite can't be wired the
    // way it is for Claude Code. Mirror the MarkItDown approach: drop a managed
    // `~/.codex/AGENTS.md` nudge telling Codex to route shell commands through
    // the managed `rtk` binary (which is already on PATH via the block above).
    if is_codex_enabled() {
        let agents = rtk_codex_agents_path();
        let (codex_changed, codex_backup) =
            upsert_managed_block(&agents, "rtk", &build_rtk_codex_nudge(managed_rtk_path))?;
        if codex_changed {
            changed_files.push(agents.display().to_string());
        }
        if let Some(path) = codex_backup {
            backup_files.push(path.display().to_string());
        }
    }

    Ok((changed_files, backup_files))
}

fn rtk_codex_agents_path() -> PathBuf {
    codex_home().join("AGENTS.md")
}

/// Codex nudge: Codex has no command-rewrite hook, so it routes shell commands
/// through the managed `rtk` binary by being told to prefix them with it.
fn build_rtk_codex_nudge(managed_rtk_path: &Path) -> String {
    let bin = managed_rtk_path.display();
    format!(
        "## Token-saving shell commands (Headroom RTK)\n\
         Run shell commands through RTK to get compact, token-optimized output:\n\
         prefix the command with `{bin} ` (for example `{bin} git status`,\n\
         `{bin} ls -la`, `{bin} cargo build`). RTK compacts output, so do NOT\n\
         use it when you need verbatim text: reading or grepping code you are\n\
         about to edit or patch (RTK grep strips indentation and truncates long\n\
         lines), or `git diff --check` (RTK drops its whitespace report). Run\n\
         those raw. Everything else (status, logs, builds, tests, listings) is\n\
         safe to prefix."
    )
}

pub fn rtk_integration_status() -> Result<(bool, bool)> {
    let path_configured = shell_block_contains_text_in_files(
        &resolve_default_shell_targets(),
        "managed_rtk",
        "export PATH=",
    )?;
    let hook_configured = claude_settings_hook_matches("headroom-rtk-rewrite.sh")?
        && headroom_rtk_hook_path().exists();
    Ok((path_configured, hook_configured))
}

/// True when the user turned RTK off via the tool status toggle.
pub fn is_rtk_disabled() -> bool {
    load_setup_state().rtk_disabled
}

/// Enable or disable RTK from the tool status toggle. Disabling tears down the
/// RTK PATH export, the Claude Code hook, and the Codex AGENTS.md nudge (without
/// touching `ANTHROPIC_BASE_URL` routing) and persists the opt-out so bootstrap
/// won't re-add them. Enabling clears the flag and re-applies the integrations.
pub fn set_rtk_enabled(
    enabled: bool,
    managed_rtk_path: &Path,
    managed_python_path: &Path,
) -> Result<()> {
    let mut state = load_setup_state();
    state.rtk_disabled = !enabled;
    write_setup_state(&state)?;

    if enabled {
        ensure_rtk_integrations(managed_rtk_path, managed_python_path)?;
    } else {
        let shell_targets = resolve_client_shell_targets_for_cleanup(&state, "claude_code")?;
        remove_shell_block(&shell_targets, "managed_rtk")?;
        for settings_path in claude_settings_candidates() {
            let _ = strip_headroom_hook_from_settings(&settings_path);
        }
        let hook_path = headroom_rtk_hook_path();
        if hook_path.exists() {
            let _ = std::fs::remove_file(&hook_path);
        }
        let _ = remove_managed_block(&rtk_codex_agents_path(), "rtk");
    }

    Ok(())
}

/// True when the error chain contains a filesystem PermissionDenied (os error 13),
/// i.e. an unwritable target -- an environment issue, not an app bug.
pub fn is_permission_denied(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.kind() == std::io::ErrorKind::PermissionDenied)
    })
}

/// True when the error chain contains a filesystem "no space left on device"
/// (ENOSPC, os error 28) -- a full disk, an environment issue not an app bug,
/// same class as `is_permission_denied`. ErrorKind::StorageFull isn't stable, so
/// match the raw errno (28 on macOS and Linux).
pub fn is_no_space(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.raw_os_error() == Some(28))
    })
}

/// Runs a shell-profile write step, tolerating an unwritable profile (e.g. a
/// read-only ~/.zshrc -> os error 13). The env that actually routes a client
/// lives in app-owned config (~/.claude/settings.json, ~/.codex/config.toml), so
/// a locked shell file costs terminal convenience, not core routing. Returns
/// `Ok(None)` when the step was skipped for that reason.
fn shell_step_best_effort(
    step: Result<(Vec<String>, Vec<String>)>,
) -> Result<Option<(Vec<String>, Vec<String>)>> {
    match step {
        Ok(updates) => Ok(Some(updates)),
        Err(err) if is_permission_denied(&err) => Ok(None),
        Err(err) => Err(err),
    }
}

pub fn apply_client_setup(client_id: &str) -> Result<ClientSetupResult> {
    let first = apply_client_setup_once(client_id)?;
    if first.verification.verified {
        return Ok(first);
    }
    // Apply-ok-but-verify-miss is a lost-update race (RUST-3W): a concurrent
    // read-modify-write on the same file — the MCP registrar re-run at boot
    // rewrites ~/.codex/config.toml non-atomically, Codex itself rewrites it on
    // exit — can clobber a just-written block before verification reads it
    // back. Re-apply once; a persistent failure still returns unverified and
    // reaches Sentry.
    let mut second = apply_client_setup_once(client_id)?;
    for file in first.changed_files {
        if !second.changed_files.contains(&file) {
            second.changed_files.push(file);
        }
    }
    for file in first.backup_files {
        if !second.backup_files.contains(&file) {
            second.backup_files.push(file);
        }
    }
    second.already_configured &= first.already_configured;
    Ok(second)
}

fn apply_client_setup_once(client_id: &str) -> Result<ClientSetupResult> {
    let mut changed_files = Vec::new();
    let mut backup_files = Vec::new();
    let mut state = load_setup_state();
    let state_id = normalized_setup_id(client_id).to_string();
    let mut shell_unwritable = false;
    let mut replaced_base_url = None;

    match client_id {
        "claude_code" => {
            let shell_targets = resolve_client_shell_targets(&state, client_id)?;
            // Critical, app-owned writes first: the ~/.claude/settings.json env is
            // what actually routes Claude Code through Headroom. Do it before the
            // shell profile so a locked ~/.zshrc can't block core setup.
            let (changed, backups, replaced) =
                configure_claude_settings_env("ANTHROPIC_BASE_URL", HEADROOM_ANTHROPIC_BASE_URL)?;
            let mut updates = (changed, backups);
            if let Some(original) = replaced {
                // A custom gateway/proxy URL was routing Claude before us:
                // remember it for restore-on-disable and tell the caller so
                // the UI can inform the user their routing changed.
                state
                    .preserved_base_urls
                    .insert(state_id.clone(), original.clone());
                replaced_base_url = Some(original);
            }
            let mut legacy_updates = remove_legacy_vscode_base_url_keys()?;
            updates.0.append(&mut legacy_updates.0);
            updates.1.append(&mut legacy_updates.1);

            // Loud-fail guard so a closed app or lost ANTHROPIC_BASE_URL routing
            // surfaces in Claude instead of silently hitting Anthropic directly.
            let mut guard = ensure_claude_guard_hook()?;
            updates.0.append(&mut guard.0);
            updates.1.append(&mut guard.1);

            // Shell profile (RTK PATH + env export) is convenience; tolerate an
            // unwritable profile rather than failing the whole setup.
            let env_block = format!("export ANTHROPIC_BASE_URL={}", HEADROOM_ANTHROPIC_BASE_URL);
            let shell_step = ensure_rtk_integrations_for_targets(
                &default_headroom_rtk_path(),
                &default_headroom_managed_python_path(),
                &shell_targets,
            )
            .and_then(|mut rtk| {
                let mut env = configure_shell_block(&shell_targets, "claude_code", &env_block)?;
                rtk.0.append(&mut env.0);
                rtk.1.append(&mut env.1);
                Ok(rtk)
            });
            match shell_step_best_effort(shell_step)? {
                Some(mut shell) => {
                    updates.0.append(&mut shell.0);
                    updates.1.append(&mut shell.1);
                }
                None => shell_unwritable = true,
            }

            changed_files.extend(updates.0);
            backup_files.extend(updates.1);
            state
                .managed_shell_files
                .insert(state_id.clone(), serialize_paths(&shell_targets));
        }
        "vscode" => {
            let (changed, backups, replaced) = configure_vscode_settings()?;
            changed_files.extend(changed);
            backup_files.extend(backups);
            if let Some(original) = replaced {
                state
                    .preserved_base_urls
                    .insert(state_id.clone(), original.clone());
                replaced_base_url = Some(original);
            }
        }
        "codex" | "codex_cli" => {
            let shell_targets = resolve_client_shell_targets(&state, client_id)?;
            // Critical, app-owned write first: the ~/.codex/config.toml provider
            // block is what routes Codex through Headroom.
            let (changed, backups, preserved_provider) = configure_codex_provider_block()?;
            let mut updates = (changed, backups);
            if let Some(original) = preserved_provider {
                // A custom root `model_provider` (gateway/alternate provider) was
                // routing Codex before us: remember it for restore-on-disable so
                // we don't silently drop the user onto api.openai.com. Restored
                // silently (no takeover notice — that copy is Claude/base_url
                // specific).
                state.preserved_base_urls.insert(state_id.clone(), original);
            }

            // Loud-fail guard so a closed app or clobbered config surfaces in
            // Codex instead of silently routing direct to OpenAI.
            let mut guard = ensure_codex_guard_hook()?;
            updates.0.append(&mut guard.0);
            updates.1.append(&mut guard.1);

            let env_block = format!("export OPENAI_BASE_URL={}", HEADROOM_OPENAI_BASE_URL);
            match shell_step_best_effort(configure_shell_block(
                &shell_targets,
                "codex_cli",
                &env_block,
            ))? {
                Some(mut shell) => {
                    updates.0.append(&mut shell.0);
                    updates.1.append(&mut shell.1);
                }
                None => shell_unwritable = true,
            }
            changed_files.extend(updates.0);
            backup_files.extend(updates.1);
            state
                .managed_shell_files
                .insert(state_id.clone(), serialize_paths(&shell_targets));
            // Pull existing native threads into the headroom-provider menu so the
            // Codex history list stays whole once it routes through Headroom.
            retag_codex_thread_providers(CODEX_NATIVE_PROVIDER, CODEX_HEADROOM_PROVIDER);
        }
        "grok_build" => {
            let shell_targets = resolve_client_shell_targets(&state, client_id)?;
            let mut updates = configure_grok_proxy_block()?;
            let env_block = format!(
                "export GROK_CLI_CHAT_PROXY_BASE_URL={}",
                HEADROOM_GROK_PROXY_BASE_URL
            );
            match shell_step_best_effort(configure_shell_block(
                &shell_targets,
                "grok_build",
                &env_block,
            ))? {
                Some(mut shell) => {
                    updates.0.append(&mut shell.0);
                    updates.1.append(&mut shell.1);
                }
                None => shell_unwritable = true,
            }
            changed_files.extend(updates.0);
            backup_files.extend(updates.1);
            state
                .managed_shell_files
                .insert(state_id.clone(), serialize_paths(&shell_targets));
        }
        other => return Err(anyhow!("Automatic setup is not supported yet for {other}.",)),
    }

    let configured_at = Utc::now().to_rfc3339();
    state.configured_clients.insert(state_id, configured_at);
    write_setup_state(&state)?;

    let already_configured = changed_files.is_empty();
    let summary = if already_configured {
        "Client was already configured for Headroom.".to_string()
    } else {
        "Client configuration updated to route through Headroom.".to_string()
    };

    let verification = verify_client_setup(client_id)?;

    Ok(ClientSetupResult {
        client_id: client_id.to_string(),
        applied: true,
        already_configured,
        summary,
        changed_files,
        backup_files,
        next_steps: {
            let mut steps = Vec::new();
            if shell_unwritable {
                steps.push(
                    "Your shell profile (e.g. ~/.zshrc) isn't writable, so the PATH/export couldn't be added. Core routing still works via the client's own config; to launch the client from a terminal, fix the file's permissions and re-run setup, or add the export manually."
                        .into(),
                );
            }
            steps.push(
                "Restart your terminal/editor session to pick up environment changes.".into(),
            );
            if normalized_setup_id(client_id) == "codex_cli" {
                steps.push(
                    "In Codex, run /hooks and trust the Headroom routing guard so it can warn you if routing breaks (re-trust if Headroom updates the guard)."
                        .into(),
                );
            }
            steps.push(format!(
                "Run one {} prompt and verify activity appears in Headroom.",
                match normalized_setup_id(client_id) {
                    "codex_cli" => "Codex",
                    "grok_build" => "Grok Build",
                    _ => "Claude Code",
                }
            ));
            steps
        },
        verification,
        shell_profile_unwritable: shell_unwritable,
        replaced_base_url,
    })
}

pub fn verify_client_setup(client_id: &str) -> Result<ClientSetupVerification> {
    let mut checks = Vec::new();
    let mut failures = Vec::new();

    match client_id {
        "claude_code" => {
            let state = load_setup_state();
            let shell_targets = resolve_client_shell_targets(&state, client_id)?;
            let shell_ok = shell_block_contains_in_files(
                &shell_targets,
                "claude_code",
                "ANTHROPIC_BASE_URL",
                HEADROOM_ANTHROPIC_BASE_URL,
            )?;
            let rtk_path_ok =
                shell_block_contains_text_in_files(&shell_targets, "managed_rtk", "export PATH=")?;
            let claude_settings_ok =
                claude_settings_env_matches("ANTHROPIC_BASE_URL", HEADROOM_ANTHROPIC_BASE_URL)?;
            let rtk_hook_ok = claude_settings_hook_matches("headroom-rtk-rewrite.sh")?
                && headroom_rtk_hook_path().exists();

            if shell_ok {
                checks.push(
                    "Found Claude Code ANTHROPIC_BASE_URL export in managed shell block.".into(),
                );
            }
            if rtk_path_ok {
                checks.push("Found Headroom-managed RTK PATH export in shell profiles.".into());
            }
            if claude_settings_ok {
                checks.push(
                    "Found ~/.claude/settings.json env.ANTHROPIC_BASE_URL pointing to Headroom."
                        .into(),
                );
            }
            if rtk_hook_ok {
                checks.push(
                    "Found Headroom-managed RTK Claude hook in ~/.claude/settings.json.".into(),
                );
            }
            if !shell_ok && !claude_settings_ok {
                failures.push(
                    "Claude Code ANTHROPIC_BASE_URL was not found in shell blocks or ~/.claude/settings.json."
                        .into(),
                );
            }
            // RTK is a separate, opt-in integration (`set_rtk_enabled` tears it
            // down without touching ANTHROPIC_BASE_URL routing). Its wiring is
            // only ever added when the managed binary exists on disk (see
            // `ensure_rtk_integrations_for_targets`), so its absence must not
            // fail Claude Code verification when RTK isn't installed or the user
            // disabled it — routing is what "connected" means here.
            let rtk_required = !state.rtk_disabled && default_headroom_rtk_path().exists();
            if rtk_required && !rtk_path_ok {
                failures.push(
                    "Headroom-managed RTK PATH export was not found in shell profiles.".into(),
                );
            }
            if rtk_required && !rtk_hook_ok {
                failures.push(
                    "Headroom-managed RTK Claude hook was not found in ~/.claude/settings.json."
                        .into(),
                );
            }

            if claude_guard_hook_path().exists() && claude_guard_registered()? {
                checks.push(
                    "Found Headroom routing guard registered in ~/.claude/settings.json.".into(),
                );
            } else {
                failures.push(
                    "Headroom routing guard was not found in ~/.claude/settings.json.".into(),
                );
            }
        }
        "vscode" => {
            let mut delegated = verify_client_setup("claude_code")?;
            delegated.client_id = "vscode".to_string();
            return Ok(delegated);
        }
        "codex" | "codex_cli" => {
            let state = load_setup_state();
            let shell_targets = resolve_client_shell_targets(&state, client_id)?;
            let shell_ok = shell_block_contains_in_files(
                &shell_targets,
                "codex_cli",
                "OPENAI_BASE_URL",
                HEADROOM_OPENAI_BASE_URL,
            )?;
            let toml_ok = codex_provider_block_matches()?;

            if shell_ok {
                checks.push("Found Codex OPENAI_BASE_URL export in managed shell block.".into());
            }
            if toml_ok {
                checks
                    .push("Found Headroom-managed provider block in ~/.codex/config.toml.".into());
            }
            if !toml_ok {
                failures.push(
                    "Headroom-managed provider block was not found in ~/.codex/config.toml.".into(),
                );
            }
            // Shell export is convenience, not routing: config.toml is what routes
            // Codex (apply tolerates an unwritable shell profile). A missing export
            // must not fail verification -- mirrors Claude, which only fails when
            // *no* routing source is present.

            if codex_guard_hook_path().exists() && codex_guard_registered()? {
                checks
                    .push("Found Headroom routing guard registered in ~/.codex/hooks.json.".into());
            } else {
                failures
                    .push("Headroom routing guard was not found in ~/.codex/hooks.json.".into());
            }

            // Independent confirmation from Codex itself, run off-thread: the
            // `codex doctor` call takes seconds and `verify` is awaited by the
            // setup UI, so block it there and we stall the flow. Detached and
            // logged (its output isn't surfaced in the result today); never a
            // `verified` failure (doctor can flag unrelated issues, and an
            // untrusted-but-installed guard is expected until the user runs
            // /hooks).
            std::thread::spawn(|| {
                if let Some(summary) = codex_doctor_summary() {
                    log::info!("codex doctor: {summary}");
                }
            });
        }
        "grok_build" => {
            let state = load_setup_state();
            let shell_targets = resolve_client_shell_targets(&state, client_id)?;
            let shell_ok = shell_block_contains_in_files(
                &shell_targets,
                "grok_build",
                "GROK_CLI_CHAT_PROXY_BASE_URL",
                HEADROOM_GROK_PROXY_BASE_URL,
            )?;
            let toml_ok = grok_proxy_block_matches()?;

            if shell_ok {
                checks.push(
                    "Found Grok Build GROK_CLI_CHAT_PROXY_BASE_URL export in managed shell block."
                        .into(),
                );
            }
            if toml_ok {
                checks.push("Found Headroom-managed proxy block in ~/.grok/config.toml.".into());
            }
            if !toml_ok {
                failures.push(
                    "Headroom-managed proxy block was not found in ~/.grok/config.toml.".into(),
                );
            }
            if !shell_ok {
                failures.push(
                    "Grok Build GROK_CLI_CHAT_PROXY_BASE_URL export was not found in shell profiles."
                        .into(),
                );
            }
        }
        other => return Err(anyhow!("Verification is not supported yet for {other}.",)),
    }

    // Proxy reachability is transient runtime state — the runtime warm-up
    // can finish after this verification runs. Surface it via the
    // `proxy_reachable` field, but don't fail `verified` on it. `verified`
    // attests only to "we wrote everything we needed to write".
    let proxy_reachable = is_headroom_proxy_reachable();
    if proxy_reachable {
        checks.push("Headroom proxy is reachable on 127.0.0.1:6767.".into());
    }

    Ok(ClientSetupVerification {
        client_id: client_id.to_string(),
        verified: failures.is_empty(),
        proxy_reachable,
        checks,
        failures,
    })
}

pub fn is_claude_code_enabled() -> bool {
    is_configured(&load_setup_state(), "claude_code")
}

pub fn is_codex_enabled() -> bool {
    is_configured(&load_setup_state(), "codex_cli")
}

pub fn is_grok_build_enabled() -> bool {
    is_configured(&load_setup_state(), "grok_build")
}

pub fn list_client_connectors(
    detected_clients: &[ClientStatus],
) -> Result<Vec<ClientConnectorStatus>> {
    let setup_state = load_setup_state();

    let connectors = MANAGED_CLIENT_SPECS
        .iter()
        .map(|spec| {
            let installed = detected_clients
                .iter()
                .find(|client| client.id == spec.id)
                .map(|client| client.installed)
                .unwrap_or(false);
            // Fall back to the remembered snapshot while restore_client_setups
            // is still re-applying on launch, so the connector doesn't flash
            // "disabled" during the async restore window after a restart.
            let enabled = is_configured(&setup_state, spec.id)
                || setup_state
                    .remembered_clients
                    .contains_key(normalized_setup_id(spec.id));
            let verified = if enabled {
                verify_client_setup(spec.id)
                    .map(|result| result.verified)
                    .unwrap_or(false)
            } else {
                false
            };

            ClientConnectorStatus {
                client_id: spec.id.to_string(),
                name: spec.name.to_string(),
                installed,
                enabled,
                verified,
                last_configured_at: configured_timestamp(&setup_state, spec.id),
            }
        })
        .collect();

    Ok(connectors)
}

pub fn disable_client_setup(client_id: &str) -> Result<()> {
    let mut state = load_setup_state();

    match client_id {
        "codex" | "codex_cli" => {
            let preserved_provider = state
                .preserved_base_urls
                .get(normalized_setup_id(client_id))
                .cloned();
            disable_codex_cli()?;
            // Restore any pre-Headroom root model_provider instead of leaving the
            // key deleted -- deleting it silently drops a gateway user onto
            // api.openai.com (mirrors the Claude base_url restore).
            if let Some(provider) = preserved_provider {
                let _ = restore_codex_model_provider(&provider);
            }
            disable_codex_gui()?;
            // Hand the threads back to the native-provider menu so the full
            // history stays visible once Codex no longer routes through Headroom.
            retag_codex_thread_providers(CODEX_HEADROOM_PROVIDER, CODEX_NATIVE_PROVIDER);
        }
        "codex_gui" => {
            disable_codex_gui()?;
        }
        "claude_code" => {
            let shell_targets = resolve_client_shell_targets_for_cleanup(&state, client_id)?;
            remove_shell_block(&shell_targets, "claude_code")?;
            // Also drop the managed_rtk PATH block so `rtk` isn't exported from
            // shell profiles after quit — otherwise the user's next shell still
            // has Headroom binaries shadowing whatever's on PATH.
            remove_shell_block(&shell_targets, "managed_rtk")?;
            // Restore any pre-Headroom gateway/proxy URL instead of deleting
            // the key — deleting it pointed gateway users at api.anthropic.com
            // where their credentials may not even work.
            let preserved = state
                .preserved_base_urls
                .get(normalized_setup_id(client_id))
                .cloned();
            remove_claude_settings_env(
                "ANTHROPIC_BASE_URL",
                HEADROOM_ANTHROPIC_BASE_URL,
                preserved.as_deref(),
            )?;
            let _ = remove_legacy_vscode_base_url_keys()?;
            // Strip the PreToolUse hook entry and delete the hook script so CC
            // behaves exactly as it did before Headroom was launched.
            for settings_path in claude_settings_candidates() {
                let _ = strip_headroom_hook_from_settings(&settings_path);
            }
            let hook_path = headroom_rtk_hook_path();
            if hook_path.exists() {
                let _ = std::fs::remove_file(&hook_path);
            }
            let _ = remove_claude_guard_hook();
        }
        "vscode" => {
            let preserved = state
                .preserved_base_urls
                .get(normalized_setup_id(client_id))
                .cloned();
            remove_vscode_connector_keys(preserved.as_deref())?;
        }
        "grok_build" => disable_grok_build()?,
        other => {
            return Err(anyhow!(
                "Automatic setup disable is not supported yet for {other}.",
            ))
        }
    }

    match client_id {
        "codex" | "codex_cli" => {
            state.configured_clients.remove("codex");
            state.configured_clients.remove("codex_cli");
            state.configured_clients.remove("codex_gui");
            state.remembered_clients.remove("codex");
            state.remembered_clients.remove("codex_cli");
            state.remembered_clients.remove("codex_gui");
            state.managed_shell_files.remove("codex");
            state.managed_shell_files.remove("codex_cli");
            state.managed_shell_files.remove("codex_gui");
            state.remembered_shell_files.remove("codex");
            state.remembered_shell_files.remove("codex_cli");
            state.remembered_shell_files.remove("codex_gui");
            // Consumed: the provider is back in the user's config now. The next
            // apply re-captures it if Headroom is re-enabled.
            state.preserved_base_urls.remove("codex_cli");
        }
        _ => {
            let state_id = normalized_setup_id(client_id);
            state.configured_clients.remove(state_id);
            state.remembered_clients.remove(state_id);
            state.managed_shell_files.remove(state_id);
            state.remembered_shell_files.remove(state_id);
            // Consumed: the URL is back in the user's config now. The next
            // apply re-captures it if Headroom is re-enabled.
            state.preserved_base_urls.remove(state_id);
        }
    }
    write_setup_state(&state)?;
    Ok(())
}

pub fn clear_client_setups() -> Result<()> {
    // Capture snapshot before disabling. We re-apply it afterwards because
    // disable_client_setup also clears remembered_clients as a side effect,
    // which would otherwise erase the snapshot we need for restore_client_setups.
    let pre = load_setup_state();
    // Merge with any prior snapshot so a second clear is idempotent: after a
    // pause, configured_clients is already empty and only remembered_clients
    // holds the restore set — a quit-time clear must not wipe it (pause then
    // Cmd-Q used to permanently lose all connectors).
    let mut snapshot_clients = pre.remembered_clients.clone();
    snapshot_clients.extend(pre.configured_clients.clone());
    let mut snapshot_shell_files = pre.remembered_shell_files.clone();
    snapshot_shell_files.extend(pre.managed_shell_files.clone());

    for spec in MANAGED_CLIENT_SPECS {
        let _ = disable_client_setup(spec.id);
    }
    let _ = disable_client_setup("codex_gui");

    // Re-save the remembered snapshot so restore_client_setups works on next launch.
    if !snapshot_clients.is_empty() {
        let mut state = load_setup_state();
        state.remembered_clients = snapshot_clients;
        state.remembered_shell_files = snapshot_shell_files;
        write_setup_state(&state)?;
    }

    Ok(())
}

/// Fully uninstalls Headroom's on-disk footprint on a best-effort basis:
/// reverses every client setup, strips Headroom's hook entry from Claude Code
/// settings (both `settings.json` and `settings.local.json`), deletes the
/// managed hook script, the Headroom application-support directory, the
/// `~/.headroom` Python runtime, the macOS LaunchAgent plist, Preferences,
/// Caches, and keychain entries.
///
/// Returns the list of paths that were successfully removed (useful for
/// surfacing to the user). Per-step failures are logged and skipped.
/// `remove_dir_all`, retrying on transient `ENOTEMPTY`. A backend/proxy
/// process killed in `stop_headroom` may still flush a log line into the
/// directory tree mid-walk, re-creating an entry so the final `rmdir` fails
/// with "Directory not empty". A short backoff lets the writer finish.
fn remove_dir_all_retry(path: &Path) -> std::io::Result<()> {
    let mut last = Ok(());
    for attempt in 0..5 {
        match std::fs::remove_dir_all(path) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => {
                last = Err(e);
                std::thread::sleep(Duration::from_millis(100 * (attempt + 1)));
            }
        }
    }
    last
}

pub fn perform_full_cleanup() -> Vec<String> {
    let mut removed: Vec<String> = Vec::new();

    // Reverse settings.json mutations and shell blocks for every known client.
    if let Err(err) = clear_client_setups() {
        log::warn!("cleanup: clear_client_setups failed: {err}");
    }

    // Strip the Headroom hook entry from both ~/.claude/settings.json and
    // ~/.claude/settings.local.json. `clear_client_setups` doesn't do this —
    // it only removes env keys — so without this step the hook entry remains,
    // points to a deleted script, and Claude Code logs errors on every call.
    for settings_path in claude_settings_candidates() {
        match strip_headroom_hook_from_settings(&settings_path) {
            Ok(true) => removed.push(settings_path.display().to_string()),
            Ok(false) => {}
            Err(err) => log::warn!(
                "cleanup: stripping hook from {} failed: {err}",
                settings_path.display()
            ),
        }
    }

    // Independently strip the ANTHROPIC_BASE_URL routing env and the Claude
    // guard hook. clear_client_setups() above also removes these via
    // disable_client_setup, but only after remove_shell_block succeeds (it runs
    // under `?` before them): a shell-rc failure there silently leaves both in
    // place, and each bricks Claude once the proxy is gone (stale base URL ->
    // dead 127.0.0.1:6767; guard hook errors on every prompt). Do them
    // unconditionally here. Idempotent: each only acts on Headroom's own value,
    // restoring any preserved pre-Headroom gateway URL.
    let preserved = load_setup_state()
        .preserved_base_urls
        .get(normalized_setup_id("claude_code"))
        .cloned();
    if let Err(err) = remove_claude_settings_env(
        "ANTHROPIC_BASE_URL",
        HEADROOM_ANTHROPIC_BASE_URL,
        preserved.as_deref(),
    ) {
        log::warn!("cleanup: removing ANTHROPIC_BASE_URL from Claude settings failed: {err}");
    }
    if let Err(err) = remove_claude_guard_hook() {
        log::warn!("cleanup: removing Claude guard hook failed: {err}");
    }

    for hook_path in [headroom_rtk_hook_path(), headroom_markitdown_hook_path()] {
        if hook_path.exists() {
            match std::fs::remove_file(&hook_path) {
                Ok(_) => removed.push(hook_path.display().to_string()),
                Err(err) => log::warn!("cleanup: removing {} failed: {err}", hook_path.display()),
            }
        }
    }

    // Drop the managed RTK nudge from ~/.codex/AGENTS.md (clear_client_setups
    // handles env/shell blocks but not these managed Markdown blocks).
    if let Err(err) = remove_managed_block(&rtk_codex_agents_path(), "rtk") {
        log::warn!("cleanup: removing rtk AGENTS.md block failed: {err}");
    }

    // Also wipe the per-client setup-state file so a reinstall starts clean.
    let setup_state = setup_state_path();
    if setup_state.exists() {
        let _ = std::fs::remove_file(&setup_state);
    }

    let app_dir = app_data_dir();
    if app_dir.exists() {
        match remove_dir_all_retry(&app_dir) {
            Ok(_) => removed.push(app_dir.display().to_string()),
            Err(err) => log::warn!("cleanup: removing {} failed: {err}", app_dir.display()),
        }
    }

    let dot_headroom = home_dir().join(".headroom");
    if dot_headroom.exists() {
        match std::fs::remove_dir_all(&dot_headroom) {
            Ok(_) => removed.push(dot_headroom.display().to_string()),
            Err(err) => log::warn!("cleanup: removing {} failed: {err}", dot_headroom.display()),
        }
    }

    #[cfg(target_os = "macos")]
    {
        removed.extend(remove_macos_launch_agents());
        removed.extend(remove_macos_preferences());
        removed.extend(remove_macos_caches());
        removed.extend(remove_macos_logs());
        removed.extend(remove_macos_bundle_dirs());
    }

    remove_known_keychain_entries();

    // Sweep `<basename>.headroom-backup-*` and `<basename>.nommer-backup-*`
    // siblings created by `backup_if_exists` for every file we ever mutated.
    // Without this, stale backups remain in ~/.claude, ~/.claude/hooks,
    // ~/.codex, ~/Library/Application Support/Code/User, and the user's
    // shell rc directory after uninstall.
    let mut backup_targets: Vec<PathBuf> = claude_settings_candidates();
    backup_targets.push(headroom_rtk_hook_path());
    backup_targets.push(headroom_markitdown_hook_path());
    backup_targets.push(claude_guard_hook_path());
    backup_targets.push(codex_config_toml_path());
    backup_targets.push(codex_hooks_json_path());
    backup_targets.push(codex_guard_hook_path());
    backup_targets.push(grok_config_toml_path());
    backup_targets.push(
        home_dir()
            .join("Library")
            .join("Application Support")
            .join("Code")
            .join("User")
            .join("settings.json"),
    );
    backup_targets.extend(all_shell_paths());
    for target in backup_targets {
        removed.extend(sweep_managed_backups(&target));
    }

    removed
}

/// Remove sibling backup files that `backup_if_exists` (or its predecessor
/// "nommer") created next to `target`. Filenames look like
/// `<basename>.headroom-backup-<timestamp>` and `<basename>.nommer-backup-<timestamp>`.
/// Returns the paths removed.
fn sweep_managed_backups(target: &Path) -> Vec<String> {
    let mut removed = Vec::new();
    let Some(parent) = target.parent() else {
        return removed;
    };
    let Some(file_name) = target.file_name().and_then(|n| n.to_str()) else {
        return removed;
    };
    let headroom_prefix = format!("{}.headroom-backup-", file_name);
    let nommer_prefix = format!("{}.nommer-backup-", file_name);

    let Ok(entries) = std::fs::read_dir(parent) else {
        return removed;
    };
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !name.starts_with(&headroom_prefix) && !name.starts_with(&nommer_prefix) {
            continue;
        }
        let path = entry.path();
        match std::fs::remove_file(&path) {
            Ok(_) => removed.push(path.display().to_string()),
            Err(err) => log::warn!("cleanup: removing {} failed: {err}", path.display()),
        }
    }
    removed
}

fn claude_settings_candidates() -> Vec<PathBuf> {
    let claude_dir = home_dir().join(".claude");
    vec![
        claude_dir.join("settings.json"),
        claude_dir.join("settings.local.json"),
    ]
}

/// Remove the PreToolUse entry pointing at `headroom-rtk-rewrite.sh`. Drops
/// the `PreToolUse` array if it becomes empty, and the `hooks` object if it
/// has no remaining event arrays. Returns true if the file was modified.
fn strip_headroom_hook_from_settings(settings_path: &Path) -> Result<bool> {
    remove_pre_tool_use_markers(
        settings_path,
        &["headroom-rtk-rewrite.sh", "headroom-markitdown-read.sh"],
    )
}

/// Removes every PreToolUse hook entry whose command contains one of `markers`,
/// pruning empty `PreToolUse`/`hooks` containers. Returns whether the file changed.
fn remove_pre_tool_use_markers(settings_path: &Path, markers: &[&str]) -> Result<bool> {
    if !settings_path.exists() {
        return Ok(false);
    }

    let raw = std::fs::read_to_string(settings_path)
        .with_context(|| format!("reading {}", settings_path.display()))?;
    if raw.trim().is_empty() {
        return Ok(false);
    }
    let mut root = parse_json_object(&raw, settings_path)?;

    let Some(hooks_val) = root.get_mut("hooks") else {
        return Ok(false);
    };
    let Some(hooks_obj) = hooks_val.as_object_mut() else {
        return Ok(false);
    };

    let mut changed = false;

    if let Some(pre_tool_use) = hooks_obj
        .get_mut("PreToolUse")
        .and_then(|value| value.as_array_mut())
    {
        let before = pre_tool_use.len();
        pre_tool_use.retain(|entry| {
            !markers
                .iter()
                .any(|marker| entry_contains_hook(entry, marker))
        });
        if pre_tool_use.len() != before {
            changed = true;
        }
        if pre_tool_use.is_empty() {
            hooks_obj.remove("PreToolUse");
        }
    }

    if hooks_obj.is_empty() {
        root.remove("hooks");
    }

    if !changed {
        return Ok(false);
    }

    let _ = backup_if_exists(settings_path)?;
    atomic_write(
        settings_path,
        &serde_json::to_vec_pretty(&Value::Object(root))
            .context("serializing Claude settings for hook cleanup")?,
    )?;

    Ok(true)
}

#[cfg(target_os = "macos")]
fn remove_macos_launch_agents() -> Vec<String> {
    let mut removed = Vec::new();
    let launch_agents_dir = home_dir().join("Library").join("LaunchAgents");

    // Bundle-id-style plist (tauri-plugin-autostart default) and the
    // "Headroom.plist" name some older builds shipped. Either can exist.
    let candidates = ["com.extraheadroom.headroom.plist", "Headroom.plist"];

    for name in candidates {
        let path = launch_agents_dir.join(name);
        if !path.exists() {
            continue;
        }
        // Best-effort unload before deletion so launchd forgets the job.
        let _ = Command::new("launchctl")
            .args(["unload", "-w"])
            .arg(&path)
            .output();
        match std::fs::remove_file(&path) {
            Ok(_) => removed.push(path.display().to_string()),
            Err(err) => log::warn!("cleanup: removing {} failed: {err}", path.display()),
        }
    }

    removed
}

#[cfg(target_os = "macos")]
fn remove_macos_preferences() -> Vec<String> {
    let mut removed = Vec::new();
    let prefs_dir = home_dir().join("Library").join("Preferences");
    let Ok(entries) = std::fs::read_dir(&prefs_dir) else {
        return removed;
    };
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !name.starts_with("com.extraheadroom.headroom") {
            continue;
        }
        let path = entry.path();
        let result = if path.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        match result {
            Ok(_) => removed.push(path.display().to_string()),
            Err(err) => log::warn!("cleanup: removing {} failed: {err}", path.display()),
        }
    }
    removed
}

#[cfg(target_os = "macos")]
fn remove_macos_caches() -> Vec<String> {
    let mut removed = Vec::new();
    let caches_dir = home_dir()
        .join("Library")
        .join("Caches")
        .join("com.extraheadroom.headroom");
    if caches_dir.exists() {
        match std::fs::remove_dir_all(&caches_dir) {
            Ok(_) => removed.push(caches_dir.display().to_string()),
            Err(err) => log::warn!("cleanup: removing {} failed: {err}", caches_dir.display()),
        }
    }
    removed
}

#[cfg(target_os = "macos")]
fn remove_macos_logs() -> Vec<String> {
    let mut removed = Vec::new();
    let logs_dir = home_dir().join("Library").join("Logs").join("Headroom");
    if logs_dir.exists() {
        match std::fs::remove_dir_all(&logs_dir) {
            Ok(_) => removed.push(logs_dir.display().to_string()),
            Err(err) => log::warn!("cleanup: removing {} failed: {err}", logs_dir.display()),
        }
    }
    removed
}

/// Sweep the per-bundle-id directories macOS creates for a GUI app outside the
/// Caches/Preferences locations already handled above: the WKWebView data
/// store, HTTP cookie/storage caches, and saved window state.
#[cfg(target_os = "macos")]
fn remove_macos_bundle_dirs() -> Vec<String> {
    let mut removed = Vec::new();
    let lib = home_dir().join("Library");
    let targets = [
        lib.join("WebKit").join("com.extraheadroom.headroom"),
        lib.join("HTTPStorages").join("com.extraheadroom.headroom"),
        lib.join("HTTPStorages")
            .join("com.extraheadroom.headroom.binarycookies"),
        lib.join("Saved Application State")
            .join("com.extraheadroom.headroom.savedState"),
    ];
    for path in targets {
        if !path.exists() {
            continue;
        }
        let result = if path.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        match result {
            Ok(_) => removed.push(path.display().to_string()),
            Err(err) => log::warn!("cleanup: removing {} failed: {err}", path.display()),
        }
    }
    removed
}

/// Delete every keychain entry Headroom is known to write. Accounts are
/// captured alongside services because macOS keychain queries require both.
fn remove_known_keychain_entries() {
    const ENTRIES: &[(&str, &str)] = &[
        ("com.extraheadroom.headroom.account", "session-token"),
        ("com.extraheadroom.headroom.device", "machine-id-digest"),
        ("com.extraheadroom.headroom.headroom-learn", "openai"),
        ("com.extraheadroom.headroom.headroom-learn", "anthropic"),
        ("com.extraheadroom.headroom.headroom-learn", "gemini"),
    ];
    for (service, account) in ENTRIES {
        if let Err(err) = crate::keychain::delete_secret(service, account) {
            log::warn!("cleanup: deleting keychain {service}/{account} failed: {err}");
        }
    }
}

/// Re-applies setup for all clients that were active at the last pause or quit.
pub fn restore_client_setups() {
    let state = load_setup_state();
    let to_restore: Vec<String> = state.remembered_clients.keys().cloned().collect();
    for client_id in to_restore {
        let _ = apply_client_setup(&client_id);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ClientSetupState {
    configured_clients: BTreeMap<String, String>,
    /// Snapshot of configured_clients taken at last pause/quit, used to restore on next startup.
    #[serde(default)]
    remembered_clients: BTreeMap<String, String>,
    #[serde(default)]
    managed_shell_files: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    remembered_shell_files: BTreeMap<String, Vec<String>>,
    /// Pre-existing custom base URLs (corporate gateway, LiteLLM, Bedrock
    /// proxy) that setup replaced with Headroom's, keyed by client state id.
    /// Restored verbatim on disable/uninstall — setup used to clobber these
    /// and never put them back, silently unrouting enterprise users from
    /// their gateway.
    #[serde(default)]
    preserved_base_urls: BTreeMap<String, String>,
    /// User opted RTK out via the tool status toggle. When true, bootstrap and
    /// client setup skip re-adding the RTK PATH export and Claude Code hook.
    #[serde(default)]
    rtk_disabled: bool,
}

fn is_configured(state: &ClientSetupState, client_id: &str) -> bool {
    configured_timestamp(state, client_id).is_some()
}

fn configured_timestamp(state: &ClientSetupState, client_id: &str) -> Option<String> {
    let primary = normalized_setup_id(client_id);
    state.configured_clients.get(primary).cloned()
}

fn load_setup_state() -> ClientSetupState {
    let path = setup_state_path();
    if !path.exists() {
        return ClientSetupState::default();
    }

    // The on-disk file is rewritten by other code paths in this module
    // (apply_client_setup, disable_client_setup, clear_client_setups). Even
    // though `write_setup_state` now publishes via tmp+rename, retry once
    // before giving up: a parse failure on an existing file is almost always
    // a transient race or a partially-written file from an older build, and
    // returning the empty default flips `is_claude_code_enabled` to false,
    // which the tray reads as "Claude Code disconnected" and notifies on.
    match try_load_setup_state(&path) {
        Ok(state) => normalize_setup_state(state),
        Err(first_err) => {
            std::thread::sleep(std::time::Duration::from_millis(15));
            match try_load_setup_state(&path) {
                Ok(state) => normalize_setup_state(state),
                Err(second_err) => {
                    log::warn!(
                        "load_setup_state: failed to read/parse {} twice ({first_err:#}; {second_err:#}); returning default",
                        path.display()
                    );
                    ClientSetupState::default()
                }
            }
        }
    }
}

fn try_load_setup_state(path: &Path) -> Result<ClientSetupState> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_slice::<ClientSetupState>(&bytes)
        .with_context(|| format!("parsing {}", path.display()))
}

fn normalize_setup_state(mut state: ClientSetupState) -> ClientSetupState {
    state.configured_clients = normalize_setup_entries(state.configured_clients);
    state.remembered_clients = normalize_setup_entries(state.remembered_clients);
    state.managed_shell_files = normalize_shell_file_entries(state.managed_shell_files);
    state.remembered_shell_files = normalize_shell_file_entries(state.remembered_shell_files);
    state
}

fn normalize_setup_entries(mut entries: BTreeMap<String, String>) -> BTreeMap<String, String> {
    // codex_gui is a removed id; codex/codex_cli are live again, keep them.
    entries.remove("codex_gui");

    entries
}

fn normalize_shell_file_entries(
    mut entries: BTreeMap<String, Vec<String>>,
) -> BTreeMap<String, Vec<String>> {
    entries.remove("codex_gui");

    for files in entries.values_mut() {
        dedupe_strings(files);
    }

    entries
}

fn write_setup_state(state: &ClientSetupState) -> Result<()> {
    let path = setup_state_path();
    let payload = serde_json::to_vec_pretty(state).context("serializing client setup state")?;

    atomic_write(&path, &payload)
}

/// Write via a sibling tmp file then rename. POSIX rename is atomic, so
/// concurrent readers (other apps parsing their own config, the tray-icon
/// thread calling `is_claude_code_enabled` every 2s) see either the old file
/// or the new one — never a half-written truncate. A plain `fs::write` also
/// leaves a truncated file behind on crash/power loss mid-write, which for
/// user-owned configs (settings.json, config.toml, shell rc files) breaks the
/// user's shell or client startup.
pub(crate) fn atomic_write(path: &Path, contents: &[u8]) -> Result<()> {
    // Per-writer unique tmp name. A fixed `<path>.tmp` is shared by concurrent
    // writers to the same file: A renames tmp->path, then B's rename finds its
    // tmp already consumed and fails ENOENT (Sentry RUST-3W / RUST-4W). pid +
    // a process-local counter makes each write's tmp its own.
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let tmp_path = {
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut s = path.as_os_str().to_os_string();
        s.push(format!(".tmp.{}.{}", std::process::id(), n));
        PathBuf::from(s)
    };
    std::fs::write(&tmp_path, contents)
        .with_context(|| format!("writing {}", tmp_path.display()))?;
    std::fs::rename(&tmp_path, path).with_context(|| {
        let _ = std::fs::remove_file(&tmp_path); // don't leak the tmp on failure
        format!("renaming {} -> {}", tmp_path.display(), path.display())
    })
}

fn setup_state_path() -> PathBuf {
    config_file(&app_data_dir(), "client-setup.json")
}

fn default_headroom_root_dir() -> PathBuf {
    app_data_dir().join("headroom")
}

fn default_headroom_rtk_path() -> PathBuf {
    default_headroom_root_dir().join("bin").join("rtk")
}

fn default_headroom_managed_python_path() -> PathBuf {
    default_headroom_root_dir()
        .join("runtime")
        .join("venv")
        .join("bin")
        .join("python3")
}

fn resolve_client_shell_targets(state: &ClientSetupState, client_id: &str) -> Result<Vec<PathBuf>> {
    let state_id = normalized_setup_id(client_id);
    let mut targets = shell_targets_from_state(state.managed_shell_files.get(state_id));
    if targets.is_empty() {
        targets = shell_targets_from_state(state.remembered_shell_files.get(state_id));
    }
    targets.extend(discover_managed_shell_targets(&[
        "claude_code",
        "managed_rtk",
        "codex_cli",
    ])?);

    let default_targets = default_shell_targets_for_family(detect_shell_family());
    if targets.is_empty() {
        targets = default_targets;
    } else {
        for file in default_targets {
            if is_profile_file(&file) {
                targets.push(file);
            }
        }
    }

    Ok(dedupe_paths(targets))
}

fn resolve_client_shell_targets_for_cleanup(
    state: &ClientSetupState,
    client_id: &str,
) -> Result<Vec<PathBuf>> {
    let mut targets = resolve_client_shell_targets(state, client_id)?;
    targets.extend(all_shell_paths());
    Ok(dedupe_paths(targets))
}

fn configure_shell_block(
    shell_targets: &[PathBuf],
    block_id: &str,
    block_body: &str,
) -> Result<(Vec<String>, Vec<String>)> {
    let mut changed = Vec::new();
    let mut backups = Vec::new();

    for file in shell_targets {
        let (did_change, backup) = upsert_managed_block(&file, block_id, block_body)?;
        if did_change {
            changed.push(file.display().to_string());
            if let Some(path) = backup {
                backups.push(path.display().to_string());
            }
        }
    }

    Ok((changed, backups))
}

fn ensure_managed_rtk_on_path(
    rtk_path: &Path,
    shell_targets: &[PathBuf],
) -> Result<(Vec<String>, Vec<String>)> {
    let managed_bin_dir = rtk_path.parent().ok_or_else(|| {
        anyhow!(
            "managed RTK path {} is missing a parent directory",
            rtk_path.display()
        )
    })?;
    let path_value = shell_double_quote(&managed_bin_dir.to_string_lossy());
    configure_shell_block(
        shell_targets,
        "managed_rtk",
        &format!("export PATH=\"{path_value}:$PATH\""),
    )
}

fn ensure_claude_code_rtk_hook(
    managed_rtk_path: &Path,
    managed_python_path: &Path,
) -> Result<(Vec<String>, Vec<String>)> {
    let hook_path = headroom_rtk_hook_path();
    let hook_body = build_headroom_rtk_hook(managed_rtk_path, managed_python_path);
    let (hook_changed, hook_backup) = write_file_if_changed(&hook_path, &hook_body, true)?;
    let mut changed_files = Vec::new();
    let mut backup_files = Vec::new();

    if hook_changed {
        changed_files.push(hook_path.display().to_string());
    }
    if let Some(path) = hook_backup {
        backup_files.push(path.display().to_string());
    }

    let (settings_changed, settings_backups) =
        ensure_claude_settings_hook(&hook_path, "Bash", "headroom-rtk-rewrite.sh")?;
    changed_files.extend(settings_changed);
    backup_files.extend(settings_backups);

    Ok((changed_files, backup_files))
}

fn markitdown_claude_md_path() -> PathBuf {
    home_dir().join(".claude").join("CLAUDE.md")
}

fn markitdown_codex_agents_path() -> PathBuf {
    codex_home().join("AGENTS.md")
}

/// Office-only nudge for Claude Code, where PDFs are already handled by the
/// PreToolUse(Read) hook.
fn build_markitdown_office_nudge(shim_path: &Path) -> String {
    let bin = shim_path.display();
    format!(
        "## Reading Office documents (Headroom MarkItDown)\n\
         The Read tool cannot open .docx, .doc, .pptx, .ppt, .xlsx, or .xls files.\n\
         To read one, run `{bin} <path>` via Bash and use the Markdown it prints.\n\
         (PDFs are handled automatically and need no special step.)"
    )
}

/// Codex nudge: Codex has no PreToolUse-style hook, so it covers PDF *and*
/// Office formats through the `markitdown` CLI.
fn build_markitdown_codex_nudge(shim_path: &Path) -> String {
    let bin = shim_path.display();
    format!(
        "## Reading documents (Headroom MarkItDown)\n\
         To read a .pdf, .docx, .doc, .pptx, .ppt, .xlsx, or .xls file, run\n\
         `{bin} <path>` in the shell and use the Markdown it prints, rather than\n\
         opening the raw file. This keeps large documents cheap to read."
    )
}

/// Enables the MarkItDown addon integration for whichever coding clients are
/// configured through Headroom: Claude Code gets the PDF Read hook plus an
/// Office nudge (managed `~/.claude/CLAUDE.md` block + scoped Bash permission);
/// Codex gets a managed `~/.codex/AGENTS.md` nudge covering PDF and Office (it
/// has no hook mechanism). Idempotent and safe to re-run.
pub fn enable_markitdown_integration(
    markitdown_entrypoint: &Path,
    markitdown_shim: &Path,
    python_path: &Path,
) -> Result<(Vec<String>, Vec<String>)> {
    let mut changed_files = Vec::new();
    let mut backup_files = Vec::new();

    if is_claude_code_enabled() {
        let hook_path = headroom_markitdown_hook_path();
        let hook_body = build_headroom_markitdown_hook(markitdown_entrypoint, python_path);
        let (hook_changed, hook_backup) = write_file_if_changed(&hook_path, &hook_body, true)?;
        if hook_changed {
            changed_files.push(hook_path.display().to_string());
        }
        if let Some(path) = hook_backup {
            backup_files.push(path.display().to_string());
        }

        let (settings_changed, settings_backups) =
            ensure_claude_settings_hook(&hook_path, "Read", "headroom-markitdown-read.sh")?;
        changed_files.extend(settings_changed);
        backup_files.extend(settings_backups);

        let claude_md = markitdown_claude_md_path();
        let (md_changed, md_backup) = upsert_managed_block(
            &claude_md,
            "markitdown_office",
            &build_markitdown_office_nudge(markitdown_shim),
        )?;
        if md_changed {
            changed_files.push(claude_md.display().to_string());
        }
        if let Some(path) = md_backup {
            backup_files.push(path.display().to_string());
        }

        if set_markitdown_bash_permission(markitdown_shim, true)? {
            changed_files.push(claude_settings_path().display().to_string());
        }
    }

    if is_codex_enabled() {
        let agents = markitdown_codex_agents_path();
        let (codex_changed, codex_backup) = upsert_managed_block(
            &agents,
            "markitdown",
            &build_markitdown_codex_nudge(markitdown_shim),
        )?;
        if codex_changed {
            changed_files.push(agents.display().to_string());
        }
        if let Some(path) = codex_backup {
            backup_files.push(path.display().to_string());
        }
    }

    Ok((changed_files, backup_files))
}

/// Removes every MarkItDown integration artifact for all clients (Claude Read
/// hook + script + Office nudge + Bash permission, and the Codex AGENTS.md
/// nudge), leaving any RTK hook untouched. Cleanup runs unconditionally so a
/// client that was later disconnected is still scrubbed.
pub fn disable_markitdown_integration(markitdown_shim: &Path) -> Result<bool> {
    let mut changed =
        remove_pre_tool_use_markers(&claude_settings_path(), &["headroom-markitdown-read.sh"])?;
    let hook_path = headroom_markitdown_hook_path();
    if hook_path.exists() {
        let _ = std::fs::remove_file(&hook_path);
    }
    changed |= remove_managed_block(&markitdown_claude_md_path(), "markitdown_office")?;
    changed |= set_markitdown_bash_permission(markitdown_shim, false)?;
    changed |= remove_managed_block(&markitdown_codex_agents_path(), "markitdown")?;
    Ok(changed)
}

/// Adds or removes a `Bash(<shim> *)` entry in `permissions.allow` so the Office
/// nudge can run `markitdown` without prompting. Returns whether settings changed.
fn set_markitdown_bash_permission(shim_path: &Path, present: bool) -> Result<bool> {
    let settings_path = claude_settings_path();
    let entry = format!("Bash({} *)", shim_path.display());

    let mut content = if settings_path.exists() {
        let raw = std::fs::read_to_string(&settings_path)
            .with_context(|| format!("reading {}", settings_path.display()))?;
        if raw.trim().is_empty() {
            Value::Object(Default::default())
        } else {
            Value::Object(parse_json_object(&raw, &settings_path)?)
        }
    } else if present {
        Value::Object(Default::default())
    } else {
        return Ok(false);
    };

    let root = content
        .as_object_mut()
        .ok_or_else(|| anyhow!("unable to write Claude permissions settings"))?;
    let allow = root
        .entry("permissions")
        .or_insert_with(|| Value::Object(Default::default()))
        .as_object_mut()
        .ok_or_else(|| anyhow!("permissions is not an object"))?
        .entry("allow")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| anyhow!("permissions.allow is not an array"))?;

    let already = allow.iter().any(|v| v.as_str() == Some(entry.as_str()));
    if present == already {
        return Ok(false);
    }
    if present {
        allow.push(Value::String(entry));
    } else {
        allow.retain(|v| v.as_str() != Some(entry.as_str()));
    }

    if let Some(parent) = settings_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let _ = backup_if_exists(&settings_path)?;
    atomic_write(
        &settings_path,
        &serde_json::to_vec_pretty(&content).context("serializing Claude permissions settings")?,
    )
    .with_context(|| format!("writing {}", settings_path.display()))?;
    Ok(true)
}

fn disable_codex_cli() -> Result<()> {
    remove_codex_provider_block()?;
    let _ = remove_codex_toml_key("openai_base_url", HEADROOM_OPENAI_BASE_URL);
    let _ = remove_codex_guard_hook();
    let shell_targets = all_shell_paths();
    let _ = remove_shell_block(&shell_targets, "codex_cli");
    let _ = remove_shell_block(&shell_targets, "codex");
    Ok(())
}

fn disable_codex_gui() -> Result<()> {
    clear_legacy_codex_gui_launch_env()?;
    Ok(())
}

fn clear_legacy_codex_gui_launch_env() -> Result<()> {
    remove_launchctl_env(&["OPENAI_BASE_URL", "OPENAI_API_BASE"])?;
    Ok(())
}

fn configure_vscode_settings() -> Result<(Vec<String>, Vec<String>, Option<String>)> {
    let (mut changed_files, mut backup_files, replaced) =
        configure_claude_settings_env("ANTHROPIC_BASE_URL", HEADROOM_ANTHROPIC_BASE_URL)?;
    let (legacy_changed, legacy_backups) = remove_legacy_vscode_base_url_keys()?;
    changed_files.extend(legacy_changed);
    backup_files.extend(legacy_backups);
    Ok((changed_files, backup_files, replaced))
}

fn remove_vscode_connector_keys(restore_value: Option<&str>) -> Result<()> {
    remove_claude_settings_env(
        "ANTHROPIC_BASE_URL",
        HEADROOM_ANTHROPIC_BASE_URL,
        restore_value,
    )?;
    let _ = remove_legacy_vscode_base_url_keys()?;
    Ok(())
}

fn set_json_string(
    obj: &mut serde_json::Map<String, Value>,
    key: &str,
    expected_value: &str,
) -> bool {
    let next = Value::String(expected_value.to_string());
    match obj.get(key) {
        Some(existing) if existing == &next => false,
        _ => {
            obj.insert(key.to_string(), next);
            true
        }
    }
}

fn remove_json_key_if_matches(
    obj: &mut serde_json::Map<String, Value>,
    key: &str,
    expected_value: &str,
) -> bool {
    match obj.get(key) {
        Some(Value::String(value)) if value == expected_value => obj.remove(key).is_some(),
        _ => false,
    }
}

/// Point `env.<env_key>` at Headroom. The third return element is a
/// pre-existing *foreign* value this write replaced (a corporate gateway,
/// LiteLLM, or Bedrock-proxy URL) — callers must preserve it and restore it
/// on disable instead of just deleting the key.
fn configure_claude_settings_env(
    env_key: &str,
    env_value: &str,
) -> Result<(Vec<String>, Vec<String>, Option<String>)> {
    let settings_path = claude_settings_path();
    let mut content = if settings_path.exists() {
        let raw = std::fs::read_to_string(&settings_path)
            .with_context(|| format!("reading {}", settings_path.display()))?;
        Value::Object(parse_json_object(&raw, &settings_path)?)
    } else {
        Value::Object(Default::default())
    };

    if !content.is_object() {
        content = Value::Object(Default::default());
    }

    let Some(root) = content.as_object_mut() else {
        return Err(anyhow!("unable to write Claude settings"));
    };

    if !root
        .get("env")
        .map(|value| value.is_object())
        .unwrap_or(false)
    {
        root.insert("env".into(), Value::Object(Default::default()));
    }

    let Some(env_obj) = root.get_mut("env").and_then(|value| value.as_object_mut()) else {
        return Err(anyhow!("unable to write Claude env settings"));
    };

    let replaced_foreign_value = env_obj
        .get(env_key)
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty() && *value != env_value)
        .map(str::to_string);

    let changed = set_json_string(env_obj, env_key, env_value);
    if !changed {
        return Ok((Vec::new(), Vec::new(), None));
    }

    if let Some(parent) = settings_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }

    let backup = backup_if_exists(&settings_path)?;
    atomic_write(
        &settings_path,
        &serde_json::to_vec_pretty(&content).context("serializing Claude settings")?,
    )
    .with_context(|| format!("writing {}", settings_path.display()))?;

    Ok((
        vec![settings_path.display().to_string()],
        backup
            .into_iter()
            .map(|path| path.display().to_string())
            .collect(),
        replaced_foreign_value,
    ))
}

fn ensure_claude_settings_hook(
    hook_path: &Path,
    matcher: &str,
    marker: &str,
) -> Result<(Vec<String>, Vec<String>)> {
    let settings_path = claude_settings_path();
    let mut content = if settings_path.exists() {
        let raw = std::fs::read_to_string(&settings_path)
            .with_context(|| format!("reading {}", settings_path.display()))?;
        Value::Object(parse_json_object(&raw, &settings_path)?)
    } else {
        Value::Object(Default::default())
    };

    if !content.is_object() {
        content = Value::Object(Default::default());
    }

    let hook_command = hook_path
        .to_str()
        .ok_or_else(|| anyhow!("hook path contains invalid UTF-8: {}", hook_path.display()))?;
    let already_present = claude_hook_present_in_value(&content, hook_command);
    if already_present {
        return Ok((Vec::new(), Vec::new()));
    }

    let Some(root) = content.as_object_mut() else {
        return Err(anyhow!("unable to write Claude hook settings"));
    };

    if !root
        .get("hooks")
        .map(|value| value.is_object())
        .unwrap_or(false)
    {
        root.insert("hooks".into(), Value::Object(Default::default()));
    }

    let Some(hooks_obj) = root
        .get_mut("hooks")
        .and_then(|value| value.as_object_mut())
    else {
        return Err(anyhow!("unable to write Claude hooks settings"));
    };
    if !hooks_obj
        .get("PreToolUse")
        .map(|value| value.is_array())
        .unwrap_or(false)
    {
        hooks_obj.insert("PreToolUse".into(), Value::Array(Vec::new()));
    }

    let Some(pre_tool_use) = hooks_obj
        .get_mut("PreToolUse")
        .and_then(|value| value.as_array_mut())
    else {
        return Err(anyhow!("unable to write Claude PreToolUse hooks"));
    };

    pre_tool_use.retain(|entry| !entry_contains_hook(entry, marker));
    pre_tool_use.push(serde_json::json!({
        "matcher": matcher,
        "hooks": [{
            "type": "command",
            "command": hook_command
        }]
    }));

    if let Some(parent) = settings_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }

    let backup = backup_if_exists(&settings_path)?;
    atomic_write(
        &settings_path,
        &serde_json::to_vec_pretty(&content).context("serializing Claude hook settings")?,
    )
    .with_context(|| format!("writing {}", settings_path.display()))?;

    Ok((
        vec![settings_path.display().to_string()],
        backup
            .into_iter()
            .map(|path| path.display().to_string())
            .collect(),
    ))
}

/// Undo `configure_claude_settings_env`: if `env.<env_key>` still equals
/// Headroom's value, put back `restore_value` (the user's pre-Headroom
/// gateway URL) when one was preserved, otherwise delete the key. A key that
/// no longer matches Headroom's value was changed by the user and is left
/// alone.
fn remove_claude_settings_env(
    env_key: &str,
    expected_value: &str,
    restore_value: Option<&str>,
) -> Result<()> {
    let settings_path = claude_settings_path();
    if !settings_path.exists() {
        return Ok(());
    }

    let raw = std::fs::read_to_string(&settings_path)
        .with_context(|| format!("reading {}", settings_path.display()))?;
    let mut root = parse_json_object(&raw, &settings_path)?;
    let mut changed = false;

    if let Some(Value::Object(env_obj)) = root.get_mut("env") {
        match restore_value {
            Some(original)
                if env_obj.get(env_key).and_then(|v| v.as_str()) == Some(expected_value) =>
            {
                env_obj.insert(env_key.into(), Value::String(original.to_string()));
                changed = true;
            }
            _ => {
                changed |= remove_json_key_if_matches(env_obj, env_key, expected_value);
            }
        }
        if env_obj.is_empty() {
            root.remove("env");
            changed = true;
        }
    }

    if !changed {
        return Ok(());
    }

    let _ = backup_if_exists(&settings_path)?;
    atomic_write(
        &settings_path,
        &serde_json::to_vec_pretty(&Value::Object(root))
            .context("serializing Claude settings for connector removal")?,
    )?;

    Ok(())
}

fn claude_hook_present_in_value(content: &Value, hook_path: &str) -> bool {
    content
        .get("hooks")
        .and_then(|value| value.get("PreToolUse"))
        .and_then(|value| value.as_array())
        .map(|entries| {
            entries.iter().any(|entry| {
                entry
                    .get("hooks")
                    .and_then(|hooks| hooks.as_array())
                    .map(|hooks| {
                        hooks.iter().any(|hook| {
                            hook.get("command")
                                .and_then(|command| command.as_str())
                                .map(|command| command == hook_path)
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn entry_contains_hook(entry: &Value, hook_fragment: &str) -> bool {
    entry
        .get("hooks")
        .and_then(|hooks| hooks.as_array())
        .map(|hooks| {
            hooks.iter().any(|hook| {
                hook.get("command")
                    .is_some_and(|c| command_contains(c, hook_fragment))
            })
        })
        .unwrap_or(false)
}

/// Match a hook `command` against a fragment, tolerating both the Claude string
/// form (`"/usr/bin/python3 /path/guard.py"`) and the argv-array form Codex
/// normalizes to (`["python3", "/path/guard.py"]`). Callers pass the guard
/// *script path* as the fragment so a differing interpreter (system vs Homebrew
/// python3) can't leave the entry behind when the script is deleted.
fn command_contains(command: &Value, fragment: &str) -> bool {
    match command {
        Value::String(s) => s.contains(fragment),
        Value::Array(parts) => parts
            .iter()
            .filter_map(Value::as_str)
            .any(|p| p.contains(fragment)),
        _ => false,
    }
}

fn remove_legacy_vscode_base_url_keys() -> Result<(Vec<String>, Vec<String>)> {
    let settings_path = home_dir()
        .join("Library")
        .join("Application Support")
        .join("Code")
        .join("User")
        .join("settings.json");
    if !settings_path.exists() {
        return Ok((Vec::new(), Vec::new()));
    }

    let raw = std::fs::read_to_string(&settings_path)
        .with_context(|| format!("reading {}", settings_path.display()))?;
    let mut obj = parse_json_object(&raw, &settings_path)?;

    let mut changed = false;
    changed |= remove_json_key_if_matches(&mut obj, "openai.baseUrl", HEADROOM_PROXY_URL);
    changed |= remove_json_key_if_matches(&mut obj, "anthropic.baseUrl", HEADROOM_PROXY_URL);
    if !changed {
        return Ok((Vec::new(), Vec::new()));
    }

    let backup = backup_if_exists(&settings_path)?;
    atomic_write(
        &settings_path,
        &serde_json::to_vec_pretty(&Value::Object(obj))
            .context("serializing VS Code settings for legacy key cleanup")?,
    )?;

    Ok((
        vec![settings_path.display().to_string()],
        backup
            .into_iter()
            .map(|path| path.display().to_string())
            .collect(),
    ))
}

fn codex_config_toml_path() -> PathBuf {
    codex_home().join("config.toml")
}

// The managed Codex config is split across two marker blocks so each lands in
// the correct TOML scope. `model_provider`/`openai_base_url` are root keys: a
// bare key belongs to the most recently opened `[table]` above it, so appending
// them at end-of-file (as a naive text upsert does) silently absorbs them into
// whatever table the user's config happens to end in (e.g. `[features]`, whose
// values must be booleans), producing
// `invalid type: string "headroom", expected a boolean in features`. The root
// keys therefore go in a block at the *top* of the file (nothing above ⇒ root
// scope), and the `[model_providers.headroom]` table goes in a block at the
// *end*. `requires_openai_auth` is emitted only for ChatGPT-OAuth users: the
// flag is what makes Codex render the account menu (profile/email/plan/usage),
// but it also forces Codex to demand an OpenAI OAuth login (issue #406), which
// would break users authenticated with an OpenAI API key. See
// `codex_uses_chatgpt_auth`.
const CODEX_ROOT_BLOCK_ID: &str = "codex_cli";
const CODEX_TABLE_BLOCK_ID: &str = "codex_cli_provider";

// Codex permanently stamps every thread with the `model_provider` it ran under,
// and its history/projects menu filters threads by the *active* provider set. So
// threads created through Headroom (provider `headroom`) disappear from the menu
// when Codex runs natively (provider `openai`) and vice-versa. To keep the menu
// whole we retag threads to match whichever provider is currently active:
// `openai -> headroom` on connect, `headroom -> openai` on disconnect/quit.
const CODEX_HEADROOM_PROVIDER: &str = "headroom";
const CODEX_NATIVE_PROVIDER: &str = "openai";

/// Directories Codex is known to keep its state store in: the v148 GUI uses
/// `<codex_home>/sqlite/`, the CLI/TUI uses `<codex_home>/`.
fn codex_state_dirs() -> Vec<PathBuf> {
    let codex = codex_home();
    vec![codex.join("sqlite"), codex]
}

/// True when Codex keeps (or kept) a sqlite-backed thread store on this machine,
/// so a *missing* recognized `state_<N>.sqlite` means the store moved/renamed --
/// the case worth a signal. The only evidence we trust is a `state_*.sqlite`-shaped
/// file in `<codex_home>/sqlite/` (GUI) or `<codex_home>/` (CLI/TUI), including a
/// renamed one whose version no longer parses -- exactly the relocation we want to
/// catch. The bare `sqlite/` dir is NOT evidence: it also holds unrelated stores
/// (logs/goals/memories), so a fresh install with those but no thread store would
/// otherwise false-fire "store moved" (Sentry RUST-3R). CLI-only or pre-sqlite
/// installs with just `config.toml`/`sessions/` stay silent -- nothing to split.
fn codex_sqlite_store_expected() -> bool {
    codex_state_dirs().iter().any(|dir| {
        std::fs::read_dir(dir)
            .map(|entries| {
                entries.flatten().any(|e| {
                    e.file_name()
                        .to_str()
                        .is_some_and(|n| n.starts_with("state_") && n.ends_with(".sqlite"))
                })
            })
            .unwrap_or(false)
    })
}

/// Discover every `*.sqlite` file under the known Codex dirs. The thread store's
/// *filename* has changed across Codex versions (`state_5.sqlite`, and whatever
/// comes next), so we no longer couple discovery to a name scheme: every sqlite
/// candidate is handed to `retag_one_codex_db`, which identifies the real store
/// by its `threads` table and no-ops on anything else (logs/goals/memories). A
/// rename can no longer silently split the history menu. A missing dir
/// (`read_dir` error) is skipped. Paths are deduped in case the two dirs ever
/// resolve to the same place.
fn discover_codex_state_dbs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for dir in codex_state_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("sqlite")
                && seen.insert(path.clone())
            {
                out.push(path);
            }
        }
    }
    out
}

/// Best-effort retag of Codex thread provider tags so the history menu stays
/// whole across the Headroom proxy boundary. Never fails the caller: a missing
/// store, a missing `threads` table, or a DB locked by a running Codex is logged
/// and skipped. Only rows whose `model_provider` equals `from` are touched, so
/// third-party providers are left alone.
fn retag_codex_thread_providers(from: &str, to: &str) {
    let mut found_thread_store = false;
    for path in discover_codex_state_dbs() {
        match retag_one_codex_db(&path, from, to) {
            // No `threads` table: unrelated sqlite store (logs/goals/memories).
            Ok(None) => {}
            Ok(Some(n)) => {
                found_thread_store = true;
                if n > 0 {
                    log::info!(
                        "codex retag {from}->{to}: {n} thread(s) in {}",
                        path.display()
                    );
                }
            }
            Err(e) => log::warn!(
                "codex retag {from}->{to} skipped for {}: {e}",
                path.display()
            ),
        }
    }
    // A `state_*.sqlite`-shaped file with no `threads` table means Codex renamed
    // the table itself (discovery already survives a file rename). Only flag when
    // the store-shaped name is present, so a clean or CLI-only / pre-sqlite
    // machine -- or one with just logs/goals/memories DBs -- stays silent
    // (Sentry RUST-3R). This is the last remaining schema-drift signal worth a
    // release (Sentry RUST-43).
    if !found_thread_store && codex_sqlite_store_expected() {
        log::warn!(
            "codex retag {from}->{to}: a state_*.sqlite is present but has no \
             `threads` table under {dirs:?}; the history menu may split. Codex \
             likely renamed the table.",
            dirs = codex_state_dirs(),
        );
    }
}

fn retag_one_codex_db(path: &Path, from: &str, to: &str) -> rusqlite::Result<Option<usize>> {
    use rusqlite::OptionalExtension;

    let conn = rusqlite::Connection::open(path)?;
    conn.busy_timeout(Duration::from_millis(750))?;
    // No-op (without erroring) on builds whose store lacks the threads table.
    let has_table = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'threads'",
            [],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !has_table {
        return Ok(None);
    }
    conn.execute(
        "UPDATE threads SET model_provider = ?2 WHERE model_provider = ?1",
        rusqlite::params![from, to],
    )
    .map(Some)
}

/// Retag Codex threads back to the native provider. Exposed for the app-quit
/// hook in `lib.rs`, which covers exit paths (Cmd-Q, dock quit, signals) that
/// bypass `clear_client_setups` and therefore the disconnect retag.
pub fn retag_codex_threads_to_native() {
    retag_codex_thread_providers(CODEX_HEADROOM_PROVIDER, CODEX_NATIVE_PROVIDER);
}

/// Pull Codex threads into the headroom provider menu. Exposed for the
/// app-launch hook in `lib.rs`, which must undo the quit-time native retag on
/// the exit paths (Cmd-Q, dock quit, app-update restart) that never populate
/// `remembered_clients` and are therefore skipped by `restore_client_setups`.
pub fn retag_codex_threads_to_headroom() {
    retag_codex_thread_providers(CODEX_NATIVE_PROVIDER, CODEX_HEADROOM_PROVIDER);
}

fn codex_root_keys_body() -> String {
    format!(
        "model_provider = \"headroom\"\n\
         openai_base_url = \"{base}\"",
        base = HEADROOM_OPENAI_BASE_URL,
    )
}

/// Whether Codex is authenticated via ChatGPT OAuth (rather than an OpenAI API
/// key), read from `~/.codex/auth.json`. Drives whether the managed provider
/// block carries `requires_openai_auth = true` (see [`codex_provider_table_body`]).
fn codex_uses_chatgpt_auth() -> bool {
    let path = codex_home().join("auth.json");
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        return false;
    };
    let Some(obj) = value.as_object() else {
        return false;
    };
    // Codex records the active method explicitly; trust it when present.
    if let Some(mode) = obj.get("auth_mode").and_then(Value::as_str) {
        return mode.eq_ignore_ascii_case("chatgpt");
    }
    // Older auth.json files predate `auth_mode`: infer ChatGPT mode from the
    // presence of an OAuth account id.
    obj.get("tokens")
        .and_then(Value::as_object)
        .and_then(|tokens| tokens.get("account_id"))
        .and_then(Value::as_str)
        .is_some_and(|id| !id.is_empty())
}

fn codex_provider_table_body(requires_openai_auth: bool) -> String {
    let mut body = format!(
        "[model_providers.headroom]\n\
         name = \"Headroom persistent proxy\"\n\
         base_url = \"{base}\"\n\
         supports_websockets = false",
        base = HEADROOM_OPENAI_BASE_URL,
    );
    if requires_openai_auth {
        body.push_str("\nrequires_openai_auth = true");
    }
    body
}

fn codex_marker_block(block_id: &str, body: &str) -> String {
    format!("# >>> headroom:{block_id} >>>\n{body}\n# <<< headroom:{block_id} <<<\n")
}

/// Remove every Headroom-managed artifact from Codex `config.toml` text: both
/// managed marker blocks, plus any orphan root keys an older (buggy) build may
/// have left absorbed into a preceding table. Leaves all other content intact.
fn strip_codex_managed_toml(content: &str) -> String {
    let without_blocks = strip_marker_block(
        &strip_marker_block(content, CODEX_ROOT_BLOCK_ID),
        CODEX_TABLE_BLOCK_ID,
    );
    let openai_orphan_prefix = "openai_base_url = \"http://127.0.0.1:";
    without_blocks
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !(trimmed == "model_provider = \"headroom\""
                || (trimmed.starts_with(openai_orphan_prefix) && trimmed.ends_with("/v1\"")))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Pure-text removal of every `# >>> headroom:<id> >>> ... <<<` block. Loops so
/// a config that already holds duplicate managed blocks (interrupted write,
/// older build) is fully cleaned, not left with one survivor that regenerates.
fn strip_marker_block(content: &str, block_id: &str) -> String {
    let start = format!("# >>> headroom:{block_id} >>>");
    let end = format!("# <<< headroom:{block_id} <<<");
    let mut out = content.to_string();
    loop {
        let (Some(start_idx), Some(end_idx)) = (out.find(&start), out.find(&end)) else {
            break;
        };
        if end_idx < start_idx {
            break; // malformed (stray end before start) — leave it alone
        }
        let tail = out[end_idx + end.len()..]
            .trim_start_matches('\n')
            .to_string();
        let head = out[..start_idx].trim_end().to_string();
        let mut rebuilt = String::with_capacity(out.len());
        rebuilt.push_str(&head);
        if !rebuilt.is_empty() && !tail.is_empty() {
            rebuilt.push('\n');
        }
        rebuilt.push_str(&tail);
        out = rebuilt;
    }
    out
}

/// The root-scope `model_provider` value in a Codex config, if set to something
/// other than our managed `headroom`. Root scope only: a `model_provider` inside
/// a `[profiles.x]`/`[model_providers.x]` table belongs to that table, not the
/// global route. This is the Codex analog of a foreign `ANTHROPIC_BASE_URL` --
/// captured on apply and restored on disable.
fn codex_foreign_model_provider(content: &str) -> Option<String> {
    let mut in_root = true;
    for raw in content.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_root = false;
            continue;
        }
        if !in_root {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            if key.trim() == "model_provider" {
                let name = value.trim().trim_matches('"');
                if !name.is_empty() && name != "headroom" {
                    return Some(name.to_string());
                }
            }
        }
    }
    None
}

/// Drop any root-scope `model_provider = ...` line so the managed block's
/// `model_provider = "headroom"` isn't a duplicate root key (which is invalid
/// TOML and makes Codex refuse to load its config). A `model_provider` inside a
/// table is left untouched.
fn strip_codex_root_model_provider(content: &str) -> String {
    let mut in_root = true;
    content
        .lines()
        .filter(|raw| {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.starts_with('[') && line.ends_with(']') {
                in_root = false;
                return true;
            }
            !(in_root
                && line
                    .split_once('=')
                    .map(|(key, _)| key.trim() == "model_provider")
                    .unwrap_or(false))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Restore a preserved pre-Headroom root `model_provider` after teardown, so a
/// gateway/alternate-provider user isn't silently left on api.openai.com. No-op
/// if the config already has a root `model_provider` (user re-added their own).
fn restore_codex_model_provider(provider: &str) -> Result<()> {
    let path = codex_config_toml_path();
    let existing = if path.exists() {
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?
    } else {
        String::new()
    };
    if codex_foreign_model_provider(&existing).is_some() {
        return Ok(());
    }
    let line = format!("model_provider = {}", toml_basic_string(provider));
    let trimmed = existing.trim();
    let rebuilt = if trimmed.is_empty() {
        format!("{line}\n")
    } else {
        format!("{line}\n{trimmed}\n")
    };
    let _ = backup_if_exists(&path)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    atomic_write(&path, rebuilt.as_bytes())?;
    Ok(())
}

/// Reconstruct `config.toml` with the managed root keys pinned to the top and
/// the provider table appended at the end, around the user's other content.
fn render_codex_config(existing: &str) -> String {
    let mid = strip_codex_managed_toml(existing);
    // Drop a foreign root model_provider too, else our managed
    // `model_provider = "headroom"` collides with it as a duplicate root key.
    let mid = strip_codex_root_model_provider(&mid);
    let mid = mid.trim();

    let mut out = codex_marker_block(CODEX_ROOT_BLOCK_ID, &codex_root_keys_body());
    if !mid.is_empty() {
        out.push('\n');
        out.push_str(mid);
        out.push('\n');
    }
    out.push('\n');
    out.push_str(&codex_marker_block(
        CODEX_TABLE_BLOCK_ID,
        &codex_provider_table_body(codex_uses_chatgpt_auth()),
    ));
    out
}

/// Returns `(changed_files, backup_files, preserved_provider)`. The third
/// element is a pre-existing *foreign* root `model_provider` this write replaced
/// -- callers must preserve it and restore it on disable instead of dropping the
/// user onto api.openai.com (mirrors [`configure_claude_settings_env`]).
fn configure_codex_provider_block() -> Result<(Vec<String>, Vec<String>, Option<String>)> {
    let path = codex_config_toml_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let existing = if path.exists() {
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?
    } else {
        String::new()
    };

    let preserved = codex_foreign_model_provider(&existing);
    let updated = render_codex_config(&existing);
    if updated == existing {
        return Ok((Vec::new(), Vec::new(), None));
    }

    let backup = backup_if_exists(&path)?;
    atomic_write(&path, updated.as_bytes())?;

    let mut backup_files = Vec::new();
    if let Some(backup_path) = backup {
        backup_files.push(backup_path.display().to_string());
    }
    Ok((vec![path.display().to_string()], backup_files, preserved))
}

/// Rewrite the `command` of the `[mcp_servers.headroom]` table in
/// `~/.codex/config.toml` to the absolute `entrypoint`. The upstream Python
/// registrar writes a bare `command = "headroom"` that relies on PATH; when
/// the managed runtime relocates, `~/.local/bin/headroom` dangles and Codex
/// fails to start the MCP server with `No such file or directory`. Desktop
/// re-runs `mcp install` on every launch, so pinning the absolute path here
/// self-heals the config. No-op when the config or table is absent. Targets
/// the table by header rather than the Headroom marker block, which the
/// upstream registrar can mis-place around unrelated user tables.
pub fn pin_codex_mcp_command(entrypoint: &Path) -> Result<Option<String>> {
    let path = codex_config_toml_path();
    if !path.exists() {
        return Ok(None);
    }
    let content =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;

    let target_line = format!(
        "command = {}",
        toml_basic_string(&entrypoint.to_string_lossy())
    );
    // The upstream registrar may resolve the server as `<python> -m headroom.cli
    // mcp serve`. Pinning only `command` to the console script would leave
    // `args = ["-m", "headroom.cli", ...]` behind, and `headroom -m ...` fails
    // with "No such option '-m'" — so the args must be pinned together.
    let target_args_line = r#"args = ["mcp", "serve"]"#;

    let mut in_headroom_table = false;
    let mut replaced = false;
    // When the replaced `args` value is a multi-line array, the continuation
    // lines ("-m", / "headroom.cli", / ]) must be dropped too, or the rebuilt
    // file is invalid TOML and Codex fails to load its config entirely.
    let mut skip_array_depth: i32 = 0;
    let mut out: Vec<String> = Vec::with_capacity(content.lines().count());
    for line in content.lines() {
        if skip_array_depth > 0 {
            skip_array_depth += bracket_delta(line);
            continue;
        }
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_headroom_table = trimmed == "[mcp_servers.headroom]";
            out.push(line.to_string());
            continue;
        }
        if in_headroom_table {
            match trimmed
                .split_once('=')
                .map(|(key, value)| (key.trim(), value))
            {
                Some(("command", _)) => {
                    out.push(target_line.clone());
                    replaced = true;
                    continue;
                }
                Some(("args", value)) => {
                    out.push(target_args_line.to_string());
                    skip_array_depth = bracket_delta(value).max(0);
                    continue;
                }
                _ => {}
            }
        }
        out.push(line.to_string());
    }

    if !replaced {
        return Ok(None);
    }
    let mut rebuilt = out.join("\n");
    if content.ends_with('\n') {
        rebuilt.push('\n');
    }
    if rebuilt == content {
        return Ok(None);
    }
    // Never publish a config Codex can't parse — bail and leave the user's
    // file untouched instead.
    toml::from_str::<toml::Value>(&rebuilt).with_context(|| {
        format!(
            "rebuilt {} is not valid TOML; refusing to overwrite",
            path.display()
        )
    })?;
    let _ = backup_if_exists(&path)?;
    atomic_write(&path, rebuilt.as_bytes())?;
    Ok(Some(path.display().to_string()))
}

/// Net `[` minus `]` on a line, ignoring brackets inside basic strings.
/// Good enough for tracking whether a TOML array value has closed.
fn bracket_delta(line: &str) -> i32 {
    let mut delta = 0;
    let mut in_string = false;
    let mut escaped = false;
    for c in line.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' if in_string => escaped = true,
            '"' => in_string = !in_string,
            '#' if !in_string => break,
            '[' if !in_string => delta += 1,
            ']' if !in_string => delta -= 1,
            _ => {}
        }
    }
    delta
}

const GROK_PROXY_BLOCK_ID: &str = "grok_build_proxy";

fn grok_config_toml_path() -> PathBuf {
    grok_home().join("config.toml")
}

fn grok_proxy_body() -> String {
    format!(
        "[model.grok-build]\nbase_url = \"{base}\"",
        base = HEADROOM_GROK_PROXY_BASE_URL
    )
}

fn strip_grok_managed_toml(content: &str) -> String {
    strip_marker_block(content, GROK_PROXY_BLOCK_ID)
}

/// Locate a `[model.grok-build]` table in `lines`: returns the header line
/// index and, when present, the index of its `base_url` line.
fn find_grok_build_table(lines: &[&str]) -> Option<(usize, Option<usize>)> {
    let mut header_idx = None;
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if trimmed == "[model.grok-build]" {
                header_idx = Some(idx);
            } else if header_idx.is_some() {
                return Some((header_idx.unwrap(), None));
            }
            continue;
        }
        if header_idx.is_some()
            && trimmed
                .split_once('=')
                .is_some_and(|(key, _)| key.trim() == "base_url")
        {
            return Some((header_idx.unwrap(), Some(idx)));
        }
    }
    header_idx.map(|h| (h, None))
}

/// Extract the quoted string value of a `key = "value"` TOML line, ignoring
/// any trailing comment.
fn toml_line_value(line: &str) -> Option<String> {
    let (_, rest) = line.split_once('=')?;
    let rest = rest.trim_start().strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Rewrite `base_url` inside a user-owned `[model.grok-build]` table (e.g.
/// written by `headroom wrap grok`), keeping the previous value in a trailing
/// `# was:` comment so disable can restore it. Mirrors the upstream Python
/// registrar (headroom/providers/grok_build/config.py). Returns `None` when no
/// such table exists.
fn redirect_existing_grok_build_base_url(content: &str) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();
    let (header_idx, base_url_idx) = find_grok_build_table(&lines)?;
    let mut out: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    match base_url_idx {
        Some(idx) => {
            let old = toml_line_value(lines[idx]);
            if old.as_deref() == Some(HEADROOM_GROK_PROXY_BASE_URL) {
                return Some(format!("{content}\n"));
            }
            let indent: String = lines[idx]
                .chars()
                .take_while(|c| c.is_whitespace())
                .collect();
            out[idx] = match old {
                Some(old) => format!(
                    "{indent}base_url = \"{HEADROOM_GROK_PROXY_BASE_URL}\"  # was: {old}"
                ),
                None => format!("{indent}base_url = \"{HEADROOM_GROK_PROXY_BASE_URL}\""),
            };
        }
        None => out.insert(
            header_idx + 1,
            format!("base_url = \"{HEADROOM_GROK_PROXY_BASE_URL}\""),
        ),
    }
    let mut rebuilt = out.join("\n");
    rebuilt.push('\n');
    Some(rebuilt)
}

/// Undo a `base_url` redirect left by [`redirect_existing_grok_build_base_url`]:
/// restore the value recorded in the `# was:` comment, or drop the line when
/// Headroom inserted it into a table that had none.
fn restore_grok_build_base_url(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let Some((_, Some(idx))) = find_grok_build_table(&lines) else {
        return content.to_string();
    };
    let line = lines[idx];
    if toml_line_value(line).as_deref() != Some(HEADROOM_GROK_PROXY_BASE_URL) {
        return content.to_string();
    }
    let mut out: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    if let Some((_, was)) = line.split_once("# was: ") {
        let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
        out[idx] = format!("{indent}base_url = \"{}\"", was.trim());
    } else {
        out.remove(idx);
    }
    out.join("\n")
}

fn render_grok_config(existing: &str) -> String {
    let mid = strip_grok_managed_toml(existing);
    let mid = mid.trim();

    // A user-owned [model.grok-build] table must not be duplicated - a second
    // table is invalid TOML. Redirect its base_url in place instead.
    if let Some(redirected) = redirect_existing_grok_build_base_url(mid) {
        return redirected;
    }

    // The managed block opens a [model.grok-build] table, so it must sit after
    // the user's content: any top-level key following the block would be
    // absorbed into the table.
    let block = codex_marker_block(GROK_PROXY_BLOCK_ID, &grok_proxy_body());
    if mid.is_empty() {
        return block;
    }
    format!("{mid}\n\n{block}")
}

fn configure_grok_proxy_block() -> Result<(Vec<String>, Vec<String>)> {
    let path = grok_config_toml_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let existing = if path.exists() {
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?
    } else {
        String::new()
    };

    let updated = render_grok_config(&existing);
    if updated == existing {
        return Ok((Vec::new(), Vec::new()));
    }

    let backup = backup_if_exists(&path)?;
    atomic_write(&path, updated.as_bytes())?;

    let mut backup_files = Vec::new();
    if let Some(backup_path) = backup {
        backup_files.push(backup_path.display().to_string());
    }
    Ok((vec![path.display().to_string()], backup_files))
}

fn grok_proxy_block_matches() -> Result<bool> {
    let path = grok_config_toml_path();
    if !path.exists() {
        return Ok(false);
    }
    let content =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let base_url = format!("base_url = \"{}\"", HEADROOM_GROK_PROXY_BASE_URL);
    if marker_block_contains(&content, GROK_PROXY_BLOCK_ID, &base_url) {
        return Ok(true);
    }
    // Redirected user-owned table (no managed block).
    let lines: Vec<&str> = content.lines().collect();
    Ok(matches!(
        find_grok_build_table(&lines),
        Some((_, Some(idx)))
            if toml_line_value(lines[idx]).as_deref() == Some(HEADROOM_GROK_PROXY_BASE_URL)
    ))
}

fn remove_grok_proxy_block() -> Result<()> {
    let path = grok_config_toml_path();
    if !path.exists() {
        return Ok(());
    }
    let existing =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let stripped = restore_grok_build_base_url(&strip_grok_managed_toml(&existing));
    let normalized = {
        let trimmed = stripped.trim();
        if trimmed.is_empty() {
            String::new()
        } else {
            format!("{trimmed}\n")
        }
    };
    if normalized == existing {
        return Ok(());
    }
    let _ = backup_if_exists(&path)?;
    atomic_write(&path, normalized.as_bytes())?;
    Ok(())
}

fn disable_grok_build() -> Result<()> {
    remove_grok_proxy_block()?;
    let shell_targets = all_shell_paths();
    let _ = remove_shell_block(&shell_targets, "grok_build");
    Ok(())
}

/// Rewrite the `command` of the `[mcp_servers.headroom]` table in
/// `~/.grok/config.toml` to the absolute `entrypoint`. Mirrors
/// [`pin_codex_mcp_command`]: the upstream Python registrar writes a bare
/// `command = "headroom"` that relies on PATH, which dangles when the managed
/// runtime relocates.
pub fn pin_grok_mcp_command(entrypoint: &Path) -> Result<Option<String>> {
    let path = grok_config_toml_path();
    if !path.exists() {
        return Ok(None);
    }
    let content =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;

    let target_line = format!(
        "command = {}",
        toml_basic_string(&entrypoint.to_string_lossy())
    );

    let mut in_headroom_table = false;
    let mut replaced = false;
    let mut out: Vec<String> = Vec::with_capacity(content.lines().count());
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_headroom_table = trimmed == "[mcp_servers.headroom]";
            out.push(line.to_string());
            continue;
        }
        if in_headroom_table
            && !replaced
            && trimmed
                .split_once('=')
                .is_some_and(|(key, _)| key.trim() == "command")
        {
            out.push(target_line.clone());
            replaced = true;
            continue;
        }
        out.push(line.to_string());
    }

    if !replaced {
        return Ok(None);
    }
    let mut rebuilt = out.join("\n");
    if content.ends_with('\n') {
        rebuilt.push('\n');
    }
    if rebuilt == content {
        return Ok(None);
    }
    let _ = backup_if_exists(&path)?;
    atomic_write(&path, rebuilt.as_bytes())?;
    Ok(Some(path.display().to_string()))
}

fn toml_basic_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn codex_provider_block_matches() -> Result<bool> {
    let path = codex_config_toml_path();
    if !path.exists() {
        return Ok(false);
    }
    let content =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let base_url = format!("base_url = \"{}\"", HEADROOM_OPENAI_BASE_URL);
    let openai_base = format!("openai_base_url = \"{}\"", HEADROOM_OPENAI_BASE_URL);
    let root_ok = marker_block_contains(
        &content,
        CODEX_ROOT_BLOCK_ID,
        "model_provider = \"headroom\"",
    ) && marker_block_contains(&content, CODEX_ROOT_BLOCK_ID, &openai_base);
    let table_ok = marker_block_contains(&content, CODEX_TABLE_BLOCK_ID, &base_url)
        && marker_block_contains(
            &content,
            CODEX_TABLE_BLOCK_ID,
            "supports_websockets = false",
        );
    Ok(root_ok && table_ok)
}

fn marker_block_contains(content: &str, block_id: &str, needle: &str) -> bool {
    let start = format!("# >>> headroom:{block_id} >>>");
    let end = format!("# <<< headroom:{block_id} <<<");
    match (content.find(&start), content.find(&end)) {
        (Some(start_idx), Some(end_idx)) if start_idx < end_idx => {
            content[start_idx..end_idx].contains(needle)
        }
        _ => false,
    }
}

fn remove_codex_provider_block() -> Result<()> {
    let path = codex_config_toml_path();
    if !path.exists() {
        return Ok(());
    }
    let existing =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let stripped = strip_codex_managed_toml(&existing);
    let normalized = {
        let trimmed = stripped.trim();
        if trimmed.is_empty() {
            String::new()
        } else {
            format!("{trimmed}\n")
        }
    };
    if normalized == existing {
        return Ok(());
    }
    let _ = backup_if_exists(&path)?;
    atomic_write(&path, normalized.as_bytes())?;
    Ok(())
}

fn remove_codex_toml_key(key: &str, expected_value: &str) -> Result<()> {
    let path = codex_config_toml_path();
    if !path.exists() {
        return Ok(());
    }
    let content =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let target_line = format!("{key} = \"{expected_value}\"");
    // Only remove the key from the root table: an identical `key = value`
    // line inside some other table ([profiles.x], a user's own server entry)
    // belongs to that table, not to the block we installed.
    let mut in_root_table = true;
    let filtered: Vec<&str> = content
        .lines()
        .filter(|l| {
            let trimmed = l.trim();
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                in_root_table = false;
            }
            !(in_root_table && trimmed == target_line)
        })
        .collect();
    if filtered.len() == content.lines().count() {
        return Ok(());
    }
    let _ = backup_if_exists(&path)?;
    let mut result = filtered.join("\n");
    if !result.ends_with('\n') && !result.is_empty() {
        result.push('\n');
    }
    atomic_write(&path, result.as_bytes())?;
    Ok(())
}

const CODEX_GUARD_STATUS_MESSAGE: &str = "Verifying Headroom route";

fn codex_hooks_json_path() -> PathBuf {
    codex_home().join("hooks.json")
}

fn codex_guard_hook_path() -> PathBuf {
    codex_home().join("hooks").join("headroom-codex-guard.py")
}

fn codex_guard_command() -> String {
    format!("/usr/bin/python3 {}", codex_guard_hook_path().display())
}

/// Informational guard that Codex runs at session start: it checks that
/// `~/.codex/config.toml` still routes through Headroom and that the desktop app
/// is reachable, and surfaces a notification when either is off so a genuinely
/// broken route is visible. It never blocks (always exits 0): Codex is the
/// user's own OpenAI account and must keep working whether or not Headroom is
/// active -- the intercept forwards Codex direct to OpenAI when the app is down
/// or the gate trips. Runs under system `/usr/bin/python3` (>=3.9), so it
/// carries a tiny TOML fallback parser for the pre-3.11 interpreters that lack
/// `tomllib`. Deliberately does NOT inspect auth mode or `OPENAI_API_KEY`:
/// routing is decided by `base_url`, so an OpenAI-API-key Codex user is a valid
/// Headroom setup, not a failure.
fn build_codex_guard_script() -> String {
    format!(
        r##"#!/usr/bin/env python3
"""Headroom Codex routing guard (managed by Headroom Desktop -- do not edit)."""
import json
import os
import pathlib
import subprocess
import sys
import time
import urllib.error
import urllib.request

try:
    import tomllib
except ModuleNotFoundError:
    tomllib = None

CODEX_HOME = pathlib.Path(os.environ.get("CODEX_HOME") or (pathlib.Path.home() / ".codex"))
CONFIG = CODEX_HOME / "config.toml"
BASE_URL = "{base}"
READYZ = "{readyz}"
# stderr fires every invocation; the macOS notification is rate-limited so an
# app restart doesn't produce a storm of alerts.
DEBOUNCE_PATH = pathlib.Path(__file__).with_name(".headroom-guard-notified")
DEBOUNCE_SECONDS = 600


def notify(message):
    try:
        if time.time() - DEBOUNCE_PATH.stat().st_mtime < DEBOUNCE_SECONDS:
            return
    except OSError:
        pass
    try:
        DEBOUNCE_PATH.touch()
        subprocess.run(
            [
                "/usr/bin/osascript",
                "-e",
                'display notification ' + json.dumps(message) + ' with title "Headroom Codex guard"',
            ],
            check=False,
            timeout=5,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    except Exception:
        pass


def toml_fallback(text):
    result, current = {{}}, []
    for raw in text.splitlines():
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        if line.startswith("[") and line.endswith("]"):
            current = [p.strip() for p in line.strip("[]").split(".") if p.strip()]
            continue
        if "=" not in line:
            continue
        key, value = (p.strip() for p in line.split("=", 1))
        if value[:1] == '"' and value[-1:] == '"':
            value = value[1:-1]
        target = result
        for part in current:
            target = target.setdefault(part, {{}})
        target[key] = value
    return result


def load_config():
    try:
        text = CONFIG.read_text()
    except OSError:
        return None
    if tomllib is not None:
        try:
            return tomllib.loads(text)
        except Exception:
            return toml_fallback(text)
    return toml_fallback(text)


def probe():
    # Any HTTP response means our server answered -- the app is up. A 503 during
    # bypass mode is still "up", so only connection errors / timeouts count as down.
    try:
        urllib.request.urlopen(READYZ, timeout=2)
        return True
    except urllib.error.HTTPError:
        return True
    except Exception:
        return False


def reachable():
    # One retry after a short pause so an app-relaunch blip doesn't read as "down".
    if probe():
        return True
    time.sleep(2)
    return probe()


def main():
    issues = []
    config = load_config()
    if config is None:
        issues.append("~/.codex/config.toml is missing or unreadable")
    else:
        provider_name = config.get("model_provider")
        if provider_name != "headroom":
            issues.append('Codex model_provider is "' + str(provider_name) + '" (expected "headroom"); Codex is not being optimized by Headroom')
        else:
            provider = (config.get("model_providers") or {{}}).get("headroom") or {{}}
            base = provider.get("base_url")
            if base != BASE_URL:
                issues.append("Headroom provider base_url is " + str(base) + " (expected " + BASE_URL + ")")
    if not reachable():
        issues.append("Headroom Desktop isn't running; open it to optimize Codex")

    # Never block (exit 2): Codex is the user's own OpenAI account and must keep
    # working whether or not Headroom is active. Surface issues as a once-per-
    # session notification so a genuinely broken route is visible, without
    # holding a paused or departing user's Codex hostage to the app being open.
    if issues:
        notify("; ".join(issues))
        sys.stderr.write("Headroom Codex guard:\n")
        for issue in issues:
            sys.stderr.write("- " + issue + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
"##,
        base = HEADROOM_OPENAI_BASE_URL,
        readyz = "http://127.0.0.1:6767/readyz",
    )
}

/// Merge guard entries for the given `events` into a hooks file.
/// Codex `hooks.json` and Claude `settings.json` share the identical
/// `{{"hooks": {{event: [{{matcher?, hooks: [...]}}]}}}}` shape, so both clients
/// use this. Preserves every other key in the file. Idempotent: an
/// already-registered guard command is left untouched.
fn register_guard_hook_entries(
    hooks_path: &Path,
    command: &str,
    status_message: &str,
    events: &[(&str, Option<&str>)],
) -> Result<(Vec<String>, Vec<String>)> {
    let mut content = if hooks_path.exists() {
        let raw = std::fs::read_to_string(hooks_path)
            .with_context(|| format!("reading {}", hooks_path.display()))?;
        Value::Object(parse_json_object(&raw, hooks_path)?)
    } else {
        Value::Object(Default::default())
    };

    let root = content
        .as_object_mut()
        .ok_or_else(|| anyhow!("unable to write hooks settings"))?;
    if !root.get("hooks").map(Value::is_object).unwrap_or(false) {
        root.insert("hooks".into(), Value::Object(Default::default()));
    }
    let hooks_obj = root
        .get_mut("hooks")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| anyhow!("unable to write hooks settings"))?;

    let mut mutated = false;
    for &(event, matcher) in events {
        if !hooks_obj.get(event).map(Value::is_array).unwrap_or(false) {
            hooks_obj.insert(event.to_string(), Value::Array(Vec::new()));
        }
        let entries = hooks_obj
            .get_mut(event)
            .and_then(Value::as_array_mut)
            .ok_or_else(|| anyhow!("unable to write hooks settings"))?;
        if entries
            .iter()
            .any(|entry| entry_contains_hook(entry, command))
        {
            continue;
        }
        let handler = serde_json::json!({
            "type": "command",
            "command": command,
            "timeout": 10,
            "statusMessage": status_message,
        });
        let mut entry = serde_json::Map::new();
        if let Some(matcher) = matcher {
            entry.insert("matcher".into(), Value::String(matcher.to_string()));
        }
        entry.insert("hooks".into(), Value::Array(vec![handler]));
        entries.push(Value::Object(entry));
        mutated = true;
    }

    if !mutated {
        return Ok((Vec::new(), Vec::new()));
    }

    let backup = backup_if_exists(hooks_path)?;
    if let Some(parent) = hooks_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    atomic_write(
        hooks_path,
        &serde_json::to_vec_pretty(&content).context("serializing hooks file")?,
    )?;

    let mut backups = Vec::new();
    if let Some(backup) = backup {
        backups.push(backup.display().to_string());
    }
    Ok((vec![hooks_path.display().to_string()], backups))
}

/// Whether `command` is registered under any event in a hooks file.
fn guard_registered_in_hooks(hooks_path: &Path, command: &str) -> Result<bool> {
    if !hooks_path.exists() {
        return Ok(false);
    }
    let raw = std::fs::read_to_string(hooks_path)
        .with_context(|| format!("reading {}", hooks_path.display()))?;
    let content = Value::Object(parse_json_object(&raw, hooks_path)?);
    Ok(content
        .get("hooks")
        .and_then(Value::as_object)
        .map(|hooks| {
            hooks.values().any(|entries| {
                entries
                    .as_array()
                    .map(|arr| arr.iter().any(|entry| entry_contains_hook(entry, command)))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false))
}

/// Strip the guard entries for `command` from a hooks file. Leaves any
/// user-authored hooks intact and drops now-empty event arrays. `delete_if_empty`
/// removes the whole file when nothing remains -- correct for Codex's standalone
/// `hooks.json`, but never for Claude's shared `settings.json`. `only_events`
/// limits the sweep to specific event names; `None` sweeps every event.
fn remove_guard_hook_entries(
    hooks_path: &Path,
    command: &str,
    delete_if_empty: bool,
    only_events: Option<&[&str]>,
) -> Result<()> {
    if !hooks_path.exists() {
        return Ok(());
    }
    let raw = std::fs::read_to_string(hooks_path)
        .with_context(|| format!("reading {}", hooks_path.display()))?;
    let mut content = Value::Object(parse_json_object(&raw, hooks_path)?);
    let mut changed = false;
    let mut hooks_empty = false;
    if let Some(hooks_obj) = content.get_mut("hooks").and_then(Value::as_object_mut) {
        // Sweep every event, not just the ones we register, so a guard that Codex
        // (or an older build) moved to another event is still stripped.
        let events: Vec<String> = hooks_obj.keys().cloned().collect();
        for event in events {
            if let Some(filter) = only_events {
                if !filter.contains(&event.as_str()) {
                    continue;
                }
            }
            if let Some(entries) = hooks_obj.get_mut(&event).and_then(Value::as_array_mut) {
                let before = entries.len();
                entries.retain(|entry| !entry_contains_hook(entry, command));
                if entries.len() != before {
                    changed = true;
                }
            }
        }
        hooks_obj.retain(|_, value| !value.as_array().map(|arr| arr.is_empty()).unwrap_or(false));
        hooks_empty = hooks_obj.is_empty();
    }
    if hooks_empty {
        if let Some(root) = content.as_object_mut() {
            root.remove("hooks");
        }
    }
    if !changed {
        return Ok(());
    }
    let _ = backup_if_exists(hooks_path)?;
    let root_empty = content.as_object().map(|o| o.is_empty()).unwrap_or(false);
    if delete_if_empty && root_empty {
        let _ = std::fs::remove_file(hooks_path);
    } else {
        atomic_write(
            hooks_path,
            &serde_json::to_vec_pretty(&content).context("serializing hooks file")?,
        )?;
    }
    Ok(())
}

/// Write the guard script and register it in `~/.codex/hooks.json` for the
/// SessionStart event only. `hooks.json` is auto-discovered by Codex (no
/// `config.toml` flag needed). The user must trust the hook once via Codex's
/// `/hooks` command before it runs (re-trust after any guard update).
///
/// SessionStart only (mirrors `ensure_claude_guard_hook`): on UserPromptSubmit a
/// nonzero exit blocks every prompt, which held a paused or departing user's own
/// OpenAI-billed Codex hostage to the desktop app being open -- the exact reason
/// users uninstalled. The guard is informational, not a gate; the intercept
/// forwards Codex direct to OpenAI when the app is down or the gate trips.
fn ensure_codex_guard_hook() -> Result<(Vec<String>, Vec<String>)> {
    let script_path = codex_guard_hook_path();
    let (script_changed, script_backup) =
        write_file_if_changed(&script_path, &build_codex_guard_script(), true)?;
    // Migration: earlier builds registered on UserPromptSubmit, where a nonzero
    // exit blocked every Codex prompt. Strip that entry from existing installs;
    // match on the script path so it lands regardless of interpreter drift.
    remove_guard_hook_entries(
        &codex_hooks_json_path(),
        &script_path.display().to_string(),
        false,
        Some(&["UserPromptSubmit"]),
    )?;
    let (mut changed, mut backups) = register_guard_hook_entries(
        &codex_hooks_json_path(),
        &codex_guard_command(),
        CODEX_GUARD_STATUS_MESSAGE,
        &[("SessionStart", Some("startup|resume|clear|compact"))],
    )?;
    if script_changed {
        changed.insert(0, script_path.display().to_string());
    }
    if let Some(backup) = script_backup {
        backups.insert(0, backup.display().to_string());
    }
    Ok((changed, backups))
}

fn codex_guard_registered() -> Result<bool> {
    guard_registered_in_hooks(&codex_hooks_json_path(), &codex_guard_command())
}

fn remove_codex_guard_hook() -> Result<()> {
    let script_path = codex_guard_hook_path();
    // Match on the script path, not the full `/usr/bin/python3 <path>` command,
    // so the registration is stripped even if the interpreter differs -- otherwise
    // deleting the script below leaves a dangling hook that fails with ENOENT.
    remove_guard_hook_entries(
        &codex_hooks_json_path(),
        &script_path.display().to_string(),
        true,
        None,
    )?;
    if script_path.exists() {
        let _ = std::fs::remove_file(&script_path);
    }
    Ok(())
}

const CLAUDE_GUARD_STATUS_MESSAGE: &str = "Verifying Headroom route";

fn claude_guard_hook_path() -> PathBuf {
    home_dir()
        .join(".claude")
        .join("hooks")
        .join("headroom-claude-guard.py")
}

fn claude_guard_command() -> String {
    format!("/usr/bin/python3 {}", claude_guard_hook_path().display())
}

/// Loud-fail guard that Claude Code runs at session start (SessionStart only:
/// exit 2 there surfaces a warning but cannot block, whereas on UserPromptSubmit
/// it blocks every prompt -- which broke Claude Desktop / Cowork VM sessions
/// that share `~/.claude/settings.json` but can never reach 127.0.0.1:6767).
/// Because the hook inherits Claude's environment, it checks the *effective*
/// routing -- `ANTHROPIC_BASE_URL` as Claude actually sees it -- rather than a
/// config file, plus that the desktop app is reachable. Pure stdlib so it runs
/// on the system `/usr/bin/python3`. Unlike Codex, Claude Code runs app-written
/// `settings.json` hooks without a manual trust step.
fn build_claude_guard_script() -> String {
    format!(
        r##"#!/usr/bin/env python3
"""Headroom Claude routing guard (managed by Headroom Desktop -- do not edit)."""
import json
import os
import pathlib
import subprocess
import sys
import time
import urllib.error
import urllib.request

BASE_URL = "{base}"
READYZ = "{readyz}"
# stderr fires every invocation; the macOS notification is rate-limited so an
# app restart doesn't produce a storm of alerts.
DEBOUNCE_PATH = pathlib.Path(__file__).with_name(".headroom-guard-notified")
DEBOUNCE_SECONDS = 600


def notify(message):
    try:
        if time.time() - DEBOUNCE_PATH.stat().st_mtime < DEBOUNCE_SECONDS:
            return
    except OSError:
        pass
    try:
        DEBOUNCE_PATH.touch()
        subprocess.run(
            [
                "/usr/bin/osascript",
                "-e",
                'display notification ' + json.dumps(message) + ' with title "Headroom Claude guard"',
            ],
            check=False,
            timeout=5,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    except Exception:
        pass


def probe():
    # Any HTTP response means our server answered -- the app is up. A 503 during
    # bypass mode is still "up", so only connection errors / timeouts count as down.
    try:
        urllib.request.urlopen(READYZ, timeout=2)
        return True
    except urllib.error.HTTPError:
        return True
    except Exception:
        return False


def reachable():
    # One retry after a short pause so an app-relaunch blip doesn't read as "down".
    if probe():
        return True
    time.sleep(2)
    return probe()


def settings_base(path):
    # env.ANTHROPIC_BASE_URL from a Claude settings file, or None if absent/unreadable.
    try:
        with open(path) as handle:
            data = json.load(handle)
    except Exception:
        return None
    env = data.get("env") if isinstance(data, dict) else None
    if isinstance(env, dict):
        value = env.get("ANTHROPIC_BASE_URL")
        return str(value) if value is not None else None
    return None


def diagnose_route(effective):
    # A real routing break is (a) a higher-precedence project-local scope pointing
    # elsewhere, or (b) neither user settings nor the session env routing to Headroom.
    # settings.json's env is what Claude Code actually applies to its API calls, so
    # a correct user settings + unset process env (GUI / `open` launch that didn't
    # inherit the shell export) is HEALTHY, not a failure -- don't warn on it.
    shown = effective if effective else "unset"
    home = os.path.expanduser("~")
    user_val = settings_base(os.path.join(home, ".claude", "settings.json"))
    cwd = os.getcwd()
    for path in (
        os.path.join(cwd, ".claude", "settings.local.json"),
        os.path.join(cwd, ".claude", "settings.json"),
    ):
        val = settings_base(path)
        if val is not None and val != BASE_URL:
            return "ANTHROPIC_BASE_URL -- " + path + " sets it to " + val + ", which overrides Headroom's route (" + BASE_URL + "). Remove or fix that entry."
    if user_val != BASE_URL and effective != BASE_URL:
        return "ANTHROPIC_BASE_URL is not routed to Headroom (user settings: " + (str(user_val) if user_val else "no entry") + ", session env: " + shown + "). Reopen the Headroom app or re-run client setup."
    return None


def main():
    issues = []
    route_issue = diagnose_route(os.environ.get("ANTHROPIC_BASE_URL"))
    if route_issue:
        issues.append(route_issue)
    if not reachable():
        issues.append("Headroom Desktop is not reachable on 127.0.0.1:6767 -- it may be restarting; open the app if it isn't")

    if issues:
        notify("; ".join(issues))
        sys.stderr.write("Headroom Claude guard failed:\n")
        for issue in issues:
            sys.stderr.write("- " + issue + "\n")
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
"##,
        base = HEADROOM_ANTHROPIC_BASE_URL,
        readyz = "http://127.0.0.1:6767/readyz",
    )
}

/// Write the Claude guard script and register it in `~/.claude/settings.json`
/// for SessionStart only. No trust step required.
///
/// Never UserPromptSubmit: exit 2 there blocks the prompt, and Claude Desktop /
/// Cowork VM sessions read the same settings.json but can never reach
/// 127.0.0.1:6767 from inside the VM, so the guard bricked every prompt in the
/// Claude desktop app while the routing it verifies didn't even apply there.
fn ensure_claude_guard_hook() -> Result<(Vec<String>, Vec<String>)> {
    let script_path = claude_guard_hook_path();
    let (script_changed, script_backup) =
        write_file_if_changed(&script_path, &build_claude_guard_script(), true)?;
    // Migration: earlier builds also registered on UserPromptSubmit; strip that
    // entry from existing installs. Match on the script path (see codex counterpart).
    remove_guard_hook_entries(
        &claude_settings_path(),
        &script_path.display().to_string(),
        false,
        Some(&["UserPromptSubmit"]),
    )?;
    let (mut changed, mut backups) = register_guard_hook_entries(
        &claude_settings_path(),
        &claude_guard_command(),
        CLAUDE_GUARD_STATUS_MESSAGE,
        &[("SessionStart", Some("startup|resume|clear|compact"))],
    )?;
    if script_changed {
        changed.insert(0, script_path.display().to_string());
    }
    if let Some(backup) = script_backup {
        backups.insert(0, backup.display().to_string());
    }
    Ok((changed, backups))
}

fn claude_guard_registered() -> Result<bool> {
    guard_registered_in_hooks(&claude_settings_path(), &claude_guard_command())
}

/// Strip the Claude guard from every settings candidate and delete the script.
/// Never deletes settings.json (it carries other keys), so `delete_if_empty` is
/// false.
fn remove_claude_guard_hook() -> Result<()> {
    let script_path = claude_guard_hook_path();
    // Match on the script path, not the full interpreter command (see codex counterpart).
    let fragment = script_path.display().to_string();
    for settings_path in claude_settings_candidates() {
        let _ = remove_guard_hook_entries(&settings_path, &fragment, false, None);
    }
    if script_path.exists() {
        let _ = std::fs::remove_file(&script_path);
    }
    Ok(())
}

/// Run `codex doctor` as an independent confirmation that Codex itself accepts
/// the route (stronger than our "is the text in the file" checks). Best-effort
/// and never a hard failure: a missing CLI or a doctor error for unrelated
/// reasons must not flip `verified`.
fn codex_doctor_summary() -> Option<String> {
    let codex = find_on_path(&["codex"])?;
    let output = Command::new(codex).arg("doctor").output().ok()?;
    if output.status.success() {
        Some("`codex doctor` reports the Codex install is healthy.".into())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let first = stderr
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("run `codex doctor` for details");
        Some(format!("`codex doctor` reported issues: {}", first.trim()))
    }
}

fn remove_launchctl_env(keys: &[&str]) -> Result<()> {
    for key in keys {
        let _ = run_launchctl(&["unsetenv", key]);
    }
    Ok(())
}

fn run_launchctl(args: &[&str]) -> Result<std::process::Output> {
    let output = Command::new("launchctl")
        .args(args)
        .output()
        .with_context(|| format!("running launchctl {}", args.join(" ")))?;
    if output.status.success() {
        return Ok(output);
    }

    Err(anyhow!(
        "launchctl {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

fn normalized_setup_id(client_id: &str) -> &str {
    match client_id {
        "codex" | "codex_gui" => "codex_cli",
        "vscode" => "claude_code",
        other => other,
    }
}

fn upsert_managed_block(
    file_path: &Path,
    block_id: &str,
    block_body: &str,
) -> Result<(bool, Option<PathBuf>)> {
    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }

    let existing = if file_path.exists() {
        std::fs::read_to_string(file_path)
            .with_context(|| format!("reading {}", file_path.display()))?
    } else {
        String::new()
    };

    let start = format!("# >>> headroom:{block_id} >>>");
    let end = format!("# <<< headroom:{block_id} <<<");
    let block = format!("{start}\n{block_body}\n{end}\n");
    let updated = match (existing.find(&start), existing.find(&end)) {
        // Only rewrite in place when the markers are well-ordered. A stray or
        // reordered end-before-start (leftover from an interrupted write, or a
        // hand-pasted/duplicated half-block) makes `end_with_marker < start_idx`,
        // so `existing[..start_idx]` re-emits the region the suffix also carries
        // and the old opening marker gets duplicated. Mirror strip_marker_block:
        // treat a malformed block as absent and append a fresh one instead.
        (Some(start_idx), Some(end_idx)) if end_idx >= start_idx => {
            let end_with_marker = end_idx + end.len();
            let mut rebuilt = String::with_capacity(existing.len() + block.len());
            rebuilt.push_str(&existing[..start_idx]);
            rebuilt.push_str(&block);
            if end_with_marker < existing.len() {
                // `block` already ends in `\n`; if the surviving suffix also
                // starts with `\n`, drop one to avoid blank-line padding
                // accumulating between managed blocks on repeat applies.
                let suffix = &existing[end_with_marker..];
                let suffix = suffix.strip_prefix('\n').unwrap_or(suffix);
                rebuilt.push_str(suffix);
            }
            rebuilt
        }
        _ if existing.trim().is_empty() => block,
        _ => format!("{}\n{}", existing.trim_end(), block),
    };

    if updated == existing {
        return Ok((false, None));
    }

    let backup = backup_if_exists(file_path)?;
    atomic_write(file_path, updated.as_bytes())?;
    Ok((true, backup))
}

fn write_file_if_changed(
    file_path: &Path,
    content: &str,
    executable: bool,
) -> Result<(bool, Option<PathBuf>)> {
    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }

    let existing = if file_path.exists() {
        Some(
            std::fs::read_to_string(file_path)
                .with_context(|| format!("reading {}", file_path.display()))?,
        )
    } else {
        None
    };

    if existing.as_deref() == Some(content) {
        #[cfg(unix)]
        if executable {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = std::fs::metadata(file_path)
                .with_context(|| format!("reading {}", file_path.display()))?
                .permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(file_path, permissions)
                .with_context(|| format!("chmod {}", file_path.display()))?;
        }
        return Ok((false, None));
    }

    let backup = backup_if_exists(file_path)?;
    atomic_write(file_path, content.as_bytes())?;

    #[cfg(unix)]
    if executable {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = std::fs::metadata(file_path)
            .with_context(|| format!("reading {}", file_path.display()))?
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(file_path, permissions)
            .with_context(|| format!("chmod {}", file_path.display()))?;
    }

    Ok((true, backup))
}

fn remove_shell_block(shell_targets: &[PathBuf], block_id: &str) -> Result<()> {
    for file in shell_targets {
        remove_managed_block(&file, block_id)?;
    }
    Ok(())
}

fn remove_managed_block(file_path: &Path, block_id: &str) -> Result<bool> {
    if !file_path.exists() {
        return Ok(false);
    }

    let existing = std::fs::read_to_string(file_path)
        .with_context(|| format!("reading {}", file_path.display()))?;
    let start = format!("# >>> headroom:{block_id} >>>");
    let end = format!("# <<< headroom:{block_id} <<<");

    let (Some(start_idx), Some(end_idx)) = (existing.find(&start), existing.find(&end)) else {
        return Ok(false);
    };

    let end_with_marker = end_idx + end.len();
    let tail = existing[end_with_marker..].trim_start_matches('\n');
    let mut rebuilt = String::with_capacity(existing.len());
    rebuilt.push_str(existing[..start_idx].trim_end());
    if !rebuilt.is_empty() && !tail.is_empty() {
        rebuilt.push('\n');
    }
    rebuilt.push_str(tail);
    if !rebuilt.is_empty() && !rebuilt.ends_with('\n') {
        rebuilt.push('\n');
    }

    let _ = backup_if_exists(file_path)?;
    atomic_write(file_path, rebuilt.as_bytes())?;
    Ok(true)
}

pub(crate) fn backup_if_exists(path: &Path) -> Result<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }

    let stamp = Utc::now().format("%Y%m%d%H%M%S");
    let backup_path = PathBuf::from(format!("{}.headroom-backup-{}", path.display(), stamp));
    std::fs::copy(path, &backup_path)
        .with_context(|| format!("creating backup {}", backup_path.display()))?;

    // Prune old backups — keep only the 3 most recent for this base path.
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let headroom_prefix = format!("{}.headroom-backup-", file_name);
    let nommer_prefix = format!("{}.nommer-backup-", file_name);
    if let Some(dir) = path.parent() {
        if let Ok(entries) = std::fs::read_dir(dir) {
            let mut backups: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n.starts_with(&headroom_prefix) || n.starts_with(&nommer_prefix))
                        .unwrap_or(false)
                })
                .collect();
            backups.sort();
            if backups.len() > 3 {
                for old in &backups[..backups.len() - 3] {
                    let _ = std::fs::remove_file(old);
                }
            }
        }
    }

    Ok(Some(backup_path))
}

fn shell_block_contains_in_files(
    shell_targets: &[PathBuf],
    block_id: &str,
    var_name: &str,
    expected_value: &str,
) -> Result<bool> {
    for file in shell_targets {
        if !file.exists() {
            continue;
        }
        let content = std::fs::read_to_string(&file)
            .with_context(|| format!("reading {}", file.display()))?;
        let start = format!("# >>> headroom:{block_id} >>>");
        let end = format!("# <<< headroom:{block_id} <<<");

        if let (Some(start_idx), Some(end_idx)) = (content.find(&start), content.find(&end)) {
            let block = &content[start_idx..end_idx];
            let expected_line = format!("export {var_name}={expected_value}");
            if block.contains(&expected_line) {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

fn shell_block_contains_text_in_files(
    shell_targets: &[PathBuf],
    block_id: &str,
    expected_text: &str,
) -> Result<bool> {
    for file in shell_targets {
        if !file.exists() {
            continue;
        }

        let content = std::fs::read_to_string(&file)
            .with_context(|| format!("reading {}", file.display()))?;
        let start = format!("# >>> headroom:{block_id} >>>");
        let end = format!("# <<< headroom:{block_id} <<<");

        if let (Some(start_idx), Some(end_idx)) = (content.find(&start), content.find(&end)) {
            if content[start_idx..end_idx].contains(expected_text) {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

fn claude_settings_env_matches(env_key: &str, expected_value: &str) -> Result<bool> {
    let path = claude_settings_path();
    if !path.exists() {
        return Ok(false);
    }

    let raw =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let content: Value = Value::Object(parse_json_object(&raw, &path)?);
    Ok(matches!(
        content.get("env").and_then(|env| env.get(env_key)),
        Some(Value::String(value)) if value == expected_value
    ))
}

fn claude_settings_hook_matches(hook_fragment: &str) -> Result<bool> {
    let path = claude_settings_path();
    if !path.exists() {
        return Ok(false);
    }

    let raw =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let content: Value = Value::Object(parse_json_object(&raw, &path)?);

    Ok(content
        .get("hooks")
        .and_then(|hooks| hooks.get("PreToolUse"))
        .and_then(|hooks| hooks.as_array())
        .map(|entries| {
            entries
                .iter()
                .any(|entry| entry_contains_hook(entry, hook_fragment))
        })
        .unwrap_or(false))
}

fn is_headroom_proxy_reachable() -> bool {
    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(500))
        .build()
    {
        Ok(client) => client,
        Err(_) => return false,
    };

    ["127.0.0.1", "localhost"].iter().any(|host| {
        client
            .get(format!("http://{host}:6767/readyz"))
            .send()
            // 404 = an older proxy build without the /readyz route, still up and
            // serving -- count it as reachable (Sentry RUST-2X).
            .map(|response| {
                let status = response.status();
                status.is_success() || status == reqwest::StatusCode::NOT_FOUND
            })
            .unwrap_or(false)
    })
}

/// Pure core for `detect_oss_remnants`: given the environment facts, produce the
/// operator-facing warnings. Stale open-source-install remnants coexisting with
/// the paid desktop app are the root cause of instability under concurrent
/// agents (duplicate `mcp serve`, `:8787` vs Cursor OAuth callback conflicts,
/// hooks pointing at a non-app binary). Kept pure so it is unit-testable.
fn oss_remnant_warnings(
    local_headroom_exists: bool,
    local_rtk_exists: bool,
    port_8787_listening: bool,
    claude_hook_points_at_local_bin: bool,
) -> Vec<String> {
    let mut warnings = Vec::new();
    if port_8787_listening {
        warnings.push(
            "An open-source Headroom proxy is listening on :8787. It conflicts with the paid \
             desktop proxy (:6767/:6768) and Cursor MCP OAuth callbacks. Stop it and remove the \
             open-source install."
                .into(),
        );
    }
    if local_headroom_exists {
        warnings.push(
            "Found a stale open-source binary at ~/.local/bin/headroom. Remove it so only the \
             app-owned runtime serves MCP."
                .into(),
        );
    }
    if local_rtk_exists {
        warnings.push(
            "Found a stale open-source binary at ~/.local/bin/rtk. Remove it so the Claude hook \
             uses the app-owned RTK binary."
                .into(),
        );
    }
    if claude_hook_points_at_local_bin {
        warnings.push(
            "The Claude hook in ~/.claude/settings.json points at ~/.local/bin (open-source \
             install) instead of the app-owned binary. Re-run client setup to repair it."
                .into(),
        );
    }
    warnings
}

/// Gather real environment facts and return OSS-remnant warnings, empty when the
/// install is clean.
pub fn detect_oss_remnants() -> Vec<String> {
    let local_bin = home_dir().join(".local").join("bin");
    let hook_points_at_local_bin = std::fs::read_to_string(claude_settings_path())
        .map(|raw| raw.contains(".local/bin/rtk") || raw.contains(".local/bin/headroom"))
        .unwrap_or(false);
    oss_remnant_warnings(
        local_bin.join("headroom").exists(),
        local_bin.join("rtk").exists(),
        port_listening(8787),
        hook_points_at_local_bin,
    )
}

/// True when something accepts a TCP connection on `127.0.0.1:<port>`.
fn port_listening(port: u16) -> bool {
    use std::net::{SocketAddr, TcpStream};
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok()
}

fn resolve_default_shell_targets() -> Vec<PathBuf> {
    let mut targets =
        discover_managed_shell_targets(&["managed_rtk", "claude_code"]).unwrap_or_default();
    if targets.is_empty() {
        targets = default_shell_targets_for_family(detect_shell_family());
    }
    dedupe_paths(targets)
}

fn detect_shell_family() -> ShellFamily {
    if let Some(shell_name) = std::env::var_os("SHELL")
        .and_then(|value| value.into_string().ok())
        .and_then(|value| {
            Path::new(&value)
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.to_ascii_lowercase())
        })
    {
        if shell_name.contains("zsh") {
            return ShellFamily::Zsh;
        }
        if shell_name.contains("bash") {
            return ShellFamily::Bash;
        }
        if shell_name == "sh" {
            return ShellFamily::Posix;
        }
    }

    let has_zsh_files = [ZSH_PROFILE_FILE, ZSH_RC_FILE]
        .into_iter()
        .map(shell_path)
        .any(|path| path.exists());
    let has_bash_files = [
        BASH_PROFILE_FILE,
        BASH_LOGIN_FILE,
        POSIX_PROFILE_FILE,
        BASH_RC_FILE,
    ]
    .into_iter()
    .map(shell_path)
    .any(|path| path.exists());

    match (has_zsh_files, has_bash_files) {
        (true, false) => ShellFamily::Zsh,
        (false, true) => ShellFamily::Bash,
        _ if cfg!(target_os = "macos") => ShellFamily::Zsh,
        _ => ShellFamily::Bash,
    }
}

fn default_shell_targets_for_family(shell_family: ShellFamily) -> Vec<PathBuf> {
    match shell_family {
        ShellFamily::Zsh => {
            dedupe_paths(vec![shell_path(ZSH_PROFILE_FILE), shell_path(ZSH_RC_FILE)])
        }
        ShellFamily::Bash => dedupe_paths(vec![
            preferred_bash_profile_path(),
            shell_path(BASH_RC_FILE),
        ]),
        ShellFamily::Posix => vec![shell_path(POSIX_PROFILE_FILE)],
    }
}

fn preferred_bash_profile_path() -> PathBuf {
    [BASH_PROFILE_FILE, BASH_LOGIN_FILE, POSIX_PROFILE_FILE]
        .into_iter()
        .map(shell_path)
        .find(|path| path.exists())
        .unwrap_or_else(|| shell_path(BASH_PROFILE_FILE))
}

fn discover_managed_shell_targets(block_ids: &[&str]) -> Result<Vec<PathBuf>> {
    let mut discovered = Vec::new();
    for file in all_shell_paths() {
        for block_id in block_ids {
            if file_has_managed_block(&file, block_id)? {
                discovered.push(file.clone());
                break;
            }
        }
    }
    Ok(dedupe_paths(discovered))
}

fn shell_targets_from_state(serialized_paths: Option<&Vec<String>>) -> Vec<PathBuf> {
    serialized_paths
        .into_iter()
        .flatten()
        .map(PathBuf::from)
        // A buggy build could persist an unexpanded, relative path (e.g.
        // `$XDG_CONFIG_HOME/zsh/.zshrc`); re-using it would create files under
        // the Finder-launch cwd `/`. Drop non-absolute stragglers.
        .filter(|p| p.is_absolute())
        .collect::<Vec<_>>()
}

fn serialize_paths(paths: &[PathBuf]) -> Vec<String> {
    let mut serialized = paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    dedupe_strings(&mut serialized);
    serialized
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = BTreeSet::new();
    let mut deduped = Vec::new();
    for path in paths {
        let key = path.display().to_string();
        if seen.insert(key) {
            deduped.push(path);
        }
    }
    deduped
}

fn dedupe_strings(values: &mut Vec<String>) {
    let mut seen = BTreeSet::new();
    values.retain(|value| seen.insert(value.clone()));
}

fn all_shell_paths() -> Vec<PathBuf> {
    ALL_SHELL_FILES.into_iter().map(shell_path).collect()
}

fn is_profile_file(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some(ZSH_PROFILE_FILE | BASH_PROFILE_FILE | BASH_LOGIN_FILE | POSIX_PROFILE_FILE)
    )
}

fn file_has_managed_block(file_path: &Path, block_id: &str) -> Result<bool> {
    if !file_path.exists() {
        return Ok(false);
    }

    let content = std::fs::read_to_string(file_path)
        .with_context(|| format!("reading {}", file_path.display()))?;
    let start = format!("# >>> headroom:{block_id} >>>");
    let end = format!("# <<< headroom:{block_id} <<<");
    Ok(content.contains(&start) && content.contains(&end))
}

fn shell_path(name: &str) -> PathBuf {
    match name {
        ZSH_PROFILE_FILE | ZSH_RC_FILE => zsh_dir().join(name),
        _ => home_dir().join(name),
    }
}

/// Directory zsh reads its rc/profile files from. zsh honors `$ZDOTDIR`
/// (falling back to `$HOME`); a Finder-launched app rarely inherits `$ZDOTDIR`
/// from the login shell, so when it's absent from our own env we recover it
/// from `~/.zshenv` — the file zsh always sources from `$HOME` and the
/// conventional place users set ZDOTDIR.
fn zsh_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("ZDOTDIR").filter(|v| !v.is_empty()) {
        let dir = PathBuf::from(dir);
        // A relative ZDOTDIR would create files under the (Finder-launch) cwd
        // of `/`. Only trust it if absolute; otherwise fall through to $HOME.
        if dir.is_absolute() {
            return dir;
        }
    }
    zdotdir_from_zshenv(&home_dir()).unwrap_or_else(home_dir)
}

/// Expand `$VAR` / `${VAR}` from the process env. Unset vars are left as the
/// literal `$VAR` so the caller can detect an unresolved (non-absolute) path
/// and fall back rather than creating a bogus relative dir.
fn expand_env_vars(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'$' {
            out.push(bytes[i] as char);
            i += 1;
            continue;
        }
        let (name, next) = if bytes.get(i + 1) == Some(&b'{') {
            match raw[i + 2..].find('}') {
                Some(end) => (&raw[i + 2..i + 2 + end], i + 2 + end + 1),
                None => (&raw[i..i], i + 1), // unterminated `${` -> emit `$` literally
            }
        } else {
            let end = raw[i + 1..]
                .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .map(|o| i + 1 + o)
                .unwrap_or(raw.len());
            (&raw[i + 1..end], end)
        };
        match (!name.is_empty())
            .then(|| std::env::var(name).ok())
            .flatten()
        {
            Some(val) => out.push_str(&val),
            None => out.push_str(&raw[i..next]), // keep literal `$VAR` when unset
        }
        i = next;
    }
    out
}

fn zdotdir_from_zshenv(home: &Path) -> Option<PathBuf> {
    let content = std::fs::read_to_string(home.join(".zshenv")).ok()?;
    for line in content.lines() {
        let line = line.trim();
        let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
        let Some(value) = line.strip_prefix("ZDOTDIR=") else {
            continue;
        };
        let raw = if let Some(inner) = value.strip_prefix('"').and_then(|v| v.split('"').next()) {
            inner
        } else if let Some(inner) = value.strip_prefix('\'').and_then(|v| v.split('\'').next()) {
            inner
        } else {
            value.split([' ', '\t', '#']).next().unwrap_or("")
        };
        if raw.is_empty() {
            continue;
        }
        let expanded = if let Some(tail) = raw.strip_prefix("~/") {
            home.join(tail)
        } else if raw == "~" {
            home.to_path_buf()
        } else if let Some(tail) = raw
            .strip_prefix("$HOME/")
            .or_else(|| raw.strip_prefix("${HOME}/"))
        {
            home.join(tail)
        } else {
            PathBuf::from(expand_env_vars(raw))
        };
        // If expansion couldn't fully resolve the value (unset env var leaves a
        // literal `$`, or it's otherwise relative), returning it would create a
        // bogus dir relative to cwd (e.g. `$XDG_CONFIG_HOME/zsh` under `/`).
        // Fall back to $HOME instead.
        if !expanded.is_absolute() {
            return None;
        }
        return Some(expanded);
    }
    None
}

fn claude_settings_path() -> PathBuf {
    home_dir().join(".claude").join("settings.json")
}

fn headroom_rtk_hook_path() -> PathBuf {
    home_dir()
        .join(".claude")
        .join("hooks")
        .join("headroom-rtk-rewrite.sh")
}

fn headroom_markitdown_hook_path() -> PathBuf {
    home_dir()
        .join(".claude")
        .join("hooks")
        .join("headroom-markitdown-read.sh")
}

/// PreToolUse(Read) hook: when Claude reads a PDF, convert it to Markdown via
/// the managed `markitdown` and redirect the read at the converted file through
/// `updatedInput.file_path`. Fails open at every step so a missing binary,
/// oversized file, or conversion error falls through to a native Read.
///
/// Scoped to PDF deliberately: Claude Code's Read tool rejects unsupported
/// binary types (docx/pptx/xlsx) at input validation *before* PreToolUse hooks
/// run, so a hook can never intercept them. Office formats are handled instead
/// by the managed CLAUDE.md nudge that points Claude at the `markitdown` CLI.
fn build_headroom_markitdown_hook(markitdown_path: &Path, python_path: &Path) -> String {
    let markitdown = shell_double_quote(&markitdown_path.to_string_lossy());
    let python = shell_double_quote(&python_path.to_string_lossy());

    format!(
        r#"#!/usr/bin/env bash
set -euo pipefail

HEADROOM_MARKITDOWN="{markitdown}"
HEADROOM_PYTHON="{python}"

if [ ! -x "$HEADROOM_MARKITDOWN" ] || [ ! -x "$HEADROOM_PYTHON" ]; then
  exit 0
fi

INPUT="$(cat)"
if [ -z "$INPUT" ]; then
  exit 0
fi

HEADROOM_MD_CACHE="${{TMPDIR:-/tmp}}/headroom-markitdown"
mkdir -p "$HEADROOM_MD_CACHE" 2>/dev/null || exit 0

HEADROOM_MARKITDOWN_BIN="$HEADROOM_MARKITDOWN" HEADROOM_MD_CACHE="$HEADROOM_MD_CACHE" "$HEADROOM_PYTHON" -c 'import json, os, sys, subprocess, hashlib
ALLOWED = {{".pdf"}}
MAX_BYTES = 25 * 1024 * 1024
try:
    data = json.load(sys.stdin)
except Exception:
    sys.exit(0)
tool_input = data.get("tool_input")
if not isinstance(tool_input, dict):
    sys.exit(0)
fp = tool_input.get("file_path")
if not isinstance(fp, str) or not fp:
    sys.exit(0)
if os.path.splitext(fp)[1].lower() not in ALLOWED:
    sys.exit(0)
try:
    st = os.stat(fp)
except OSError:
    sys.exit(0)
if st.st_size > MAX_BYTES:
    sys.exit(0)
binpath = os.environ["HEADROOM_MARKITDOWN_BIN"]
cache = os.environ["HEADROOM_MD_CACHE"]
key = hashlib.sha256((os.path.abspath(fp) + ":" + str(st.st_mtime_ns)).encode()).hexdigest()[:16]
out = os.path.join(cache, key + ".md")
if not (os.path.exists(out) and os.path.getsize(out) > 0):
    try:
        subprocess.run([binpath, fp, "-o", out], check=True, capture_output=True, timeout=120)
    except Exception:
        sys.exit(0)
if not (os.path.exists(out) and os.path.getsize(out) > 0):
    sys.exit(0)
updated = dict(tool_input)
updated["file_path"] = out
json.dump({{"hookSpecificOutput": {{"hookEventName": "PreToolUse", "permissionDecision": "allow", "permissionDecisionReason": "Headroom MarkItDown conversion", "updatedInput": updated}}}}, sys.stdout)' <<<"$INPUT" 2>/dev/null || exit 0
"#
    )
}

fn shell_double_quote(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`")
}

fn build_headroom_rtk_hook(managed_rtk_path: &Path, managed_python_path: &Path) -> String {
    let rtk = shell_double_quote(&managed_rtk_path.to_string_lossy());
    let python = shell_double_quote(&managed_python_path.to_string_lossy());

    format!(
        r#"#!/usr/bin/env bash
set -euo pipefail

HEADROOM_RTK="{rtk}"
HEADROOM_PYTHON="{python}"

if [ ! -x "$HEADROOM_RTK" ] || [ ! -x "$HEADROOM_PYTHON" ]; then
  exit 0
fi

INPUT="$(cat)"
if [ -z "$INPUT" ]; then
  exit 0
fi

CMD="$("$HEADROOM_PYTHON" -c 'import json, sys; data = json.load(sys.stdin); cmd = data.get("tool_input", {{}}).get("command", ""); print(cmd if isinstance(cmd, str) else "")' <<<"$INPUT" 2>/dev/null || true)"
if [ -z "$CMD" ]; then
  exit 0
fi

# `rtk git diff --check` swallows the whitespace-error report the flag exists to
# produce (only the exit code survives), so any --check command must stay raw.
case " $CMD " in
  *" --check "*) exit 0 ;;
esac

REWRITTEN="$("$HEADROOM_RTK" rewrite "$CMD" 2>/dev/null || true)"
if [ -z "$REWRITTEN" ] || [ "$CMD" = "$REWRITTEN" ]; then
  exit 0
fi

# `rtk rewrite` emits a bare `rtk` leading token, which only resolves if the
# managed PATH export has propagated into this session's environment. GUI apps
# (VSCode, terminals) launched before rtk was enabled inherit a stale PATH, so
# `rtk` is missing and the rewrite would fail with "command not found". Pin the
# leading token to the managed binary's absolute path so it works regardless.
if [ "${{REWRITTEN%% *}}" = "rtk" ]; then
  REWRITTEN="$HEADROOM_RTK${{REWRITTEN#rtk}}"
fi

# Defense-in-depth: if the rewritten command's first token isn't resolvable
# (e.g. a partial uninstall left `rtk` missing from PATH), fall through to the
# original command instead of handing Claude Code a command that will fail with
# "command not found".
FIRST_TOKEN="${{REWRITTEN%% *}}"
case "$FIRST_TOKEN" in
  /*)
    [ -x "$FIRST_TOKEN" ] || exit 0
    ;;
  *)
    command -v "$FIRST_TOKEN" >/dev/null 2>&1 || exit 0
    ;;
esac

# The pin above only fixes the LEADING token. `rtk rewrite` also emits `rtk`
# embedded after a `&&`, `;`, or `|` (e.g. `cd web && rtk npx ...`), and those
# stay bare -- they fail with "command not found: rtk" in the non-interactive,
# non-login shell Claude Code's Bash tool spawns, which sources only ~/.zshenv
# (never the .zprofile/.zshrc where the managed PATH export lands). Prepend the
# managed bin dir to PATH for this one invocation so every `rtk`, at any
# position, resolves regardless of which profile files the shell sourced.
REWRITTEN="export PATH=\"$(dirname "$HEADROOM_RTK"):\$PATH\"; $REWRITTEN"

HEADROOM_RTK_REWRITTEN="$REWRITTEN" "$HEADROOM_PYTHON" -c 'import json, os, sys; data = json.load(sys.stdin); tool_input = data.get("tool_input"); 
if not isinstance(tool_input, dict):
    sys.exit(0)
updated = dict(tool_input)
updated["command"] = os.environ["HEADROOM_RTK_REWRITTEN"]
json.dump({{"hookSpecificOutput": {{"hookEventName": "PreToolUse", "permissionDecision": "allow", "permissionDecisionReason": "Headroom RTK auto-rewrite", "updatedInput": updated}}}}, sys.stdout)' <<<"$INPUT" 2>/dev/null || exit 0
"#
    )
}

fn home_dir() -> PathBuf {
    dirs::home_dir()
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| std::env::temp_dir())
}

/// Codex's home directory. Mirrors the Codex CLI and the upstream Headroom
/// proxy: honor `$CODEX_HOME` when set, else `~/.codex`. Staying in sync with
/// the proxy matters — if the two layers disagree on where Codex lives, the
/// provider retag rewrites a different store than the config it edited.
fn codex_home() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".codex"))
}

/// Grok Build's home directory. Honors `$GROK_HOME` when set, else `~/.grok`.
fn grok_home() -> PathBuf {
    std::env::var_os("GROK_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".grok"))
}

fn detect_claude_code_client(configured: bool) -> ClientStatus {
    let executable = claude_code_candidate_paths()
        .into_iter()
        .find(|path| path.exists())
        .or_else(|| find_on_path(&["claude", "claude-code"]));

    if let Some(path) = executable {
        return ClientStatus {
            id: "claude_code".into(),
            name: "Claude Code".into(),
            installed: true,
            configured,
            health: if configured {
                ClientHealth::Healthy
            } else {
                ClientHealth::Attention
            },
            notes: if configured {
                vec![
                    format!("Detected at {}", path.display()),
                    "Configured by Headroom.".into(),
                ]
            } else {
                vec![
                    format!("Detected at {}", path.display()),
                    "Route Claude Code through Headroom's localhost proxy so prompts stay lean."
                        .into(),
                ]
            },
        };
    }

    if claude_code_user_state_exists(&home_dir()) {
        return ClientStatus {
            id: "claude_code".into(),
            name: "Claude Code".into(),
            installed: true,
            configured,
            health: if configured {
                ClientHealth::Healthy
            } else {
                ClientHealth::Attention
            },
            notes: if configured {
                vec![
                    "Detected Claude Code data in ~/.claude.".into(),
                    "Configured by Headroom.".into(),
                ]
            } else {
                vec![
                    "Detected Claude Code data in ~/.claude.".into(),
                    "Claude Code appears to be installed, but Headroom could not resolve the CLI from its current launch PATH. This is common when Headroom starts outside your shell and Claude was installed via nvm or another user-local toolchain.".into(),
                ]
            },
        };
    }

    ClientStatus {
        id: "claude_code".into(),
        name: "Claude Code".into(),
        installed: false,
        configured: false,
        health: ClientHealth::NotDetected,
        notes: vec!["Not detected on this machine yet.".into()],
    }
}

fn claude_code_candidate_paths() -> Vec<PathBuf> {
    let home = home_dir();
    let binary_names = ["claude", "claude-code"];
    let mut candidates = vec![
        PathBuf::from("/usr/local/bin/claude"),
        PathBuf::from("/opt/homebrew/bin/claude"),
        PathBuf::from("/usr/local/bin/claude-code"),
        PathBuf::from("/opt/homebrew/bin/claude-code"),
    ];

    let user_bin_dirs = vec![
        home.join(".local").join("bin"),
        home.join("bin"),
        home.join(".npm-global").join("bin"),
        home.join(".yarn").join("bin"),
        home.join(".config")
            .join("yarn")
            .join("global")
            .join("node_modules")
            .join(".bin"),
        home.join(".volta").join("bin"),
        home.join(".bun").join("bin"),
        home.join(".asdf").join("shims"),
        home.join(".mise").join("shims"),
        home.join(".nodenv").join("shims"),
    ];

    candidates.extend(binary_candidates_in_dirs(&user_bin_dirs, &binary_names));
    candidates.extend(nvm_binary_candidates(&home, &binary_names));
    dedupe_paths(candidates)
}

fn binary_candidates_in_dirs(directories: &[PathBuf], binary_names: &[&str]) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for directory in directories {
        for binary_name in binary_names {
            candidates.push(directory.join(binary_name));
            if cfg!(windows) {
                for ext in windows_path_extensions() {
                    candidates.push(directory.join(format!("{binary_name}{ext}")));
                }
            }
        }
    }
    candidates
}

fn nvm_binary_candidates(home: &Path, binary_names: &[&str]) -> Vec<PathBuf> {
    let mut candidates = binary_candidates_in_dirs(
        &[home.join(".nvm").join("current").join("bin")],
        binary_names,
    );
    let versions_dir = home.join(".nvm").join("versions").join("node");
    let Ok(entries) = std::fs::read_dir(versions_dir) else {
        return candidates;
    };

    let mut version_bins = entries
        .flatten()
        .map(|entry| entry.path().join("bin"))
        .collect::<Vec<_>>();
    version_bins.sort();
    version_bins.reverse();
    candidates.extend(binary_candidates_in_dirs(&version_bins, binary_names));
    candidates
}

fn claude_code_user_state_exists(home: &Path) -> bool {
    let claude_root = home.join(".claude");
    claude_root.join("settings.json").exists()
        || claude_root.join("projects").exists()
        || claude_root.join("sessions").exists()
        || claude_root.join("statsig").exists()
}

fn detect_codex_client(configured: bool) -> ClientStatus {
    let executable = codex_candidate_paths()
        .into_iter()
        .find(|path| path.exists())
        .or_else(|| find_on_path(&["codex"]));

    let detected = executable
        .as_ref()
        .map(|path| format!("Detected at {}", path.display()))
        .or_else(|| {
            codex_user_state_exists()
                .then(|| format!("Detected Codex data in {}.", codex_home().display()))
        });

    if let Some(detected_note) = detected {
        return ClientStatus {
            id: "codex".into(),
            name: "Codex".into(),
            installed: true,
            configured,
            health: if configured {
                ClientHealth::Healthy
            } else {
                ClientHealth::Attention
            },
            notes: if configured {
                vec![detected_note, "Configured by Headroom.".into()]
            } else {
                vec![
                    detected_note,
                    "Route Codex through Headroom's localhost proxy so prompts stay lean.".into(),
                ]
            },
        };
    }

    ClientStatus {
        id: "codex".into(),
        name: "Codex".into(),
        installed: false,
        configured: false,
        health: ClientHealth::NotDetected,
        notes: vec!["Not detected on this machine yet.".into()],
    }
}

fn detect_grok_build_client(configured: bool) -> ClientStatus {
    let executable = grok_candidate_paths()
        .into_iter()
        .find(|path| path.exists())
        .or_else(|| find_on_path(&["grok"]));

    let detected = executable
        .as_ref()
        .map(|path| format!("Detected at {}", path.display()))
        .or_else(|| {
            grok_user_state_exists()
                .then(|| format!("Detected Grok Build data in {}.", grok_home().display()))
        });

    if let Some(detected_note) = detected {
        return ClientStatus {
            id: "grok_build".into(),
            name: "Grok Build".into(),
            installed: true,
            configured,
            health: if configured {
                ClientHealth::Healthy
            } else {
                ClientHealth::Attention
            },
            notes: if configured {
                vec![detected_note, "Configured by Headroom.".into()]
            } else {
                vec![
                    detected_note,
                    "Route Grok Build through Headroom's localhost proxy so prompts stay lean."
                        .into(),
                ]
            },
        };
    }

    ClientStatus {
        id: "grok_build".into(),
        name: "Grok Build".into(),
        installed: false,
        configured: false,
        health: ClientHealth::NotDetected,
        notes: vec!["Not detected on this machine yet.".into()],
    }
}

fn grok_candidate_paths() -> Vec<PathBuf> {
    let home = home_dir();
    let mut candidates = vec![
        PathBuf::from("/usr/local/bin/grok"),
        PathBuf::from("/opt/homebrew/bin/grok"),
        home.join(".grok")
            .join("downloads")
            .join("grok-macos-aarch64"),
        home.join(".grok")
            .join("downloads")
            .join("grok-macos-x86_64"),
    ];

    let user_bin_dirs = vec![
        home.join(".local").join("bin"),
        home.join("bin"),
        home.join(".cargo").join("bin"),
    ];
    candidates.extend(binary_candidates_in_dirs(&user_bin_dirs, &["grok"]));
    dedupe_paths(candidates)
}

fn grok_user_state_exists() -> bool {
    let grok_root = grok_home();
    grok_root.join("config.toml").exists()
        || grok_root.join("auth.json").exists()
        || grok_root.join("sessions").exists()
        || grok_root.join("downloads").exists()
}

fn codex_candidate_paths() -> Vec<PathBuf> {
    let home = home_dir();
    let binary_names = ["codex"];
    let mut candidates = vec![
        PathBuf::from("/usr/local/bin/codex"),
        PathBuf::from("/opt/homebrew/bin/codex"),
    ];

    let user_bin_dirs = vec![
        home.join(".local").join("bin"),
        home.join(".cargo").join("bin"),
        home.join("bin"),
        home.join(".npm-global").join("bin"),
        home.join(".yarn").join("bin"),
        home.join(".volta").join("bin"),
        home.join(".bun").join("bin"),
        home.join(".asdf").join("shims"),
        home.join(".mise").join("shims"),
        home.join(".nodenv").join("shims"),
    ];

    candidates.extend(binary_candidates_in_dirs(&user_bin_dirs, &binary_names));
    candidates.extend(nvm_binary_candidates(&home, &binary_names));
    dedupe_paths(candidates)
}

fn codex_user_state_exists() -> bool {
    let codex_root = codex_home();
    codex_root.join("config.toml").exists()
        || codex_root.join("auth.json").exists()
        || codex_root.join("sessions").exists()
}

/// Locate the Codex CLI binary the same way [`detect_codex_client`] does: known
/// install locations first, then a PATH lookup. Used as the Headroom Learn
/// analysis backend (`codex exec`) for Codex sessions.
pub(crate) fn detect_codex_cli() -> Option<PathBuf> {
    codex_candidate_paths()
        .into_iter()
        .find(|path| path.exists())
        .or_else(|| find_on_path(&["codex"]))
}

/// True once the user has signed in to Codex with their ChatGPT account — the
/// OAuth token lands in `~/.codex/auth.json`. Required for the keyless
/// `codex exec` analysis backend.
pub(crate) fn codex_logged_in() -> bool {
    codex_home().join("auth.json").is_file()
}

fn parse_json_object(raw: &str, path: &Path) -> Result<serde_json::Map<String, Value>> {
    let value: Value = match serde_json::from_str(raw) {
        Ok(value) => value,
        Err(_) => {
            let value = json5::from_str(raw).with_context(|| {
                format!(
                    "parsing {} failed (JSON/JSON5); refusing to overwrite potentially valid user settings",
                    path.display()
                )
            })?;
            // Writers re-serialize with serde_json, which strips the
            // comments/relaxed syntax that forced the JSON5 fallback. Log it
            // locally so the .headroom-backup is discoverable, but do NOT
            // capture to Sentry: this is expected, benign behavior (user keeps
            // comments in their settings), and the capture just inflated
            // RUST-4R with 120+ no-action events. Local info only.
            log::info!(
                "{} contains JSON5 syntax (comments/trailing commas); a Headroom rewrite will normalize it to strict JSON — the original is kept as a .headroom-backup file",
                path.display()
            );
            value
        }
    };
    value
        .as_object()
        .cloned()
        .ok_or_else(|| anyhow!("{} must contain a top-level JSON object", path.display()))
}

fn find_on_path(binary_names: &[&str]) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    find_on_path_entries(std::env::split_paths(&path_var), binary_names)
}

fn find_on_path_entries<I>(path_entries: I, binary_names: &[&str]) -> Option<PathBuf>
where
    I: IntoIterator<Item = PathBuf>,
{
    for entry in path_entries {
        for binary_name in binary_names {
            let candidate = entry.join(binary_name);
            if candidate.exists() {
                return Some(candidate);
            }

            if cfg!(windows) {
                for ext in windows_path_extensions() {
                    let with_ext = entry.join(format!("{binary_name}{ext}"));
                    if with_ext.exists() {
                        return Some(with_ext);
                    }
                }
            }
        }
    }

    None
}

fn windows_path_extensions() -> Vec<String> {
    std::env::var_os("PATHEXT")
        .unwrap_or_else(|| OsStr::new(".COM;.EXE;.BAT;.CMD").to_os_string())
        .to_string_lossy()
        .split(';')
        .filter(|value| !value.is_empty())
        .map(|value| {
            if value.starts_with('.') {
                value.to_string()
            } else {
                format!(".{value}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::json;

    use super::{
        build_claude_guard_script, build_codex_guard_script, build_headroom_markitdown_hook,
        build_headroom_rtk_hook, build_markitdown_codex_nudge, build_markitdown_office_nudge,
        claude_code_user_state_exists, claude_hook_present_in_value, codex_home,
        codex_sqlite_store_expected, default_shell_targets_for_family, discover_codex_state_dbs,
        entry_contains_hook, find_on_path_entries, is_no_space, is_permission_denied,
        normalize_setup_state, normalized_setup_id, nvm_binary_candidates, oss_remnant_warnings,
        parse_json_object, pin_codex_mcp_command, remove_managed_block,
        remove_pre_tool_use_markers, render_codex_config, retag_codex_thread_providers,
        retag_codex_threads_to_headroom, retag_one_codex_db, serialize_paths,
        shell_block_contains_in_files, shell_block_contains_text_in_files, shell_double_quote,
        strip_headroom_hook_from_settings, upsert_managed_block, write_file_if_changed,
        ClientSetupState, ShellFamily,
    };
    use rusqlite::Connection;

    #[test]
    fn is_permission_denied_matches_only_permission_errors() {
        let denied = anyhow::Error::new(std::io::Error::from_raw_os_error(13))
            .context("writing /Users/x/.zshrc");
        assert!(is_permission_denied(&denied));

        let not_found = anyhow::Error::new(std::io::Error::from_raw_os_error(2))
            .context("writing /Users/x/.zshrc");
        assert!(!is_permission_denied(&not_found));

        assert!(!is_permission_denied(&anyhow::anyhow!("Permission denied")));
    }

    #[test]
    fn is_no_space_matches_only_enospc() {
        let full = anyhow::Error::new(std::io::Error::from_raw_os_error(28))
            .context("creating backup /Users/x/.claude/settings.json.headroom-backup");
        assert!(is_no_space(&full));

        let denied = anyhow::Error::new(std::io::Error::from_raw_os_error(13))
            .context("writing /Users/x/.zshrc");
        assert!(!is_no_space(&denied));

        assert!(!is_no_space(&anyhow::anyhow!("No space left on device")));
    }

    #[test]
    fn normalize_setup_state_keeps_codex_but_drops_legacy_codex_gui() {
        let state = ClientSetupState {
            configured_clients: BTreeMap::from([
                ("claude_code".into(), "2026-03-27T10:00:00Z".into()),
                ("codex_cli".into(), "2026-03-27T10:01:00Z".into()),
                ("codex_gui".into(), "2026-03-27T10:02:00Z".into()),
            ]),
            remembered_clients: BTreeMap::from([
                ("codex".into(), "2026-03-27T10:03:00Z".into()),
                ("claude_code".into(), "2026-03-27T10:04:00Z".into()),
            ]),
            managed_shell_files: BTreeMap::from([
                ("claude_code".into(), vec!["/Users/test/.zprofile".into()]),
                ("codex_cli".into(), vec!["/Users/test/.zshrc".into()]),
                ("codex_gui".into(), vec!["/Users/test/.zshrc".into()]),
            ]),
            remembered_shell_files: BTreeMap::from([
                ("codex".into(), vec!["/Users/test/.bash_profile".into()]),
                ("claude_code".into(), vec!["/Users/test/.bashrc".into()]),
            ]),
            preserved_base_urls: BTreeMap::new(),
            rtk_disabled: false,
        };

        let normalized = normalize_setup_state(state);

        // codex_cli stays configured; only the removed codex_gui id is stripped.
        assert!(normalized.configured_clients.contains_key("claude_code"));
        assert!(normalized.configured_clients.contains_key("codex_cli"));
        assert!(!normalized.configured_clients.contains_key("codex_gui"));

        assert!(normalized.remembered_clients.contains_key("claude_code"));
        assert!(normalized.remembered_clients.contains_key("codex"));

        assert!(normalized.managed_shell_files.contains_key("claude_code"));
        assert!(normalized.managed_shell_files.contains_key("codex_cli"));
        assert!(!normalized.managed_shell_files.contains_key("codex_gui"));

        assert!(normalized
            .remembered_shell_files
            .contains_key("claude_code"));
        assert!(normalized.remembered_shell_files.contains_key("codex"));
    }

    #[test]
    fn parse_json_object_accepts_json5_but_rejects_non_objects() {
        let parsed = parse_json_object(
            "{ unquoted: 'value', trailing: true, }",
            Path::new("settings.json"),
        )
        .expect("json5 object should parse");
        assert_eq!(
            parsed.get("unquoted").and_then(|value| value.as_str()),
            Some("value")
        );
        assert_eq!(
            parsed.get("trailing").and_then(|value| value.as_bool()),
            Some(true)
        );

        let err =
            parse_json_object("[]", Path::new("settings.json")).expect_err("arrays are rejected");
        assert!(err
            .to_string()
            .contains("must contain a top-level JSON object"));
    }

    #[test]
    fn setup_aliases_map_to_current_primary_ids() {
        assert_eq!(normalized_setup_id("codex"), "codex_cli");
        assert_eq!(normalized_setup_id("codex_gui"), "codex_cli");
        assert_eq!(normalized_setup_id("vscode"), "claude_code");
        assert_eq!(normalized_setup_id("claude_code"), "claude_code");
    }

    #[test]
    fn shell_double_quote_escapes_shell_sensitive_characters() {
        let escaped = shell_double_quote("path with spaces/$HOME/\"quoted\"`cmd`\\tail");
        assert_eq!(
            escaped,
            "path with spaces/\\$HOME/\\\"quoted\\\"\\`cmd\\`\\\\tail"
        );
    }

    #[test]
    fn shell_targets_include_profile_and_rc_for_supported_shells() {
        let zsh_targets = default_shell_targets_for_family(ShellFamily::Zsh);
        let bash_targets = default_shell_targets_for_family(ShellFamily::Bash);

        assert!(zsh_targets.iter().any(|path| path.ends_with(".zprofile")));
        assert!(zsh_targets.iter().any(|path| path.ends_with(".zshrc")));
        assert!(bash_targets.iter().any(|path| {
            path.ends_with(".bash_profile")
                || path.ends_with(".bash_login")
                || path.ends_with(".profile")
        }));
        assert!(bash_targets.iter().any(|path| path.ends_with(".bashrc")));
    }

    #[test]
    fn serialize_paths_dedupes_repeated_entries() {
        let serialized = serialize_paths(&[
            PathBuf::from("/Users/test/.zprofile"),
            PathBuf::from("/Users/test/.zprofile"),
            PathBuf::from("/Users/test/.zshrc"),
        ]);

        assert_eq!(
            serialized,
            vec![
                "/Users/test/.zprofile".to_string(),
                "/Users/test/.zshrc".to_string()
            ]
        );
    }

    #[test]
    fn generated_rtk_hook_uses_escaped_paths_and_rewrite_reason() {
        let hook = build_headroom_rtk_hook(
            Path::new("/tmp/head room/bin/rtk"),
            Path::new("/tmp/head room/runtime/$python"),
        );

        assert!(hook.contains("HEADROOM_RTK=\"/tmp/head room/bin/rtk\""));
        assert!(hook.contains("HEADROOM_PYTHON=\"/tmp/head room/runtime/\\$python\""));
        assert!(hook.contains("Headroom RTK auto-rewrite"));
        assert!(hook.contains("\"updatedInput\": updated"));
    }

    #[test]
    fn generated_markitdown_hook_escapes_paths_and_redirects_read() {
        let hook = build_headroom_markitdown_hook(
            Path::new("/tmp/head room/venv/bin/markitdown"),
            Path::new("/tmp/head room/venv/bin/$python"),
        );

        assert!(hook.contains("HEADROOM_MARKITDOWN=\"/tmp/head room/venv/bin/markitdown\""));
        assert!(hook.contains("HEADROOM_PYTHON=\"/tmp/head room/venv/bin/\\$python\""));
        // Scoped to PDF only (Office is handled by the nudge, not the hook),
        // redirects via updatedInput, and fails open.
        assert!(hook.contains("ALLOWED = {\".pdf\"}"));
        assert!(!hook.contains(".docx"));
        assert!(hook.contains("updated[\"file_path\"] = out"));
        assert!(hook.contains("\"updatedInput\": updated"));
        assert!(hook.contains("Headroom MarkItDown conversion"));
        assert!(hook.contains("sys.exit(0)"));
    }

    #[test]
    fn disabling_markitdown_marker_leaves_rtk_hook_intact() {
        let root = unique_temp_dir("headroom-strip-markitdown");
        fs::create_dir_all(&root).expect("create root");
        let settings = root.join("settings.json");
        fs::write(
            &settings,
            serde_json::to_string_pretty(&json!({
                "hooks": {
                    "PreToolUse": [
                        { "matcher": "Bash", "hooks": [{ "type": "command", "command": "/h/headroom-rtk-rewrite.sh" }] },
                        { "matcher": "Read", "hooks": [{ "type": "command", "command": "/h/headroom-markitdown-read.sh" }] }
                    ]
                }
            }))
            .unwrap(),
        )
        .expect("write settings");

        let changed = remove_pre_tool_use_markers(&settings, &["headroom-markitdown-read.sh"])
            .expect("strip");
        assert!(changed);

        let after: serde_json::Value =
            serde_json::from_slice(&fs::read(&settings).unwrap()).unwrap();
        let entries = after["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entry_contains_hook(&entries[0], "headroom-rtk-rewrite.sh"));
    }

    #[test]
    fn markitdown_office_nudge_points_at_the_shim_and_skips_pdf() {
        let nudge = build_markitdown_office_nudge(Path::new("/h/bin/markitdown"));
        assert!(nudge.contains("/h/bin/markitdown <path>"));
        assert!(nudge.contains(".docx"));
        assert!(nudge.contains("PDFs are handled automatically"));
    }

    #[test]
    fn markitdown_codex_nudge_covers_pdf_and_office() {
        let nudge = build_markitdown_codex_nudge(Path::new("/h/bin/markitdown"));
        assert!(nudge.contains("/h/bin/markitdown <path>"));
        // Codex has no hook, so PDF is covered by the CLI nudge too.
        assert!(nudge.contains(".pdf"));
        assert!(nudge.contains(".docx"));
    }

    #[test]
    fn hook_detection_finds_nested_hook_commands() {
        let hook_path = "/Users/test/.claude/hooks/headroom-rtk-rewrite.sh";
        let content = json!({
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "bash",
                        "hooks": [
                            { "type": "command", "command": hook_path }
                        ]
                    }
                ]
            }
        });

        assert!(claude_hook_present_in_value(&content, hook_path));
        assert!(entry_contains_hook(
            &content["hooks"]["PreToolUse"][0],
            "headroom-rtk-rewrite.sh"
        ));
        assert!(!entry_contains_hook(
            &json!({ "hooks": [] }),
            "headroom-rtk-rewrite.sh"
        ));
    }

    #[test]
    fn nvm_binary_candidates_include_installed_versions() {
        let home = unique_temp_dir("headroom-nvm-detect");
        let version_bin = home
            .join(".nvm")
            .join("versions")
            .join("node")
            .join("v22.17.1")
            .join("bin");
        fs::create_dir_all(&version_bin).expect("create nvm bin");
        fs::write(version_bin.join("claude"), "").expect("write fake claude binary");

        let candidates = nvm_binary_candidates(&home, &["claude"]);

        assert!(candidates
            .iter()
            .any(|candidate| candidate == &version_bin.join("claude")));

        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn path_lookup_scans_supplied_entries() {
        let home = unique_temp_dir("headroom-path-detect");
        let bin_dir = home.join("custom-bin");
        fs::create_dir_all(&bin_dir).expect("create custom bin");
        fs::write(bin_dir.join("claude"), "").expect("write fake claude binary");

        let detected = find_on_path_entries(vec![bin_dir.clone()], &["claude"]);

        assert_eq!(detected, Some(bin_dir.join("claude")));

        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn claude_user_state_detection_accepts_settings_or_projects() {
        let home = unique_temp_dir("headroom-claude-home");
        let claude_root = home.join(".claude");
        fs::create_dir_all(&claude_root).expect("create claude root");
        assert!(!claude_code_user_state_exists(&home));

        fs::write(claude_root.join("settings.json"), "{}").expect("write settings");
        assert!(claude_code_user_state_exists(&home));

        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn managed_block_upsert_replaces_existing_block_without_duplication() {
        let root = unique_temp_dir("headroom-managed-block");
        fs::create_dir_all(&root).expect("create root");
        let path = root.join(".zshrc");
        fs::write(&path, "export PATH=/usr/bin\n").expect("write shell file");

        let first = upsert_managed_block(
            &path,
            "claude_code",
            "export ANTHROPIC_BASE_URL=http://127.0.0.1:6767",
        )
        .expect("insert managed block");
        assert!(first.0);
        assert!(first.1.is_some());

        upsert_managed_block(
            &path,
            "claude_code",
            "export ANTHROPIC_BASE_URL=http://127.0.0.1:6767\nexport HEADROOM=1",
        )
        .expect("replace managed block");

        let content = fs::read_to_string(&path).expect("read updated shell file");
        assert_eq!(content.matches("# >>> headroom:claude_code >>>").count(), 1);
        assert!(content.contains("export PATH=/usr/bin"));
        assert!(content.contains("export HEADROOM=1"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn managed_block_upsert_treats_reordered_markers_as_absent() {
        // A stray end-before-start block (leftover from an interrupted write).
        // The old slice-based rewrite duplicated the stray fragments and left a
        // dangling opening marker at the tail; the guarded path appends a fresh,
        // well-formed block instead.
        let root = unique_temp_dir("headroom-reordered-markers");
        fs::create_dir_all(&root).expect("create root");
        let path = root.join(".zshrc");
        fs::write(
            &path,
            "# <<< headroom:claude_code <<<\nstray old body\n# >>> headroom:claude_code >>>\n",
        )
        .expect("write malformed shell file");

        upsert_managed_block(
            &path,
            "claude_code",
            "export ANTHROPIC_BASE_URL=http://127.0.0.1:6767",
        )
        .expect("upsert over malformed block");

        let content = fs::read_to_string(&path).expect("read updated shell file");
        // Tail must be a well-ordered block: the last opening marker precedes the
        // last closing marker, and the file ends on the closing marker (not on a
        // dangling opener as the buggy slice produced).
        let last_start = content
            .rfind("# >>> headroom:claude_code >>>")
            .expect("start marker present");
        let last_end = content
            .rfind("# <<< headroom:claude_code <<<")
            .expect("end marker present");
        assert!(last_start < last_end, "tail block must be well-ordered");
        assert!(content
            .trim_end()
            .ends_with("# <<< headroom:claude_code <<<"));
        assert!(content.contains("export ANTHROPIC_BASE_URL=http://127.0.0.1:6767"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn remove_managed_block_keeps_surrounding_shell_content_intact() {
        let root = unique_temp_dir("headroom-remove-block");
        fs::create_dir_all(&root).expect("create root");
        let path = root.join(".zprofile");
        fs::write(
            &path,
            "export PATH=/usr/bin\n# >>> headroom:claude_code >>>\nexport ANTHROPIC_BASE_URL=http://127.0.0.1:6767\n# <<< headroom:claude_code <<<\nexport EDITOR=vim\n",
        )
        .expect("write shell file");

        let removed = remove_managed_block(&path, "claude_code").expect("remove managed block");

        assert!(removed);
        assert_eq!(
            fs::read_to_string(&path).expect("read cleaned shell file"),
            "export PATH=/usr/bin\nexport EDITOR=vim\n"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn shell_block_helpers_only_match_content_inside_the_named_block() {
        let root = unique_temp_dir("headroom-shell-match");
        fs::create_dir_all(&root).expect("create root");
        let path = root.join(".bashrc");
        fs::write(
            &path,
            "export ANTHROPIC_BASE_URL=https://example.com\n# >>> headroom:claude_code >>>\nexport ANTHROPIC_BASE_URL=http://127.0.0.1:6767\nexport PATH=/tmp/headroom:$PATH\n# <<< headroom:claude_code <<<\n",
        )
        .expect("write shell file");

        assert!(shell_block_contains_in_files(
            &[path.clone()],
            "claude_code",
            "ANTHROPIC_BASE_URL",
            "http://127.0.0.1:6767",
        )
        .expect("detect managed export"));
        assert!(
            shell_block_contains_text_in_files(&[path.clone()], "claude_code", "export PATH=",)
                .expect("detect managed text")
        );
        assert!(!shell_block_contains_in_files(
            &[path],
            "managed_rtk",
            "ANTHROPIC_BASE_URL",
            "http://127.0.0.1:6767",
        )
        .expect("ignore other block ids"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn write_file_if_changed_skips_backups_when_content_is_unchanged() {
        let root = unique_temp_dir("headroom-write-file");
        fs::create_dir_all(&root).expect("create root");
        let path = root.join("headroom-rtk-rewrite.sh");
        fs::write(&path, "#!/bin/sh\necho headroom\n").expect("write hook file");

        let changed = write_file_if_changed(&path, "#!/bin/sh\necho headroom\n", false)
            .expect("skip unchanged write");

        assert_eq!(changed, (false, None));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn managed_block_round_trip_preserves_realistic_zshrc_content() {
        let root = unique_temp_dir("headroom-zshrc-roundtrip");
        fs::create_dir_all(&root).expect("create root");
        let path = root.join(".zshrc");
        let original = r#"export NVM_DIR="$HOME/.nvm"
[ -s "$NVM_DIR/nvm.sh" ] && \. "$NVM_DIR/nvm.sh"

# pnpm
export PNPM_HOME="/Users/test/Library/pnpm"
case ":$PATH:" in
  *":$PNPM_HOME:"*) ;;
  *) export PATH="$PNPM_HOME:$PATH" ;;
esac

export BUN_INSTALL="$HOME/.bun"
export PATH="$BUN_INSTALL/bin:$PATH"
"#;
        fs::write(&path, original).expect("write zshrc");

        upsert_managed_block(
            &path,
            "managed_rtk",
            "export PATH=\"/tmp/headroom/bin:$PATH\"",
        )
        .expect("add managed rtk block");
        upsert_managed_block(
            &path,
            "claude_code",
            "export ANTHROPIC_BASE_URL=http://127.0.0.1:6767",
        )
        .expect("add claude block");

        remove_managed_block(&path, "claude_code").expect("remove claude block");
        remove_managed_block(&path, "managed_rtk").expect("remove managed rtk block");

        let final_content = fs::read_to_string(&path).expect("read round-tripped zshrc");
        assert_eq!(final_content, original);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn updating_one_managed_block_does_not_touch_other_blocks_or_user_content() {
        let root = unique_temp_dir("headroom-multi-block-update");
        fs::create_dir_all(&root).expect("create root");
        let path = root.join(".zprofile");
        let original = r#"eval "$(/opt/homebrew/bin/brew shellenv)"

# >>> headroom:managed_rtk >>>
export PATH="/old/headroom/bin:$PATH"
# <<< headroom:managed_rtk <<<

# >>> headroom:claude_code >>>
export ANTHROPIC_BASE_URL=http://127.0.0.1:6767
# <<< headroom:claude_code <<<

eval "$(/opt/homebrew/bin/rbenv init - zsh)"
"#;
        fs::write(&path, original).expect("write zprofile");

        upsert_managed_block(
            &path,
            "managed_rtk",
            "export PATH=\"/new/headroom/bin:$PATH\"",
        )
        .expect("update managed rtk block");

        let updated = fs::read_to_string(&path).expect("read updated zprofile");
        assert!(updated.contains("eval \"$(/opt/homebrew/bin/brew shellenv)\""));
        assert!(updated.contains("eval \"$(/opt/homebrew/bin/rbenv init - zsh)\""));
        assert!(updated.contains("export PATH=\"/new/headroom/bin:$PATH\""));
        assert!(updated.contains("export ANTHROPIC_BASE_URL=http://127.0.0.1:6767"));
        assert_eq!(updated.matches("# >>> headroom:managed_rtk >>>").count(), 1);
        assert_eq!(updated.matches("# >>> headroom:claude_code >>>").count(), 1);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn removing_one_managed_block_leaves_other_managed_blocks_and_user_content() {
        let root = unique_temp_dir("headroom-remove-single-block");
        fs::create_dir_all(&root).expect("create root");
        let path = root.join(".zshrc");
        fs::write(
            &path,
            r#"export NVM_DIR="$HOME/.nvm"
[ -s "$NVM_DIR/nvm.sh" ] && \. "$NVM_DIR/nvm.sh"

# >>> headroom:managed_rtk >>>
export PATH="/tmp/headroom/bin:$PATH"
# <<< headroom:managed_rtk <<<

# >>> headroom:claude_code >>>
export ANTHROPIC_BASE_URL=http://127.0.0.1:6767
# <<< headroom:claude_code <<<
"#,
        )
        .expect("write zshrc");

        remove_managed_block(&path, "claude_code").expect("remove claude block");

        let updated = fs::read_to_string(&path).expect("read cleaned zshrc");
        assert!(updated.contains("export NVM_DIR=\"$HOME/.nvm\""));
        assert!(updated.contains("[ -s \"$NVM_DIR/nvm.sh\" ] && \\. \"$NVM_DIR/nvm.sh\""));
        assert!(updated.contains("# >>> headroom:managed_rtk >>>"));
        assert!(updated.contains("export PATH=\"/tmp/headroom/bin:$PATH\""));
        assert!(!updated.contains("# >>> headroom:claude_code >>>"));

        let _ = fs::remove_dir_all(root);
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{nanos}"))
    }

    #[test]
    fn zdotdir_from_zshenv_parses_common_forms() {
        let cases: [(&str, Option<&str>); 6] = [
            (
                "export ZDOTDIR=\"$HOME/.config/zsh\"\n",
                Some(".config/zsh"),
            ),
            ("ZDOTDIR=~/.config/zsh\n", Some(".config/zsh")),
            (
                "export ZDOTDIR='${HOME}/dotfiles/zsh'\n",
                Some("dotfiles/zsh"),
            ),
            ("# comment\nexport ZDOTDIR=$HOME/z  # trailing\n", Some("z")),
            ("export ZDOTDIR=~\n", Some("")),
            ("# no zdotdir here\nexport FOO=bar\n", None),
        ];
        for (i, (contents, expected_tail)) in cases.into_iter().enumerate() {
            let home = unique_temp_dir(&format!("headroom-zdotdir-{i}"));
            fs::create_dir_all(&home).unwrap();
            fs::write(home.join(".zshenv"), contents).unwrap();
            let got = super::zdotdir_from_zshenv(&home);
            let expected = expected_tail.map(|tail| {
                if tail.is_empty() {
                    home.clone()
                } else {
                    home.join(tail)
                }
            });
            assert_eq!(got, expected, "case {i}");
        }
        // Missing file -> None.
        let empty = unique_temp_dir("headroom-zdotdir-none");
        fs::create_dir_all(&empty).unwrap();
        assert_eq!(super::zdotdir_from_zshenv(&empty), None);
    }

    #[test]
    fn zdotdir_unresolved_env_var_falls_back_to_none() {
        // Reproduces os error 30: `$XDG_CONFIG_HOME` unset under a Finder launch
        // must NOT yield a relative `$XDG_CONFIG_HOME/zsh` path.
        std::env::remove_var("XDG_CONFIG_HOME");
        let home = unique_temp_dir("headroom-zdotdir-unresolved");
        fs::create_dir_all(&home).unwrap();
        fs::write(
            home.join(".zshenv"),
            "export ZDOTDIR=\"$XDG_CONFIG_HOME/zsh\"\n",
        )
        .unwrap();
        assert_eq!(super::zdotdir_from_zshenv(&home), None);
    }

    #[test]
    fn strip_hook_returns_false_when_file_missing() {
        let root = unique_temp_dir("headroom-strip-missing");
        let settings = root.join("does-not-exist.json");
        let changed = strip_headroom_hook_from_settings(&settings).expect("strip should succeed");
        assert!(!changed, "missing file should report no change");
        assert!(!settings.exists(), "should not create the file");
    }

    #[test]
    fn strip_hook_removes_headroom_entry_and_leaves_other_entries() {
        let root = unique_temp_dir("headroom-strip-mixed");
        fs::create_dir_all(&root).expect("create root");
        let settings = root.join("settings.json");
        let content = json!({
            "env": { "SOME_KEY": "keep-me" },
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [
                            { "type": "command", "command": "/other/tool/script.sh" }
                        ]
                    },
                    {
                        "matcher": "Bash",
                        "hooks": [
                            {
                                "type": "command",
                                "command": "/Users/test/.claude/hooks/headroom-rtk-rewrite.sh"
                            }
                        ]
                    }
                ]
            }
        });
        fs::write(&settings, serde_json::to_string_pretty(&content).unwrap())
            .expect("write settings");

        let changed = strip_headroom_hook_from_settings(&settings).expect("strip should succeed");
        assert!(changed, "should report change");

        let raw = fs::read_to_string(&settings).expect("read settings");
        let parsed: serde_json::Value = serde_json::from_str(&raw).expect("parse settings");
        let entries = parsed
            .get("hooks")
            .and_then(|v| v.get("PreToolUse"))
            .and_then(|v| v.as_array())
            .expect("PreToolUse preserved");
        assert_eq!(entries.len(), 1, "only the non-headroom entry remains");
        assert!(
            entry_contains_hook(&entries[0], "other/tool/script.sh"),
            "unrelated entry preserved"
        );
        assert_eq!(
            parsed.get("env").and_then(|v| v.get("SOME_KEY")),
            Some(&json!("keep-me")),
            "unrelated top-level keys untouched"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn strip_hook_drops_empty_pre_tool_use_and_hooks_keys() {
        let root = unique_temp_dir("headroom-strip-empty");
        fs::create_dir_all(&root).expect("create root");
        let settings = root.join("settings.json");
        let content = json!({
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [
                            {
                                "type": "command",
                                "command": "/path/to/headroom-rtk-rewrite.sh"
                            }
                        ]
                    }
                ]
            }
        });
        fs::write(&settings, serde_json::to_string_pretty(&content).unwrap())
            .expect("write settings");

        let changed = strip_headroom_hook_from_settings(&settings).expect("strip should succeed");
        assert!(changed);

        let raw = fs::read_to_string(&settings).expect("read settings");
        let parsed: serde_json::Value = serde_json::from_str(&raw).expect("parse settings");
        assert!(
            parsed.get("hooks").is_none(),
            "empty hooks object should be removed, got {parsed}"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn strip_hook_leaves_file_untouched_when_no_headroom_entry_present() {
        let root = unique_temp_dir("headroom-strip-noop");
        fs::create_dir_all(&root).expect("create root");
        let settings = root.join("settings.json");
        let original = serde_json::to_string_pretty(&json!({
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [
                            { "type": "command", "command": "/unrelated.sh" }
                        ]
                    }
                ]
            }
        }))
        .unwrap();
        fs::write(&settings, &original).expect("write settings");

        let changed = strip_headroom_hook_from_settings(&settings).expect("strip should succeed");
        assert!(!changed, "should report no change");

        let after = fs::read_to_string(&settings).expect("read settings");
        assert_eq!(after, original, "file should be byte-identical");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn strip_hook_tolerates_empty_file() {
        let root = unique_temp_dir("headroom-strip-empty-file");
        fs::create_dir_all(&root).expect("create root");
        let settings = root.join("settings.json");
        fs::write(&settings, "").expect("write empty file");

        let changed = strip_headroom_hook_from_settings(&settings).expect("strip should succeed");
        assert!(!changed, "empty file should report no change");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn hook_script_falls_through_when_rewritten_first_token_missing_from_path() {
        // The hook has an OR guard that exits 0 when the binaries are missing,
        // so we give it real paths and verify the PATH-resolution check kicks in
        // when `rtk rewrite` produces a command whose first token can't be
        // resolved. That's the regression-prone slice added this session.
        let root = unique_temp_dir("headroom-hook-bash");
        fs::create_dir_all(&root).expect("create root");

        // Fake rtk that always prepends a made-up binary name that won't be on PATH.
        let fake_rtk = root.join("fake-rtk");
        fs::write(
            &fake_rtk,
            "#!/usr/bin/env bash\nshift  # drop the 'rewrite' arg\necho \"__headroom_nonexistent_binary_xyzzy__ $*\"\n",
        )
        .expect("write fake rtk");
        fs::set_permissions(
            &fake_rtk,
            <fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
        )
        .expect("chmod rtk");

        // Use the real system python3 so the embedded Python snippets run.
        let system_python = PathBuf::from("/usr/bin/python3");
        assert!(system_python.exists(), "this test assumes /usr/bin/python3");

        let hook_body = build_headroom_rtk_hook(&fake_rtk, &system_python);
        let hook_path = root.join("hook.sh");
        fs::write(&hook_path, &hook_body).expect("write hook");
        fs::set_permissions(
            &hook_path,
            <fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
        )
        .expect("chmod hook");

        // Hook expects a JSON object on stdin with tool_input.command.
        let stdin = r#"{"tool_input":{"command":"git status"}}"#;
        let output = std::process::Command::new("bash")
            .arg(&hook_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                child
                    .stdin
                    .as_mut()
                    .unwrap()
                    .write_all(stdin.as_bytes())
                    .unwrap();
                child.wait_with_output()
            })
            .expect("run hook");

        assert!(output.status.success(), "hook should exit 0");
        assert!(
            output.stdout.is_empty(),
            "hook should emit no rewrite when first token isn't resolvable, got: {:?}",
            String::from_utf8_lossy(&output.stdout)
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn hook_script_passes_through_check_commands() {
        // `rtk git diff --check` swallows the whitespace report; the hook must
        // leave any --check command unrewritten even when rtk would rewrite it.
        let root = unique_temp_dir("headroom-hook-check");
        fs::create_dir_all(&root).expect("create root");

        let fake_rtk = root.join("fake-rtk");
        fs::write(
            &fake_rtk,
            "#!/usr/bin/env bash\nshift\necho \"/bin/echo $*\"\n",
        )
        .expect("write fake rtk");
        fs::set_permissions(
            &fake_rtk,
            <fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
        )
        .expect("chmod rtk");

        let system_python = PathBuf::from("/usr/bin/python3");
        let hook_body = build_headroom_rtk_hook(&fake_rtk, &system_python);
        let hook_path = root.join("hook.sh");
        fs::write(&hook_path, &hook_body).expect("write hook");
        fs::set_permissions(
            &hook_path,
            <fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
        )
        .expect("chmod hook");

        for cmd in ["git diff --cached --check", "git diff --check"] {
            let stdin = format!(r#"{{"tool_input":{{"command":"{cmd}"}}}}"#);
            let output = std::process::Command::new("bash")
                .arg(&hook_path)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .and_then(|mut child| {
                    use std::io::Write;
                    child
                        .stdin
                        .as_mut()
                        .unwrap()
                        .write_all(stdin.as_bytes())
                        .unwrap();
                    child.wait_with_output()
                })
                .expect("run hook");
            assert!(output.status.success(), "hook should exit 0 for {cmd}");
            assert!(
                output.stdout.is_empty(),
                "hook must not rewrite {cmd}, got: {:?}",
                String::from_utf8_lossy(&output.stdout)
            );
        }

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn hook_script_emits_rewrite_when_first_token_is_valid_absolute_path() {
        let root = unique_temp_dir("headroom-hook-bash-ok");
        fs::create_dir_all(&root).expect("create root");

        // Pick a binary that definitely exists on macOS/Linux test hosts.
        let real_binary = "/bin/echo";
        assert!(Path::new(real_binary).exists());

        // Fake rtk rewrites to use an absolute path that *does* exist.
        let fake_rtk = root.join("fake-rtk");
        fs::write(
            &fake_rtk,
            format!("#!/usr/bin/env bash\nshift\necho \"{real_binary} $*\"\n"),
        )
        .expect("write fake rtk");
        fs::set_permissions(
            &fake_rtk,
            <fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
        )
        .expect("chmod rtk");

        let system_python = PathBuf::from("/usr/bin/python3");
        let hook_body = build_headroom_rtk_hook(&fake_rtk, &system_python);
        let hook_path = root.join("hook.sh");
        fs::write(&hook_path, &hook_body).expect("write hook");
        fs::set_permissions(
            &hook_path,
            <fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
        )
        .expect("chmod hook");

        let stdin = r#"{"tool_input":{"command":"git status"}}"#;
        let output = std::process::Command::new("bash")
            .arg(&hook_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                child
                    .stdin
                    .as_mut()
                    .unwrap()
                    .write_all(stdin.as_bytes())
                    .unwrap();
                child.wait_with_output()
            })
            .expect("run hook");

        assert!(output.status.success(), "hook should exit 0");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains(real_binary),
            "rewrite should be emitted when first token is a valid absolute path, got stdout: {stdout:?}, stderr: {:?}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            stdout.contains("Headroom RTK auto-rewrite"),
            "should be a rewrite hookSpecificOutput payload"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn hook_script_pins_bare_rtk_token_to_managed_absolute_path() {
        let root = unique_temp_dir("headroom-hook-pin-rtk");
        fs::create_dir_all(&root).expect("create root");

        // Fake rtk emits a bare `rtk` leading token, like the real binary.
        // `rtk` is NOT on PATH here, so without pinning the rewrite would be a
        // "command not found" landmine and the defense-in-depth guard would
        // drop it. Pinning to the managed absolute path must keep the rewrite.
        let fake_rtk = root.join("rtk");
        fs::write(&fake_rtk, "#!/usr/bin/env bash\nshift\necho \"rtk $*\"\n")
            .expect("write fake rtk");
        fs::set_permissions(
            &fake_rtk,
            <fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
        )
        .expect("chmod rtk");

        let system_python = PathBuf::from("/usr/bin/python3");
        let hook_body = build_headroom_rtk_hook(&fake_rtk, &system_python);
        let hook_path = root.join("hook.sh");
        fs::write(&hook_path, &hook_body).expect("write hook");
        fs::set_permissions(
            &hook_path,
            <fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
        )
        .expect("chmod hook");

        let stdin = r#"{"tool_input":{"command":"git status"}}"#;
        let output = std::process::Command::new("bash")
            .arg(&hook_path)
            .env("PATH", "/usr/bin:/bin") // ensure bare `rtk` is unresolvable
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                child
                    .stdin
                    .as_mut()
                    .unwrap()
                    .write_all(stdin.as_bytes())
                    .unwrap();
                child.wait_with_output()
            })
            .expect("run hook");

        assert!(output.status.success(), "hook should exit 0");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("Headroom RTK auto-rewrite"),
            "rewrite should survive when bare `rtk` is pinned to absolute path, got stdout: {stdout:?}, stderr: {:?}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            stdout.contains(&fake_rtk.to_string_lossy().replace('"', "\\\"")),
            "rewritten command should invoke the managed rtk by absolute path, got: {stdout:?}"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn hook_script_prepends_managed_path_so_embedded_rtk_resolves() {
        // Regression for compound commands: `rtk rewrite` embeds a bare `rtk`
        // after `&&`/`;`/`|`, which the leading-token pin never touches. The
        // hook must prepend the managed bin dir to PATH so the embedded token
        // resolves in the non-interactive, non-login shell Claude Code spawns.
        let root = unique_temp_dir("headroom-hook-embedded-rtk");
        fs::create_dir_all(&root).expect("create root");

        // Fake rtk emits a compound command with rtk embedded mid-chain, like
        // the real binary does for `cd x && <cmd>`. The leading token is `cd`.
        let fake_rtk = root.join("rtk");
        fs::write(
            &fake_rtk,
            "#!/usr/bin/env bash\nshift\necho \"cd /tmp && rtk $*\"\n",
        )
        .expect("write fake rtk");
        fs::set_permissions(
            &fake_rtk,
            <fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
        )
        .expect("chmod rtk");

        let system_python = PathBuf::from("/usr/bin/python3");
        let hook_body = build_headroom_rtk_hook(&fake_rtk, &system_python);
        let hook_path = root.join("hook.sh");
        fs::write(&hook_path, &hook_body).expect("write hook");
        fs::set_permissions(
            &hook_path,
            <fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
        )
        .expect("chmod hook");

        let stdin = r#"{"tool_input":{"command":"git status"}}"#;
        let output = std::process::Command::new("bash")
            .arg(&hook_path)
            .env("PATH", "/usr/bin:/bin") // bare `rtk` unresolvable without the prepend
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                child
                    .stdin
                    .as_mut()
                    .unwrap()
                    .write_all(stdin.as_bytes())
                    .unwrap();
                child.wait_with_output()
            })
            .expect("run hook");

        assert!(output.status.success(), "hook should exit 0");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("Headroom RTK auto-rewrite"),
            "compound rewrite should be emitted, got stdout: {stdout:?}, stderr: {:?}",
            String::from_utf8_lossy(&output.stderr)
        );
        // The emitted command must export the managed bin dir onto PATH so the
        // embedded `rtk` resolves, and must preserve that embedded token.
        assert!(
            stdout.contains("export PATH="),
            "rewrite must prepend a PATH export, got: {stdout:?}"
        );
        assert!(
            stdout.contains(&root.to_string_lossy().replace('"', "\\\"")),
            "PATH export must point at the managed bin dir, got: {stdout:?}"
        );
        assert!(
            stdout.contains("&& rtk "),
            "embedded rtk token must be preserved, got: {stdout:?}"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn hook_script_emits_rewrite_even_when_rtk_rewrite_exits_nonzero() {
        let root = unique_temp_dir("headroom-hook-bash-nonzero");
        fs::create_dir_all(&root).expect("create root");

        let real_binary = "/bin/echo";
        assert!(Path::new(real_binary).exists());

        // Match the real rtk behavior we observed during smoke testing:
        // emit a rewrite, then exit non-zero. The hook's `|| true` should
        // still preserve the rewritten command.
        let fake_rtk = root.join("fake-rtk");
        fs::write(
            &fake_rtk,
            format!("#!/usr/bin/env bash\nshift\necho \"{real_binary} $*\"\nexit 3\n"),
        )
        .expect("write fake rtk");
        fs::set_permissions(
            &fake_rtk,
            <fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
        )
        .expect("chmod rtk");

        let system_python = PathBuf::from("/usr/bin/python3");
        let hook_body = build_headroom_rtk_hook(&fake_rtk, &system_python);
        let hook_path = root.join("hook.sh");
        fs::write(&hook_path, &hook_body).expect("write hook");
        fs::set_permissions(
            &hook_path,
            <fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
        )
        .expect("chmod hook");

        let stdin = r#"{"tool_input":{"command":"git status"}}"#;
        let output = std::process::Command::new("bash")
            .arg(&hook_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                child
                    .stdin
                    .as_mut()
                    .unwrap()
                    .write_all(stdin.as_bytes())
                    .unwrap();
                child.wait_with_output()
            })
            .expect("run hook");

        assert!(output.status.success(), "hook should exit 0");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains(real_binary),
            "rewrite output should survive non-zero RTK exit, got stdout: {stdout:?}, stderr: {:?}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            stdout.contains("Headroom RTK auto-rewrite"),
            "should still emit a rewrite hookSpecificOutput payload"
        );

        let _ = fs::remove_dir_all(root);
    }

    // ── Lifecycle integration tests ──────────────────────────────────────────
    //
    // These tests drive `apply_client_setup` / `verify_client_setup` /
    // `disable_client_setup` / `clear_client_setups` against a temp $HOME so we
    // catch regressions in the user-visible setup-then-teardown flow. Tests are
    // serialized via `serial_test` because they mutate process-wide env vars
    // (HOME, XDG_DATA_HOME, SHELL).

    /// RAII-style guard that snapshots HOME / XDG_DATA_HOME / SHELL, points
    /// them at a fresh tempdir, and restores them on drop. Used to keep
    /// lifecycle tests from touching the developer's real profile.
    struct TestHome {
        _tmp: tempfile::TempDir,
        home: PathBuf,
        prev_home: Option<std::ffi::OsString>,
        prev_xdg: Option<std::ffi::OsString>,
        prev_shell: Option<std::ffi::OsString>,
        prev_codex: Option<std::ffi::OsString>,
        prev_zdotdir: Option<std::ffi::OsString>,
        // Held for the guard's lifetime: env vars are process-global, so two
        // TestHome tests running on parallel threads corrupt each other's HOME
        // (and can leak writes into the developer's real profile). serial_test
        // only covers tests that opted in; this lock covers every TestHome user.
        _env_lock: std::sync::MutexGuard<'static, ()>,
    }

    impl TestHome {
        fn new() -> Self {
            let env_lock = crate::test_env_lock::HOME_ENV_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let tmp = tempfile::tempdir().expect("create temp home");
            let home = tmp.path().to_path_buf();
            let prev_home = std::env::var_os("HOME");
            let prev_xdg = std::env::var_os("XDG_DATA_HOME");
            let prev_shell = std::env::var_os("SHELL");
            let prev_codex = std::env::var_os("CODEX_HOME");
            let prev_zdotdir = std::env::var_os("ZDOTDIR");
            std::env::set_var("HOME", &home);
            std::env::set_var("XDG_DATA_HOME", home.join(".local").join("share"));
            // Force a deterministic shell family so tests don't depend on the
            // dev's login shell.
            std::env::set_var("SHELL", "/bin/zsh");
            // Clear any real CODEX_HOME so codex_home() falls back to the temp
            // $HOME/.codex and the Codex tests stay hermetic on dev machines.
            std::env::remove_var("CODEX_HOME");
            // Clear any real ZDOTDIR so zsh_dir() resolves against the temp
            // $HOME and the shell-block tests stay hermetic on dev machines.
            std::env::remove_var("ZDOTDIR");
            // Mirror what the app does at startup so write_setup_state has a
            // config dir to land in.
            crate::storage::ensure_data_dirs(&crate::storage::app_data_dir())
                .expect("ensure_data_dirs in test home");
            TestHome {
                _tmp: tmp,
                home,
                prev_home,
                prev_xdg,
                prev_shell,
                prev_codex,
                prev_zdotdir,
                _env_lock: env_lock,
            }
        }

        fn path(&self) -> &Path {
            &self.home
        }
    }

    impl Drop for TestHome {
        fn drop(&mut self) {
            match self.prev_home.take() {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
            match self.prev_xdg.take() {
                Some(v) => std::env::set_var("XDG_DATA_HOME", v),
                None => std::env::remove_var("XDG_DATA_HOME"),
            }
            match self.prev_shell.take() {
                Some(v) => std::env::set_var("SHELL", v),
                None => std::env::remove_var("SHELL"),
            }
            match self.prev_codex.take() {
                Some(v) => std::env::set_var("CODEX_HOME", v),
                None => std::env::remove_var("CODEX_HOME"),
            }
            match self.prev_zdotdir.take() {
                Some(v) => std::env::set_var("ZDOTDIR", v),
                None => std::env::remove_var("ZDOTDIR"),
            }
        }
    }

    /// RTK is opt-in: its PATH block and Claude Code hook are only wired when the
    /// managed binary exists on disk. Drop a fake one at the default location so
    /// tests covering a fully-configured environment exercise the RTK wiring.
    fn seed_installed_rtk() {
        let rtk = super::default_headroom_rtk_path();
        fs::create_dir_all(rtk.parent().unwrap()).unwrap();
        fs::write(&rtk, "#!/bin/sh\n").unwrap();
    }

    fn read_settings_json(path: &Path) -> serde_json::Value {
        let raw = fs::read_to_string(path).expect("read settings.json");
        serde_json::from_str(&raw).expect("parse settings.json")
    }

    #[test]
    #[serial_test::serial]
    fn apply_then_verify_claude_code_writes_expected_files() {
        let home = TestHome::new();
        // Seed an empty zshrc/zshenv so the shell-block writers have files to
        // edit and don't depend on the dev's real shell config layout.
        fs::write(home.path().join(".zshrc"), "# user zshrc\n").unwrap();
        fs::write(home.path().join(".zshenv"), "# user zshenv\n").unwrap();
        fs::create_dir_all(home.path().join(".claude")).unwrap();
        fs::write(
            home.path().join(".claude").join("settings.json"),
            r#"{"hooks": {}}"#,
        )
        .unwrap();
        seed_installed_rtk();

        let result = super::apply_client_setup("claude_code").expect("apply_client_setup succeeds");
        assert!(result.applied);
        assert_eq!(result.client_id, "claude_code");

        // Hook script and settings.json hook entry must be present.
        let hook_path = home
            .path()
            .join(".claude")
            .join("hooks")
            .join("headroom-rtk-rewrite.sh");
        assert!(hook_path.exists(), "hook script written to disk");
        let hook_contents = fs::read_to_string(&hook_path).unwrap();
        assert!(
            hook_contents.starts_with("#!/usr/bin/env bash"),
            "hook has expected shebang"
        );

        let settings = read_settings_json(&home.path().join(".claude").join("settings.json"));
        assert_eq!(
            settings["env"]["ANTHROPIC_BASE_URL"].as_str(),
            Some("http://127.0.0.1:6767"),
            "claude settings.json points env at headroom proxy"
        );
        let pre_tool_use = &settings["hooks"]["PreToolUse"];
        assert!(
            pre_tool_use.is_array() && !pre_tool_use.as_array().unwrap().is_empty(),
            "PreToolUse hook entry exists, got: {settings}"
        );

        // Shell block in zshenv (or whichever profile the writer chose) should
        // export ANTHROPIC_BASE_URL pointing at the loopback proxy.
        let zshrc = fs::read_to_string(home.path().join(".zshrc")).unwrap();
        let zshenv = fs::read_to_string(home.path().join(".zshenv")).unwrap();
        let combined = format!("{zshrc}\n{zshenv}");
        assert!(
            combined.contains("ANTHROPIC_BASE_URL=http://127.0.0.1:6767"),
            "ANTHROPIC_BASE_URL exported from a managed shell block, got:\n{combined}"
        );

        // verify_client_setup should report all the configured checks.
        // Proxy reachability is reported via `proxy_reachable` only, so a
        // missing proxy in the test environment no longer flips `verified`.
        let verification =
            super::verify_client_setup("claude_code").expect("verify_client_setup succeeds");
        assert_eq!(verification.client_id, "claude_code");
        assert!(
            verification
                .checks
                .iter()
                .any(|c| c.contains("ANTHROPIC_BASE_URL")),
            "verification reports the env check, got: {:?}",
            verification.checks
        );
        assert!(
            verification
                .checks
                .iter()
                .any(|c| c.contains("RTK Claude hook")),
            "verification reports the hook check, got: {:?}",
            verification.checks
        );
    }

    #[test]
    #[serial_test::serial]
    fn apply_claude_writes_guard_and_disable_preserves_env_and_user_hooks() {
        let home = TestHome::new();
        fs::write(home.path().join(".zshrc"), "# user zshrc\n").unwrap();
        fs::write(home.path().join(".zshenv"), "# user zshenv\n").unwrap();
        fs::create_dir_all(home.path().join(".claude")).unwrap();
        // Pre-existing user-authored hook that must survive apply and disable.
        fs::write(
            home.path().join(".claude").join("settings.json"),
            r#"{"hooks":{"UserPromptSubmit":[{"hooks":[{"type":"command","command":"echo mine"}]}]}}"#,
        )
        .unwrap();
        seed_installed_rtk();

        super::apply_client_setup("claude_code").expect("first apply");
        super::apply_client_setup("claude_code").expect("second apply");

        let script = home
            .path()
            .join(".claude")
            .join("hooks")
            .join("headroom-claude-guard.py");
        assert!(script.exists(), "guard script written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&script).unwrap().permissions().mode();
            assert!(
                mode & 0o111 != 0,
                "guard script is executable, got {mode:o}"
            );
        }

        let settings_path = home.path().join(".claude").join("settings.json");
        let settings = read_settings_json(&settings_path);
        let command = format!("/usr/bin/python3 {}", script.display());
        let guard_count = |event: &str| {
            settings["hooks"][event]
                .as_array()
                .unwrap()
                .iter()
                .filter(|entry| {
                    entry["hooks"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|h| h["command"] == serde_json::Value::String(command.clone()))
                })
                .count()
        };
        // Guard registered exactly once on SessionStart despite the double-apply.
        assert_eq!(
            guard_count("SessionStart"),
            1,
            "guard registered once for SessionStart, got:\n{settings:#}"
        );
        // Never on UserPromptSubmit: exit 2 there blocks every prompt in Claude
        // Desktop / Cowork VM sessions that can't reach the app.
        assert_eq!(
            guard_count("UserPromptSubmit"),
            0,
            "guard must not register on UserPromptSubmit, got:\n{settings:#}"
        );
        assert_eq!(
            settings["hooks"]["SessionStart"][0]["matcher"],
            "startup|resume|clear|compact"
        );

        super::disable_client_setup("claude_code").expect("disable");

        assert!(!script.exists(), "guard script removed on disable");
        let after = read_settings_json(&settings_path);
        let after_str = serde_json::to_string(&after).unwrap();
        assert!(
            !after_str.contains("headroom-claude-guard.py"),
            "guard stripped from settings.json, got:\n{after:#}"
        );
        assert!(
            after_str.contains("echo mine"),
            "user-authored hook preserved, got:\n{after:#}"
        );
        // settings.json must NOT be deleted even if it were otherwise empty.
        assert!(settings_path.exists(), "settings.json preserved on disable");
    }

    #[test]
    #[serial_test::serial]
    fn apply_migrates_guard_off_user_prompt_submit() {
        let home = TestHome::new();
        fs::write(home.path().join(".zshrc"), "# user zshrc\n").unwrap();
        fs::write(home.path().join(".zshenv"), "# user zshenv\n").unwrap();
        fs::create_dir_all(home.path().join(".claude")).unwrap();
        // An older build registered the guard on UserPromptSubmit, where exit 2
        // blocks every prompt in Claude Desktop / Cowork VM sessions. A user
        // hook on the same event must survive the migration.
        let script = home
            .path()
            .join(".claude")
            .join("hooks")
            .join("headroom-claude-guard.py");
        fs::write(
            home.path().join(".claude").join("settings.json"),
            format!(
                r#"{{"hooks":{{
                    "SessionStart":[{{"matcher":"startup|resume|clear|compact","hooks":[{{"type":"command","command":"/usr/bin/python3 {script}"}}]}}],
                    "UserPromptSubmit":[
                        {{"hooks":[{{"type":"command","command":"/usr/bin/python3 {script}"}}]}},
                        {{"hooks":[{{"type":"command","command":"echo mine"}}]}}
                    ]
                }}}}"#,
                script = script.display()
            ),
        )
        .unwrap();
        seed_installed_rtk();

        super::apply_client_setup("claude_code").expect("apply");

        let settings = read_settings_json(&home.path().join(".claude").join("settings.json"));
        let ups = serde_json::to_string(&settings["hooks"]["UserPromptSubmit"]).unwrap();
        assert!(
            !ups.contains("headroom-claude-guard.py"),
            "guard stripped from UserPromptSubmit, got:\n{settings:#}"
        );
        assert!(
            ups.contains("echo mine"),
            "user-authored UserPromptSubmit hook preserved, got:\n{settings:#}"
        );
        let ss = serde_json::to_string(&settings["hooks"]["SessionStart"]).unwrap();
        assert!(
            ss.contains("headroom-claude-guard.py"),
            "guard still registered on SessionStart, got:\n{settings:#}"
        );
    }

    #[test]
    #[serial_test::serial]
    fn full_cleanup_strips_base_url_and_guard_when_shell_block_removal_fails() {
        // Regression: perform_full_cleanup used to remove ANTHROPIC_BASE_URL and
        // the guard hook ONLY via clear_client_setups -> disable_client_setup,
        // where remove_shell_block runs first under `?`. A failure there left the
        // routing env and the guard hook in place, both of which brick Claude
        // once the proxy is gone. Force that failure and confirm cleanup still
        // strips them.
        let home = TestHome::new();
        fs::write(home.path().join(".zshrc"), "# user zshrc\n").unwrap();
        fs::write(home.path().join(".zshenv"), "# user zshenv\n").unwrap();
        fs::create_dir_all(home.path().join(".claude")).unwrap();
        fs::write(
            home.path().join(".claude").join("settings.json"),
            r#"{"hooks": {}}"#,
        )
        .unwrap();
        seed_installed_rtk();

        super::apply_client_setup("claude_code").expect("apply");

        let settings_path = home.path().join(".claude").join("settings.json");
        let guard_script = home
            .path()
            .join(".claude")
            .join("hooks")
            .join("headroom-claude-guard.py");
        assert_eq!(
            read_settings_json(&settings_path)["env"]["ANTHROPIC_BASE_URL"].as_str(),
            Some("http://127.0.0.1:6767"),
            "precondition: base url wired"
        );
        assert!(guard_script.exists(), "precondition: guard script written");

        // Sabotage a shell target: a directory where remove_managed_block expects
        // a file makes read_to_string fail, so disable_client_setup("claude_code")
        // bails before it reaches base-url / guard removal.
        let zshrc = home.path().join(".zshrc");
        fs::remove_file(&zshrc).unwrap();
        fs::create_dir(&zshrc).unwrap();

        super::perform_full_cleanup();

        assert!(settings_path.exists(), "settings.json preserved");
        let after = read_settings_json(&settings_path);
        assert!(
            after["env"]["ANTHROPIC_BASE_URL"].is_null(),
            "base url stripped despite shell-block failure, got:\n{after:#}"
        );
        assert!(
            !serde_json::to_string(&after["hooks"])
                .unwrap()
                .contains("headroom-claude-guard.py"),
            "guard hook stripped despite shell-block failure, got:\n{after:#}"
        );
        assert!(!guard_script.exists(), "guard script deleted");
    }

    #[test]
    #[serial_test::serial]
    fn apply_preserves_and_disable_restores_custom_base_url() {
        let home = TestHome::new();
        fs::write(home.path().join(".zshrc"), "# user zshrc\n").unwrap();
        fs::write(home.path().join(".zshenv"), "# user zshenv\n").unwrap();
        fs::create_dir_all(home.path().join(".claude")).unwrap();
        // A corporate gateway already routes Claude Code before Headroom.
        let gateway = "https://gateway.corp.example/anthropic";
        fs::write(
            home.path().join(".claude").join("settings.json"),
            format!(r#"{{"env":{{"ANTHROPIC_BASE_URL":"{gateway}"}}}}"#),
        )
        .unwrap();
        seed_installed_rtk();

        let result = super::apply_client_setup("claude_code").expect("apply");
        // Setup captured the gateway and told the caller it took over routing.
        assert_eq!(result.replaced_base_url.as_deref(), Some(gateway));
        let settings_path = home.path().join(".claude").join("settings.json");
        let after_apply = read_settings_json(&settings_path);
        assert_eq!(
            after_apply["env"]["ANTHROPIC_BASE_URL"],
            serde_json::Value::String(super::HEADROOM_ANTHROPIC_BASE_URL.to_string())
        );
        assert_eq!(
            super::load_setup_state().preserved_base_urls["claude_code"],
            gateway
        );

        super::disable_client_setup("claude_code").expect("disable");
        // The gateway URL is restored, not deleted.
        let after_disable = read_settings_json(&settings_path);
        assert_eq!(
            after_disable["env"]["ANTHROPIC_BASE_URL"],
            serde_json::Value::String(gateway.to_string()),
            "custom base URL restored on disable, got:\n{after_disable:#}"
        );
        assert!(
            !super::load_setup_state()
                .preserved_base_urls
                .contains_key("claude_code"),
            "preserved entry consumed after restore"
        );
    }

    #[test]
    #[serial_test::serial]
    fn apply_without_custom_base_url_does_not_report_takeover() {
        let home = TestHome::new();
        fs::write(home.path().join(".zshrc"), "# user zshrc\n").unwrap();
        fs::write(home.path().join(".zshenv"), "# user zshenv\n").unwrap();
        seed_installed_rtk();

        let result = super::apply_client_setup("claude_code").expect("apply");
        assert!(result.replaced_base_url.is_none());
        assert!(super::load_setup_state().preserved_base_urls.is_empty());

        // Disable deletes the key (nothing to restore).
        super::disable_client_setup("claude_code").expect("disable");
        let settings_path = home.path().join(".claude").join("settings.json");
        if settings_path.exists() {
            let after = read_settings_json(&settings_path);
            assert!(after["env"]["ANTHROPIC_BASE_URL"].is_null());
        }
    }

    #[test]
    #[serial_test::serial]
    fn verify_claude_code_passes_when_rtk_deliberately_disabled() {
        let home = TestHome::new();
        fs::write(home.path().join(".zshrc"), "# user zshrc\n").unwrap();
        fs::write(home.path().join(".zshenv"), "# user zshenv\n").unwrap();
        fs::create_dir_all(home.path().join(".claude")).unwrap();
        fs::write(
            home.path().join(".claude").join("settings.json"),
            r#"{"hooks": {}}"#,
        )
        .unwrap();

        super::apply_client_setup("claude_code").expect("apply_client_setup succeeds");

        // User turns RTK off: this strips the RTK PATH block + hook but leaves
        // ANTHROPIC_BASE_URL routing intact, and persists the opt-out.
        super::set_rtk_enabled(false, home.path(), home.path()).expect("disable RTK");

        let hook_path = home
            .path()
            .join(".claude")
            .join("hooks")
            .join("headroom-rtk-rewrite.sh");
        assert!(!hook_path.exists(), "RTK hook removed when RTK disabled");

        // Routing config is still present, so Claude Code must verify green
        // even though the RTK pieces are gone.
        let verification =
            super::verify_client_setup("claude_code").expect("verify_client_setup succeeds");
        assert!(
            verification.verified,
            "claude_code verifies on routing alone when RTK is disabled, failures: {:?}",
            verification.failures
        );
        assert!(
            verification.failures.iter().all(|f| !f.contains("RTK")),
            "no RTK failures reported when RTK is disabled, got: {:?}",
            verification.failures
        );
    }

    #[test]
    #[serial_test::serial]
    fn verify_claude_code_passes_when_rtk_not_installed() {
        let home = TestHome::new();
        fs::write(home.path().join(".zshrc"), "# user zshrc\n").unwrap();
        fs::write(home.path().join(".zshenv"), "# user zshenv\n").unwrap();
        fs::create_dir_all(home.path().join(".claude")).unwrap();
        fs::write(
            home.path().join(".claude").join("settings.json"),
            r#"{"hooks": {}}"#,
        )
        .unwrap();

        // Clean install with RTK auto-install removed: routing is configured but
        // the managed RTK binary was never dropped on disk and the user never
        // toggled RTK off (rtk_disabled stays false). Claude Code must still
        // verify green on routing alone.
        super::apply_client_setup("claude_code").expect("apply_client_setup succeeds");

        assert!(
            !super::default_headroom_rtk_path().exists(),
            "RTK binary must be absent for this test"
        );
        let state = super::load_setup_state();
        assert!(
            !state.rtk_disabled,
            "rtk_disabled stays false when untoggled"
        );

        let verification =
            super::verify_client_setup("claude_code").expect("verify_client_setup succeeds");
        assert!(
            verification.verified,
            "claude_code verifies on routing alone when RTK isn't installed, failures: {:?}",
            verification.failures
        );
        assert!(
            verification.failures.iter().all(|f| !f.contains("RTK")),
            "no RTK failures reported when RTK isn't installed, got: {:?}",
            verification.failures
        );
    }

    #[test]
    #[serial_test::serial]
    fn ensure_rtk_integrations_writes_codex_nudge_and_disable_removes_it() {
        let home = TestHome::new();
        fs::write(home.path().join(".zshrc"), "# user zshrc\n").unwrap();
        fs::write(home.path().join(".zshenv"), "# user zshenv\n").unwrap();
        fs::create_dir_all(home.path().join(".claude")).unwrap();
        fs::write(home.path().join(".claude").join("settings.json"), "{}").unwrap();

        // Mark Codex as a configured client so the AGENTS.md nudge path runs.
        let mut state = super::load_setup_state();
        state
            .configured_clients
            .insert("codex_cli".into(), "now".into());
        super::write_setup_state(&state).unwrap();

        // Fake managed rtk + python binaries so the binary-present guard passes.
        let bin_dir = home.path().join("managed-bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let rtk = bin_dir.join("rtk");
        fs::write(&rtk, "#!/bin/sh\n").unwrap();
        let python = bin_dir.join("python3");
        fs::write(&python, "#!/bin/sh\n").unwrap();

        super::ensure_rtk_integrations(&rtk, &python).expect("ensure_rtk_integrations");

        let agents = home.path().join(".codex").join("AGENTS.md");
        let body = fs::read_to_string(&agents).expect("AGENTS.md written");
        assert!(
            body.contains("Headroom RTK"),
            "nudge heading present: {body}"
        );
        assert!(
            body.contains(&rtk.display().to_string()),
            "nudge references the managed rtk path: {body}"
        );

        // Disabling RTK must remove the managed block.
        super::set_rtk_enabled(false, &rtk, &python).expect("disable rtk");
        let after = fs::read_to_string(&agents).unwrap_or_default();
        assert!(
            !after.contains("Headroom RTK"),
            "nudge removed on disable: {after}"
        );
    }

    #[test]
    #[serial_test::serial]
    fn apply_claude_code_is_byte_idempotent() {
        // Regression: a second apply used to add blank-line padding between
        // managed blocks, so byte-exact idempotency now holds and is
        // asserted here.
        let home = TestHome::new();
        fs::write(home.path().join(".zshrc"), "# user zshrc\n").unwrap();
        fs::write(home.path().join(".zshenv"), "# user zshenv\n").unwrap();
        seed_installed_rtk();

        super::apply_client_setup("claude_code").expect("first apply");
        let zshrc_after_first = fs::read_to_string(home.path().join(".zshrc")).unwrap();
        let zshenv_after_first = fs::read_to_string(home.path().join(".zshenv")).unwrap();
        let settings_after_first =
            fs::read_to_string(home.path().join(".claude").join("settings.json")).unwrap();
        let hook_after_first = fs::read_to_string(
            home.path()
                .join(".claude")
                .join("hooks")
                .join("headroom-rtk-rewrite.sh"),
        )
        .unwrap();

        super::apply_client_setup("claude_code").expect("second apply");
        let zshrc_after_second = fs::read_to_string(home.path().join(".zshrc")).unwrap();
        let zshenv_after_second = fs::read_to_string(home.path().join(".zshenv")).unwrap();
        let settings_after_second =
            fs::read_to_string(home.path().join(".claude").join("settings.json")).unwrap();
        let hook_after_second = fs::read_to_string(
            home.path()
                .join(".claude")
                .join("hooks")
                .join("headroom-rtk-rewrite.sh"),
        )
        .unwrap();

        assert_eq!(zshrc_after_first, zshrc_after_second, "zshrc byte-stable");
        assert_eq!(
            zshenv_after_first, zshenv_after_second,
            "zshenv byte-stable"
        );
        assert_eq!(
            settings_after_first, settings_after_second,
            "settings.json byte-stable"
        );
        assert_eq!(
            hook_after_first, hook_after_second,
            "hook script byte-stable"
        );

        // Sanity: each managed block still appears exactly once.
        let combined = format!("{zshrc_after_second}\n{zshenv_after_second}");
        assert_eq!(
            combined.matches("# >>> headroom:claude_code >>>").count(),
            1
        );
        assert_eq!(
            combined.matches("# >>> headroom:managed_rtk >>>").count(),
            1
        );
    }

    #[test]
    #[serial_test::serial]
    fn disable_then_clear_claude_code_removes_traces() {
        let home = TestHome::new();
        fs::write(home.path().join(".zshrc"), "# user zshrc\n").unwrap();
        fs::write(home.path().join(".zshenv"), "# user zshenv\n").unwrap();
        seed_installed_rtk();

        super::apply_client_setup("claude_code").expect("apply");
        let hook_path = home
            .path()
            .join(".claude")
            .join("hooks")
            .join("headroom-rtk-rewrite.sh");
        assert!(hook_path.exists(), "hook present after apply");

        super::disable_client_setup("claude_code").expect("disable");

        // Hook script removed.
        assert!(!hook_path.exists(), "hook removed after disable");

        // Shell blocks removed.
        let zshrc = fs::read_to_string(home.path().join(".zshrc")).unwrap();
        let zshenv = fs::read_to_string(home.path().join(".zshenv")).unwrap();
        let combined = format!("{zshrc}\n{zshenv}");
        assert!(
            !combined.contains("ANTHROPIC_BASE_URL=http://127.0.0.1:6767"),
            "ANTHROPIC_BASE_URL export removed, got:\n{combined}"
        );

        // settings.json no longer points env at the proxy and no longer carries
        // the Headroom hook entry.
        let settings = read_settings_json(&home.path().join(".claude").join("settings.json"));
        assert!(
            settings["env"]["ANTHROPIC_BASE_URL"].is_null(),
            "ANTHROPIC_BASE_URL stripped from settings.json env, got: {settings}"
        );
        let still_has_headroom_hook =
            claude_hook_present_in_value(&settings, "headroom-rtk-rewrite.sh");
        assert!(
            !still_has_headroom_hook,
            "Headroom hook entry stripped from settings.json, got: {settings}"
        );

        // clear_client_setups runs disable across all clients without error,
        // and the setup state file is left without a `claude_code` entry.
        super::clear_client_setups().expect("clear");
        let post = super::load_setup_state();
        assert!(
            post.configured_clients.get("claude_code").is_none(),
            "claude_code dropped from configured_clients, got: {:?}",
            post.configured_clients
        );
    }

    #[test]
    #[serial_test::serial]
    fn clear_client_setups_twice_preserves_remembered_snapshot() {
        // Regression: pause (first clear) moves configured -> remembered; the
        // quit-time second clear used to wipe remembered_clients because the
        // re-save was skipped while configured was empty — so a pause
        // followed by Cmd-Q permanently lost every connector.
        let home = TestHome::new();
        fs::write(home.path().join(".zshrc"), "# user zshrc\n").unwrap();
        fs::write(home.path().join(".zshenv"), "# user zshenv\n").unwrap();
        seed_installed_rtk();

        super::apply_client_setup("claude_code").expect("apply");

        super::clear_client_setups().expect("first clear (pause)");
        let state = super::load_setup_state();
        assert!(state.configured_clients.is_empty());
        assert!(
            state.remembered_clients.contains_key("claude_code"),
            "pause snapshots the configured client, got: {:?}",
            state.remembered_clients
        );

        super::clear_client_setups().expect("second clear (quit)");
        let state = super::load_setup_state();
        assert!(
            state.remembered_clients.contains_key("claude_code"),
            "quit-time clear after a pause must keep the restore snapshot, got: {:?}",
            state.remembered_clients
        );
    }

    #[test]
    #[serial_test::serial]
    fn apply_then_verify_then_disable_codex_round_trip() {
        let home = TestHome::new();
        fs::write(home.path().join(".zshrc"), "# user zshrc\n").unwrap();
        fs::write(home.path().join(".zshenv"), "# user zshenv\n").unwrap();

        let result = super::apply_client_setup("codex").expect("apply_client_setup succeeds");
        assert!(result.applied);
        assert_eq!(result.client_id, "codex");

        // Managed provider block lands in ~/.codex/config.toml.
        let config_toml = home.path().join(".codex").join("config.toml");
        let toml = fs::read_to_string(&config_toml).expect("codex config.toml written");
        assert!(
            toml.contains("# >>> headroom:codex_cli >>>"),
            "managed marker present, got:\n{toml}"
        );
        assert!(
            toml.contains("model_provider = \"headroom\""),
            "model_provider set, got:\n{toml}"
        );
        assert!(
            toml.contains("base_url = \"http://127.0.0.1:6767/v1\""),
            "provider base_url points at proxy, got:\n{toml}"
        );
        assert!(
            toml.contains("supports_websockets = false"),
            "Codex must use the reliable HTTP Responses transport, got:\n{toml}"
        );
        // No ~/.codex/auth.json in this test ⇒ not ChatGPT-OAuth ⇒ the flag is
        // omitted (it would force an OpenAI OAuth login for API-key users, #406).
        assert!(
            !toml.contains("requires_openai_auth"),
            "requires_openai_auth must NOT be written without ChatGPT auth, got:\n{toml}"
        );

        // OPENAI_BASE_URL exported from a managed shell block.
        let zshrc = fs::read_to_string(home.path().join(".zshrc")).unwrap();
        let zshenv = fs::read_to_string(home.path().join(".zshenv")).unwrap();
        let combined = format!("{zshrc}\n{zshenv}");
        assert!(
            combined.contains("OPENAI_BASE_URL=http://127.0.0.1:6767/v1"),
            "OPENAI_BASE_URL exported from a managed shell block, got:\n{combined}"
        );

        // verify_client_setup reports the configured checks and passes.
        let verification =
            super::verify_client_setup("codex").expect("verify_client_setup succeeds");
        assert_eq!(verification.client_id, "codex");
        assert!(
            verification.failures.is_empty(),
            "no verification failures, got: {:?}",
            verification.failures
        );
        assert!(
            verification
                .checks
                .iter()
                .any(|c| c.contains("config.toml")),
            "verification reports the toml check, got: {:?}",
            verification.checks
        );

        // Disable strips both the toml block and the shell export.
        super::disable_client_setup("codex").expect("disable_client_setup succeeds");
        let toml_after = fs::read_to_string(&config_toml).unwrap_or_default();
        assert!(
            !toml_after.contains("# >>> headroom:codex_cli >>>"),
            "managed block removed on disable, got:\n{toml_after}"
        );
        let combined_after = format!(
            "{}\n{}",
            fs::read_to_string(home.path().join(".zshrc")).unwrap(),
            fs::read_to_string(home.path().join(".zshenv")).unwrap(),
        );
        assert!(
            !combined_after.contains("OPENAI_BASE_URL=http://127.0.0.1:6767/v1"),
            "shell export removed on disable, got:\n{combined_after}"
        );
    }

    #[test]
    #[serial_test::serial]
    fn grok_config_preserves_user_top_level_keys() {
        let home = TestHome::new();
        fs::write(home.path().join(".zshrc"), "# user zshrc\n").unwrap();
        fs::write(home.path().join(".zshenv"), "# user zshenv\n").unwrap();
        let grok_dir = home.path().join(".grok");
        fs::create_dir_all(&grok_dir).unwrap();
        fs::write(grok_dir.join("config.toml"), "default_model = \"grok-4\"\n").unwrap();

        super::apply_client_setup("grok_build").expect("apply_client_setup succeeds");

        let toml = fs::read_to_string(grok_dir.join("config.toml")).unwrap();
        let key_pos = toml.find("default_model").expect("user key kept");
        let table_pos = toml.find("[model.grok-build]").expect("managed table present");
        assert!(
            key_pos < table_pos,
            "top-level key must precede the managed table, got:\n{toml}"
        );
    }

    #[test]
    #[serial_test::serial]
    fn grok_config_redirects_existing_grok_build_table_and_restores_on_disable() {
        let home = TestHome::new();
        fs::write(home.path().join(".zshrc"), "# user zshrc\n").unwrap();
        fs::write(home.path().join(".zshenv"), "# user zshenv\n").unwrap();
        let grok_dir = home.path().join(".grok");
        fs::create_dir_all(&grok_dir).unwrap();
        fs::write(
            grok_dir.join("config.toml"),
            "[model.grok-build]\nbase_url = \"http://127.0.0.1:8787/v1\"\n",
        )
        .unwrap();

        super::apply_client_setup("grok_build").expect("apply_client_setup succeeds");

        let toml = fs::read_to_string(grok_dir.join("config.toml")).unwrap();
        assert_eq!(
            toml.matches("[model.grok-build]").count(),
            1,
            "no duplicate table, got:\n{toml}"
        );
        assert!(
            toml.contains(
                "base_url = \"http://127.0.0.1:6767/v1\"  # was: http://127.0.0.1:8787/v1"
            ),
            "base_url redirected in place, got:\n{toml}"
        );

        let verification =
            super::verify_client_setup("grok_build").expect("verify_client_setup succeeds");
        assert!(verification.failures.is_empty(), "{:?}", verification.failures);

        super::disable_client_setup("grok_build").expect("disable_client_setup succeeds");
        let after = fs::read_to_string(grok_dir.join("config.toml")).unwrap();
        assert!(
            after.contains("base_url = \"http://127.0.0.1:8787/v1\""),
            "original base_url restored, got:\n{after}"
        );
        assert!(!after.contains("# was:"), "redirect comment removed, got:\n{after}");
    }

    #[test]
    #[serial_test::serial]
    fn apply_then_verify_then_disable_grok_build_round_trip() {
        let home = TestHome::new();
        fs::write(home.path().join(".zshrc"), "# user zshrc\n").unwrap();
        fs::write(home.path().join(".zshenv"), "# user zshenv\n").unwrap();

        let result = super::apply_client_setup("grok_build").expect("apply_client_setup succeeds");
        assert!(result.applied);
        assert_eq!(result.client_id, "grok_build");

        let config_toml = home.path().join(".grok").join("config.toml");
        let toml = fs::read_to_string(&config_toml).expect("grok config.toml written");
        assert!(
            toml.contains("# >>> headroom:grok_build_proxy >>>"),
            "managed marker present, got:\n{toml}"
        );
        assert!(
            toml.contains("base_url = \"http://127.0.0.1:6767/v1\""),
            "proxy base_url set, got:\n{toml}"
        );

        let zshrc = fs::read_to_string(home.path().join(".zshrc")).unwrap();
        let zshenv = fs::read_to_string(home.path().join(".zshenv")).unwrap();
        let combined = format!("{zshrc}\n{zshenv}");
        assert!(
            combined.contains("GROK_CLI_CHAT_PROXY_BASE_URL=http://127.0.0.1:6767/v1"),
            "GROK_CLI_CHAT_PROXY_BASE_URL exported, got:\n{combined}"
        );

        let verification =
            super::verify_client_setup("grok_build").expect("verify_client_setup succeeds");
        assert!(
            verification.failures.is_empty(),
            "{:?}",
            verification.failures
        );

        super::disable_client_setup("grok_build").expect("disable_client_setup succeeds");
        let toml_after = fs::read_to_string(&config_toml).unwrap_or_default();
        assert!(
            !toml_after.contains("# >>> headroom:grok_build_proxy >>>"),
            "managed block removed on disable, got:\n{toml_after}"
        );
    }

    #[test]
    #[serial_test::serial]
    fn apply_codex_is_byte_idempotent() {
        let home = TestHome::new();
        fs::write(home.path().join(".zshrc"), "# user zshrc\n").unwrap();
        fs::write(home.path().join(".zshenv"), "# user zshenv\n").unwrap();

        super::apply_client_setup("codex").expect("first apply");
        let config_toml = home.path().join(".codex").join("config.toml");
        let toml_first = fs::read_to_string(&config_toml).unwrap();
        let zshenv_first = fs::read_to_string(home.path().join(".zshenv")).unwrap();

        super::apply_client_setup("codex").expect("second apply");
        let toml_second = fs::read_to_string(&config_toml).unwrap();
        let zshenv_second = fs::read_to_string(home.path().join(".zshenv")).unwrap();

        assert_eq!(toml_first, toml_second, "config.toml byte-stable");
        assert_eq!(zshenv_first, zshenv_second, "zshenv byte-stable");
        assert_eq!(
            toml_second.matches("# >>> headroom:codex_cli >>>").count(),
            1,
            "managed block appears exactly once"
        );
    }

    #[test]
    #[serial_test::serial]
    fn apply_codex_writes_and_registers_guard() {
        let home = TestHome::new();
        fs::write(home.path().join(".zshrc"), "# user zshrc\n").unwrap();
        fs::write(home.path().join(".zshenv"), "# user zshenv\n").unwrap();

        super::apply_client_setup("codex").expect("apply");

        let script = home
            .path()
            .join(".codex")
            .join("hooks")
            .join("headroom-codex-guard.py");
        assert!(script.exists(), "guard script written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&script).unwrap().permissions().mode();
            assert!(
                mode & 0o111 != 0,
                "guard script is executable, got {mode:o}"
            );
        }

        let hooks: serde_json::Value =
            read_settings_json(&home.path().join(".codex").join("hooks.json"));
        let command = format!("/usr/bin/python3 {}", script.display());
        // SessionStart only: on UserPromptSubmit a nonzero exit blocks the prompt.
        let session_registered = hooks["hooks"]["SessionStart"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| {
                entry["hooks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|h| h["command"] == serde_json::Value::String(command.clone()))
            });
        assert!(
            session_registered,
            "guard registered on SessionStart, got:\n{hooks:#}"
        );
        assert!(
            !hooks["hooks"]["UserPromptSubmit"]
                .to_string()
                .contains("headroom-codex-guard.py"),
            "guard must not register on UserPromptSubmit, got:\n{hooks:#}"
        );
        assert_eq!(
            hooks["hooks"]["SessionStart"][0]["matcher"],
            "startup|resume|clear|compact"
        );
    }

    #[test]
    #[serial_test::serial]
    fn apply_codex_guard_is_idempotent_and_disable_preserves_user_hooks() {
        let home = TestHome::new();
        fs::write(home.path().join(".zshrc"), "# user zshrc\n").unwrap();
        fs::write(home.path().join(".zshenv"), "# user zshenv\n").unwrap();
        // Pre-existing user-authored hook that must survive apply and disable.
        fs::create_dir_all(home.path().join(".codex")).unwrap();
        fs::write(
            home.path().join(".codex").join("hooks.json"),
            r#"{"hooks":{"UserPromptSubmit":[{"hooks":[{"type":"command","command":"echo mine"}]}]}}"#,
        )
        .unwrap();

        super::apply_client_setup("codex").expect("first apply");
        super::apply_client_setup("codex").expect("second apply");

        let hooks_path = home.path().join(".codex").join("hooks.json");
        let hooks = read_settings_json(&hooks_path);
        // Guard registered on SessionStart exactly once (not UserPromptSubmit,
        // where a nonzero exit would block every prompt).
        let guard_count = hooks["hooks"]["SessionStart"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| {
                entry["hooks"].as_array().unwrap().iter().any(|h| {
                    h["command"]
                        .as_str()
                        .map(|c| c.contains("headroom-codex-guard.py"))
                        .unwrap_or(false)
                })
            })
            .count();
        assert_eq!(
            guard_count, 1,
            "guard registered exactly once on SessionStart, got:\n{hooks:#}"
        );
        // The guard must NOT be on UserPromptSubmit; the pre-existing user hook
        // there survives untouched.
        let user_prompt = hooks["hooks"]["UserPromptSubmit"].to_string();
        assert!(
            !user_prompt.contains("headroom-codex-guard.py"),
            "guard must not register on UserPromptSubmit, got:\n{hooks:#}"
        );
        assert!(
            user_prompt.contains("echo mine"),
            "user's UserPromptSubmit hook preserved, got:\n{hooks:#}"
        );

        super::disable_client_setup("codex").expect("disable");

        let script = home
            .path()
            .join(".codex")
            .join("hooks")
            .join("headroom-codex-guard.py");
        assert!(!script.exists(), "guard script removed on disable");
        let after = read_settings_json(&hooks_path);
        let after_str = serde_json::to_string(&after).unwrap();
        assert!(
            !after_str.contains("headroom-codex-guard.py"),
            "guard stripped from hooks.json, got:\n{after:#}"
        );
        assert!(
            after_str.contains("echo mine"),
            "user-authored hook preserved, got:\n{after:#}"
        );
    }

    #[test]
    fn remove_guard_hook_entries_strips_stale_interpreter_and_argv_forms() {
        // Regression: an entry written by another build (different interpreter) or
        // normalized by Codex into argv-array form under an unregistered event must
        // still be stripped -- otherwise deleting the script leaves a dangling hook.
        let home = TestHome::new();
        let hooks_path = home.path().join("hooks.json");
        let script = "/Users/x/.codex/hooks/headroom-codex-guard.py";
        fs::write(
            &hooks_path,
            format!(
                r#"{{"hooks":{{
                    "SessionStart":[{{"hooks":[{{"type":"command","command":"/opt/homebrew/bin/python3 {script}"}}]}}],
                    "SessionEnd":[{{"hooks":[{{"type":"command","command":["python3","{script}"]}}]}}],
                    "UserPromptSubmit":[{{"hooks":[{{"type":"command","command":"echo mine"}}]}}]
                }}}}"#
            ),
        )
        .unwrap();

        super::remove_guard_hook_entries(&hooks_path, script, true, None).unwrap();

        let after = read_settings_json(&hooks_path);
        let after_str = serde_json::to_string(&after).unwrap();
        assert!(
            !after_str.contains("headroom-codex-guard.py"),
            "stale guard forms stripped, got:\n{after:#}"
        );
        assert!(
            after_str.contains("echo mine"),
            "user hook preserved, got:\n{after:#}"
        );
    }

    #[test]
    fn codex_guard_script_is_informational_never_blocks() {
        // Regression: a nonzero exit on a Codex hook blocks the session, which
        // held lapsed users' own OpenAI-billed Codex hostage to the app. The
        // guard must only notify, never block.
        let script = super::build_codex_guard_script();
        assert!(
            !script.contains("return 2"),
            "codex guard must never block (exit 2)"
        );
        assert!(script.contains("return 0"));
    }

    #[test]
    #[serial_test::serial]
    fn ensure_codex_guard_migrates_off_user_prompt_submit() {
        let home = TestHome::new();
        let codex_dir = home.path().join(".codex");
        fs::create_dir_all(codex_dir.join("hooks")).unwrap();
        let hooks_path = codex_dir.join("hooks.json");
        let cmd = format!(
            "/usr/bin/python3 {}",
            codex_dir
                .join("hooks")
                .join("headroom-codex-guard.py")
                .display()
        );
        // Old install: guard registered on both SessionStart and UserPromptSubmit.
        fs::write(
            &hooks_path,
            format!(
                r#"{{"hooks":{{
                    "SessionStart":[{{"matcher":"startup|resume|clear|compact","hooks":[{{"type":"command","command":"{cmd}"}}]}}],
                    "UserPromptSubmit":[{{"hooks":[{{"type":"command","command":"{cmd}"}}]}}]
                }}}}"#
            ),
        )
        .unwrap();

        super::ensure_codex_guard_hook().unwrap();

        let after = read_settings_json(&hooks_path);
        let dump = serde_json::to_string_pretty(&after).unwrap();
        assert!(
            !after["hooks"]["UserPromptSubmit"]
                .to_string()
                .contains("headroom-codex-guard.py"),
            "guard stripped from UserPromptSubmit, got:\n{dump}"
        );
        assert!(
            after["hooks"]["SessionStart"]
                .to_string()
                .contains("headroom-codex-guard.py"),
            "guard kept on SessionStart, got:\n{dump}"
        );
    }

    #[test]
    #[serial_test::serial]
    fn apply_codex_emits_requires_openai_auth_for_chatgpt_users() {
        let home = TestHome::new();
        fs::write(home.path().join(".zshrc"), "# user zshrc\n").unwrap();
        let codex_dir = home.path().join(".codex");
        fs::create_dir_all(&codex_dir).unwrap();
        fs::write(
            codex_dir.join("auth.json"),
            "{\"auth_mode\":\"chatgpt\",\"tokens\":{\"account_id\":\"acct_123\"}}",
        )
        .unwrap();

        super::apply_client_setup("codex").expect("apply_client_setup succeeds");
        let toml = fs::read_to_string(codex_dir.join("config.toml")).unwrap();
        assert!(
            toml.contains("requires_openai_auth = true"),
            "ChatGPT-OAuth users need the flag for the account menu, got:\n{toml}"
        );
    }

    #[test]
    #[serial_test::serial]
    fn apply_codex_omits_requires_openai_auth_for_api_key_users() {
        let home = TestHome::new();
        fs::write(home.path().join(".zshrc"), "# user zshrc\n").unwrap();
        let codex_dir = home.path().join(".codex");
        fs::create_dir_all(&codex_dir).unwrap();
        fs::write(
            codex_dir.join("auth.json"),
            "{\"auth_mode\":\"apikey\",\"OPENAI_API_KEY\":\"sk-test\"}",
        )
        .unwrap();

        super::apply_client_setup("codex").expect("apply_client_setup succeeds");
        let toml = fs::read_to_string(codex_dir.join("config.toml")).unwrap();
        assert!(
            !toml.contains("requires_openai_auth"),
            "API-key users must not be forced into an OpenAI OAuth login (#406), got:\n{toml}"
        );
    }

    #[test]
    #[serial_test::serial]
    fn apply_codex_keeps_root_keys_at_root_scope_when_config_ends_in_a_table() {
        // Regression for the `invalid type: string "headroom", expected a
        // boolean in features` error: a config whose last table is `[features]`
        // (boolean-only values) used to absorb the appended root keys.
        let home = TestHome::new();
        fs::write(home.path().join(".zshrc"), "# user zshrc\n").unwrap();
        let codex_dir = home.path().join(".codex");
        fs::create_dir_all(&codex_dir).unwrap();
        let config_toml = codex_dir.join("config.toml");
        fs::write(
            &config_toml,
            "model = \"gpt-5.4\"\n\n[features]\njs_repl = false\n",
        )
        .unwrap();

        super::apply_client_setup("codex").expect("apply succeeds");

        let raw = fs::read_to_string(&config_toml).unwrap();
        let parsed: toml::Value = raw
            .parse()
            .unwrap_or_else(|e| panic!("valid toml: {e}\n{raw}"));

        assert_eq!(
            parsed.get("model_provider").and_then(|v| v.as_str()),
            Some("headroom"),
            "model_provider must resolve at root scope, got:\n{raw}"
        );
        assert!(
            parsed
                .get("features")
                .and_then(|f| f.get("model_provider"))
                .is_none(),
            "model_provider must not leak into [features], got:\n{raw}"
        );
        assert_eq!(
            parsed
                .get("model_providers")
                .and_then(|m| m.get("headroom"))
                .and_then(|h| h.get("base_url"))
                .and_then(|v| v.as_str()),
            Some(super::HEADROOM_OPENAI_BASE_URL),
            "provider table base_url points at the proxy, got:\n{raw}"
        );
        // The user's own content survives untouched.
        assert_eq!(
            parsed.get("model").and_then(|v| v.as_str()),
            Some("gpt-5.4"),
            "existing root key preserved, got:\n{raw}"
        );
        assert_eq!(
            parsed
                .get("features")
                .and_then(|f| f.get("js_repl"))
                .and_then(|v| v.as_bool()),
            Some(false),
            "existing [features] table preserved, got:\n{raw}"
        );
    }

    #[test]
    fn oss_remnant_warnings_clean_install_is_silent() {
        assert!(oss_remnant_warnings(false, false, false, false).is_empty());
    }

    #[test]
    fn oss_remnant_warnings_flags_each_remnant() {
        let w = oss_remnant_warnings(true, true, true, true);
        assert_eq!(w.len(), 4, "one warning per remnant, got: {w:?}");
        assert!(w.iter().any(|m| m.contains(":8787")));
        assert!(w.iter().any(|m| m.contains("~/.local/bin/headroom")));
        assert!(w.iter().any(|m| m.contains("~/.local/bin/rtk")));
        assert!(w.iter().any(|m| m.contains("settings.json")));
    }

    #[test]
    fn render_codex_config_collapses_duplicate_managed_blocks() {
        // Regression: a config left with TWO managed provider blocks (interrupted
        // write / older build) used to keep one survivor that regenerated forever,
        // surfacing as a duplicate [model_providers.headroom] the user deleted by
        // hand. render must collapse all duplicates down to exactly one.
        let dup = "# >>> headroom:codex_cli_provider >>>\n\
                   [model_providers.headroom]\n\
                   base_url = \"http://stale/v1\"\n\
                   # <<< headroom:codex_cli_provider <<<\n\
                   model = \"gpt-5.4\"\n\
                   # >>> headroom:codex_cli_provider >>>\n\
                   [model_providers.headroom]\n\
                   base_url = \"http://stale2/v1\"\n\
                   # <<< headroom:codex_cli_provider <<<\n";

        let rendered = render_codex_config(dup);

        assert_eq!(
            rendered.matches("[model_providers.headroom]").count(),
            1,
            "exactly one managed provider table after render, got:\n{rendered}"
        );
        assert!(
            rendered.parse::<toml::Value>().is_ok(),
            "rendered config is valid toml, got:\n{rendered}"
        );
        assert!(
            rendered.contains("model = \"gpt-5.4\""),
            "user content between the duplicates is preserved, got:\n{rendered}"
        );
    }

    #[test]
    fn codex_foreign_model_provider_is_root_scope_only() {
        assert_eq!(
            super::codex_foreign_model_provider("model_provider = \"gateway\"\n").as_deref(),
            Some("gateway"),
        );
        // Our own managed value is not "foreign".
        assert_eq!(
            super::codex_foreign_model_provider("model_provider = \"headroom\"\n"),
            None,
        );
        // A model_provider inside a table belongs to that table, not the route.
        assert_eq!(
            super::codex_foreign_model_provider("[profiles.work]\nmodel_provider = \"gateway\"\n"),
            None,
        );
        assert_eq!(super::codex_foreign_model_provider(""), None);
    }

    #[test]
    fn render_codex_config_does_not_duplicate_a_foreign_root_model_provider() {
        // Regression: a pre-existing root model_provider used to survive into the
        // rendered body, colliding with the managed `model_provider = "headroom"`
        // as a duplicate root key -> invalid TOML, Codex refuses to load config.
        let existing = "model_provider = \"gateway\"\n\
                        [model_providers.gateway]\n\
                        base_url = \"http://gw/v1\"\n";
        let rendered = render_codex_config(existing);
        let parsed: toml::Value = rendered
            .parse()
            .unwrap_or_else(|e| panic!("rendered config is valid toml: {e}\n{rendered}"));
        assert_eq!(
            parsed.get("model_provider").and_then(|v| v.as_str()),
            Some("headroom"),
            "managed provider wins at root, got:\n{rendered}"
        );
        assert_eq!(
            rendered.matches("model_provider =").count(),
            1,
            "exactly one root model_provider, got:\n{rendered}"
        );
        // The user's own provider table is left untouched for the restore.
        assert!(
            rendered.contains("[model_providers.gateway]"),
            "user provider table preserved, got:\n{rendered}"
        );
    }

    #[test]
    #[serial_test::serial]
    fn apply_then_disable_codex_restores_a_foreign_model_provider() {
        let home = TestHome::new();
        fs::write(home.path().join(".zshrc"), "# user zshrc\n").unwrap();
        let codex_dir = home.path().join(".codex");
        fs::create_dir_all(&codex_dir).unwrap();
        let config_toml = codex_dir.join("config.toml");
        fs::write(
            &config_toml,
            "model_provider = \"gateway\"\n\n[model_providers.gateway]\nbase_url = \"http://gw/v1\"\n",
        )
        .unwrap();

        super::apply_client_setup("codex").expect("apply succeeds");

        let after_apply = fs::read_to_string(&config_toml).unwrap();
        let parsed: toml::Value = after_apply
            .parse()
            .unwrap_or_else(|e| panic!("valid toml after apply: {e}\n{after_apply}"));
        assert_eq!(
            parsed.get("model_provider").and_then(|v| v.as_str()),
            Some("headroom"),
            "Headroom takes over routing while enabled, got:\n{after_apply}"
        );

        super::disable_client_setup("codex").expect("disable succeeds");

        let after_disable = fs::read_to_string(&config_toml).unwrap();
        let parsed: toml::Value = after_disable
            .parse()
            .unwrap_or_else(|e| panic!("valid toml after disable: {e}\n{after_disable}"));
        assert_eq!(
            parsed.get("model_provider").and_then(|v| v.as_str()),
            Some("gateway"),
            "the pre-Headroom provider is restored on disable, got:\n{after_disable}"
        );
        assert!(
            parsed
                .get("model_providers")
                .and_then(|m| m.get("gateway"))
                .is_some(),
            "user provider table survives the round trip, got:\n{after_disable}"
        );
    }

    #[test]
    #[serial_test::serial]
    fn apply_codex_repairs_a_previously_corrupted_features_block() {
        // A machine upgraded mid-bug: the old single block sits at end-of-file,
        // its root keys absorbed into [features]. Re-applying must repair it so
        // the file parses and the keys resolve at root scope.
        let home = TestHome::new();
        fs::write(home.path().join(".zshrc"), "# user zshrc\n").unwrap();
        let codex_dir = home.path().join(".codex");
        fs::create_dir_all(&codex_dir).unwrap();
        let config_toml = codex_dir.join("config.toml");
        fs::write(
            &config_toml,
            "[features]\njs_repl = false\n\
             # >>> headroom:codex_cli >>>\n\
             model_provider = \"headroom\"\n\
             openai_base_url = \"http://127.0.0.1:6767/v1\"\n\n\
             [model_providers.headroom]\n\
             name = \"Headroom persistent proxy\"\n\
             base_url = \"http://127.0.0.1:6767/v1\"\n\
             supports_websockets = true\n\
             # <<< headroom:codex_cli <<<\n",
        )
        .unwrap();

        // The corrupted file is invalid against Codex's schema, but still parses
        // as TOML with the key wrongly nested under [features].
        let before: toml::Value = fs::read_to_string(&config_toml).unwrap().parse().unwrap();
        assert_eq!(
            before
                .get("features")
                .and_then(|f| f.get("model_provider"))
                .and_then(|v| v.as_str()),
            Some("headroom"),
            "precondition: corruption present"
        );

        super::apply_client_setup("codex").expect("re-apply repairs config");

        let after: toml::Value = fs::read_to_string(&config_toml).unwrap().parse().unwrap();
        assert_eq!(
            after.get("model_provider").and_then(|v| v.as_str()),
            Some("headroom")
        );
        assert!(after
            .get("features")
            .and_then(|f| f.get("model_provider"))
            .is_none());
    }

    #[test]
    fn sweep_managed_backups_removes_headroom_and_nommer_siblings_only() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let target = tmp.path().join("settings.json");
        fs::write(&target, "{}").unwrap();

        let headroom_backup = tmp
            .path()
            .join("settings.json.headroom-backup-20260101000000");
        let nommer_backup = tmp
            .path()
            .join("settings.json.nommer-backup-20250101000000");
        let unrelated = tmp.path().join("settings.json.bak");
        let other_target_backup = tmp
            .path()
            .join("config.toml.headroom-backup-20260101000000");
        fs::write(&headroom_backup, "old").unwrap();
        fs::write(&nommer_backup, "older").unwrap();
        fs::write(&unrelated, "user-owned").unwrap();
        fs::write(&other_target_backup, "different file's backup").unwrap();

        let removed = super::sweep_managed_backups(&target);

        assert_eq!(removed.len(), 2, "removed: {removed:?}");
        assert!(!headroom_backup.exists(), "headroom backup should be gone");
        assert!(!nommer_backup.exists(), "nommer backup should be gone");
        assert!(unrelated.exists(), "unrelated .bak should survive");
        assert!(
            other_target_backup.exists(),
            "another file's backup should survive"
        );
        assert!(target.exists(), "target file itself should survive");
    }

    #[test]
    fn sweep_managed_backups_is_quiet_when_parent_missing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let missing = tmp.path().join("does-not-exist").join("settings.json");
        let removed = super::sweep_managed_backups(&missing);
        assert!(removed.is_empty());
    }

    #[test]
    #[serial_test::serial]
    fn write_setup_state_publishes_atomically() {
        let _home = TestHome::new();
        let mut state = super::ClientSetupState::default();
        state
            .configured_clients
            .insert("claude_code".into(), "2026-01-01T00:00:00+00:00".into());
        super::write_setup_state(&state).expect("write");

        let path = super::setup_state_path();
        assert!(path.exists(), "setup state file written");

        // No sibling .tmp* file may be left behind after a successful publish —
        // its presence would mean the rename step never happened.
        let dir = path.parent().unwrap();
        let stem = path.file_name().unwrap().to_string_lossy().into_owned();
        let leftover: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(&format!("{stem}.tmp")))
            .collect();
        assert!(
            leftover.is_empty(),
            "tmp files cleaned up by rename, got: {leftover:?}"
        );

        // Round-trip survives.
        let reloaded = super::load_setup_state();
        assert!(reloaded.configured_clients.contains_key("claude_code"));
    }

    #[test]
    fn atomic_write_concurrent_same_path_no_enoent() {
        // Regression for Sentry RUST-3W / RUST-4W: a shared `<path>.tmp` made
        // concurrent writers race — one rename consumed the tmp, the other hit
        // ENOENT. Unique per-writer tmp names must let all writers succeed.
        let dir = std::env::temp_dir().join(format!("aw_race_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.json");
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let p = path.clone();
                std::thread::spawn(move || {
                    let body = format!("{{\"n\":{i}}}");
                    super::atomic_write(&p, body.as_bytes())
                })
            })
            .collect();
        for h in handles {
            h.join()
                .unwrap()
                .expect("concurrent atomic_write must not ENOENT");
        }
        assert!(path.exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    #[serial_test::serial]
    fn load_setup_state_falls_back_to_default_on_corrupt_file() {
        let _home = TestHome::new();
        let path = super::setup_state_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // Simulate a torn / partial write that would have happened with the
        // pre-fix non-atomic writer. The retry path inside load_setup_state
        // re-reads after a short backoff and, when the file is still bad,
        // logs a warning and returns the default rather than panicking.
        std::fs::write(&path, b"{ not json").unwrap();

        let state = super::load_setup_state();
        assert!(state.configured_clients.is_empty());
        assert!(state.remembered_clients.is_empty());
    }

    fn seed_codex_threads_db(path: &Path, rows: &[(&str, &str)]) {
        let conn = Connection::open(path).unwrap();
        conn.execute(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT NOT NULL)",
            [],
        )
        .unwrap();
        for (id, provider) in rows {
            conn.execute(
                "INSERT INTO threads (id, model_provider) VALUES (?, ?)",
                [id, provider],
            )
            .unwrap();
        }
    }

    fn provider_count(path: &Path, provider: &str) -> i64 {
        let conn = Connection::open(path).unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM threads WHERE model_provider = ?1",
            [provider],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn retag_one_codex_db_moves_only_matching_provider() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("state_5.sqlite");
        seed_codex_threads_db(
            &db,
            &[
                ("a", "openai"),
                ("b", "openai"),
                ("c", "headroom"),
                ("d", "anthropic"),
            ],
        );

        let moved = retag_one_codex_db(&db, "openai", "headroom").unwrap();
        assert_eq!(moved, Some(2));
        assert_eq!(provider_count(&db, "openai"), 0);
        assert_eq!(provider_count(&db, "headroom"), 3);
        // Third-party providers are untouched.
        assert_eq!(provider_count(&db, "anthropic"), 1);

        // Reverse direction round-trips only the headroom rows.
        let back = retag_one_codex_db(&db, "headroom", "openai").unwrap();
        assert_eq!(back, Some(3));
        assert_eq!(provider_count(&db, "headroom"), 0);
        assert_eq!(provider_count(&db, "openai"), 3);
        assert_eq!(provider_count(&db, "anthropic"), 1);
    }

    #[test]
    fn retag_one_codex_db_noop_without_threads_table() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("state_5.sqlite");
        // Open creates an empty DB with no `threads` table.
        Connection::open(&db).unwrap();
        assert_eq!(retag_one_codex_db(&db, "openai", "headroom").unwrap(), None);
    }

    #[test]
    #[serial_test::serial]
    fn retag_codex_thread_providers_silent_when_no_store() {
        let _home = TestHome::new();
        // No ~/.codex stores exist under the temp home: must not panic.
        retag_codex_thread_providers("openai", "headroom");
    }

    #[test]
    #[serial_test::serial]
    fn codex_sqlite_store_expected_gates_on_state_file_not_dir() {
        let home = TestHome::new();
        let codex = home.path().join(".codex");
        // CLI-only / pre-sqlite Codex: config + sessions but no sqlite/ store.
        std::fs::create_dir_all(codex.join("sessions")).unwrap();
        std::fs::write(codex.join("config.toml"), "").unwrap();
        assert!(
            !codex_sqlite_store_expected(),
            "config/sessions alone must not trigger the moved-store warning"
        );
        // sqlite/ dir holding only unrelated stores (logs/goals/memories) but no
        // thread store must NOT fire -- the false positive behind Sentry RUST-3R.
        std::fs::create_dir_all(codex.join("sqlite")).unwrap();
        std::fs::write(codex.join("sqlite").join("logs_2.sqlite"), "").unwrap();
        std::fs::write(codex.join("sqlite").join("goals_1.sqlite"), "").unwrap();
        assert!(
            !codex_sqlite_store_expected(),
            "unrelated sqlite stores must not trigger the moved-store warning"
        );
        // CLI store renamed loose in codex_home (version no longer parses) ->
        // expected, so the relocation gets flagged.
        std::fs::write(codex.join("state_5x.sqlite"), "").unwrap();
        assert!(codex_sqlite_store_expected());
        std::fs::remove_file(codex.join("state_5x.sqlite")).unwrap();
        // GUI thread store present under sqlite/ -> expected.
        std::fs::write(codex.join("sqlite").join("state_6.sqlite"), "").unwrap();
        assert!(codex_sqlite_store_expected());
    }

    #[test]
    #[serial_test::serial]
    fn retag_codex_threads_to_headroom_pulls_native_threads_back() {
        // Reproduces the app-update restart path: the quit handler left threads
        // tagged `openai`; launch must retag them back to `headroom`.
        let home = TestHome::new();
        let db = home.path().join(".codex").join("state_5.sqlite");
        std::fs::create_dir_all(db.parent().unwrap()).unwrap();
        seed_codex_threads_db(&db, &[("a", "openai"), ("b", "openai"), ("c", "anthropic")]);

        retag_codex_threads_to_headroom();

        assert_eq!(provider_count(&db, "headroom"), 2);
        assert_eq!(provider_count(&db, "openai"), 0);
        // Third-party threads are untouched.
        assert_eq!(provider_count(&db, "anthropic"), 1);
    }

    #[test]
    #[serial_test::serial]
    fn codex_home_honors_env_else_default() {
        let home = TestHome::new();
        // TestHome clears CODEX_HOME, so we fall back to $HOME/.codex.
        assert_eq!(codex_home(), home.path().join(".codex"));

        let custom = home.path().join("custom-codex");
        std::env::set_var("CODEX_HOME", &custom);
        assert_eq!(codex_home(), custom);

        // An empty value is ignored (treated as unset).
        std::env::set_var("CODEX_HOME", "");
        assert_eq!(codex_home(), home.path().join(".codex"));
    }

    #[test]
    #[serial_test::serial]
    fn pin_codex_mcp_command_rewrites_only_headroom_table() {
        let home = TestHome::new();
        let codex = home.path().join(".codex");
        std::fs::create_dir_all(&codex).unwrap();
        let config = codex.join("config.toml");
        std::fs::write(
            &config,
            "# --- Headroom MCP server ---\n\
             [mcp_servers.headroom]\n\
             command = \"headroom\"\n\
             args = [\"mcp\", \"serve\"]\n\
             \n\
             [mcp_servers.headroom.env]\n\
             HEADROOM_PROXY_URL = \"http://127.0.0.1:6767\"\n\
             \n\
             [mcp_servers.node_repl]\n\
             command = \"/Applications/Codex.app/node_repl\"\n",
        )
        .unwrap();

        let entrypoint = home.path().join("App Support/venv/bin/headroom");
        let changed = pin_codex_mcp_command(&entrypoint).unwrap();
        assert!(changed.is_some(), "config should have been rewritten");

        let after = std::fs::read_to_string(&config).unwrap();
        let abs = entrypoint.display().to_string();
        assert!(
            after.contains(&format!("command = \"{abs}\"")),
            "headroom command pinned to absolute path, got:\n{after}"
        );
        // The unrelated server's command must be untouched.
        assert!(after.contains("command = \"/Applications/Codex.app/node_repl\""));
        // The headroom env sub-table has no `command`; nothing spurious added.
        assert_eq!(after.matches("command = ").count(), 2);

        // Idempotent: a second run with the same entrypoint is a no-op.
        assert!(pin_codex_mcp_command(&entrypoint).unwrap().is_none());
    }

    #[test]
    #[serial_test::serial]
    fn pin_codex_mcp_command_normalizes_python_module_args() {
        // Upstream may register `<python> -m headroom.cli mcp serve`. Pinning
        // command to the console script must also rewrite the args, otherwise
        // `headroom -m headroom.cli ...` fails with "No such option '-m'".
        let home = TestHome::new();
        let codex = home.path().join(".codex");
        std::fs::create_dir_all(&codex).unwrap();
        let config = codex.join("config.toml");
        std::fs::write(
            &config,
            "[mcp_servers.headroom]\n\
             command = \"/somewhere/venv/bin/python3\"\n\
             args = [\"-m\", \"headroom.cli\", \"mcp\", \"serve\"]\n\
             \n\
             [mcp_servers.headroom.env]\n\
             HEADROOM_PROXY_URL = \"http://127.0.0.1:6767\"\n",
        )
        .unwrap();

        let entrypoint = home.path().join("venv/bin/headroom");
        assert!(pin_codex_mcp_command(&entrypoint).unwrap().is_some());

        let after = std::fs::read_to_string(&config).unwrap();
        assert!(
            after.contains("args = [\"mcp\", \"serve\"]"),
            "python -m args must be normalized, got:\n{after}"
        );
        assert!(!after.contains("-m"), "no -m leftovers, got:\n{after}");
    }

    #[test]
    #[serial_test::serial]
    fn pin_codex_mcp_command_handles_multi_line_args_array() {
        let home = TestHome::new();
        let codex = home.path().join(".codex");
        std::fs::create_dir_all(&codex).unwrap();
        let config = codex.join("config.toml");
        std::fs::write(
            &config,
            "[mcp_servers.headroom]\n\
             command = \"/somewhere/venv/bin/python3\"\n\
             args = [\n  \"-m\",\n  \"headroom.cli\",\n  \"mcp\",\n  \"serve\",\n]\n\
             \n\
             [mcp_servers.headroom.env]\n\
             HEADROOM_PROXY_URL = \"http://127.0.0.1:6767\"\n",
        )
        .unwrap();

        let entrypoint = home.path().join("venv/bin/headroom");
        assert!(pin_codex_mcp_command(&entrypoint).unwrap().is_some());

        let after = std::fs::read_to_string(&config).unwrap();
        // No orphaned continuation lines — the rebuilt file must parse.
        let parsed: toml::Value = toml::from_str(&after).expect("rebuilt config parses");
        assert_eq!(
            parsed["mcp_servers"]["headroom"]["args"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert!(after.contains("[mcp_servers.headroom.env]"));
        assert!(!after.contains("headroom.cli"));
    }

    #[test]
    #[serial_test::serial]
    fn discover_codex_state_dbs_finds_any_sqlite_regardless_of_name() {
        let home = TestHome::new();
        let codex = home.path().join(".codex");
        std::fs::create_dir_all(codex.join("sqlite")).unwrap();
        // GUI store under sqlite/, CLI store at the root, plus a renamed store
        // whose name no longer follows the `state_<N>` scheme -- discovery is
        // content-based now, so it must still be picked up (the actual fix).
        std::fs::File::create(codex.join("sqlite").join("state_6.sqlite")).unwrap();
        std::fs::File::create(codex.join("state_5.sqlite")).unwrap();
        std::fs::File::create(codex.join("sqlite").join("threads.sqlite")).unwrap();
        // A non-sqlite file in the same dir must be ignored.
        std::fs::File::create(codex.join("config.toml")).unwrap();

        let names: BTreeSet<String> = discover_codex_state_dbs()
            .into_iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap().to_owned())
            .collect();
        assert_eq!(
            names,
            BTreeSet::from([
                "state_6.sqlite".to_owned(),
                "state_5.sqlite".to_owned(),
                "threads.sqlite".to_owned(),
            ])
        );
    }

    #[test]
    #[serial_test::serial]
    fn retag_handles_unknown_store_version() {
        // Future-proofing: a Codex store-version bump (here state_99) must still
        // retag, not silently no-op for every user at once.
        let home = TestHome::new();
        let db = home.path().join(".codex").join("state_99.sqlite");
        std::fs::create_dir_all(db.parent().unwrap()).unwrap();
        seed_codex_threads_db(&db, &[("a", "openai"), ("b", "openai"), ("c", "anthropic")]);

        retag_codex_threads_to_headroom();

        assert_eq!(provider_count(&db, "headroom"), 2);
        assert_eq!(provider_count(&db, "openai"), 0);
        assert_eq!(provider_count(&db, "anthropic"), 1);
    }

    #[test]
    #[serial_test::serial]
    fn retag_handles_store_renamed_off_state_scheme() {
        // The regression this change fixes: Codex renames the store off the
        // `state_<N>.sqlite` scheme entirely. Content-based discovery must still
        // find and retag it by its `threads` table, not the filename.
        let home = TestHome::new();
        let db = home
            .path()
            .join(".codex")
            .join("sqlite")
            .join("threads.sqlite");
        std::fs::create_dir_all(db.parent().unwrap()).unwrap();
        seed_codex_threads_db(&db, &[("a", "openai"), ("b", "openai"), ("c", "anthropic")]);

        retag_codex_threads_to_headroom();

        assert_eq!(provider_count(&db, "headroom"), 2);
        assert_eq!(provider_count(&db, "openai"), 0);
        assert_eq!(provider_count(&db, "anthropic"), 1);
    }

    #[test]
    fn claude_guard_script_is_diagnostic_and_reachable_tolerates_any_response() {
        let script = build_claude_guard_script();
        // reachable() no longer flags a 503-during-bypass as "app down".
        assert!(!script.contains("return response.status < 500"));
        assert!(script.contains("except urllib.error.HTTPError:\n        return True"));
        // main() explains WHY instead of the flat "is not" message.
        assert!(script.contains("def diagnose_route"));
        assert!(script.contains("overrides Headroom's route"));
        assert!(!script.contains("ANTHROPIC_BASE_URL is not \" + BASE_URL"));
        // A correct user settings + unset process env (GUI / `open` launch) is
        // healthy and must NOT trigger the old "restart Claude Code" nag.
        assert!(!script.contains("did not inherit the Headroom shell env"));
        assert!(script.contains("user_val != BASE_URL and effective != BASE_URL"));
        // Notifications are debounced and reachability retries once, so an app
        // relaunch doesn't produce a notification storm.
        assert!(script.contains("DEBOUNCE_PATH.touch()"));
        assert!(script.contains("time.sleep(2)\n    return probe()"));
    }

    #[test]
    fn codex_guard_script_names_actual_values_and_tolerates_any_response() {
        let script = build_codex_guard_script();
        assert!(!script.contains("return response.status < 500"));
        assert!(script.contains("except urllib.error.HTTPError:\n        return True"));
        // Messages include the actual found value, not just "is not headroom".
        assert!(script.contains("(expected \"headroom\")"));
        assert!(script.contains("(expected \" + BASE_URL + \")"));
        assert!(script.contains("DEBOUNCE_PATH.touch()"));
        assert!(script.contains("time.sleep(2)\n    return probe()"));
    }
}
