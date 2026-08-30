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
        Autonomy, PreferenceApplyDetail, PreferenceApplyOutcome, PreferenceMapping,
        PreferenceTranslation, Preferences, SandboxScope, summarize_apply,
    },
    router::CompatibilityRoute,
};

use crate::shared::json_config;

struct AutonomyKeys {
    agent_mode: &'static str,
    tool_permission: &'static str,
}

fn autonomy_mapping(autonomy: Autonomy) -> AutonomyKeys {
    match autonomy {
        Autonomy::Manual => AutonomyKeys {
            agent_mode: "default",
            tool_permission: "strict",
        },
        Autonomy::Balanced => AutonomyKeys {
            agent_mode: "accept-edits",
            tool_permission: "request-review",
        },
        Autonomy::Auto => AutonomyKeys {
            agent_mode: "accept-edits",
            tool_permission: "proceed-in-sandbox",
        },
        Autonomy::Unattended => AutonomyKeys {
            agent_mode: "accept-edits",
            tool_permission: "always-proceed",
        },
    }
}

fn sandbox_mapping(sandbox: SandboxScope) -> (CompatibilityRoute, bool, bool) {
    // (route, enableTerminalSandbox, allowNonWorkspaceAccess)
    match sandbox {
        // No verified mechanism blocks writes *within* the sandboxed
        // workspace itself — only sandboxing and the workspace boundary are
        // confirmed, so this is weaker than a real read-only guarantee.
        SandboxScope::ReadOnly => (CompatibilityRoute::Degraded, true, false),
        SandboxScope::WorkspaceWrite => (CompatibilityRoute::Adaptable, true, false),
        SandboxScope::FullAccess => (CompatibilityRoute::Adaptable, false, true),
    }
}

pub(crate) fn translate(preferences: &Preferences) -> PreferenceTranslation {
    let keys = autonomy_mapping(preferences.autonomy);
    let (sandbox_route, sandbox, non_workspace) = sandbox_mapping(preferences.sandbox);
    PreferenceTranslation {
        autonomy: PreferenceMapping {
            route: CompatibilityRoute::Adaptable,
            native_summary: format!(
                "agentMode = {}, toolPermission = {}",
                keys.agent_mode, keys.tool_permission
            ),
        },
        sandbox: PreferenceMapping {
            route: sandbox_route,
            native_summary: format!(
                "enableTerminalSandbox = {sandbox}, allowNonWorkspaceAccess = {non_workspace}"
            ),
        },
        model: PreferenceMapping {
            route: CompatibilityRoute::Unsupported,
            native_summary: "no confirmed persisted model key".to_owned(),
        },
    }
}

pub(crate) fn apply(
    settings_path: &Path,
    preferences: &Preferences,
) -> Result<PreferenceApplyOutcome> {
    let keys = autonomy_mapping(preferences.autonomy);
    let (sandbox_route, sandbox, non_workspace) = sandbox_mapping(preferences.sandbox);

    json_config::merge(settings_path, |config| {
        json_config::set_path(config, &["agentMode"], serde_json::json!(keys.agent_mode))?;
        json_config::set_path(
            config,
            &["toolPermission"],
            serde_json::json!(keys.tool_permission),
        )?;
        json_config::set_path(
            config,
            &["enableTerminalSandbox"],
            serde_json::json!(sandbox),
        )?;
        json_config::set_path(
            config,
            &["allowNonWorkspaceAccess"],
            serde_json::json!(non_workspace),
        )?;
        Ok(())
    })?;

    // `model` is never written: no persisted key was confirmed, so guessing
    // one risks silently corrupting a real user setting.
    Ok(summarize_apply([
        PreferenceApplyDetail {
            route: CompatibilityRoute::Adaptable,
            changed_keys: vec!["agentMode".to_owned(), "toolPermission".to_owned()],
            note: Some(
                "Antigravity has no single autonomy key; translated across agentMode and \
                 toolPermission (unverified against current docs — re-check before relying on \
                 this)"
                    .to_owned(),
            ),
        },
        PreferenceApplyDetail {
            route: sandbox_route,
            changed_keys: vec![
                "enableTerminalSandbox".to_owned(),
                "allowNonWorkspaceAccess".to_owned(),
            ],
            note: (sandbox_route == CompatibilityRoute::Degraded).then(|| {
                "no verified mechanism blocks writes within the sandboxed workspace itself, only \
                 the workspace boundary"
                    .to_owned()
            }),
        },
        PreferenceApplyDetail {
            route: CompatibilityRoute::Unsupported,
            changed_keys: Vec::new(),
            note: Some("Antigravity has no confirmed persisted model key".to_owned()),
        },
    ]))
}

#[cfg(test)]
mod tests {
    use uze_core::preference::ModelPreference;

    use super::*;

    fn temp_path(label: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "uze-antigravity-preferences-{label}-{}-{nonce}.json",
            std::process::id()
        ))
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
