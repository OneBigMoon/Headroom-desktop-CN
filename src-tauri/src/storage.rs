use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub fn app_data_dir() -> PathBuf {
    // Explicit override, used for test hermeticity: nextest runs each test in
    // its own process, so an in-process env lock cannot stop parallel tests
    // from sharing (and corrupting) the real profile's Headroom dir — on
    // macOS/Windows dirs::data_local_dir() ignores every env var TestHome
    // sets. Production never sets this. Relative paths are ignored so a stray
    // value can't scatter state under an arbitrary cwd.
    if let Some(dir) = std::env::var_os("HEADROOM_DATA_DIR").filter(|v| !v.is_empty()) {
        let dir = PathBuf::from(dir);
        if dir.is_absolute() {
            return dir;
        }
    }
    let base = dirs::data_local_dir()
        .or_else(|| std::env::var_os("XDG_DATA_HOME").map(PathBuf::from))
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("share"))
        })
        .unwrap_or_else(std::env::temp_dir);
    if crate::edition::LOCAL_COMMUNITY {
        base.join("HeadroomLocalCommunity")
    } else {
        base.join("HeadroomLocalCommunity")
    }
}

pub fn ensure_data_dirs(base_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(base_dir)
        .with_context(|| format!("creating app data dir {}", base_dir.display()))?;
    std::fs::create_dir_all(base_dir.join("telemetry"))
        .with_context(|| format!("creating telemetry dir under {}", base_dir.display()))?;
    std::fs::create_dir_all(base_dir.join("config"))
        .with_context(|| format!("creating config dir under {}", base_dir.display()))?;
    Ok(())
}

pub fn config_file(base_dir: &Path, name: &str) -> PathBuf {
    base_dir.join("config").join(name)
}

/// The user-facing calendar day ("YYYY-MM-DD", local timezone) for an
/// instant. Canonical: every "today"/day-bucket decision that the user can
/// see goes through this, regardless of the instant's source timezone —
/// mixed UTC/local day keys gave US users mid-afternoon daily resets. UTC-
/// bucketed data from the backend is the one exception (keyed by its UTC
/// date, labeled as such). See the Persistence Rules in CLAUDE.md.
pub fn user_day_key<Tz: chrono::TimeZone>(instant: chrono::DateTime<Tz>) -> String {
    instant
        .with_timezone(&chrono::Local)
        .format("%Y-%m-%d")
        .to_string()
}

/// Local `NaiveDate` counterpart of [`user_day_key`].
pub fn user_day<Tz: chrono::TimeZone>(instant: chrono::DateTime<Tz>) -> chrono::NaiveDate {
    instant.with_timezone(&chrono::Local).date_naive()
}

pub fn memory_db_path(base_dir: &Path) -> PathBuf {
    base_dir.join("memory.db")
}

pub fn telemetry_file(base_dir: &Path, name: &str) -> PathBuf {
    base_dir.join("telemetry").join(name)
}
