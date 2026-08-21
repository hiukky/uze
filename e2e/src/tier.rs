//! The three conformance tiers, separated by what each one can fail on.
//!
//! The separation is the point. Tier 1 and Tier 2 are deterministic: given
//! the same image they either pass or expose a real defect, and they need no
//! network, no provider credential and no model. Tier 3 is the only tier a
//! model can fail, and it is the only tier that costs money. Collapsing them
//! — which the earlier per-harness shell scripts did — makes a deterministic
//! fixture defect present as a flaky environment, which is exactly how one
//! was missed for hours.
//!
//! Nothing here is harness-specific: every tier is generic over
//! [`crate::harness::HarnessSpec`].

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::Serialize;

use crate::{
    EvidenceState, HarnessRunResult, HarnessRunSpec,
    harness::{ArtifactKind, HarnessSpec},
    run,
};

/// Everything a tier needs that is not the harness under test. Constructed
/// once per run so all three tiers observe the same disposable environment.
#[derive(Clone, Debug)]
pub struct LabEnvironment {
    pub uze: PathBuf,
    pub home: PathBuf,
    pub uze_home: PathBuf,
    pub workspace: PathBuf,
    pub timeout: Duration,
    /// Resolved path of the fixture MCP server, so a probe can identify this
    /// package through the binary a harness reports rather than through a
    /// name the harness may have rewritten.
    pub mcp_binary: PathBuf,
    /// Values every spawned process receives on top of HOME/UZE_HOME. The
    /// runner clears the ambient environment, so anything a harness needs
    /// must be declared rather than inherited from the operator's shell.
    pub environment: BTreeMap<String, String>,
}

impl LabEnvironment {
    fn spec(&self, executable: &str, arguments: Vec<String>) -> HarnessRunSpec {
        HarnessRunSpec {
            executable: PathBuf::from(executable),
            arguments,
            environment: self.environment.clone(),
            home: self.home.clone(),
            uze_home: self.uze_home.clone(),
            working_directory: self.workspace.clone(),
            stdin: None,
            timeout: self.timeout,
        }
    }
}

/// One thing UZE attached, as UZE itself reports it. The `name` is whatever
/// the harness can be asked about for that delivery shape, never a constant
/// this crate carries.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Attachment {
    pub kind_tag: String,
    pub name: String,
    pub state: String,
    pub reason: String,
    pub resource_identity: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProbeOutcome {
    pub arguments: Vec<String>,
    /// What a pass on this probe establishes. Carried into the evidence so
    /// equal states across harnesses are never read as equal depth.
    pub claim: String,
    pub attachment: Attachment,
    pub state: EvidenceState,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct TierReport {
    pub harness: String,
    pub tier: &'static str,
    pub state: EvidenceState,
    pub attachments: Vec<Attachment>,
    pub probes: Vec<ProbeOutcome>,
    pub detail: Option<String>,
}

impl TierReport {
    pub fn passed(&self) -> bool {
        matches!(
            self.state,
            EvidenceState::AttachmentVerified
                | EvidenceState::DiscoveryVerified
                | EvidenceState::LocalBehaviorVerified
                | EvidenceState::VendorBehaviorVerified
        )
    }
}

fn combined_output(result: &HarnessRunResult) -> String {
    let mut text = String::from_utf8_lossy(&result.stdout).into_owned();
    text.push('\n');
    text.push_str(&String::from_utf8_lossy(&result.stderr));
    text
}

/// Truncated output for evidence. Full harness output can be megabytes —
/// OpenCode's skill listing embeds whole skill bodies — and an evidence
/// record nobody can read is not evidence.
fn excerpt(text: &str) -> String {
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

/// Tier 1 — attachment. Asks UZE what it attached for this harness and
/// requires every receipt to reconcile. No harness binary is executed, so
/// this tier runs even for a harness that is not installed.
pub fn attachment(
    environment: &LabEnvironment,
    harness: &HarnessSpec,
    package_id: &str,
) -> TierReport {
    let uze = environment.uze.to_string_lossy().into_owned();
    let result = run(&environment.spec(
        &uze,
        vec![
            "inspect".to_owned(),
            package_id.to_owned(),
            "--format".to_owned(),
            "json".to_owned(),
        ],
    ));
    let mut report = TierReport {
        harness: harness.id.to_owned(),
        tier: "attachment",
        state: EvidenceState::Failed,
        attachments: Vec::new(),
        probes: Vec::new(),
        detail: None,
    };
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            report.state = EvidenceState::BlockedByEnvironment;
            report.detail = Some(error.to_string());
            return report;
        }
    };
    if result.timed_out {
        report.state = EvidenceState::TimedOut;
        report.detail = Some("uze inspect did not return before the tier timeout".to_owned());
        return report;
    }
    if result.exit_code != Some(0) {
        report.detail = Some(format!(
            "uze inspect exited with {:?}: {}",
            result.exit_code,
            excerpt(&combined_output(&result))
        ));
        return report;
    }

    let document: serde_json::Value = match serde_json::from_slice(&result.stdout) {
        Ok(document) => document,
        Err(error) => {
            report.detail = Some(format!("uze inspect emitted unparsable JSON: {error}"));
            return report;
        }
    };
    let receipts = document
        .get("reconciliation")
        .and_then(|value| value.get("receipts"))
        .and_then(serde_json::Value::as_array);
    let Some(receipts) = receipts else {
        report.detail = Some("uze inspect JSON has no reconciliation.receipts array".to_owned());
        return report;
    };

    let mut unreconciled = Vec::new();
    for entry in receipts {
        let receipt = entry.get("receipt");
        let integration = receipt
            .and_then(|value| value.get("integration"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if integration != harness.uze_name {
            continue;
        }
        let inspection = entry.get("inspection");
        let state = inspection
            .and_then(|value| value.get("state"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("UNKNOWN")
            .to_owned();
        let reason = inspection
            .and_then(|value| value.get("reason"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let Some((tag, body)) = receipt
            .and_then(|value| value.get("artifact"))
            .and_then(serde_json::Value::as_object)
            .and_then(|artifact| artifact.iter().next())
        else {
            unreconciled.push("a receipt carries no artifact".to_owned());
            continue;
        };
        let Some(name) = attachment_name(tag, body) else {
            unreconciled.push(format!("artifact {tag} exposes no probe-able name"));
            continue;
        };
        if state != "MATCHED" {
            unreconciled.push(format!("{name} is {state}: {reason}"));
        }
        if let Some(over) = exceeds_tool_name_budget(tag, &name) {
            unreconciled.push(over);
        }
        report.attachments.push(Attachment {
            kind_tag: tag.clone(),
            name,
            state,
            reason,
            resource_identity: receipt
                .and_then(|value| value.get("resource_identity"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
        });
    }

    if report.attachments.is_empty() {
        report.detail = Some(format!(
            "UZE reports no attachment for integration {}",
            harness.uze_name
        ));
        return report;
    }
    if !unreconciled.is_empty() {
        report.detail = Some(unreconciled.join("; "));
        return report;
    }
    report.state = EvidenceState::AttachmentVerified;
    report
}

/// Signals that a run failed for a reason outside the integration under
/// test. Kept as literal response fragments rather than parsed status codes
/// because each harness reports upstream failures in its own text.
const ENVIRONMENT_BLOCKS: [(&str, &str); 6] = [
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

fn environment_block(output: &str) -> Option<&'static str> {
    ENVIRONMENT_BLOCKS
        .iter()
        .find(|(fragment, _)| output.contains(fragment))
        .map(|(_, reason)| *reason)
}

/// Providers cap the function name of a tool call. OpenAI rejects anything
/// over 64 characters with `Invalid 'messages[..].tool_calls[0].function.name:
/// string too long`, and Anthropic applies its own limit.
const TOOL_NAME_LIMIT: usize = 64;

/// Room left for the tool name a harness appends to a UZE-attached MCP entry.
/// The composed name is `<entry_name>_<tool_name>`, and the tool name belongs
/// to the MCP server rather than to UZE, so UZE's portion must leave space.
///
/// This is a floor, not a guarantee: it is the length of this fixture's own
/// `_uze_conformance` suffix, so a package exposing a longer tool name can
/// still exceed the limit while passing here. It exists to catch the
/// egregious case — an entry name with no realistic room at all — at the tier
/// that costs nothing.
const TOOL_NAME_RESERVE: usize = 16;

/// Catches, deterministically and for free, a defect that otherwise only
/// appears as a provider 400 in the paid behavior tier: an entry name long
/// enough that no tool it exposes can be called. Discovery still passes with
/// an over-long name, which is what makes this easy to reintroduce.
fn exceeds_tool_name_budget(tag: &str, name: &str) -> Option<String> {
    if ArtifactKind::from_tag(tag)? != ArtifactKind::VendorConfigEntry {
        return None;
    }
    let budget = TOOL_NAME_LIMIT - TOOL_NAME_RESERVE;
    (name.len() > budget).then(|| {
        format!(
            "{name} is {} characters; an MCP entry name over {budget} leaves no room for a tool \
             name inside the provider's {TOOL_NAME_LIMIT}-character limit",
            name.len()
        )
    })
}

/// The name a harness can be asked about, per delivery shape. A vendor
/// config entry is named by UZE; a managed symlink is found by its directory
/// entry; a natively installed package is named by its marketplace selector.
fn attachment_name(tag: &str, body: &serde_json::Value) -> Option<String> {
    match ArtifactKind::from_tag(tag)? {
        ArtifactKind::VendorConfigEntry => body
            .get("entry_name")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        ArtifactKind::SymlinkReference => body
            .get("path")
            .and_then(serde_json::Value::as_str)
            .and_then(|path| {
                Path::new(path)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            }),
        ArtifactKind::IntegrationOwned => body
            .get("selector")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
    }
}

/// Tier 2 — discovery. Asks the harness itself whether it can see each thing
/// UZE attached, using only the harness's own reporting subcommands. Still no
/// model, no network, no credential.
///
/// An attachment whose kind the harness cannot report on without a model is
/// recorded `Unverified`, never silently passed: absence of a probe is a
/// known gap, not evidence.
pub fn discovery(
    environment: &LabEnvironment,
    harness: &HarnessSpec,
    attachments: &[Attachment],
) -> TierReport {
    let mut report = TierReport {
        harness: harness.id.to_owned(),
        tier: "discovery",
        state: EvidenceState::DiscoveryVerified,
        attachments: attachments.to_vec(),
        probes: Vec::new(),
        detail: None,
    };
    let mut failures = Vec::new();

    for attachment in attachments {
        let Some(kind) = ArtifactKind::from_tag(&attachment.kind_tag) else {
            continue;
        };
        let probes = harness.probes_for(kind);
        if probes.is_empty() {
            report.probes.push(ProbeOutcome {
                arguments: Vec::new(),
                attachment: attachment.clone(),
                claim: String::new(),
                state: EvidenceState::Unverified,
                detail: format!(
                    "{} declares no model-free probe for {}",
                    harness.id, attachment.kind_tag
                ),
            });
            continue;
        }
        for probe in probes {
            let arguments: Vec<String> = probe
                .arguments
                .iter()
                .map(|value| value.to_string())
                .collect();
            let outcome = match run(&environment.spec(harness.executable, arguments.clone())) {
                Err(error) => ProbeOutcome {
                    arguments,
                    claim: probe.claim.to_owned(),
                    attachment: attachment.clone(),
                    state: EvidenceState::BlockedByEnvironment,
                    detail: error.to_string(),
                },
                Ok(result) if result.timed_out => ProbeOutcome {
                    arguments,
                    claim: probe.claim.to_owned(),
                    attachment: attachment.clone(),
                    state: EvidenceState::TimedOut,
                    detail: excerpt(&combined_output(&result)),
                },
                Ok(result) => {
                    let output = combined_output(&result);
                    let mut missing = Vec::new();
                    if probe.matches_attached_name && !output.contains(&attachment.name) {
                        missing.push(format!("attached name {}", attachment.name));
                    }
                    for fragment in probe.required {
                        let fragment = fragment
                            .replace("{mcp_binary}", &environment.mcp_binary.to_string_lossy());
                        if !output.contains(&fragment) {
                            missing.push(format!("required fragment {fragment}"));
                        }
                    }
                    if result.exit_code != Some(0) {
                        missing.push(format!("non-zero exit {:?}", result.exit_code));
                    }
                    if missing.is_empty() {
                        ProbeOutcome {
                            arguments,
                            claim: probe.claim.to_owned(),
                            attachment: attachment.clone(),
                            state: EvidenceState::DiscoveryVerified,
                            detail: String::new(),
                        }
                    } else {
                        ProbeOutcome {
                            arguments,
                            claim: probe.claim.to_owned(),
                            attachment: attachment.clone(),
                            state: EvidenceState::Failed,
                            detail: format!(
                                "missing {}; output: {}",
                                missing.join(", "),
                                excerpt(&output)
                            ),
                        }
                    }
                }
            };
            if !matches!(
                outcome.state,
                EvidenceState::DiscoveryVerified | EvidenceState::Unverified
            ) {
                failures.push(format!(
                    "{} [{}]: {}",
                    outcome.attachment.name,
                    outcome.arguments.join(" "),
                    outcome.detail
                ));
            }
            report.probes.push(outcome);
        }
    }

    if !failures.is_empty() {
        report.state = EvidenceState::Failed;
        report.detail = Some(failures.join(" | "));
    } else if report
        .probes
        .iter()
        .all(|probe| matches!(probe.state, EvidenceState::Unverified))
    {
        report.state = EvidenceState::Unverified;
        report.detail = Some("no model-free probe covered any attachment".to_owned());
    }
    report
}

/// Tier 3 — behavior. One real model turn that must surface a proof token the
/// prompt never contains. This is the only tier a model can fail and the only
/// one that needs the gateway.
///
/// It deliberately does not retry. A retry loop here previously hid a
/// deterministic fixture defect by making it look intermittent; a failure
/// must stay visible so it can be attributed.
pub fn behavior(
    environment: &LabEnvironment,
    harness: &HarnessSpec,
    gateway: &str,
    prompt: &str,
    proof: &str,
) -> TierReport {
    let mut report = TierReport {
        harness: harness.id.to_owned(),
        tier: "behavior",
        state: EvidenceState::Failed,
        attachments: Vec::new(),
        probes: Vec::new(),
        detail: None,
    };
    let Some(behavior) = harness.behavior else {
        // Recorded, never silently skipped: this harness has no routable
        // behavioral tier, which is a known gap rather than a pass.
        report.state = EvidenceState::Unverified;
        report.detail = Some(format!(
            "{} declares no gateway-routable behavioral tier; deterministic tiers cover it",
            harness.id
        ));
        return report;
    };
    let workspace = environment.workspace.to_string_lossy().into_owned();
    let substitute = |value: &str| {
        value
            .replace("{model}", behavior.model)
            .replace("{gateway}", gateway)
            .replace("{workspace}", &workspace)
            .replace("{prompt}", prompt)
    };
    let arguments: Vec<String> = behavior
        .arguments
        .iter()
        .map(|value| substitute(value))
        .collect();
    let mut spec = environment.spec(harness.executable, arguments);
    for (name, value) in behavior.environment {
        spec.environment
            .insert((*name).to_owned(), substitute(value));
    }

    match run(&spec) {
        Err(error) => {
            report.state = EvidenceState::BlockedByEnvironment;
            report.detail = Some(error.to_string());
        }
        Ok(result) if result.timed_out => {
            report.state = EvidenceState::TimedOut;
            report.detail = Some(excerpt(&combined_output(&result)));
        }
        Ok(result) => {
            let output = combined_output(&result);
            if output.contains(proof) {
                report.state = EvidenceState::LocalBehaviorVerified;
            } else if result.exit_code == Some(0) {
                // The harness ran cleanly and the model simply did not use
                // the capability. That is a model failure, and per the L2
                // spec it must never be rewritten into an integration
                // incompatibility claim.
                report.state = EvidenceState::ModelFailure;
                report.detail = Some(excerpt(&output));
            } else if let Some(reason) = environment_block(&output) {
                // Quota, auth and transport problems belong to the
                // environment. Reporting them as a harness defect would let
                // an expired key or a rate limit read as an integration
                // regression — the exact conflation the tiers exist to
                // prevent.
                report.state = EvidenceState::BlockedByEnvironment;
                report.detail = Some(format!("{reason}: {}", excerpt(&output)));
            } else {
                report.state = EvidenceState::HarnessFailure;
                report.detail = Some(format!("exit {:?}: {}", result.exit_code, excerpt(&output)));
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn attachment_names_follow_the_delivery_shape() {
        assert_eq!(
            attachment_name("VENDOR_CONFIG_ENTRY", &json!({"entry_name": "uze-x-y"})),
            Some("uze-x-y".to_owned())
        );
        assert_eq!(
            attachment_name(
                "SYMLINK_REFERENCE",
                &json!({"path": "/home/n/.agents/skills/uze-x"})
            ),
            Some("uze-x".to_owned())
        );
        assert_eq!(
            attachment_name("INTEGRATION_OWNED", &json!({"selector": "pkg@market"})),
            Some("pkg@market".to_owned())
        );
        assert_eq!(attachment_name("UNKNOWN_SHAPE", &json!({})), None);
    }

    #[test]
    fn quota_and_credential_failures_are_environment_not_harness_defects() {
        // The exact text a rate-limited Codex run emitted.
        assert_eq!(
            environment_block("ERROR: exceeded retry limit, last status: 429 Too Many Requests"),
            Some("provider rate limit or quota exhausted")
        );
        assert_eq!(
            environment_block("{\"code\":\"invalid_api_key\"}"),
            Some("provider rejected the credential")
        );
        // A genuine harness defect must not be excused as environment.
        assert_eq!(
            environment_block("error=failed to parse function arguments: missing field `name`"),
            None
        );
    }

    #[test]
    fn an_over_long_mcp_entry_name_fails_the_cheap_tier() {
        // The exact name that produced a provider 400 in the behavior tier:
        // uze-uze-plugin-first-conformance-uze-plugin-first-conformance.
        let over_long = "uze-uze-plugin-first-conformance-uze-plugin-first-conformance";
        assert!(exceeds_tool_name_budget("VENDOR_CONFIG_ENTRY", over_long).is_some());
        assert!(
            exceeds_tool_name_budget(
                "VENDOR_CONFIG_ENTRY",
                "uze-uze-plugin-first-conformance-conformance"
            )
            .is_none()
        );
    }

    #[test]
    fn the_budget_only_constrains_names_uze_itself_composes() {
        // An integration-owned selector never becomes a tool-call function name.
        let over_long = "uze-uze-plugin-first-conformance-uze-plugin-first-conformance";
        assert!(exceeds_tool_name_budget("INTEGRATION_OWNED", over_long).is_none());
        assert!(exceeds_tool_name_budget("SYMLINK_REFERENCE", over_long).is_none());
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

    #[test]
    fn tier_report_only_claims_a_pass_for_verified_states() {
        let base = TierReport {
            harness: "x".to_owned(),
            tier: "discovery",
            state: EvidenceState::DiscoveryVerified,
            attachments: Vec::new(),
            probes: Vec::new(),
            detail: None,
        };
        assert!(base.passed());
        for state in [
            EvidenceState::Unverified,
            EvidenceState::Failed,
            EvidenceState::ModelFailure,
            EvidenceState::HarnessFailure,
            EvidenceState::TimedOut,
            EvidenceState::BlockedByEnvironment,
        ] {
            let report = TierReport {
                state,
                ..base.clone()
            };
            assert!(!report.passed(), "{state:?} must not count as a pass");
        }
    }
}
