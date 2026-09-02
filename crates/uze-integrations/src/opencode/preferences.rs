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
        Autonomy, ModelPreference, PreferenceApplyOutcome, PreferenceTranslation, Preferences,
        SandboxScope,
    },
    router::CompatibilityRoute,
};

use crate::shared::preference::{Axis, Mapping, Value};

const EDIT: &[&str] = &["permission", "edit"];
const BASH: &[&str] = &["permission", "bash"];
const WEBFETCH: &[&str] = &["permission", "webfetch"];
const WEBSEARCH: &[&str] = &["permission", "websearch"];
const EXTERNAL_DIRECTORY: &[&str] = &["permission", "external_directory"];

fn mapping(preferences: &Preferences) -> Mapping {
    Mapping {
        autonomy: autonomy(preferences.autonomy),
        // Ordered after autonomy on purpose: read-only re-writes
        // `permission.edit`, and the later write is the one that stands.
        sandbox: sandbox(preferences.sandbox),
        model: model(preferences.model),
    }
}

fn autonomy(autonomy: Autonomy) -> Axis {
    // OpenCode's persisted config has no distinction between "mostly
    // autonomous" and "never ask" beyond explicit deny rules (the closer
    // `--auto` CLI flag is a runtime flag, not persisted) — Auto and
    // Unattended produce the same category values.
    let (edit, bash, webfetch, websearch) = match autonomy {
        Autonomy::Manual => ("ask", "ask", "ask", "ask"),
        Autonomy::Balanced => ("allow", "ask", "ask", "allow"),
        Autonomy::Auto | Autonomy::Unattended => ("allow", "allow", "allow", "allow"),
    };
    Axis::new(
        CompatibilityRoute::Adaptable,
        format!(
            "permission.edit={edit}, permission.bash={bash}, permission.webfetch={webfetch}, \
             permission.websearch={websearch}"
        ),
    )
    .set(EDIT, Value::Text(edit))
    .set(BASH, Value::Text(bash))
    .set(WEBFETCH, Value::Text(webfetch))
    .set(WEBSEARCH, Value::Text(websearch))
    .note("OpenCode has no single autonomy key; translated across several permission categories")
}

fn sandbox(sandbox: SandboxScope) -> Axis {
    let external_directory = match sandbox {
        SandboxScope::ReadOnly | SandboxScope::WorkspaceWrite => "deny",
        SandboxScope::FullAccess => "allow",
    };
    let read_only = sandbox == SandboxScope::ReadOnly;
    let axis = Axis::new(
        CompatibilityRoute::Adaptable,
        format!(
            "permission.external_directory={external_directory}{}",
            if read_only {
                " (also forces permission.edit=deny)"
            } else {
                ""
            }
        ),
    )
    .set(EXTERNAL_DIRECTORY, Value::Text(external_directory));
    if read_only {
        axis.set(EDIT, Value::Text("deny")).note(
            "OpenCode's bash tool can still write files; read-only only blocks the edit tool and \
             paths outside the workspace",
        )
    } else {
        axis
    }
}

fn model(model: ModelPreference) -> Axis {
    match model {
        ModelPreference::Default => {
            Axis::new(CompatibilityRoute::Native, "unset (OpenCode's own default)")
        }
        _ => Axis::new(
            CompatibilityRoute::Unsupported,
            "unsupported: no verified model catalog",
        )
        .note(
            "OpenCode's model id is provider-specific; no verified fast/capable catalog to \
             translate against",
        ),
    }
}

pub(crate) fn translate(preferences: &Preferences) -> PreferenceTranslation {
    mapping(preferences).translate()
}

pub(crate) fn apply(
    config_path: &Path,
    preferences: &Preferences,
) -> Result<PreferenceApplyOutcome> {
    mapping(preferences).apply_json(config_path)
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
