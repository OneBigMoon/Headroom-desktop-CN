//! Runtime-selected backend port for the Python proxy.
//!
//! `6867` (the intercept port) is fixed because clients are configured to point
//! at it. `6868` (the internal port between intercept and Python proxy) is
//! purely internal and can move when something else on the machine has already
//! grabbed it — most commonly Apple's `rapportd`, which gets a kernel-assigned
//! port at login and registers it via Bonjour. On affected machines the only
//! reliable way to free 6868 is a reboot, which is not a fix we can ship.
//!
//! At proxy spawn time, [`select_available`] probes 6868 and falls back to
//! 6869..=6890. The chosen port is stored in [`BACKEND_PORT`]; the intercept
//! forwarder, spawn args, and health probes all read it through [`get`].
//!
//! Default value is [`DEFAULT_BACKEND_PORT`] so anything that reads the atomic
//! before selection runs gets today's behavior.

use std::sync::atomic::{AtomicU16, Ordering};

pub const DEFAULT_BACKEND_PORT: u16 = 6868;
pub const FALLBACK_RANGE_START: u16 = 6869;
pub const FALLBACK_RANGE_END: u16 = 6890;

static BACKEND_PORT: AtomicU16 = AtomicU16::new(DEFAULT_BACKEND_PORT);

pub fn get() -> u16 {
    BACKEND_PORT.load(Ordering::Acquire)
}

pub fn set(port: u16) {
    BACKEND_PORT.store(port, Ordering::Release);
}

/// Test-only: reset the atomic to [`DEFAULT_BACKEND_PORT`] so test cases that
/// mutate it don't leak state into other tests in the same binary.
#[cfg(test)]
pub fn reset_for_tests() {
    BACKEND_PORT.store(DEFAULT_BACKEND_PORT, Ordering::Release);
}

/// Successful selection of a port from the fallback range when the default
/// port is foreign-held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedFallback {
    pub port: u16,
    pub original_occupant: String,
    pub original_pid: Option<u32>,
}

/// All probed ports were foreign-held. Vanishingly rare in practice (rapportd
/// only takes one port) but possible if the user has 23 unrelated daemons in
/// the 6868-6890 range, so we surface a structured error instead of looping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllForeign {
    pub original_occupant: String,
    pub original_pid: Option<u32>,
    pub fallback_range: (u16, u16),
}

/// Pick a port from `FALLBACK_RANGE_START..=FALLBACK_RANGE_END` for the
/// Python proxy when the default port is foreign-held. `try_bind` is called
/// for each candidate; the first that returns true wins. Caller should
/// have already determined that the default port is unavailable AND that
/// it isn't a stale headroom proxy of ours.
pub fn select_fallback(
    original_occupant: String,
    original_pid: Option<u32>,
    try_bind: impl Fn(u16) -> bool,
) -> Result<SelectedFallback, AllForeign> {
    for port in FALLBACK_RANGE_START..=FALLBACK_RANGE_END {
        if try_bind(port) {
            return Ok(SelectedFallback {
                port,
                original_occupant,
                original_pid,
            });
        }
    }
    Err(AllForeign {
        original_occupant,
        original_pid,
        fallback_range: (FALLBACK_RANGE_START, FALLBACK_RANGE_END),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_port_is_6868() {
        reset_for_tests();
        assert_eq!(get(), DEFAULT_BACKEND_PORT);
    }

    #[test]
    fn set_then_get_round_trips() {
        reset_for_tests();
        set(FALLBACK_RANGE_START + 1);
        assert_eq!(get(), FALLBACK_RANGE_START + 1);
        reset_for_tests();
    }

    #[test]
    fn select_fallback_returns_first_bindable_port() {
        // The first two fallback ports fail; the third succeeds.
        let result = select_fallback("rapportd pid 594".to_string(), Some(594), |port: u16| {
            port >= FALLBACK_RANGE_START + 2
        })
        .expect("ok");
        assert_eq!(
            result,
            SelectedFallback {
                port: FALLBACK_RANGE_START + 2,
                original_occupant: "rapportd pid 594".to_string(),
                original_pid: Some(594),
            }
        );
    }

    #[test]
    fn select_fallback_returns_range_start_when_all_bindable() {
        let result = select_fallback("x pid 1".to_string(), Some(1), |_| true).expect("ok");
        assert_eq!(result.port, FALLBACK_RANGE_START);
    }

    #[test]
    fn select_fallback_returns_all_foreign_when_no_port_binds() {
        let result = select_fallback("rapportd pid 594".to_string(), Some(594), |_| false)
            .expect_err("should be all-foreign");
        assert_eq!(result.original_occupant, "rapportd pid 594");
        assert_eq!(result.original_pid, Some(594));
        assert_eq!(
            result.fallback_range,
            (FALLBACK_RANGE_START, FALLBACK_RANGE_END)
        );
    }

    #[test]
    fn select_fallback_preserves_unknown_occupant_pid() {
        let result = select_fallback("unknown process".to_string(), None, |port| {
            port == FALLBACK_RANGE_START + 1
        })
        .expect("ok");
        assert_eq!(result.port, FALLBACK_RANGE_START + 1);
        assert_eq!(result.original_pid, None);
        assert_eq!(result.original_occupant, "unknown process");
    }
}
