//! Translates universal [`Preferences`] into Antigravity's
//! `~/.gemini/antigravity-cli/settings.json`.
//!
//! **Lower confidence than the other three integrations.** Antigravity is a
//! very new product (rebranded from Gemini CLI, announced May 2026) and its
//! docs site gave inconsistent results across repeat fetches during
//! research; no persisted `model` key could be confirmed at all. Every
//! mapping here is deliberately conservative: never `Native`, and `model` is
//! `Unsupported` outright rather than guessed. Re-verify against
//! `antigravity.google/docs/cli/{settings,modes,permissions}` before
//! treating any of this as settled.

use std::path::Path;

use uze_core::{
    Result,
    preference::{
        Autonomy, PreferenceApplyOutcome, PreferenceTranslation, Preferences, SandboxScope,
    },
    router::CompatibilityRoute,
};

use crate::shared::preference::{Axis, Mapping, Value};

const AGENT_MODE: &[&str] = &["agentMode"];
const TOOL_PERMISSION: &[&str] = &["toolPermission"];
const TERMINAL_SANDBOX: &[&str] = &["enableTerminalSandbox"];
const NON_WORKSPACE: &[&str] = &["allowNonWorkspaceAccess"];

fn mapping(preferences: &Preferences) -> Mapping {
    Mapping {
        autonomy: autonomy(preferences.autonomy),
        sandbox: sandbox(preferences.sandbox),
        // Never written: no persisted key was confirmed, and guessing one
        // risks silently corrupting a real user setting.
        model: Axis::new(
            CompatibilityRoute::Unsupported,
            "no confirmed persisted model key",
        )
        .note("Antigravity has no confirmed persisted model key"),
    }
}

fn autonomy(autonomy: Autonomy) -> Axis {
    let (agent_mode, tool_permission) = match autonomy {
        Autonomy::Manual => ("default", "strict"),
        Autonomy::Balanced => ("accept-edits", "request-review"),
        Autonomy::Auto => ("accept-edits", "proceed-in-sandbox"),
        Autonomy::Unattended => ("accept-edits", "always-proceed"),
    };
    Axis::new(
        CompatibilityRoute::Adaptable,
        format!("agentMode = {agent_mode}, toolPermission = {tool_permission}"),
    )
    .set(AGENT_MODE, Value::Text(agent_mode))
    .set(TOOL_PERMISSION, Value::Text(tool_permission))
    .note(
        "Antigravity has no single autonomy key; translated across agentMode and toolPermission \
         (unverified against current docs — re-check before relying on this)",
    )
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

#[cfg(test)]
mod tests {
    use uze_core::preference::ModelPreference;

    use super::*;

    fn temp_path(label: &str) -> std::path::PathBuf {
        uze_testkit::temp::scratch(label).join("antigravity-preferences.json")
    }

    #[test]
    fn no_preference_ever_claims_native() {
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
            assert_ne!(translation.model.route, CompatibilityRoute::Native);
        }
    }

    #[test]
    fn model_is_always_unsupported() {
        for model in [
            ModelPreference::Default,
            ModelPreference::Fast,
            ModelPreference::Capable,
        ] {
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
}
