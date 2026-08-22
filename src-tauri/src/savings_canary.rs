//! Zero-savings canary.
//!
//! The 2026-08-21 Codex CLI 0.149.0 regression (tools moved out of the
//! top-level `tools` array into `additional_tools` input items) ran 7+ peak
//! hours before a human spotted it on the admin page. Nothing errored:
//! requests forwarded and streamed normally, they just compressed to exactly
//! zero. Every alarm we had was error-based or fleet-aggregated, and the
//! server-side per-client rate canary that looks like the obvious answer
//! cannot see it - codex is almost never a user's only client, so its
//! dominant-client cohort was n=1 on the day of the incident.
//!
//! The signature is local and per-request: a large request that ran through
//! the compression pipeline and saved nothing. This module reads it off the
//! `/transformations/feed` batch `run_activity_observation` already polls, and
//! reports once per process to Sentry, where events aggregate across the
//! fleet on a fixed fingerprint. One machine reporting is a lead; a hundred
//! is a graph.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::models::TransformationFeedEvent;

/// Requests below this are too small to draw a conclusion from - a short turn
/// with nothing compressible legitimately saves zero.
const MIN_INPUT_TOKENS: u64 = 10_000;
/// Qualifying requests needed before the ratio means anything. The observer
/// pulls 150 events per pass, so a busy machine clears this easily and a
/// barely-used one stays quiet instead of paging on three samples.
const MIN_SAMPLE: usize = 20;
/// Healthy traffic scatters; a wire-format change zeroes essentially all of
/// it. Anything short of near-total is noise, not a regression.
const ZERO_RATIO: f64 = 0.9;

/// One report per process. A wedged client produces the same finding on every
/// observer pass, and the point is to learn that it happened, not how often.
static REPORTED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, PartialEq)]
pub struct Anomaly {
    /// Large, compression-eligible requests examined.
    pub sample: usize,
    /// How many of those saved nothing.
    pub zero: usize,
    /// Distinct output-shaper strata seen on the zero-saved requests. The
    /// `|notools` suffix is what named the Codex regression.
    pub strata: Vec<String>,
    /// Distinct `provider/model` pairs, to point at which client broke.
    pub models: Vec<String>,
}

/// Pick out the anomaly, or `None` when the batch looks healthy or is too
/// thin to judge. Pure: `observe` owns the reporting side effects.
pub fn detect(events: &[TransformationFeedEvent]) -> Option<Anomaly> {
    // An empty transform list means the pipeline never ran (bypass header,
    // optimize disabled, unlicensed). Those save zero by design and must not
    // page - only requests that were actually compressed count.
    let considered: Vec<&TransformationFeedEvent> = events
        .iter()
        .filter(|e| {
            !e.transforms_applied.is_empty()
                && e.input_tokens_original.unwrap_or(0) >= MIN_INPUT_TOKENS
        })
        .collect();

    if considered.len() < MIN_SAMPLE {
        return None;
    }

    let zeroed: Vec<&&TransformationFeedEvent> = considered
        .iter()
        .filter(|e| e.tokens_saved.unwrap_or(0) <= 0)
        .collect();

    if (zeroed.len() as f64) < considered.len() as f64 * ZERO_RATIO {
        return None;
    }

    // BTreeSet: deduped and ordered, so the Sentry extras are stable across
    // machines instead of reshuffling per batch.
    let mut strata = BTreeSet::new();
    let mut models = BTreeSet::new();
    for event in &zeroed {
        for transform in &event.transforms_applied {
            if let Some(stratum) = transform.strip_prefix("output_shaper:stratum:") {
                strata.insert(stratum.to_string());
            }
        }
        let provider = event.provider.as_deref().unwrap_or("?");
        let model = event.model.as_deref().unwrap_or("?");
        models.insert(format!("{provider}/{model}"));
    }

    Some(Anomaly {
        sample: considered.len(),
        zero: zeroed.len(),
        strata: strata.into_iter().take(5).collect(),
        models: models.into_iter().take(5).collect(),
    })
}

/// Report a detected anomaly to Sentry, at most once per process.
pub fn observe(events: &[TransformationFeedEvent]) {
    let Some(anomaly) = detect(events) else {
        return;
    };
    if REPORTED.swap(true, Ordering::AcqRel) {
        return;
    }

    let strata = anomaly.strata.join(", ");
    let models = anomaly.models.join(", ");
    // Fixed fingerprint: every affected machine lands in one issue, so the
    // event count is the blast radius. Counts stay out of it deliberately.
    let fingerprint: [&str; 1] = ["zero_savings_canary"];
    sentry::with_scope(
        |scope| {
            scope.set_tag("flow", "zero_savings_canary");
            scope.set_extra("sample", (anomaly.sample as u64).into());
            scope.set_extra("zero_saved", (anomaly.zero as u64).into());
            scope.set_extra("min_input_tokens", MIN_INPUT_TOKENS.into());
            scope.set_extra("strata", strata.clone().into());
            scope.set_extra("models", models.clone().into());
            scope.set_fingerprint(Some(fingerprint.as_slice()));
        },
        || {
            sentry::capture_message(
                &format!(
                    "zero_savings_canary: {}/{} large requests compressed to nothing \
                     (models: {models}; strata: {strata})",
                    anomaly.zero, anomaly.sample
                ),
                sentry::Level::Warning,
            );
        },
    );
    log::warn!(
        "zero-savings canary: {}/{} requests over {MIN_INPUT_TOKENS} tokens saved nothing \
         (models: {models}; strata: {strata})",
        anomaly.zero,
        anomaly.sample
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(input_tokens: u64, saved: i64, transforms: &[&str]) -> TransformationFeedEvent {
        TransformationFeedEvent {
            request_id: None,
            timestamp: None,
            provider: Some("openai".to_string()),
            model: Some("gpt-5.6-sol".to_string()),
            input_tokens_original: Some(input_tokens),
            input_tokens_optimized: Some(input_tokens.saturating_sub(saved.max(0) as u64)),
            tokens_saved: Some(saved),
            savings_percent: None,
            transforms_applied: transforms.iter().map(|t| t.to_string()).collect(),
            workspace: None,
            turn_id: None,
            request_messages: None,
            compressed_messages: None,
        }
    }

    /// The Codex 0.149.0 shape: compression ran, classified `notools`, saved 0.
    fn zeroed_codex_batch(count: usize) -> Vec<TransformationFeedEvent> {
        (0..count)
            .map(|_| {
                event(
                    40_000,
                    0,
                    &[
                        "output_shaper:stratum:gpt|new_user_ask|m|notools",
                        "output_shaper:verbosity:L2",
                    ],
                )
            })
            .collect()
    }

    #[test]
    fn detects_a_fleet_of_large_requests_saving_nothing() {
        let anomaly = detect(&zeroed_codex_batch(MIN_SAMPLE)).expect("anomaly");
        assert_eq!(anomaly.sample, MIN_SAMPLE);
        assert_eq!(anomaly.zero, MIN_SAMPLE);
        assert_eq!(anomaly.strata, vec!["gpt|new_user_ask|m|notools"]);
        assert_eq!(anomaly.models, vec!["openai/gpt-5.6-sol"]);
    }

    #[test]
    fn stays_quiet_below_the_sample_floor() {
        assert!(detect(&zeroed_codex_batch(MIN_SAMPLE - 1)).is_none());
    }

    #[test]
    fn stays_quiet_when_compression_is_working() {
        let healthy: Vec<_> = (0..MIN_SAMPLE * 2)
            .map(|_| event(40_000, 608, &["openai:responses:tool_schema_compaction"]))
            .collect();
        assert!(detect(&healthy).is_none());
    }

    /// Passthrough (bypass header, optimize off, unlicensed) saves zero by
    /// design and must never page: the pipeline never ran, so no transforms.
    #[test]
    fn ignores_passthrough_requests() {
        let passthrough: Vec<_> = (0..MIN_SAMPLE * 2).map(|_| event(40_000, 0, &[])).collect();
        assert!(detect(&passthrough).is_none());
    }

    /// Small turns legitimately have nothing to compress.
    #[test]
    fn ignores_small_requests() {
        let small: Vec<_> = (0..MIN_SAMPLE * 2)
            .map(|_| event(MIN_INPUT_TOKENS - 1, 0, &["output_shaper:verbosity:L2"]))
            .collect();
        assert!(detect(&small).is_none());
    }

    /// A minority of zero-saved requests is ordinary scatter, not a break.
    #[test]
    fn tolerates_a_minority_of_zero_saved_requests() {
        let mut mixed = zeroed_codex_batch(4);
        mixed.extend(
            (0..16).map(|_| event(40_000, 900, &["openai:responses:tool_schema_compaction"])),
        );
        assert!(detect(&mixed).is_none());
    }

    /// Missing `tokens_saved` (older proxy, or an outcome that never recorded
    /// one) reads as zero rather than being silently dropped from the count.
    #[test]
    fn treats_absent_tokens_saved_as_zero() {
        let mut batch = zeroed_codex_batch(MIN_SAMPLE);
        for entry in &mut batch {
            entry.tokens_saved = None;
        }
        assert_eq!(detect(&batch).expect("anomaly").zero, MIN_SAMPLE);
    }

    #[test]
    fn observe_reports_at_most_once_per_process() {
        // The static is process-wide, so this asserts the swap contract
        // directly rather than fighting the other tests over Sentry state.
        assert!(!REPORTED.swap(true, Ordering::AcqRel));
        assert!(REPORTED.swap(true, Ordering::AcqRel));
        REPORTED.store(false, Ordering::Release);
    }
}
