//! Translates universal [`Preferences`] into Claude Code's `settings.json`.
//!
//! Verified against current official docs (code.claude.com, Sep 2026):
//! `docs/en/permission-modes`, `docs/en/settings-reference`,
//! `docs/en/sandboxing`, `docs/en/model-config` — and against the binary
//! itself (2.1.268), whose session `init` reports the permission mode and
//! model it actually resolved:
//!
//! - every `permissions.defaultMode` written here is one it resolves as
//!   written; an unknown one silently falls back to `default`. `auto` does
//!   too when the model cannot run it — Haiku cannot — so `auto` is never
//!   paired with the fast tier.
//! - `model: "default"` is *not* resolved, whatever the docs say: the
//!   binary reports `unrecognized_model` and sends the literal `default`
//!   to the API. The harness's own default is the key being absent, so
//!   that word is removed; any model that does resolve is left where it is.
//! - With the sandbox enabled, commands may write the working directory,
//!   the session temp directory and added directories — which is exactly
//!   "workspace-write". Relative sandbox paths in user-scope settings
//!   resolve against `~/.claude`, so no user-scope setting can take the
//!   working directory's write access away: "read-only" cannot be
//!   delivered from here.
//! - On Linux and WSL2 the sandbox needs `bubblewrap` and `socat`; without
//!   them Claude warns and runs every command unsandboxed.

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

const AUTONOMY: &[&str] = &["permissions", "defaultMode"];
const SANDBOX: &[&str] = &["sandbox", "enabled"];
const MODEL: &[&str] = &["model"];

/// The one `model` value removed on sight: an earlier release wrote it and
/// Claude cannot resolve it, so nobody would have chosen it. A `haiku` or
/// `opus` there may be the operator's own choice — nothing in the file says
/// who wrote it — so asking for Claude's default leaves those alone.
const UNRESOLVABLE_MODELS: &[&str] = &["default"];

/// What the machine offers Claude's sandbox. Read by the integration, not
/// here, so the translation stays a function of its inputs.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct SandboxHost {
    /// Programs Claude's sandbox needs that this machine does not have.
    pub(crate) missing: Vec<&'static str>,
}

impl SandboxHost {
    /// Claude's Linux and WSL2 sandbox needs both programs on `PATH`;
    /// macOS's Seatbelt needs nothing installed.
    pub(crate) fn detect() -> Self {
        if !cfg!(target_os = "linux") {
            return Self::default();
        }
        Self {
            missing: [("bubblewrap", "bwrap"), ("socat", "socat")]
                .into_iter()
                .filter(|(_, program)| !uze_core::subprocess::program_on_path(program))
                .map(|(package, _)| package)
                .collect(),
        }
    }
}

fn mapping(preferences: &Preferences, host: &SandboxHost) -> Mapping {
    Mapping {
        autonomy: autonomy(preferences.autonomy, preferences.model),
        sandbox: sandbox(preferences.sandbox, host),
        model: model(preferences.model),
    }
}

fn autonomy(autonomy: Autonomy, model: ModelPreference) -> Axis {
    // Auto mode needs a model that can run its classifier, and Haiku is not
    // one: Claude then starts in Manual, the opposite of what was asked.
    // Accepting edits is the closest mode that still runs on it.
    if autonomy == Autonomy::Auto && model == ModelPreference::Fast {
        return Axis::new(
            CompatibilityRoute::Degraded,
            "permissions.defaultMode = acceptEdits (auto mode does not run on haiku)",
        )
        .set(AUTONOMY, Value::Text("acceptEdits"))
        .note(
            "Claude's auto mode does not run on Haiku and would start in Manual instead; edits \
             are accepted without asking, commands still ask — pick another model for auto mode",
        );
    }
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

fn sandbox(sandbox: SandboxScope, host: &SandboxHost) -> Axis {
    let axis = match sandbox {
        SandboxScope::FullAccess => {
            return Axis::new(CompatibilityRoute::Native, "sandbox.enabled = false")
                .set(SANDBOX, Value::Flag(false));
        }
        SandboxScope::WorkspaceWrite => Axis::new(
            CompatibilityRoute::Native,
            "sandbox.enabled = true (writes limited to the working directory)",
        ),
        SandboxScope::ReadOnly => Axis::new(
            CompatibilityRoute::Degraded,
            "sandbox.enabled = true (the working directory stays writable)",
        )
        .note(
            "Claude's sandbox always lets commands write the working directory, and user-scope \
             settings cannot take that away; read-only is delivered as workspace-write",
        ),
    }
    .set(SANDBOX, Value::Flag(true));
    if host.missing.is_empty() {
        return axis;
    }
    let missing = host.missing.join(" and ");
    let unavailable = format!(
        "Claude cannot start its sandbox here without {missing}, and runs every command \
         unsandboxed instead — install {missing}"
    );
    let note = match axis.note.clone() {
        Some(existing) => format!("{existing}; {unavailable}"),
        None => unavailable,
    };
    Axis {
        route: CompatibilityRoute::Degraded,
        ..axis
    }
    .note(note)
}

fn model(model: ModelPreference) -> Axis {
    match model {
        ModelPreference::Default => Axis::new(
            CompatibilityRoute::Native,
            "model unset (Claude Code's own default; a model you chose is kept)",
        )
        .release(MODEL, UNRESOLVABLE_MODELS),
        ModelPreference::Fast => {
            Axis::new(CompatibilityRoute::Native, "model = haiku").set(MODEL, Value::Text("haiku"))
        }
        ModelPreference::Capable => {
            Axis::new(CompatibilityRoute::Native, "model = opus").set(MODEL, Value::Text("opus"))
        }
    }
}

pub(crate) fn translate(preferences: &Preferences, host: &SandboxHost) -> PreferenceTranslation {
    mapping(preferences, host).translate()
}

pub(crate) fn apply(
    settings_path: &Path,
    preferences: &Preferences,
    host: &SandboxHost,
) -> Result<PreferenceApplyOutcome> {
    mapping(preferences, host).apply_json(settings_path)
}

pub(crate) fn plan(
    settings_path: &Path,
    preferences: &Preferences,
    host: &SandboxHost,
) -> Result<PreferencePlan> {
    mapping(preferences, host).plan_json(settings_path)
}

#[cfg(test)]
mod tests {
    use uze_core::preference::{PlannedValue, PreferenceAxis};

    use super::*;

    fn temp_path(label: &str) -> std::path::PathBuf {
        uze_testkit::temp::scratch(label).join("claude-preferences.json")
    }

    fn equipped() -> SandboxHost {
        SandboxHost::default()
    }

    fn written(path: &Path) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn autonomy_and_model_translate_as_native() {
        let translation = translate(&Preferences::default(), &equipped());
        assert_eq!(translation.autonomy.route, CompatibilityRoute::Native);
        assert_eq!(translation.model.route, CompatibilityRoute::Native);
    }

    #[test]
    fn workspace_write_is_claudes_own_sandbox_scope() {
        let translation = translate(
            &Preferences {
                sandbox: SandboxScope::WorkspaceWrite,
                ..Preferences::default()
            },
            &equipped(),
        );
        assert_eq!(translation.sandbox.route, CompatibilityRoute::Native);
    }

    #[test]
    fn read_only_is_degraded_because_the_working_directory_stays_writable() {
        let translation = translate(
            &Preferences {
                sandbox: SandboxScope::ReadOnly,
                ..Preferences::default()
            },
            &equipped(),
        );
        assert_eq!(translation.sandbox.route, CompatibilityRoute::Degraded);
    }

    #[test]
    fn a_sandbox_this_machine_cannot_start_is_degraded_and_says_what_is_missing() {
        let host = SandboxHost {
            missing: vec!["socat"],
        };
        let path = temp_path("sandbox-missing");
        let outcome = apply(&path, &Preferences::default(), &host).unwrap();
        let PreferenceApplyOutcome::AppliedWithApproximation { notes, .. } = outcome else {
            panic!("a sandbox that cannot start must not report as applied: {outcome:?}");
        };
        assert!(notes.iter().any(|note| note.contains("socat")), "{notes:?}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn full_access_needs_no_sandbox_dependency() {
        let host = SandboxHost {
            missing: vec!["bubblewrap", "socat"],
        };
        let translation = translate(
            &Preferences {
                sandbox: SandboxScope::FullAccess,
                ..Preferences::default()
            },
            &host,
        );
        assert_eq!(translation.sandbox.route, CompatibilityRoute::Native);
    }

    #[test]
    fn apply_writes_expected_keys_and_preserves_foreign_content() {
        let path = temp_path("preserve");
        std::fs::write(
            &path,
            serde_json::json!({
                "foreignKey": "untouched",
                "permissions": {"allow": ["Bash(ls)"]},
                "sandbox": {"filesystem": {"allowWrite": ["~/.kube"]}}
            })
            .to_string(),
        )
        .unwrap();

        let outcome = apply(
            &path,
            &Preferences {
                autonomy: Autonomy::Balanced,
                sandbox: SandboxScope::ReadOnly,
                model: ModelPreference::Capable,
            },
            &equipped(),
        )
        .unwrap();
        assert!(matches!(
            outcome,
            PreferenceApplyOutcome::AppliedWithApproximation { .. }
        ));

        let written = written(&path);
        assert_eq!(written["foreignKey"], "untouched");
        assert_eq!(
            written["permissions"]["allow"],
            serde_json::json!(["Bash(ls)"])
        );
        assert_eq!(
            written["sandbox"]["filesystem"]["allowWrite"],
            serde_json::json!(["~/.kube"]),
            "the operator's own write allowlist is theirs"
        );
        assert_eq!(written["permissions"]["defaultMode"], "acceptEdits");
        assert_eq!(written["sandbox"]["enabled"], true);
        assert_eq!(written["model"], "opus");
        let _ = std::fs::remove_file(&path);
    }

    /// The reported failure: a `default` profile wrote `model: "default"`,
    /// which Claude Code 2.1.268 answers with `unrecognized_model`.
    #[test]
    fn the_default_model_is_the_key_being_absent_never_the_word_default() {
        let path = temp_path("model-default");
        apply(&path, &Preferences::default(), &equipped()).unwrap();
        assert!(written(&path).get("model").is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_default_model_removes_the_word_claude_cannot_resolve() {
        let path = temp_path("model-release");
        std::fs::write(&path, r#"{"model":"default"}"#).unwrap();
        apply(&path, &Preferences::default(), &equipped()).unwrap();
        assert!(written(&path).get("model").is_none());
        let _ = std::fs::remove_file(&path);
    }

    /// Nothing in the file says who wrote `opus` — a `capable` profile or
    /// the operator by hand — so it is never taken for UZE's own.
    #[test]
    fn the_default_model_keeps_any_model_that_resolves() {
        for chosen in ["sonnet[1m]", "opus", "haiku"] {
            let path = temp_path("model-kept");
            std::fs::write(&path, serde_json::json!({ "model": chosen }).to_string()).unwrap();
            let plan = plan(&path, &Preferences::default(), &equipped()).unwrap();
            let model = plan
                .axes
                .iter()
                .find(|axis| axis.axis == PreferenceAxis::Model)
                .unwrap();
            assert_eq!(model.keys[0].planned, PlannedValue::Kept, "{chosen}");
            apply(&path, &Preferences::default(), &equipped()).unwrap();
            assert_eq!(written(&path)["model"], chosen);
            let _ = std::fs::remove_file(&path);
        }
    }

    /// A parent some other tool shaped differently is left alone by
    /// `apply`; the preview has to say so instead of promising the write.
    #[test]
    fn a_key_apply_would_refuse_fails_the_preview_the_same_way() {
        let path = temp_path("foreign-shape");
        std::fs::write(&path, r#"{"permissions":[]}"#).unwrap();
        let planned = plan(&path, &Preferences::default(), &equipped())
            .expect_err("the preview must not promise permissions.defaultMode");
        let applied = apply(&path, &Preferences::default(), &equipped())
            .expect_err("apply refuses to clobber the array");
        assert_eq!(planned.to_string(), applied.to_string());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            r#"{"permissions":[]}"#
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn plan_reads_what_is_there_and_writes_nothing() {
        let path = temp_path("plan");
        let before = r#"{"model":"default","permissions":{"defaultMode":"auto"}}"#;
        std::fs::write(&path, before).unwrap();
        let plan = plan(&path, &Preferences::default(), &equipped()).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        assert_eq!(plan.config_path, path);
        let keys: Vec<_> = plan.axes.iter().flat_map(|axis| &axis.keys).collect();
        let key = |name: &str| *keys.iter().find(|key| key.key == name).unwrap();
        assert_eq!(
            key("permissions.defaultMode").current.as_deref(),
            Some("\"auto\"")
        );
        assert_eq!(
            key("permissions.defaultMode").planned,
            PlannedValue::Set("\"acceptEdits\"".to_owned())
        );
        assert_eq!(key("model").planned, PlannedValue::Removed);
        assert_eq!(plan.pending(), 3);

        apply(&path, &Preferences::default(), &equipped()).unwrap();
        let after = super::plan(&path, &Preferences::default(), &equipped()).unwrap();
        assert_eq!(after.pending(), 0, "applying leaves nothing pending");
        let _ = std::fs::remove_file(&path);
    }

    /// Claude Code starts in Manual when `auto` is asked of a model that
    /// cannot run it; with Haiku that was every `auto` + `fast` profile.
    #[test]
    fn auto_on_the_fast_model_is_never_silently_manual() {
        let translation = translate(
            &Preferences {
                autonomy: Autonomy::Auto,
                model: ModelPreference::Fast,
                ..Preferences::default()
            },
            &equipped(),
        );
        assert_eq!(translation.autonomy.route, CompatibilityRoute::Degraded);
        assert!(!translation.autonomy.native_summary.contains("= auto "));
        for model in [ModelPreference::Default, ModelPreference::Capable] {
            let translation = translate(
                &Preferences {
                    autonomy: Autonomy::Auto,
                    model,
                    ..Preferences::default()
                },
                &equipped(),
            );
            assert_eq!(
                translation.autonomy.native_summary,
                "permissions.defaultMode = auto"
            );
        }
    }
}
