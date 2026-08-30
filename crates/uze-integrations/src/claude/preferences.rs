//! Translates universal [`Preferences`] into Claude Code's `settings.json`.
//!
//! Verified against current official docs (code.claude.com, Aug 2026):
//! `docs/en/permission-modes`, `docs/en/settings-reference`,
//! `docs/en/sandboxing`, `docs/en/model-config`.
//!
//! `sandbox.filesystem.allowWrite`/`denyWrite` are path-based, but this
//! writes the *user-scope* `settings.json` — there is no project directory
//! in scope at that layer (that's future precedence-chain work), so the
//! `sandbox` preference only ever sets the coarse `sandbox.enabled` switch
//! (plus an empty allow-write list for `ReadOnly`, which is path-independent
//! and unambiguous). Neither non-default value claims `Native`: enabling the
//! sandbox without a workspace-scoped allowlist does not precisely deliver
//! "workspace-write", only "sandboxed with Claude's own default scope".

use std::path::Path;

use uze_core::{
    Result,
    preference::{
        Autonomy, ModelPreference, PreferenceApplyDetail, PreferenceApplyOutcome,
        PreferenceMapping, PreferenceTranslation, Preferences, SandboxScope, summarize_apply,
    },
    router::CompatibilityRoute,
};

use crate::shared::json_config;

fn autonomy_mapping(autonomy: Autonomy) -> (CompatibilityRoute, &'static str) {
    match autonomy {
        Autonomy::Manual => (CompatibilityRoute::Native, "default"),
        Autonomy::Balanced => (CompatibilityRoute::Native, "acceptEdits"),
        Autonomy::Auto => (CompatibilityRoute::Native, "auto"),
        // The documented equivalent of "never ask" is `bypassPermissions`
        // (the modern name for what `--dangerously-skip-permissions`
        // invoked) — an exact key/value match, hence still Native, though
        // the TUI colors it as the highest-risk value on its own axis.
        Autonomy::Unattended => (CompatibilityRoute::Native, "bypassPermissions"),
    }
}

fn sandbox_mapping(sandbox: SandboxScope) -> (CompatibilityRoute, &'static str) {
    match sandbox {
        SandboxScope::FullAccess => (
            CompatibilityRoute::Native,
            "sandbox.enabled=false (default)",
        ),
        SandboxScope::WorkspaceWrite => (
            CompatibilityRoute::Adaptable,
            "sandbox.enabled=true (default filesystem scope; no workspace allowlist at user scope)",
        ),
        SandboxScope::ReadOnly => (
            CompatibilityRoute::Adaptable,
            "sandbox.enabled=true, sandbox.filesystem.allowWrite=[] (no writes anywhere)",
        ),
    }
}

fn model_mapping(model: ModelPreference) -> (CompatibilityRoute, &'static str) {
    match model {
        ModelPreference::Default => (CompatibilityRoute::Native, "default"),
        ModelPreference::Fast => (CompatibilityRoute::Native, "haiku"),
        ModelPreference::Capable => (CompatibilityRoute::Native, "opus"),
    }
}

pub(crate) fn translate(preferences: &Preferences) -> PreferenceTranslation {
    let (autonomy_route, autonomy_value) = autonomy_mapping(preferences.autonomy);
    let (sandbox_route, sandbox_value) = sandbox_mapping(preferences.sandbox);
    let (model_route, model_value) = model_mapping(preferences.model);
    PreferenceTranslation {
        autonomy: PreferenceMapping {
            route: autonomy_route,
            native_summary: format!("permissions.defaultMode = {autonomy_value}"),
        },
        sandbox: PreferenceMapping {
            route: sandbox_route,
            native_summary: sandbox_value.to_owned(),
        },
        model: PreferenceMapping {
            route: model_route,
            native_summary: format!("model = {model_value}"),
        },
    }
}

pub(crate) fn apply(
    settings_path: &Path,
    preferences: &Preferences,
) -> Result<PreferenceApplyOutcome> {
    let (autonomy_route, autonomy_value) = autonomy_mapping(preferences.autonomy);
    let (sandbox_route, _) = sandbox_mapping(preferences.sandbox);
    let (model_route, model_value) = model_mapping(preferences.model);

    json_config::merge(settings_path, |config| {
        json_config::set_path(
            config,
            &["permissions", "defaultMode"],
            serde_json::json!(autonomy_value),
        )?;
        match preferences.sandbox {
            SandboxScope::FullAccess => {
                json_config::set_path(config, &["sandbox", "enabled"], serde_json::json!(false))?;
            }
            SandboxScope::WorkspaceWrite => {
                json_config::set_path(config, &["sandbox", "enabled"], serde_json::json!(true))?;
            }
            SandboxScope::ReadOnly => {
                json_config::set_path(config, &["sandbox", "enabled"], serde_json::json!(true))?;
                json_config::set_path(
                    config,
                    &["sandbox", "filesystem", "allowWrite"],
                    serde_json::json!([]),
                )?;
            }
        }
        json_config::set_path(config, &["model"], serde_json::json!(model_value))?;
        Ok(())
    })?;

    let sandbox_keys = match preferences.sandbox {
        SandboxScope::ReadOnly => vec![
            "sandbox.enabled".to_owned(),
            "sandbox.filesystem.allowWrite".to_owned(),
        ],
        _ => vec!["sandbox.enabled".to_owned()],
    };
    Ok(summarize_apply([
        PreferenceApplyDetail {
            route: autonomy_route,
            changed_keys: vec!["permissions.defaultMode".to_owned()],
            note: None,
        },
        PreferenceApplyDetail {
            route: sandbox_route,
            changed_keys: sandbox_keys,
            note: (sandbox_route != CompatibilityRoute::Native).then(|| {
                "Claude has no workspace-scoped write allowlist at user scope; sandbox is set \
                 to its coarse enabled/disabled state only"
                    .to_owned()
            }),
        },
        PreferenceApplyDetail {
            route: model_route,
            changed_keys: vec!["model".to_owned()],
            note: None,
        },
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(label: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "uze-claude-preferences-{label}-{}-{nonce}.json",
            std::process::id()
        ))
    }

    #[test]
    fn autonomy_and_model_translate_as_native() {
        let translation = translate(&Preferences::default());
        assert_eq!(translation.autonomy.route, CompatibilityRoute::Native);
        assert_eq!(translation.model.route, CompatibilityRoute::Native);
    }

    #[test]
    fn non_default_sandbox_values_are_adaptable_not_native() {
        let translation = translate(&Preferences {
            sandbox: SandboxScope::WorkspaceWrite,
            ..Preferences::default()
        });
        assert_eq!(translation.sandbox.route, CompatibilityRoute::Adaptable);
    }

    #[test]
    fn apply_writes_expected_keys_and_preserves_foreign_content() {
        let path = temp_path("preserve");
        std::fs::write(
            &path,
            serde_json::json!({"foreignKey": "untouched", "permissions": {"allow": ["Bash(ls)"]}})
                .to_string(),
        )
        .unwrap();

        let outcome = apply(
            &path,
            &Preferences {
                autonomy: Autonomy::Balanced,
                sandbox: SandboxScope::FullAccess,
                model: ModelPreference::Capable,
            },
        )
        .unwrap();
        assert!(matches!(outcome, PreferenceApplyOutcome::Applied { .. }));

        let written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written["foreignKey"], "untouched");
        assert_eq!(
            written["permissions"]["allow"],
            serde_json::json!(["Bash(ls)"])
        );
        assert_eq!(written["permissions"]["defaultMode"], "acceptEdits");
        assert_eq!(written["sandbox"]["enabled"], false);
        assert_eq!(written["model"], "opus");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn read_only_sandbox_sets_an_empty_allow_write_list() {
        let path = temp_path("read-only");
        let outcome = apply(
            &path,
            &Preferences {
                sandbox: SandboxScope::ReadOnly,
                ..Preferences::default()
            },
        )
        .unwrap();
        assert!(matches!(
            outcome,
            PreferenceApplyOutcome::AppliedWithApproximation { .. }
        ));
        let written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            written["sandbox"]["filesystem"]["allowWrite"],
            serde_json::json!([])
        );
        let _ = std::fs::remove_file(&path);
    }
}
