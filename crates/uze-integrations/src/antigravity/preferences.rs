//! Translates universal [`Preferences`] into Antigravity's
//! `~/.gemini/antigravity-cli/settings.json`.
//!
//! **Lower confidence than the other three integrations.** Antigravity is a
//! very new product (rebranded from Gemini CLI, announced May 2026) and its
//! docs site gave inconsistent results across repeat fetches during
//! research. Every write here is deliberately conservative: never `Native`.
//!
//! Checked against the binary itself (agy 1.2.0, reading its own log of
//! the settings it loaded): every `toolPermission` and the two sandbox
//! booleans written here load, and `agentMode` accepts `accept-edits` and
//! `plan` — but *not* `default`, which is the name the UI gives the
//! built-in mode. A settings file carrying it is rejected whole ("invalid
//! settings … using defaults"), so the default mode is the key being
//! absent.
//!
//! A persisted `model` key does exist, and holds a label from the signed-in
//! account's own catalog ("Gemini 3.1 Pro (Low)"); an unknown label falls
//! back to the default. With no tier in that catalog to translate against,
//! `Fast` and `Capable` stay `Unsupported`, and the key is never written.

use std::path::Path;

use uze_core::{
    Result,
    preference::{
        Autonomy, ModelPreference, PreferenceApplyOutcome, PreferencePlan, PreferenceTranslation,
        Preferences, SandboxScope,
    },
    router::CompatibilityRoute,
};

use crate::shared::preference::{Axis, Mapping, Value};

const AGENT_MODE: &[&str] = &["agentMode"];
/// The `agentMode` values `Manual` removes, whoever wrote them. `default`
/// is rejected outright (an earlier release wrote it); `accept-edits` stops
/// asking before edits, which is the opposite of what `Manual` asks for.
/// `plan` is more careful than the default mode, so it stays.
const AGENT_MODES_MANUAL_REMOVES: &[&str] = &["default", "accept-edits"];
const TOOL_PERMISSION: &[&str] = &["toolPermission"];
const TERMINAL_SANDBOX: &[&str] = &["enableTerminalSandbox"];
const NON_WORKSPACE: &[&str] = &["allowNonWorkspaceAccess"];

fn mapping(preferences: &Preferences) -> Mapping {
    Mapping {
        autonomy: autonomy(preferences.autonomy),
        sandbox: sandbox(preferences.sandbox),
        model: model(preferences.model),
    }
}

fn model(model: ModelPreference) -> Axis {
    match model {
        ModelPreference::Default => Axis::new(
            CompatibilityRoute::Native,
            "unset (Antigravity's own default; a model you chose is kept)",
        ),
        _ => Axis::new(
            CompatibilityRoute::Unsupported,
            "unsupported: the model catalog is the account's own",
        )
        .note(
            "Antigravity's model is a label from the signed-in account's catalog; there is no \
             fast/capable tier to translate against",
        ),
    }
}

fn autonomy(autonomy: Autonomy) -> Axis {
    let tool_permission = match autonomy {
        Autonomy::Manual => "strict",
        Autonomy::Balanced => "request-review",
        Autonomy::Auto => "proceed-in-sandbox",
        Autonomy::Unattended => "always-proceed",
    };
    let axis = if autonomy == Autonomy::Manual {
        Axis::new(
            CompatibilityRoute::Adaptable,
            format!("agentMode unset (default mode), toolPermission = {tool_permission}"),
        )
        .release(AGENT_MODE, AGENT_MODES_MANUAL_REMOVES)
    } else {
        Axis::new(
            CompatibilityRoute::Adaptable,
            format!("agentMode = accept-edits, toolPermission = {tool_permission}"),
        )
        .set(AGENT_MODE, Value::Text("accept-edits"))
    };
    axis.set(TOOL_PERMISSION, Value::Text(tool_permission))
    .note("Antigravity has no single autonomy key; translated across agentMode and toolPermission")
}

fn sandbox(sandbox: SandboxScope) -> Axis {
    // No verified mechanism blocks writes *within* the sandboxed workspace
    // itself — only sandboxing and the workspace boundary are confirmed,
    // so read-only is weaker here than a real read-only guarantee.
    let (route, terminal_sandbox, non_workspace) = match sandbox {
        SandboxScope::ReadOnly => (CompatibilityRoute::Degraded, true, false),
        SandboxScope::WorkspaceWrite => (CompatibilityRoute::Adaptable, true, false),
        SandboxScope::FullAccess => (CompatibilityRoute::Adaptable, false, true),
    };
    let axis = Axis::new(
        route,
        format!(
            "enableTerminalSandbox = {terminal_sandbox}, allowNonWorkspaceAccess = {non_workspace}"
        ),
    )
    .set(TERMINAL_SANDBOX, Value::Flag(terminal_sandbox))
    .set(NON_WORKSPACE, Value::Flag(non_workspace));
    if route == CompatibilityRoute::Degraded {
        axis.note(
            "no verified mechanism blocks writes within the sandboxed workspace itself, only the \
             workspace boundary",
        )
    } else {
        axis
    }
}

pub(crate) fn translate(preferences: &Preferences) -> PreferenceTranslation {
    mapping(preferences).translate()
}

pub(crate) fn apply(
    settings_path: &Path,
    preferences: &Preferences,
) -> Result<PreferenceApplyOutcome> {
    mapping(preferences).apply_json(settings_path)
}

pub(crate) fn plan(path: &Path, preferences: &Preferences) -> Result<PreferencePlan> {
    mapping(preferences).plan_json(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(label: &str) -> std::path::PathBuf {
        uze_testkit::temp::scratch(label).join("antigravity-preferences.json")
    }

    #[test]
    fn no_write_ever_claims_native() {
        for autonomy in [
            Autonomy::Manual,
            Autonomy::Balanced,
            Autonomy::Auto,
            Autonomy::Unattended,
        ] {
            let translation = translate(&Preferences {
                autonomy,
                ..Preferences::default()
            });
            assert_ne!(translation.autonomy.route, CompatibilityRoute::Native);
            assert_ne!(translation.sandbox.route, CompatibilityRoute::Native);
        }
    }

    #[test]
    fn a_model_tier_is_unsupported() {
        for model in [ModelPreference::Fast, ModelPreference::Capable] {
            let translation = translate(&Preferences {
                model,
                ..Preferences::default()
            });
            assert_eq!(translation.model.route, CompatibilityRoute::Unsupported);
        }
    }

    #[test]
    fn apply_never_writes_a_model_key() {
        let path = temp_path("no-model-key");
        apply(&path, &Preferences::default()).unwrap();
        let written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(written.get("model").is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn apply_preserves_foreign_keys() {
        let path = temp_path("preserve");
        std::fs::write(&path, serde_json::json!({"theme": "dark"}).to_string()).unwrap();
        let outcome = apply(&path, &Preferences::default()).unwrap();
        assert!(matches!(
            outcome,
            PreferenceApplyOutcome::AppliedWithApproximation { .. }
        ));
        let written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written["theme"], "dark");
        assert_eq!(written["agentMode"], "accept-edits");
        let _ = std::fs::remove_file(&path);
    }

    /// Antigravity rejects `agentMode = "default"` and drops the whole
    /// settings file for its defaults — what `Manual` used to write.
    #[test]
    fn manual_leaves_agent_mode_absent_and_releases_the_rejected_value() {
        let path = temp_path("manual");
        std::fs::write(
            &path,
            r#"{"agentMode":"default","model":"GPT-OSS 120B (Medium)"}"#,
        )
        .unwrap();
        let manual = Preferences {
            autonomy: Autonomy::Manual,
            ..Preferences::default()
        };
        apply(&path, &manual).unwrap();
        let written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(written.get("agentMode").is_none(), "{written}");
        assert_eq!(written["toolPermission"], "strict");
        assert_eq!(written["model"], "GPT-OSS 120B (Medium)");

        std::fs::write(&path, r#"{"agentMode":"plan"}"#).unwrap();
        apply(&path, &manual).unwrap();
        let written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            written["agentMode"], "plan",
            "a mode the operator chose is kept"
        );
        let _ = std::fs::remove_file(&path);
    }
}
