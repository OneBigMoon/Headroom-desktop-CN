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
}
