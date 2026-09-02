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
        Autonomy, ModelPreference, PreferenceApplyOutcome, PreferenceTranslation, Preferences,
        SandboxScope,
    },
    router::CompatibilityRoute,
};

use crate::shared::preference::{Axis, Mapping, Value};

const AUTONOMY: &[&str] = &["permissions", "defaultMode"];
const SANDBOX: &[&str] = &["sandbox", "enabled"];
const ALLOW_WRITE: &[&str] = &["sandbox", "filesystem", "allowWrite"];
const MODEL: &[&str] = &["model"];

/// Neither non-default sandbox value claims `Native`: enabling the sandbox
/// without a workspace-scoped allowlist does not precisely deliver
/// "workspace-write", only "sandboxed with Claude's own default scope".
const SANDBOX_NOTE: &str = "Claude has no workspace-scoped write allowlist at user scope; \
                            sandbox is set to its coarse enabled/disabled state only";

fn mapping(preferences: &Preferences) -> Mapping {
    Mapping {
        autonomy: autonomy(preferences.autonomy),
        sandbox: sandbox(preferences.sandbox),
        model: model(preferences.model),
    }
}

fn autonomy(autonomy: Autonomy) -> Axis {
    // The documented equivalent of "never ask" is `bypassPermissions` (the
    // modern name for what `--dangerously-skip-permissions` invoked) — an
    // exact key/value match, hence still Native, though the TUI colors it
    // as the highest-risk value on its own axis.
    let value = match autonomy {
        Autonomy::Manual => "default",
        Autonomy::Balanced => "acceptEdits",
        Autonomy::Auto => "auto",
        Autonomy::Unattended => "bypassPermissions",
    };
    Axis::new(
        CompatibilityRoute::Native,
        format!("permissions.defaultMode = {value}"),
    )
    .set(AUTONOMY, Value::Text(value))
}

fn sandbox(sandbox: SandboxScope) -> Axis {
    match sandbox {
        SandboxScope::FullAccess => Axis::new(
            CompatibilityRoute::Native,
            "sandbox.enabled=false (default)",
        )
        .set(SANDBOX, Value::Flag(false)),
        SandboxScope::WorkspaceWrite => Axis::new(
            CompatibilityRoute::Adaptable,
            "sandbox.enabled=true (default filesystem scope; no workspace allowlist at user scope)",
        )
        .set(SANDBOX, Value::Flag(true))
        .note(SANDBOX_NOTE),
        SandboxScope::ReadOnly => Axis::new(
            CompatibilityRoute::Adaptable,
            "sandbox.enabled=true, sandbox.filesystem.allowWrite=[] (no writes anywhere)",
        )
        .set(SANDBOX, Value::Flag(true))
        .set(ALLOW_WRITE, Value::EmptyList)
        .note(SANDBOX_NOTE),
    }
}

fn model(model: ModelPreference) -> Axis {
    let value = match model {
        ModelPreference::Default => "default",
        ModelPreference::Fast => "haiku",
        ModelPreference::Capable => "opus",
    };
    Axis::new(CompatibilityRoute::Native, format!("model = {value}")).set(MODEL, Value::Text(value))
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
    use super::*;

    fn temp_path(label: &str) -> std::path::PathBuf {
        uze_testkit::temp::scratch(label).join("claude-preferences.json")
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
