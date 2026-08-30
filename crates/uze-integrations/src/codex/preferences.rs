//! Translates universal [`Preferences`] into Codex's `config.toml`.
//!
//! Verified against current official docs (learn.chatgpt.com, Aug 2026):
//! `docs/config-file/config-basic`, `docs/config-file/config-reference`,
//! `docs/config-file/config-advanced`, `docs/agent-approvals-security`.
//!
//! Model tiers beyond `Default` are `Unsupported`: Codex requires an exact
//! model id string and current docs give no tier-alias catalog to translate
//! against (unlike Claude's documented `sonnet`/`opus`/`haiku` aliases) — a
//! guessed id could silently point at a model that does not exist. `Default`
//! is translated by not touching the `model` key at all, which is the
//! correct "use your own default" translation and requires no vendor
//! knowledge.

use std::path::Path;

use uze_core::{
    Result,
    preference::{
        Autonomy, ModelPreference, PreferenceApplyDetail, PreferenceApplyOutcome,
        PreferenceMapping, PreferenceTranslation, Preferences, SandboxScope, summarize_apply,
    },
    router::CompatibilityRoute,
};

use crate::shared::toml_config;

fn autonomy_mapping(autonomy: Autonomy) -> (CompatibilityRoute, &'static str) {
    match autonomy {
        Autonomy::Manual => (CompatibilityRoute::Native, "untrusted"),
        Autonomy::Balanced => (CompatibilityRoute::Native, "on-request"),
        // `approval_policy` has no tier between "on-request" and "never" —
        // Auto collapses onto the same value as Balanced.
        Autonomy::Auto => (CompatibilityRoute::Adaptable, "on-request"),
        Autonomy::Unattended => (CompatibilityRoute::Native, "never"),
    }
}

fn sandbox_mapping(sandbox: SandboxScope) -> (CompatibilityRoute, &'static str) {
    match sandbox {
        SandboxScope::ReadOnly => (CompatibilityRoute::Native, "read-only"),
        SandboxScope::WorkspaceWrite => (CompatibilityRoute::Native, "workspace-write"),
        SandboxScope::FullAccess => (CompatibilityRoute::Native, "danger-full-access"),
    }
}

fn model_mapping(model: ModelPreference) -> CompatibilityRoute {
    match model {
        ModelPreference::Default => CompatibilityRoute::Native,
        ModelPreference::Fast | ModelPreference::Capable => CompatibilityRoute::Unsupported,
    }
}

pub(crate) fn translate(preferences: &Preferences) -> PreferenceTranslation {
    let (autonomy_route, autonomy_value) = autonomy_mapping(preferences.autonomy);
    let (sandbox_route, sandbox_value) = sandbox_mapping(preferences.sandbox);
    let model_route = model_mapping(preferences.model);
    PreferenceTranslation {
        autonomy: PreferenceMapping {
            route: autonomy_route,
            native_summary: format!("approval_policy = \"{autonomy_value}\""),
        },
        sandbox: PreferenceMapping {
            route: sandbox_route,
            native_summary: format!("sandbox_mode = \"{sandbox_value}\""),
        },
        model: PreferenceMapping {
            route: model_route,
            native_summary: match preferences.model {
                ModelPreference::Default => "unset (Codex's own default)".to_owned(),
                _ => "unsupported: no verified model catalog".to_owned(),
            },
        },
    }
}

pub(crate) fn apply(
    config_path: &Path,
    preferences: &Preferences,
) -> Result<PreferenceApplyOutcome> {
    let (autonomy_route, autonomy_value) = autonomy_mapping(preferences.autonomy);
    let (sandbox_route, sandbox_value) = sandbox_mapping(preferences.sandbox);
    let model_route = model_mapping(preferences.model);

    toml_config::merge(config_path, |document| {
        toml_config::set_path(document, &["approval_policy"], autonomy_value)?;
        toml_config::set_path(document, &["sandbox_mode"], sandbox_value)?;
        if preferences.sandbox == SandboxScope::WorkspaceWrite {
            toml_config::set_path(
                document,
                &["sandbox_workspace_write", "network_access"],
                false,
            )?;
        }
        Ok(())
    })?;

    let mut sandbox_keys = vec!["sandbox_mode".to_owned()];
    if preferences.sandbox == SandboxScope::WorkspaceWrite {
        sandbox_keys.push("sandbox_workspace_write.network_access".to_owned());
    }

    Ok(summarize_apply([
        PreferenceApplyDetail {
            route: autonomy_route,
            changed_keys: vec!["approval_policy".to_owned()],
            note: (autonomy_route != CompatibilityRoute::Native).then(|| {
                "Codex has no tier between on-request and never; auto uses the same value as \
                 balanced"
                    .to_owned()
            }),
        },
        PreferenceApplyDetail {
            route: sandbox_route,
            changed_keys: sandbox_keys,
            note: None,
        },
        PreferenceApplyDetail {
            route: model_route,
            changed_keys: Vec::new(),
            note: (model_route == CompatibilityRoute::Unsupported).then(|| {
                "Codex requires an exact model id; no verified fast/capable catalog to translate \
                 against"
                    .to_owned()
            }),
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
            "uze-codex-preferences-{label}-{}-{nonce}.toml",
            std::process::id()
        ))
    }

    #[test]
    fn sandbox_mode_translates_as_native_for_every_tier() {
        for sandbox in [
            SandboxScope::ReadOnly,
            SandboxScope::WorkspaceWrite,
            SandboxScope::FullAccess,
        ] {
            let translation = translate(&Preferences {
                sandbox,
                ..Preferences::default()
            });
            assert_eq!(translation.sandbox.route, CompatibilityRoute::Native);
        }
    }

    #[test]
    fn auto_autonomy_collapses_onto_balanced_and_is_flagged_adaptable() {
        let translation = translate(&Preferences {
            autonomy: Autonomy::Auto,
            ..Preferences::default()
        });
        assert_eq!(translation.autonomy.route, CompatibilityRoute::Adaptable);
    }

    #[test]
    fn fast_and_capable_model_are_unsupported() {
        for model in [ModelPreference::Fast, ModelPreference::Capable] {
            let translation = translate(&Preferences {
                model,
                ..Preferences::default()
            });
            assert_eq!(translation.model.route, CompatibilityRoute::Unsupported);
        }
    }

    #[test]
    fn apply_preserves_foreign_keys_tables_and_comments() {
        let path = temp_path("preserve");
        std::fs::write(
            &path,
            "# a user comment\nmodel = \"gpt-5.6\"\n\n[model_providers.openai]\nname = \"OpenAI\"\n",
        )
        .unwrap();

        let outcome = apply(
            &path,
            &Preferences {
                autonomy: Autonomy::Unattended,
                sandbox: SandboxScope::FullAccess,
                model: ModelPreference::Default,
            },
        )
        .unwrap();
        assert!(matches!(outcome, PreferenceApplyOutcome::Applied { .. }));

        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("# a user comment"));
        assert!(
            contents.contains("model = \"gpt-5.6\""),
            "model preference must not touch an existing model key when Default"
        );
        assert!(contents.contains("[model_providers.openai]"));
        assert!(contents.contains("approval_policy = \"never\""));
        assert!(contents.contains("sandbox_mode = \"danger-full-access\""));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn workspace_write_explicitly_disables_network_access() {
        let path = temp_path("network");
        let outcome = apply(
            &path,
            &Preferences {
                sandbox: SandboxScope::WorkspaceWrite,
                ..Preferences::default()
            },
        )
        .unwrap();
        assert!(matches!(outcome, PreferenceApplyOutcome::Applied { .. }));
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("[sandbox_workspace_write]"));
        assert!(contents.contains("network_access = false"));
        let _ = std::fs::remove_file(&path);
    }
}
