//! Child-process spawning that stays invisible on Windows.
//!
//! Headroom is a GUI-subsystem binary, so it owns no console. When it spawns a
//! console-subsystem child (python.exe, pip, reg, powershell, taskkill) Windows
//! allocates a *new* console for that child and shows it. Because we pipe the
//! child's stdio, that window is an empty black rectangle the user has to look
//! at for the length of the install.
//!
//! `CREATE_NO_WINDOW` suppresses the console allocation without changing stdio,
//! exit-code, or lifetime semantics. Every spawn in this crate goes through
//! `command()` so a new call site can't reintroduce the flash;
//! `scripts/check-no-console.sh` fails the build if a bare `Command::new`
//! reappears.

use std::ffi::OsStr;
use std::process::Command;

/// <https://learn.microsoft.com/windows/win32/procthread/process-creation-flags>
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Drop-in replacement for `std::process::Command::new`.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    // Python picks its stdio encoding from the locale, and we redirect every
    // child's stdout/stderr to a log file — so on Windows it resolves to the
    // ANSI codepage (cp1252), not UTF-8. Upstream's startup banner is not
    // ASCII, so `print()` raised UnicodeEncodeError and the backend died
    // before opening its port, with the app reporting only an opaque exit
    // 0xc000013a (Sentry RUST-7C). Applied to every spawn rather than the one
    // that crashed: the prefetch and smoke-test children print too, and a
    // POSIX box running under LC_ALL=C has the identical exposure.
    // `backslashreplace` over plain `utf-8` because a log stream must never be
    // the thing that kills the process (a lone surrogate still encodes).
    command.env("PYTHONIOENCODING", "utf-8:backslashreplace");
    command
}

#[cfg(test)]
mod tests {
    #[test]
    fn command_still_runs_and_captures_output() {
        // The flag must not disturb stdio or exit codes on any platform.
        let program = if cfg!(windows) { "cmd" } else { "/bin/echo" };
        let args: &[&str] = if cfg!(windows) {
            &["/C", "echo headroom"]
        } else {
            &["headroom"]
        };
        let out = super::command(program).args(args).output().expect("spawn");
        assert!(out.status.success());
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "headroom");
    }

    /// RUST-7C: the backend died printing a non-ASCII banner to a redirected
    /// stdout because Python fell back to the platform codepage. Deterministic
    /// half -- the variable must be on every command we hand out.
    #[test]
    fn command_forces_utf8_python_stdio() {
        let cmd = super::command("python3");
        let set = cmd
            .get_envs()
            .find(|(k, _)| *k == std::ffi::OsStr::new("PYTHONIOENCODING"))
            .and_then(|(_, v)| v)
            .expect("PYTHONIOENCODING is set");
        assert_eq!(set, std::ffi::OsStr::new("utf-8:backslashreplace"));
    }

    /// Behavioural half: the exact shape that crashed -- non-ASCII `print()`
    /// with stdout captured (not a tty), which is how we spawn the backend.
    /// Skipped where no interpreter is on PATH; the assert above still guards.
    #[test]
    fn python_prints_non_ascii_to_piped_stdout_without_dying() {
        let banner = "\u{250c}\u{2500} Headroom \u{2192} 100% \u{2713}";
        let script = format!("print('{banner}')");
        let out = match super::command("python3").args(["-c", &script]).output() {
            Ok(out) => out,
            // No python3 on PATH in this environment.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => panic!("spawn failed: {e}"),
        };
        assert!(
            out.status.success(),
            "non-ASCII print killed the child: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(String::from_utf8_lossy(&out.stdout).contains("Headroom"));
    }
}
