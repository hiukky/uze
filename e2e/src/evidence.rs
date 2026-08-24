//! The Lab evidence model: **L2** (real-harness conformance), **L4**
//! (model/provider behavior) and **CONTROL** scenarios.
//!
//! One status vocabulary for every scenario, one record shape for every
//! claim. A model failure (L4) can never rewrite an L2 result: they are
//! separate records with separate levels, and only L2 (plus the explicitly
//! gating scenarios) decides the Lab's exit code.
//!
//! Statuses are deliberately distinct so a reader can attribute a failure
//! without reading raw output:
//!
//! - [`Status::Pass`] — the exact claim was proven.
//! - [`Status::Unverified`] — no probe exists for this surface; absence of
//!   evidence, never a silent pass.
//! - [`Status::Skipped`] — the harness binary is not available (clean skip,
//!   not a failure).
//! - [`Status::InfraFailure`] — the Lab's own machinery failed (spawn,
//!   timeout, materialization).
//! - [`Status::HarnessFailure`] — the harness crashed or errored.
//! - [`Status::ProviderFailure`] — quota/credential/transport upstream.
//! - [`Status::ModelFailure`] — the model ran cleanly but did not exercise
//!   the capability (L4 only).
//! - [`Status::CapabilityFailure`] — the harness ran cleanly but does not
//!   exhibit the claimed capability.

use std::time::Duration;

use serde::Serialize;

/// Evidence level. `L2` and `L4` are the Lab's own evidence levels;
/// `Control` is a harness/provider-path control that is never a UZE verdict.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Level {
    L2,
    L4,
    Control,
}

impl Level {
    pub fn label(self) -> &'static str {
        match self {
            Level::L2 => "l2",
            Level::L4 => "l4",
            Level::Control => "control",
        }
    }
}

/// One verdict for one scenario/harness/capability surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Status {
    Pass,
    Unverified,
    Skipped,
    InfraFailure,
    HarnessFailure,
    ProviderFailure,
    ModelFailure,
    CapabilityFailure,
}

impl Status {
    /// The statuses that count as evidence of the claim.
    pub fn is_pass(self) -> bool {
        matches!(self, Status::Pass)
    }

    /// The statuses that must gate a release gate (real claim proven).
    pub fn is_evidence(self) -> bool {
        matches!(self, Status::Pass | Status::Unverified | Status::Skipped)
    }
}

/// One evidence record: what was claimed, at which level, and what happened.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct EvidenceRecord {
    pub harness: String,
    pub scenario: String,
    pub level: Level,
    /// The vendor-facing surface this record is about: skill / mcp / package
    /// / invocation-policy / shim / lifecycle / golden.
    pub capability: String,
    pub status: Status,
    /// Exactly what a `Pass` establishes. Two harnesses can both pass while
    /// proving different depths — the claim carries the depth.
    pub claim: String,
    /// Human-readable output excerpt / reason. Never the full megabyte dump.
    pub evidence: String,
    pub elapsed: Duration,
}

impl EvidenceRecord {
    #[allow(clippy::too_many_arguments)] // every field is a first-class part of the record
    pub fn new(
        harness: &str,
        scenario: &str,
        level: Level,
        capability: &str,
        status: Status,
        claim: &str,
        evidence: String,
        elapsed: Duration,
    ) -> Self {
        EvidenceRecord {
            harness: harness.to_owned(),
            scenario: scenario.to_owned(),
            level,
            capability: capability.to_owned(),
            status,
            claim: claim.to_owned(),
            evidence,
            elapsed,
        }
    }
}

/// Provider/quota/credential failure fragments. Kept as literal response
/// fragments because every harness reports upstream failures in its own text.
const PROVIDER_BLOCKS: [(&str, &str); 6] = [
    ("429", "provider rate limit or quota exhausted"),
    (
        "Too Many Requests",
        "provider rate limit or quota exhausted",
    ),
    ("insufficient_quota", "provider quota exhausted"),
    ("401", "provider rejected the credential"),
    ("invalid_api_key", "provider rejected the credential"),
    (
        "exceeded retry limit",
        "harness gave up reaching the provider",
    ),
];

/// `Some(ProviderFailure-reason)` when the output is upstream provider noise,
/// `None` when the failure belongs to the harness or the capability.
pub fn provider_failure(output: &str) -> Option<&'static str> {
    PROVIDER_BLOCKS
        .iter()
        .find(|(fragment, _)| output.contains(fragment))
        .map(|(_, reason)| *reason)
}

/// Truncated output for evidence. Full harness output can be megabytes —
/// OpenCode's skill listing embeds whole skill bodies — and an evidence
/// record nobody can read is not evidence.
pub fn excerpt(text: &str) -> String {
    const LIMIT: usize = 2000;
    if text.len() <= LIMIT {
        return text.to_owned();
    }
    let mut end = LIMIT;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… [{} bytes truncated]", &text[..end], text.len() - end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_and_credential_failures_are_provider_not_harness() {
        // The exact text a rate-limited Codex run emitted.
        assert_eq!(
            provider_failure("ERROR: exceeded retry limit, last status: 429 Too Many Requests"),
            Some("provider rate limit or quota exhausted")
        );
        assert_eq!(
            provider_failure("{\"code\":\"invalid_api_key\"}"),
            Some("provider rejected the credential")
        );
        // A genuine harness defect must not be excused as environment.
        assert_eq!(
            provider_failure("error=failed to parse function arguments: missing field `name`"),
            None
        );
    }

    #[test]
    fn only_pass_counts_as_evidence_of_the_claim() {
        assert!(Status::Pass.is_evidence());
        // Absence of a probe is a known gap, and a missing binary is a clean
        // skip — they count as honest results, not failures.
        for status in [Status::Unverified, Status::Skipped] {
            assert!(
                status.is_evidence(),
                "{status:?} must count as evidence-free honesty"
            );
            assert!(!status.is_pass());
        }
        for status in [
            Status::InfraFailure,
            Status::HarnessFailure,
            Status::ProviderFailure,
            Status::ModelFailure,
            Status::CapabilityFailure,
        ] {
            assert!(!status.is_evidence(), "{status:?} must gate");
            assert!(!status.is_pass());
        }
    }

    #[test]
    fn excerpt_truncates_without_splitting_a_character() {
        let text = "é".repeat(4000);
        let cut = excerpt(&text);
        assert!(cut.contains("truncated"));
        // Round-trips as valid UTF-8 precisely because the cut respects
        // character boundaries.
        assert!(cut.chars().next().is_some());
    }

    #[test]
    fn excerpt_leaves_short_output_untouched() {
        assert_eq!(excerpt("small"), "small");
    }
}
