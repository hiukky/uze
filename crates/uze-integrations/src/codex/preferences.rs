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
        Autonomy, ModelPreference, PreferenceApplyOutcome, PreferenceTranslation, Preferences,
        SandboxScope,
    },
    router::CompatibilityRoute,
};

use crate::shared::preference::{Axis, Mapping, Value};

const APPROVAL_POLICY: &[&str] = &["approval_policy"];
const SANDBOX_MODE: &[&str] = &["sandbox_mode"];
const NETWORK_ACCESS: &[&str] = &["sandbox_workspace_write", "network_access"];

fn mapping(preferences: &Preferences) -> Mapping {
    Mapping {
        autonomy: autonomy(preferences.autonomy),
        sandbox: sandbox(preferences),
        model: model(preferences.model),
    }
}

fn autonomy(autonomy: Autonomy) -> Axis {
    // Codex expresses a Claude-style auto mode as no command approvals
    // while retaining the separately configured sandbox boundary.
    let value = match autonomy {
        Autonomy::Manual => "untrusted",
        Autonomy::Balanced => "on-request",
        Autonomy::Auto | Autonomy::Unattended => "never",
    };
    Axis::new(
        CompatibilityRoute::Native,
        format!("approval_policy = \"{value}\""),
    )
    .set(APPROVAL_POLICY, Value::Text(value))
}

/// Codex's automatic mode is intended to run the whole development loop,
/// including the container-backed conformance lab. Keeping a workspace-only
/// sandbox here makes that mode silently unable to reach the Docker socket
/// even though it no longer asks for approval. Treat `Auto` as the cohesive
/// "run it for me" preset and use Codex's full-access sandbox.
fn effective_sandbox(preferences: &Preferences) -> SandboxScope {
    match preferences.autonomy {
        Autonomy::Auto => SandboxScope::FullAccess,
        _ => preferences.sandbox,
    }
}

fn sandbox(preferences: &Preferences) -> Axis {
    let effective = effective_sandbox(preferences);
    let value = match effective {
        SandboxScope::ReadOnly => "read-only",
        SandboxScope::WorkspaceWrite => "workspace-write",
        SandboxScope::FullAccess => "danger-full-access",
    };
    // The override must never apply silently: someone who chose
    // `ReadOnly`/`WorkspaceWrite` and then picked `Auto` autonomy has to be
    // able to see that their sandbox choice was not the one applied — so it
    // can never report as a plain `Native` match either.
    let override_note = (effective != preferences.sandbox).then(|| {
        format!(
            "sandbox raised to full-access because autonomy is Auto (overrides your configured \
             {:?})",
            preferences.sandbox
        )
    });
    let summary = match &override_note {
        Some(note) => format!("sandbox_mode = \"{value}\" ({note})"),
        None => format!("sandbox_mode = \"{value}\""),
    };
    let mut axis = Axis::new(
        match &override_note {
            Some(_) => CompatibilityRoute::Degraded,
            None => CompatibilityRoute::Native,
        },
        summary,
    )
    .set(SANDBOX_MODE, Value::Text(value));
    axis = if effective == SandboxScope::WorkspaceWrite {
        axis.set(NETWORK_ACCESS, Value::Flag(false))
    } else {
        // Cleared rather than written false: an absent key is Codex's own
        // default, and a value we invented is not.
        axis.clear(NETWORK_ACCESS)
    };
    match override_note {
        Some(note) => axis.note(note),
        None => axis,
    }
}

fn model(model: ModelPreference) -> Axis {
    match model {
        ModelPreference::Default => {
            Axis::new(CompatibilityRoute::Native, "unset (Codex's own default)")
        }
        _ => Axis::new(
            CompatibilityRoute::Unsupported,
            "unsupported: no verified model catalog",
        )
        .note(
            "Codex requires an exact model id; no verified fast/capable catalog to translate \
             against",
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
    mapping(preferences).apply_toml(config_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(label: &str) -> std::path::PathBuf {
        uze_testkit::temp::scratch(label).join("codex-preferences.toml")
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
    fn auto_autonomy_disables_command_approvals() {
        let translation = translate(&Preferences {
            autonomy: Autonomy::Auto,
            ..Preferences::default()
        });
        assert_eq!(translation.autonomy.route, CompatibilityRoute::Native);
        assert_eq!(
            translation.autonomy.native_summary,
            "approval_policy = \"never\""
        );
    }

    #[test]
    fn auto_uses_full_access_even_when_the_profile_sandbox_is_workspace_write() {
        let path = temp_path("auto-full-access");
        std::fs::write(&path, "[sandbox_workspace_write]\nnetwork_access = false\n").unwrap();
        let outcome = apply(
            &path,
            &Preferences {
                autonomy: Autonomy::Auto,
                sandbox: SandboxScope::WorkspaceWrite,
                ..Preferences::default()
            },
        )
        .unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("approval_policy = \"never\""));
        assert!(contents.contains("sandbox_mode = \"danger-full-access\""));
        assert!(
            !contents.contains("network_access = false"),
            "Auto must not retain the workspace-only network restriction"
        );
        // The override must never be silent: it must surface as an
        // approximation, not a plain, unremarkable `Applied`.
        match outcome {
            PreferenceApplyOutcome::AppliedWithApproximation { notes, .. } => {
                assert!(
                    notes
                        .iter()
                        .any(|note| note.contains("Auto") && note.contains("WorkspaceWrite")),
                    "expected a note naming the overridden sandbox, got: {notes:?}"
                );
            }
            other => {
                panic!("expected AppliedWithApproximation with an override note, got {other:?}")
            }
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn auto_translation_reports_the_effective_full_access_sandbox() {
        let translation = translate(&Preferences {
            autonomy: Autonomy::Auto,
            sandbox: SandboxScope::ReadOnly,
            ..Preferences::default()
        });
        assert_eq!(translation.sandbox.route, CompatibilityRoute::Degraded);
        assert!(
            translation
                .sandbox
                .native_summary
                .starts_with("sandbox_mode = \"danger-full-access\""),
        );
        assert!(
            translation.sandbox.native_summary.contains("ReadOnly"),
            "the override must name what it overrode: {}",
            translation.sandbox.native_summary
        );
    }

    #[test]
    fn auto_with_full_access_already_configured_is_not_reported_as_an_override() {
        // Auto's effective sandbox equals the configured one here, so there
        // is nothing to disclose — this must stay a plain `Native` match.
        let translation = translate(&Preferences {
            autonomy: Autonomy::Auto,
            sandbox: SandboxScope::FullAccess,
            ..Preferences::default()
        });
        assert_eq!(translation.sandbox.route, CompatibilityRoute::Native);
        assert_eq!(
            translation.sandbox.native_summary,
            "sandbox_mode = \"danger-full-access\""
        );
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
