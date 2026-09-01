//! Translates universal [`Preferences`] into OpenCode's `opencode.json`.
//!
//! Verified against current official docs (opencode.ai, Aug 2026):
//! `docs/config/`, `docs/permissions/`, and the live schema at
//! `opencode.ai/config.json`.
//!
//! OpenCode has no single autonomy/mode key — `permission` is a set of
//! independent per-category (`edit`/`bash`/`webfetch`/`websearch`) ask/allow/
//! deny values — so `autonomy` is always `Adaptable` here, decomposed across
//! several keys rather than matching one. `sandbox` is scoped to
//! `permission.external_directory` alone so it never fights `autonomy` for
//! ownership of `permission.edit`, with one deliberate exception: `ReadOnly`
//! additionally forces `permission.edit = "deny"` (applied *after*
//! autonomy's own value, so it wins) because a "read only" preference that
//! still allowed edits would not honor its own name — flagged `Adaptable`
//! with a note, since OpenCode's `bash` tool can still write files this
//! doesn't block.

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

struct AutonomyKeys {
    edit: &'static str,
    bash: &'static str,
    webfetch: &'static str,
    websearch: &'static str,
}

fn autonomy_mapping(autonomy: Autonomy) -> (CompatibilityRoute, AutonomyKeys) {
    let keys = match autonomy {
        Autonomy::Manual => AutonomyKeys {
            edit: "ask",
            bash: "ask",
            webfetch: "ask",
            websearch: "ask",
        },
        Autonomy::Balanced => AutonomyKeys {
            edit: "allow",
            bash: "ask",
            webfetch: "ask",
            websearch: "allow",
        },
        // OpenCode's persisted config has no distinction between "mostly
        // autonomous" and "never ask" beyond explicit deny rules (the
        // closer `--auto` CLI flag is a runtime flag, not persisted) — Auto
        // and Unattended produce the same category values.
        Autonomy::Auto | Autonomy::Unattended => AutonomyKeys {
            edit: "allow",
            bash: "allow",
            webfetch: "allow",
            websearch: "allow",
        },
    };
    (CompatibilityRoute::Adaptable, keys)
}

fn sandbox_mapping(sandbox: SandboxScope) -> (CompatibilityRoute, &'static str) {
    match sandbox {
        SandboxScope::ReadOnly => (CompatibilityRoute::Adaptable, "deny"),
        SandboxScope::WorkspaceWrite => (CompatibilityRoute::Adaptable, "deny"),
        SandboxScope::FullAccess => (CompatibilityRoute::Adaptable, "allow"),
    }
}

fn model_mapping(model: ModelPreference) -> CompatibilityRoute {
    match model {
        ModelPreference::Default => CompatibilityRoute::Native,
        ModelPreference::Fast | ModelPreference::Capable => CompatibilityRoute::Unsupported,
    }
}

pub(crate) fn translate(preferences: &Preferences) -> PreferenceTranslation {
    let (autonomy_route, keys) = autonomy_mapping(preferences.autonomy);
    let (sandbox_route, external_directory) = sandbox_mapping(preferences.sandbox);
    let model_route = model_mapping(preferences.model);
    PreferenceTranslation {
        autonomy: PreferenceMapping {
            route: autonomy_route,
            native_summary: format!(
                "permission.edit={}, permission.bash={}, permission.webfetch={}, \
                 permission.websearch={}",
                keys.edit, keys.bash, keys.webfetch, keys.websearch
            ),
        },
        sandbox: PreferenceMapping {
            route: sandbox_route,
            native_summary: format!(
                "permission.external_directory={external_directory}{}",
                if preferences.sandbox == SandboxScope::ReadOnly {
                    " (also forces permission.edit=deny)"
                } else {
                    ""
                }
            ),
        },
        model: PreferenceMapping {
            route: model_route,
            native_summary: match preferences.model {
                ModelPreference::Default => "unset (OpenCode's own default)".to_owned(),
                _ => "unsupported: no verified model catalog".to_owned(),
            },
        },
    }
}

pub(crate) fn apply(
    config_path: &Path,
    preferences: &Preferences,
) -> Result<PreferenceApplyOutcome> {
    let (autonomy_route, keys) = autonomy_mapping(preferences.autonomy);
    let (sandbox_route, external_directory) = sandbox_mapping(preferences.sandbox);
    let model_route = model_mapping(preferences.model);

    json_config::merge(config_path, |config| {
        json_config::set_path(
            config,
            &["permission", "edit"],
            serde_json::json!(keys.edit),
        )?;
        json_config::set_path(
            config,
            &["permission", "bash"],
            serde_json::json!(keys.bash),
        )?;
        json_config::set_path(
            config,
            &["permission", "webfetch"],
            serde_json::json!(keys.webfetch),
        )?;
        json_config::set_path(
            config,
            &["permission", "websearch"],
            serde_json::json!(keys.websearch),
        )?;
        json_config::set_path(
            config,
            &["permission", "external_directory"],
            serde_json::json!(external_directory),
        )?;
        // Applied after autonomy's own `edit` value so "read only" wins.
        if preferences.sandbox == SandboxScope::ReadOnly {
            json_config::set_path(config, &["permission", "edit"], serde_json::json!("deny"))?;
        }
        Ok(())
    })?;

    let mut sandbox_keys = vec!["permission.external_directory".to_owned()];
    if preferences.sandbox == SandboxScope::ReadOnly {
        sandbox_keys.push("permission.edit".to_owned());
    }

    Ok(summarize_apply([
        PreferenceApplyDetail {
            route: autonomy_route,
            changed_keys: vec![
                "permission.edit".to_owned(),
                "permission.bash".to_owned(),
                "permission.webfetch".to_owned(),
                "permission.websearch".to_owned(),
            ],
            note: Some(
                "OpenCode has no single autonomy key; translated across several permission \
                 categories"
                    .to_owned(),
            ),
        },
        PreferenceApplyDetail {
            route: sandbox_route,
            changed_keys: sandbox_keys,
            note: (preferences.sandbox == SandboxScope::ReadOnly).then(|| {
                "OpenCode's bash tool can still write files; read-only only blocks the edit \
                 tool and paths outside the workspace"
                    .to_owned()
            }),
        },
        PreferenceApplyDetail {
            route: model_route,
            changed_keys: Vec::new(),
            note: (model_route == CompatibilityRoute::Unsupported).then(|| {
                "OpenCode's model id is provider-specific; no verified fast/capable catalog to \
                 translate against"
                    .to_owned()
            }),
        },
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(label: &str) -> std::path::PathBuf {
        uze_testkit::temp::scratch(label).join("opencode-preferences.json")
    }

    #[test]
    fn autonomy_is_always_adaptable_never_native() {
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
            assert_eq!(translation.autonomy.route, CompatibilityRoute::Adaptable);
        }
    }

    #[test]
    fn read_only_sandbox_overrides_autonomys_edit_value() {
        let path = temp_path("read-only-override");
        apply(
            &path,
            &Preferences {
                autonomy: Autonomy::Auto, // would set permission.edit = "allow"
                sandbox: SandboxScope::ReadOnly,
                ..Preferences::default()
            },
        )
        .unwrap();
        let written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written["permission"]["edit"], "deny");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn apply_preserves_foreign_keys() {
        let path = temp_path("preserve");
        std::fs::write(
            &path,
            serde_json::json!({"$schema": "https://opencode.ai/config.json", "mcp": {"x": {}}})
                .to_string(),
        )
        .unwrap();
        apply(&path, &Preferences::default()).unwrap();
        let written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written["$schema"], "https://opencode.ai/config.json");
        assert_eq!(written["mcp"]["x"], serde_json::json!({}));
        assert_eq!(written["permission"]["bash"], "ask");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn default_model_does_not_write_a_model_key() {
        let path = temp_path("default-model");
        apply(&path, &Preferences::default()).unwrap();
        let written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(written.get("model").is_none());
        let _ = std::fs::remove_file(&path);
    }
}
