//! Local recomputation of the output-shaper savings estimate.
//!
//! `/stats` reports whatever the backend's `SavingsLedger.best_estimate()`
//! returns, which has two defects we cannot wait on upstream to fix:
//!
//! 1. **Global-mean credit.** A treatment stratum the baseline never observed
//!    falls back to the all-requests mean. The seeded baseline only ever covers
//!    the model family the user ran *before* installing, so every later family
//!    (fable, sonnet, gpt) is scored against one number. On a real ledger that
//!    fallback produced 74% of the reported savings, crediting ~1,010 saved
//!    tokens to `sonnet|new_user_ask|m|notools` requests whose replies average
//!    73 tokens.
//! 2. **Ungated A/B switchover.** `best_estimate` prefers the measured holdout
//!    number as soon as a *single* stratum holds one sample in both arms, which
//!    is how three stale control samples once produced a -1439.9% reduction.
//!
//! We recompute from the same ledger file the proxy writes, which carries every
//! accumulator both estimators need, and apply our own rules: no global-mean
//! credit, and the measured number only wins once it covers a real share of
//! traffic at a usable confidence band.
//!
//! Read-only. The proxy owns this file; we never write it here.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;

/// The measured (A/B holdout) estimate replaces the synthetic-control one only
/// once strata with data in both arms account for this share of treatment
/// volume. Below it the holdout describes a corner of the traffic, not the
/// traffic.
const MEASURED_MIN_COVERAGE: f64 = 0.5;

/// ...and only once its 95% band is this tight. At a 3% holdout this takes a
/// heavy user a couple of months; a lighter one may never reach it, which is
/// the correct outcome -- they keep the synthetic-control number.
const MEASURED_MAX_CI_HALF_WIDTH_PCT: f64 = 10.0;

/// Running count / sum / sum-of-squares, mirroring the backend's `_Accum` so
/// mean and variance come out bit-comparable.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(default)]
struct Accum {
    n: u64,
    sum: f64,
    sumsq: f64,
}

impl Accum {
    fn mean(&self) -> f64 {
        if self.n == 0 {
            0.0
        } else {
            self.sum / self.n as f64
        }
    }

    /// Sample variance (unbiased); 0 below two observations, as upstream.
    fn var(&self) -> f64 {
        if self.n < 2 {
            return 0.0;
        }
        let n = self.n as f64;
        ((self.sumsq - self.sum * self.sum / n) / (n - 1.0)).max(0.0)
    }

    fn merge(&mut self, other: &Accum) {
        self.n += other.n;
        self.sum += other.sum;
        self.sumsq += other.sumsq;
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct Baseline {
    strata: HashMap<String, Accum>,
    /// The all-requests accumulator. Deliberately unused: crediting a stratum
    /// the baseline never saw against the global mean is the overcount this
    /// module exists to remove.
    #[allow(dead_code)]
    glob: Accum,
}

impl Baseline {
    /// `(mean, var, n)` for *key*: the exact stratum, else every stratum
    /// sharing its longest matching prefix, merged.
    ///
    /// Upstream trims trailing fields the same way but takes the first hash
    /// order match, so a re-serialized ledger can change its answer. Merging
    /// every match is order-independent and uses more evidence. Returns `None`
    /// rather than falling back to the global mean.
    fn lookup(&self, key: &str) -> Option<(f64, f64, u64)> {
        if let Some(acc) = self.strata.get(key) {
            if acc.n > 0 {
                return Some((acc.mean(), acc.var(), acc.n));
            }
        }
        let mut parts: Vec<&str> = key.split('|').collect();
        while parts.len() > 1 {
            parts.pop();
            let prefix = format!("{}|", parts.join("|"));
            let mut merged = Accum::default();
            for (k, acc) in &self.strata {
                if acc.n > 0 && k.starts_with(&prefix) {
                    merged.merge(acc);
                }
            }
            if merged.n > 0 {
                return Some((merged.mean(), merged.var(), merged.n));
            }
        }
        None
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct Ledger {
    baseline: Baseline,
    treatment: HashMap<String, Accum>,
    control: HashMap<String, Accum>,
}

/// One side of the counterfactual, ready for the dashboard.
#[derive(Debug, Clone, PartialEq)]
pub struct OutputEstimate {
    /// "estimated" (synthetic control) or "measured" (A/B holdout).
    pub method: &'static str,
    pub reduction_percent: f64,
    pub ci_low_percent: f64,
    pub ci_high_percent: f64,
    /// Treatment requests the estimate actually covers -- not every shaped
    /// request, since strata without baseline evidence are excluded.
    pub requests: u64,
    pub tokens_saved: u64,
    pub baseline_tokens: u64,
}

/// Path to the proxy's savings ledger. Mirrors the backend's `workspace_dir()`
/// default of `~/.headroom` (neither the proxy nor the seeding run sets
/// `HEADROOM_WORKSPACE_DIR`, so both resolve here).
pub fn ledger_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(dirs::home_dir)?;
    Some(home.join(".headroom").join("output_savings.json"))
}

/// Best available estimate from the on-disk ledger, or `None` when there is no
/// readable ledger, no baseline evidence, or the result fails a sanity check.
///
/// Cheap enough to call per dashboard build (a few KB of JSON), and tolerant of
/// a torn read: the backend writes this file with a plain `write_text`, so a
/// concurrent flush can hand us a truncated document. That parses to `None` and
/// the caller falls back to the previous value rather than showing a wrong one.
pub fn estimate() -> Option<OutputEstimate> {
    let bytes = std::fs::read(ledger_path()?).ok()?;
    estimate_from_bytes(&bytes)
}

fn estimate_from_bytes(bytes: &[u8]) -> Option<OutputEstimate> {
    let ledger: Ledger = serde_json::from_slice(bytes).ok()?;
    let estimated = estimate_from_baseline(&ledger);
    match measured_if_ready(&ledger, &estimated) {
        Some(measured) => Some(measured),
        None => estimated,
    }
}

/// Synthetic control: treatment output against the learned baseline, over the
/// strata the baseline can actually speak to.
///
/// `Σ n·(μ − ȳ)` with `Var ≈ Σ [ n·σ²_y + n²·σ²_μ/m ]`, matching the backend's
/// derivation so the only difference is which strata qualify.
fn estimate_from_baseline(ledger: &Ledger) -> Option<OutputEstimate> {
    let mut saved = 0.0;
    let mut baseline_tokens = 0.0;
    let mut var = 0.0;
    let mut requests = 0u64;

    for (key, acc) in &ledger.treatment {
        if acc.n == 0 {
            continue;
        }
        let Some((mu, mu_var, m)) = ledger.baseline.lookup(key) else {
            continue;
        };
        let n = acc.n as f64;
        requests += acc.n;
        saved += n * (mu - acc.mean());
        baseline_tokens += n * mu;
        var += n * acc.var() + (n * n) * (mu_var / m as f64);
    }

    finalize("estimated", saved, baseline_tokens, var, requests)
}

/// A/B measurement: per-stratum control mean minus treatment mean, over strata
/// with data in both arms. `None` until the holdout has produced any.
fn estimate_from_holdout(ledger: &Ledger) -> Option<OutputEstimate> {
    let mut saved = 0.0;
    let mut baseline_tokens = 0.0;
    let mut var = 0.0;
    let mut requests = 0u64;

    for (key, t) in &ledger.treatment {
        let Some(c) = ledger.control.get(key) else {
            continue;
        };
        if t.n == 0 || c.n == 0 {
            continue;
        }
        let n = t.n as f64;
        requests += t.n;
        saved += n * (c.mean() - t.mean());
        baseline_tokens += n * c.mean();
        var += (n * n) * (c.var() / c.n as f64 + t.var() / t.n as f64);
    }

    finalize("measured", saved, baseline_tokens, var, requests)
}

/// The measured estimate, but only once it is worth showing: it must cover
/// [`MEASURED_MIN_COVERAGE`] of the shaped traffic and carry a band no wider
/// than [`MEASURED_MAX_CI_HALF_WIDTH_PCT`].
///
/// Coverage is measured against the same denominator the synthetic control
/// uses, so a holdout only displaces an estimate it genuinely outgrew.
fn measured_if_ready(
    ledger: &Ledger,
    estimated: &Option<OutputEstimate>,
) -> Option<OutputEstimate> {
    let measured = estimate_from_holdout(ledger)?;
    let shaped: u64 = ledger.treatment.values().map(|acc| acc.n).sum();
    if shaped == 0 {
        return None;
    }
    let covered = measured.requests as f64 / shaped as f64;
    if covered < MEASURED_MIN_COVERAGE {
        return None;
    }
    let half_width = (measured.ci_high_percent - measured.ci_low_percent) / 2.0;
    if half_width > MEASURED_MAX_CI_HALF_WIDTH_PCT {
        return None;
    }
    // Never trade a usable estimate for a measurement of less traffic.
    if estimated
        .as_ref()
        .is_some_and(|e| e.requests > measured.requests)
    {
        return None;
    }
    Some(measured)
}

/// Percent + 95% normal-approximation band, or `None` when the result is not a
/// reduction we can honestly display. A reduction is definitionally within
/// [0, 100]: a freshly seeded baseline divides by near-zero and blows up, and
/// the dashboard used to render that as "Output --6,130.7%".
fn finalize(
    method: &'static str,
    saved: f64,
    baseline_tokens: f64,
    var: f64,
    requests: u64,
) -> Option<OutputEstimate> {
    if requests == 0 || baseline_tokens <= 0.0 || !baseline_tokens.is_finite() {
        return None;
    }
    let pct = saved / baseline_tokens * 100.0;
    if !pct.is_finite() || !(0.0..=100.0).contains(&pct) {
        return None;
    }
    let se = var.max(0.0).sqrt();
    Some(OutputEstimate {
        method,
        reduction_percent: pct,
        ci_low_percent: (saved - 1.96 * se) / baseline_tokens * 100.0,
        ci_high_percent: (saved + 1.96 * se) / baseline_tokens * 100.0,
        requests,
        tokens_saved: saved.max(0.0).round() as u64,
        baseline_tokens: baseline_tokens.round() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Baseline covers `opus` only; treatment also ran `fable`. Mirrors the
    /// real shape of a seeded ledger.
    const MIXED: &str = r#"{
      "baseline": {
        "strata": {
          "opus|ask|l|tools":  {"n": 10, "sum": 10000, "sumsq": 10200000},
          "opus|ask|xl|tools": {"n": 5,  "sum": 10000, "sumsq": 20500000}
        },
        "glob": {"n": 15, "sum": 20000, "sumsq": 30700000}
      },
      "treatment": {
        "opus|ask|l|tools":   {"n": 4, "sum": 3200, "sumsq": 2580000},
        "opus|ask|l|notools": {"n": 2, "sum": 1500, "sumsq": 1130000},
        "fable|ask|l|tools":  {"n": 3, "sum": 2100, "sumsq": 1490000}
      },
      "control": {}
    }"#;

    /// One stratum, a healthy holdout on it.
    const HOLDOUT: &str = r#"{
      "baseline": {
        "strata": {"opus|ask|l|tools": {"n": 100, "sum": 95000, "sumsq": 90490000}},
        "glob":   {"n": 100, "sum": 95000, "sumsq": 90490000}
      },
      "treatment": {"opus|ask|l|tools": {"n": 1000, "sum": 800000, "sumsq": 649990000}},
      "control":   {"opus|ask|l|tools": {"n": 100,  "sum": 100000, "sumsq": 100990000}}
    }"#;

    fn with_control(control: &str) -> String {
        let idx = HOLDOUT
            .find("\"control\"")
            .expect("fixture has a control arm");
        format!("{}{control}\n    }}", &HOLDOUT[..idx])
    }

    fn estimate(json: &str) -> OutputEstimate {
        estimate_from_bytes(json.as_bytes()).expect("estimate")
    }

    #[test]
    fn excludes_strata_the_baseline_never_saw() {
        let e = estimate(MIXED);
        assert_eq!(e.method, "estimated");
        // 4 opus|ask|l|tools + 2 opus|ask|l|notools; the 3 fable requests have
        // no baseline evidence and must not be credited against the global mean.
        assert_eq!(e.requests, 6);
        assert_eq!(e.tokens_saved, 1300);
        assert_eq!(e.baseline_tokens, 6000);
        assert!((e.reduction_percent - 21.666_667).abs() < 1e-5);
        assert!((e.ci_low_percent - 12.363_196).abs() < 1e-5);
        assert!((e.ci_high_percent - 30.970_137).abs() < 1e-5);
    }

    #[test]
    fn prefix_backoff_supplies_a_near_neighbour() {
        // opus|ask|l|notools has no exact stratum but shares the opus|ask|l|
        // prefix with one, so it is scored -- against 1000, not the 1333 global.
        let ledger: Ledger = serde_json::from_str(MIXED).expect("parse");
        let (mu, _var, n) = ledger
            .baseline
            .lookup("opus|ask|l|notools")
            .expect("prefix hit");
        assert_eq!((mu, n), (1000.0, 10));
        assert!(ledger.baseline.lookup("fable|ask|l|tools").is_none());
    }

    #[test]
    fn measured_replaces_estimated_once_it_is_solid() {
        let e = estimate(HOLDOUT);
        assert_eq!(e.method, "measured");
        assert!((e.reduction_percent - 20.0).abs() < 1e-9);
        assert_eq!(e.requests, 1000);
    }

    #[test]
    fn a_thin_control_arm_never_takes_over() {
        // Three-sample-control territory: the point estimate looks fine (20%)
        // but the band spans -257%..297%, so the synthetic control stands.
        let e = estimate(&with_control(
            r#""control": {"opus|ask|l|tools": {"n": 2, "sum": 2000, "sumsq": 6000000}}"#,
        ));
        assert_eq!(e.method, "estimated");
        assert!((e.reduction_percent - 15.789_474).abs() < 1e-5);
    }

    #[test]
    fn a_holdout_covering_a_corner_of_traffic_never_takes_over() {
        // Same tight holdout, but a second stratum carries 4/5 of the shaped
        // requests and has no control samples at all.
        let json = HOLDOUT.replace(
            r#""treatment": {"opus|ask|l|tools": {"n": 1000, "sum": 800000, "sumsq": 649990000}}"#,
            r#""treatment": {"opus|ask|l|tools": {"n": 1000, "sum": 800000, "sumsq": 649990000},
                             "opus|ask|xl|tools": {"n": 4000, "sum": 4000000, "sumsq": 4009990000}}"#,
        ).replace(
            r#""strata": {"opus|ask|l|tools": {"n": 100, "sum": 95000, "sumsq": 90490000}}"#,
            r#""strata": {"opus|ask|l|tools": {"n": 100, "sum": 95000, "sumsq": 90490000},
                          "opus|ask|xl|tools": {"n": 100, "sum": 120000, "sumsq": 144990000}}"#,
        );
        let e = estimate(&json);
        assert_eq!(e.method, "estimated");
        assert_eq!(e.requests, 5000);
    }

    #[test]
    fn no_baseline_evidence_yields_nothing() {
        let json = r#"{"baseline": {"strata": {}, "glob": {"n": 15, "sum": 20000, "sumsq": 30700000}},
                       "treatment": {"fable|ask|l|tools": {"n": 3, "sum": 2100, "sumsq": 1490000}}}"#;
        assert!(estimate_from_bytes(json.as_bytes()).is_none());
    }

    #[test]
    fn a_torn_or_absent_ledger_is_not_an_estimate() {
        assert!(estimate_from_bytes(b"").is_none());
        assert!(estimate_from_bytes(br#"{"baseline": {"strata": {"opus|ask|l|to"#).is_none());
        assert!(estimate_from_bytes(b"{}").is_none());
    }

    #[test]
    fn a_shaper_that_made_replies_longer_is_not_shown_as_a_reduction() {
        // Treatment above baseline gives a negative percent, which the tile
        // used to render as "Output --6,130.7%".
        let json = r#"{
          "baseline": {"strata": {"opus|ask|l|tools": {"n": 10, "sum": 1000, "sumsq": 110000}}, "glob": {"n": 0, "sum": 0, "sumsq": 0}},
          "treatment": {"opus|ask|l|tools": {"n": 4, "sum": 3200, "sumsq": 2580000}}
        }"#;
        assert!(estimate_from_bytes(json.as_bytes()).is_none());
    }
}
