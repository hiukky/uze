//! Universal user preferences and their per-harness translation contract.
//!
//! UZE models user *intent* ("I want to work autonomously, keep writes
//! scoped to the workspace, use a capable model"), never vendor
//! configuration. Each integration is responsible for translating a resolved
//! [`Preferences`] value into its own native settings — this module only
//! defines the vocabulary both sides speak. Translation fidelity reuses
//! [`CompatibilityRoute`], the same vocabulary the capability router already
//! uses, rather than a second Exact/Approximate/Unsupported scale.
//!
//! [`PreferencePort::apply`] takes a fully-resolved [`Preferences`] value,
//! not a `Profile` — this is what leaves room for a future precedence chain
//! (defaults → profile → harness/project/session overrides) without
//! implementing it now: the translation/apply layer never needs to know
//! where the value came from.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{Result, router::CompatibilityRoute};

/// How much a harness may act without asking. The same underlying knob every
/// researched harness exposes as its permission/approval mode — UZE does not
/// additionally expose a separate "confirmations" axis, since that would be
/// the same intent under a different name.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Autonomy {
    /// Confirm everything.
    Manual,
    /// Auto-accept safe edits; confirm riskier operations.
    #[default]
    Balanced,
    /// Mostly autonomous; minimal interruptions.
    Auto,
    /// Never ask — headless/CI use.
    Unattended,
}

/// How far a harness's writes/execution may reach. Network access is folded
/// into this preference's per-harness translation rather than exposed as its
/// own axis: every harness researched nests network access under its
/// sandbox/workspace concept rather than exposing it independently.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SandboxScope {
    ReadOnly,
    #[default]
    WorkspaceWrite,
    FullAccess,
}

/// A capability/cost tier, not a literal vendor model id — model catalogues
/// change per vendor far too often to be user-facing intent.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ModelPreference {
    #[default]
    Default,
    Fast,
    Capable,
}

/// The complete v1 preference set. Deliberately small — see the module doc
/// and the profile feature's design notes for why `network` and
/// `confirmations` are not separate fields.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Preferences {
    pub autonomy: Autonomy,
    pub sandbox: SandboxScope,
    pub model: ModelPreference,
}

/// One preference's translation fidelity for one harness, plus a short
/// human-readable rendering of the native value it would set (or did set).
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PreferenceMapping {
    pub route: CompatibilityRoute,
    pub native_summary: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PreferenceTranslation {
    pub autonomy: PreferenceMapping,
    pub sandbox: PreferenceMapping,
    pub model: PreferenceMapping,
}

/// One of the three universal axes, named so a plan can say which axis a
/// native key answers to.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PreferenceAxis {
    Autonomy,
    Sandbox,
    Model,
}

impl PreferenceAxis {
    pub fn label(self) -> &'static str {
        match self {
            Self::Autonomy => "autonomy",
            Self::Sandbox => "sandbox",
            Self::Model => "model",
        }
    }
}

/// What a native key will hold once the preferences are applied.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PlannedValue {
    /// Written with this value, rendered the way the configuration spells it.
    Set(String),
    /// Removed, so the harness falls back to its own default.
    Removed,
    /// Left exactly as it is: the value there is the operator's own, and
    /// the preference asks for nothing that would justify replacing it.
    Kept,
}

/// One native key: what the configuration holds now, and what it will hold.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct KeyPlan {
    /// Dot path inside the configuration file (`permissions.defaultMode`).
    pub key: String,
    /// `None` when the key is absent today.
    pub current: Option<String>,
    pub planned: PlannedValue,
}

impl KeyPlan {
    /// Whether applying would leave this key exactly as it already is.
    pub fn in_effect(&self) -> bool {
        match &self.planned {
            PlannedValue::Set(value) => self.current.as_deref() == Some(value.as_str()),
            PlannedValue::Removed => self.current.is_none(),
            PlannedValue::Kept => true,
        }
    }
}

/// How one axis lands in one harness, key by key.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AxisPlan {
    pub axis: PreferenceAxis,
    pub route: CompatibilityRoute,
    pub summary: String,
    /// Why the route is not `Native`, or what the harness needs to honour it.
    pub note: Option<String>,
    /// Empty when the harness has no key for this axis, or when honouring
    /// it means leaving the configuration alone.
    pub keys: Vec<KeyPlan>,
}

/// What applying a set of preferences would do to one harness's own
/// configuration, read against the file as it is now — the preview shown
/// before anything is written, and the answer to "is this in effect".
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PreferencePlan {
    pub config_path: PathBuf,
    pub axes: Vec<AxisPlan>,
}

impl PreferencePlan {
    /// Keys whose value on disk differs from what applying would leave.
    pub fn pending(&self) -> usize {
        self.axes
            .iter()
            .flat_map(|axis| &axis.keys)
            .filter(|key| !key.in_effect())
            .count()
    }
}

/// The result of applying preferences to one harness. Mirrors the
/// Applied/AppliedWithApproximation/Unsupported/Failed vocabulary requested
/// for the feature, derived from the per-preference `CompatibilityRoute`s:
/// Native/Adaptable collapse to `Applied`/`AppliedWithApproximation`,
/// Degraded to `AppliedWithApproximation`, Unsupported stays `Unsupported`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PreferenceApplyOutcome {
    Applied {
        changed_keys: Vec<String>,
    },
    AppliedWithApproximation {
        changed_keys: Vec<String>,
        notes: Vec<String>,
    },
    Unsupported {
        reason: String,
    },
    Failed {
        reason: String,
    },
}

/// One preference's outcome from an `apply` attempt: the route it took, the
/// native keys it changed (empty when unsupported), and — for anything
/// short of `Native` — a short note explaining the approximation or gap.
pub struct PreferenceApplyDetail {
    pub route: CompatibilityRoute,
    pub changed_keys: Vec<String>,
    pub note: Option<String>,
}

/// Aggregates each preference's per-field apply detail into one harness-level
/// outcome. Pure vocabulary, shared by every integration's `apply` so the
/// Applied/AppliedWithApproximation/Unsupported rule is defined once: a
/// harness is `Unsupported` only when none of the three preferences could be
/// applied at all; otherwise `AppliedWithApproximation` when any field was
/// non-`Native`, else `Applied`. Never returns `Failed` — that variant is
/// reserved for the caller catching a hard `Err` from `apply` itself (an
/// I/O/parse failure), not a semantic gap.
pub fn summarize_apply(details: [PreferenceApplyDetail; 3]) -> PreferenceApplyOutcome {
    let mut changed_keys = Vec::new();
    let mut notes = Vec::new();
    let mut any_supported = false;
    let mut any_non_native = false;
    for detail in details {
        match detail.route {
            CompatibilityRoute::Unsupported => {
                if let Some(note) = detail.note {
                    notes.push(note);
                }
            }
            CompatibilityRoute::Native => {
                any_supported = true;
                changed_keys.extend(detail.changed_keys);
            }
            CompatibilityRoute::Adaptable | CompatibilityRoute::Degraded => {
                any_supported = true;
                any_non_native = true;
                changed_keys.extend(detail.changed_keys);
                if let Some(note) = detail.note {
                    notes.push(note);
                }
            }
        }
    }
    if !any_supported {
        return PreferenceApplyOutcome::Unsupported {
            reason: notes.join("; "),
        };
    }
    if any_non_native || !notes.is_empty() {
        return PreferenceApplyOutcome::AppliedWithApproximation {
            changed_keys,
            notes,
        };
    }
    PreferenceApplyOutcome::Applied { changed_keys }
}

/// A harness's preference translation/application contract: a narrow trait
/// implemented by the integrations, registered in its own slice on
/// `IntegrationRegistry` rather than folded into `IntegrationPort`, which is
/// about capability kinds, not runtime settings. Named `preference_id`, not
/// `id`, so a type implementing both traits has no method ambiguity.
pub trait PreferencePort: Send + Sync {
    fn preference_id(&self) -> &'static str;

    /// How each axis maps onto this harness, independent of what its
    /// configuration holds today.
    fn translate(&self, preferences: &Preferences) -> PreferenceTranslation;

    /// What applying `preferences` would change in this harness's
    /// configuration as it stands now, without writing anything. Reads the
    /// file, so it is an `Err` when that file cannot be read — the same
    /// condition under which `apply` would refuse to write it.
    fn plan(&self, preferences: &Preferences) -> Result<PreferencePlan>;

    /// Writes `preferences` into this harness's native configuration,
    /// non-destructively: only UZE-owned keys change, foreign content is
    /// preserved verbatim, and the write is atomic. Returns
    /// `Ok(Unsupported { .. })` when no field could be applied — never
    /// `Ok(Failed { .. })`; a hard I/O/parse error propagates as `Err` and
    /// the caller (which applies to many harnesses) turns that into a
    /// `Failed` outcome for this harness without aborting the others.
    fn apply(&self, preferences: &Preferences) -> Result<PreferenceApplyOutcome>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_default_to_a_balanced_workspace_scoped_profile() {
        let preferences = Preferences::default();
        assert_eq!(preferences.autonomy, Autonomy::Balanced);
        assert_eq!(preferences.sandbox, SandboxScope::WorkspaceWrite);
        assert_eq!(preferences.model, ModelPreference::Default);
    }

    #[test]
    fn preferences_round_trip_through_json() {
        let preferences = Preferences {
            autonomy: Autonomy::Unattended,
            sandbox: SandboxScope::FullAccess,
            model: ModelPreference::Capable,
        };
        let json = serde_json::to_string(&preferences).unwrap();
        assert_eq!(
            json,
            r#"{"autonomy":"UNATTENDED","sandbox":"FULL_ACCESS","model":"CAPABLE"}"#
        );
        let round_tripped: Preferences = serde_json::from_str(&json).unwrap();
        assert_eq!(round_tripped, preferences);
    }

    #[test]
    fn invalid_preference_value_is_rejected() {
        let malformed = r#"{"autonomy":"YOLO","sandbox":"WORKSPACE_WRITE","model":"DEFAULT"}"#;
        assert!(serde_json::from_str::<Preferences>(malformed).is_err());
    }

    fn detail(route: CompatibilityRoute, key: &str) -> PreferenceApplyDetail {
        PreferenceApplyDetail {
            route,
            changed_keys: vec![key.to_owned()],
            note: (route != CompatibilityRoute::Native).then(|| format!("{key} is approximate")),
        }
    }

    #[test]
    fn three_native_fields_summarize_as_applied() {
        let outcome = summarize_apply([
            detail(CompatibilityRoute::Native, "autonomy"),
            detail(CompatibilityRoute::Native, "sandbox"),
            detail(CompatibilityRoute::Native, "model"),
        ]);
        assert!(
            matches!(outcome, PreferenceApplyOutcome::Applied { changed_keys } if changed_keys.len() == 3)
        );
    }

    #[test]
    fn one_adaptable_field_summarizes_as_approximated() {
        let outcome = summarize_apply([
            detail(CompatibilityRoute::Native, "autonomy"),
            detail(CompatibilityRoute::Adaptable, "sandbox"),
            detail(CompatibilityRoute::Native, "model"),
        ]);
        assert!(matches!(
            outcome,
            PreferenceApplyOutcome::AppliedWithApproximation { notes, .. } if notes.len() == 1
        ));
    }

    #[test]
    fn all_unsupported_fields_summarize_as_unsupported() {
        let outcome = summarize_apply([
            PreferenceApplyDetail {
                route: CompatibilityRoute::Unsupported,
                changed_keys: Vec::new(),
                note: Some("no autonomy key".to_owned()),
            },
            PreferenceApplyDetail {
                route: CompatibilityRoute::Unsupported,
                changed_keys: Vec::new(),
                note: Some("no sandbox key".to_owned()),
            },
            PreferenceApplyDetail {
                route: CompatibilityRoute::Unsupported,
                changed_keys: Vec::new(),
                note: Some("no model key".to_owned()),
            },
        ]);
        assert!(matches!(
            outcome,
            PreferenceApplyOutcome::Unsupported { .. }
        ));
    }

    #[test]
    fn a_partially_unsupported_field_still_applies_the_rest() {
        let outcome = summarize_apply([
            detail(CompatibilityRoute::Native, "autonomy"),
            detail(CompatibilityRoute::Native, "sandbox"),
            PreferenceApplyDetail {
                route: CompatibilityRoute::Unsupported,
                changed_keys: Vec::new(),
                note: Some("no model key".to_owned()),
            },
        ]);
        assert!(matches!(
            outcome,
            PreferenceApplyOutcome::AppliedWithApproximation { changed_keys, notes }
                if changed_keys.len() == 2 && notes == vec!["no model key".to_owned()]
        ));
    }

    #[test]
    fn apply_outcome_tags_round_trip() {
        let outcome = PreferenceApplyOutcome::AppliedWithApproximation {
            changed_keys: vec!["permissions.defaultMode".to_owned()],
            notes: vec!["auto collapses to on-request".to_owned()],
        };
        let json = serde_json::to_string(&outcome).unwrap();
        assert!(json.contains(r#""outcome":"APPLIED_WITH_APPROXIMATION""#));
    }
}
