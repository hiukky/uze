//! Profiles/Preferences orchestration: `TUI -> UzeApplication -> Preferences
//! -> PreferencePort -> integration adapters`. Mirrors `harness_health()`'s
//! "iterate `self.integrations`/adapters, no second detection loop" pattern
//! and `setup()`'s per-harness partial-failure isolation.

use serde::Serialize;
use uze_core::{
    Result, UzeError,
    preference::{PreferenceApplyOutcome, PreferencePlan, PreferencePort, Preferences},
    profile_state,
};

use super::services::Profiles;

/// A profile as shown in a list. Includes `preferences` — unlike a vendor
/// probe (e.g. `PluginSummary`'s "detail is a separate, expensive read"
/// convention), a profile is small, local, already-loaded JSON, so there is
/// no cost reason to split a lighter summary from a heavier detail read.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProfileSummary {
    pub id: String,
    pub description: Option<String>,
    pub active: bool,
    pub preferences: Preferences,
}

/// One harness's result from applying a profile's preferences to it.
#[derive(Clone, Debug, Serialize)]
pub struct ProfileApplyResult {
    pub integration: String,
    pub outcome: PreferenceApplyOutcome,
}

/// What applying one set of preferences would do to each harness, read
/// against every harness's configuration as it is now.
#[derive(Clone, Debug, Serialize)]
pub struct ProfilePreview {
    pub preferences: Preferences,
    pub harnesses: Vec<HarnessPreview>,
}

/// One harness's part of a [`ProfilePreview`]. `plan` is the reason as
/// text when the configuration cannot be read — the same condition under
/// which applying would fail for this harness alone.
#[derive(Clone, Debug, Serialize)]
pub struct HarnessPreview {
    pub integration: String,
    pub plan: std::result::Result<PreferencePlan, String>,
}

impl ProfilePreview {
    /// Native keys, across every harness it could read, that applying
    /// would change.
    pub fn pending(&self) -> usize {
        self.harnesses
            .iter()
            .filter_map(|harness| harness.plan.as_ref().ok())
            .map(PreferencePlan::pending)
            .sum()
    }
}

impl Profiles<'_> {
    #[tracing::instrument(name = "profiles.list", skip_all, err)]
    pub fn list(&self) -> Result<Vec<ProfileSummary>> {
        // A listing writes only on a home that has no profile yet; on every
        // other read the mutation lock would be taken for nothing.
        if profile_state::load(&self.0.home)?.is_empty() {
            let _mutation = uze_core::persistence::MutationLock::acquire(&self.0.home)?;
            profile_state::ensure_default(&self.0.home)?;
        }
        let active = profile_state::active(&self.0.home)?;
        Ok(profile_state::load(&self.0.home)?
            .into_values()
            .map(|record| ProfileSummary {
                active: active.as_deref() == Some(record.id.as_str()),
                id: record.id,
                description: record.description,
                preferences: record.preferences,
            })
            .collect())
    }

    #[tracing::instrument(name = "profiles.get", skip_all, fields(id = %id), err)]
    pub fn get(&self, id: &str) -> Result<Option<profile_state::ProfileRecord>> {
        profile_state::get(&self.0.home, id)
    }

    #[tracing::instrument(name = "profiles.create", skip_all, fields(id = %id), err)]
    pub fn create(
        &self,
        id: &str,
        description: Option<String>,
        preferences: Preferences,
    ) -> Result<()> {
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.0.home)?;
        if id != profile_state::DEFAULT_PROFILE_ID {
            profile_state::ensure_default(&self.0.home)?;
        }
        profile_state::create(&self.0.home, id, description, preferences)?;
        if profile_state::active(&self.0.home)?.is_none() {
            profile_state::set_active(&self.0.home, id)?;
        }
        Ok(())
    }

    #[tracing::instrument(name = "profiles.update_preferences", skip_all, fields(id = %id), err)]
    pub fn update_preferences(&self, id: &str, preferences: Preferences) -> Result<()> {
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.0.home)?;
        profile_state::update_preferences(&self.0.home, id, preferences)
    }

    #[tracing::instrument(name = "profiles.delete", skip_all, fields(id = %id), err)]
    pub fn delete(&self, id: &str) -> Result<()> {
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.0.home)?;
        profile_state::ensure_default(&self.0.home)?;
        profile_state::delete(&self.0.home, id)
    }

    #[tracing::instrument(name = "profiles.set_active", skip_all, fields(id = %id), err)]
    pub fn set_active(&self, id: &str) -> Result<()> {
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.0.home)?;
        profile_state::set_active(&self.0.home, id)
    }

    /// What applying `preferences` to each of `harness_ids` would write,
    /// without writing anything. Takes the preferences rather than a
    /// profile id: the editor changes them optimistically and persists in
    /// the background, and a preview read from disk would race that write.
    #[tracing::instrument(name = "profiles.preview", skip_all)]
    pub fn preview(&self, preferences: &Preferences, harness_ids: &[String]) -> ProfilePreview {
        ProfilePreview {
            preferences: *preferences,
            harnesses: harness_ids
                .iter()
                .map(|harness_id| HarnessPreview {
                    integration: harness_id.clone(),
                    plan: self
                        .adapter(harness_id)
                        .ok_or_else(|| unregistered(harness_id))
                        .and_then(|adapter| {
                            adapter.plan(preferences).map_err(|error| error.to_string())
                        }),
                })
                .collect(),
        }
    }

    fn adapter(&self, harness_id: &str) -> Option<&dyn PreferencePort> {
        self.0
            .preference_adapters
            .iter()
            .find(|adapter| adapter.preference_id() == harness_id)
            .map(Box::as_ref)
    }

    /// Applies one profile's preferences to exactly the requested harnesses.
    /// A single harness failing (a hard `Err` from its adapter, or no
    /// registered adapter for the id) never aborts the rest — it becomes a
    /// `Failed` result for that harness alone, matching `setup()`'s
    /// per-harness partial-failure isolation.
    #[tracing::instrument(name = "profiles.apply", skip_all, fields(id = %id), err)]
    pub fn apply(&self, id: &str, harness_ids: &[String]) -> Result<Vec<ProfileApplyResult>> {
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.0.home)?;
        let record = profile_state::get(&self.0.home, id)?
            .ok_or_else(|| UzeError::UnknownProfile(id.to_owned()))?;
        Ok(harness_ids
            .iter()
            .map(|harness_id| {
                let outcome = match self.adapter(harness_id) {
                    Some(adapter) => adapter.apply(&record.preferences).unwrap_or_else(|error| {
                        PreferenceApplyOutcome::Failed {
                            reason: error.to_string(),
                        }
                    }),
                    None => PreferenceApplyOutcome::Failed {
                        reason: unregistered(harness_id),
                    },
                };
                ProfileApplyResult {
                    integration: harness_id.clone(),
                    outcome,
                }
            })
            .collect())
    }
}

fn unregistered(harness_id: &str) -> String {
    format!("no preference adapter registered for `{harness_id}`")
}

#[cfg(test)]
mod tests {
    use crate::UzeApplication;
    use uze_core::provisioning::SystemProcessRunner;
    use uze_core::{
        home::UzeHome,
        preference::{
            Autonomy, ModelPreference, PreferenceMapping, PreferencePort, PreferenceTranslation,
            SandboxScope,
        },
        router::CompatibilityRoute,
    };

    use super::*;

    fn temp_home(label: &str) -> UzeHome {
        UzeHome::at(uze_testkit::temp::scratch(label))
    }

    struct FakeAdapter {
        id: &'static str,
        result: std::sync::Mutex<Option<Result<PreferenceApplyOutcome>>>,
    }

    impl FakeAdapter {
        fn succeeding(id: &'static str) -> Self {
            Self {
                id,
                result: std::sync::Mutex::new(Some(Ok(PreferenceApplyOutcome::Applied {
                    changed_keys: vec!["fake.key".to_owned()],
                }))),
            }
        }

        fn failing(id: &'static str) -> Self {
            Self {
                id,
                result: std::sync::Mutex::new(Some(Err(UzeError::ExposureUnavailable(
                    "boom".to_owned(),
                )))),
            }
        }
    }

    impl PreferencePort for FakeAdapter {
        fn preference_id(&self) -> &'static str {
            self.id
        }
        fn translate(&self, _preferences: &Preferences) -> PreferenceTranslation {
            // The profile lifecycle tests exercise apply (and its outcome),
            // not translation; a static mapping keeps the fake honest
            // without pretending to model any vendor's encoding.
            let mapping = |route: CompatibilityRoute| PreferenceMapping {
                route,
                native_summary: "fake".to_owned(),
            };
            PreferenceTranslation {
                autonomy: mapping(CompatibilityRoute::Native),
                sandbox: mapping(CompatibilityRoute::Native),
                model: mapping(CompatibilityRoute::Native),
            }
        }
        fn plan(&self, _preferences: &Preferences) -> Result<PreferencePlan> {
            Ok(PreferencePlan {
                config_path: std::path::PathBuf::from(format!("/fake/{}.json", self.id)),
                axes: Vec::new(),
            })
        }
        fn apply(&self, _preferences: &Preferences) -> Result<PreferenceApplyOutcome> {
            self.result
                .lock()
                .unwrap()
                .take()
                .expect("apply called once")
        }
    }

    fn app_with_adapters(home: UzeHome, adapters: Vec<Box<dyn PreferencePort>>) -> UzeApplication {
        UzeApplication::new_with_runner_and_preferences(
            home,
            Vec::new(),
            adapters,
            Box::new(SystemProcessRunner),
        )
    }

    #[test]
    fn create_list_and_delete_round_trip_preserves_one_active_profile() {
        let home = temp_home("crud");
        let app = UzeApplication::new(home.clone(), Vec::new());
        app.profiles()
            .create("default", None, Preferences::default())
            .unwrap();
        assert_eq!(app.profiles().list().unwrap().len(), 1);
        assert!(app.profiles().list().unwrap()[0].active);
        app.profiles()
            .create("coding", None, Preferences::default())
            .unwrap();
        app.profiles().delete("default").unwrap();
        let profiles = app.profiles().list().unwrap();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].id, "coding");
        assert!(profiles[0].active);
        let _ = std::fs::remove_dir_all(home.root());
    }

    #[test]
    fn apply_profile_isolates_one_harness_failure_from_the_rest() {
        let home = temp_home("partial-failure");
        let bootstrap = UzeApplication::new(home.clone(), Vec::new());
        bootstrap
            .profiles()
            .create("default", None, Preferences::default())
            .unwrap();

        let app = app_with_adapters(
            home.clone(),
            vec![
                Box::new(FakeAdapter::succeeding("good")),
                Box::new(FakeAdapter::failing("bad")),
            ],
        );
        let results = app
            .profiles()
            .apply("default", &["good".to_owned(), "bad".to_owned()])
            .unwrap();
        assert_eq!(results.len(), 2);
        assert!(matches!(
            results[0].outcome,
            PreferenceApplyOutcome::Applied { .. }
        ));
        assert!(matches!(
            results[1].outcome,
            PreferenceApplyOutcome::Failed { .. }
        ));
        let _ = std::fs::remove_dir_all(home.root());
    }

    #[test]
    fn applying_to_an_unregistered_harness_id_fails_only_that_entry() {
        let home = temp_home("unregistered");
        let bootstrap = UzeApplication::new(home.clone(), Vec::new());
        bootstrap
            .profiles()
            .create("default", None, Preferences::default())
            .unwrap();
        let app = app_with_adapters(home.clone(), Vec::new());
        let results = app
            .profiles()
            .apply("default", &["ghost".to_owned()])
            .unwrap();
        assert!(matches!(
            results[0].outcome,
            PreferenceApplyOutcome::Failed { .. }
        ));
        let _ = std::fs::remove_dir_all(home.root());
    }

    #[test]
    fn applying_an_unknown_profile_id_is_an_error() {
        let home = temp_home("unknown-profile");
        let app = UzeApplication::new(home.clone(), Vec::new());
        assert!(matches!(
            app.profiles().apply("ghost", &[]),
            Err(UzeError::UnknownProfile(_))
        ));
        let _ = std::fs::remove_dir_all(home.root());
    }

    #[test]
    fn updating_preferences_changes_only_the_targeted_profile() {
        let home = temp_home("update");
        let app = UzeApplication::new(home.clone(), Vec::new());
        app.profiles()
            .create("a", None, Preferences::default())
            .unwrap();
        app.profiles()
            .create("b", None, Preferences::default())
            .unwrap();
        app.profiles()
            .update_preferences(
                "a",
                Preferences {
                    autonomy: Autonomy::Unattended,
                    sandbox: SandboxScope::FullAccess,
                    model: ModelPreference::Capable,
                },
            )
            .unwrap();
        assert_eq!(
            app.profiles()
                .get("a")
                .unwrap()
                .unwrap()
                .preferences
                .autonomy,
            Autonomy::Unattended
        );
        assert_eq!(
            app.profiles()
                .get("b")
                .unwrap()
                .unwrap()
                .preferences
                .autonomy,
            Autonomy::Balanced
        );
        let _ = std::fs::remove_dir_all(home.root());
    }

    /// End-to-end: create a profile, apply it to two real (isolated, not
    /// faked — preference writes are pure filesystem, no vendor CLI needed)
    /// integrations, and assert the exact native keys landed while
    /// pre-existing foreign content in each harness's config survived.
    #[test]
    fn create_configure_select_apply_writes_expected_native_keys_and_preserves_foreign_content() {
        let root = uze_testkit::temp::scratch("profile-e2e");
        let home = UzeHome::at(root.join("uze"));

        // Pre-existing, foreign (non-UZE) content in each harness's shared
        // config file — the writer must leave it untouched.
        let claude_settings = root.join("claude").join("settings.json");
        std::fs::create_dir_all(claude_settings.parent().unwrap()).unwrap();
        std::fs::write(
            &claude_settings,
            serde_json::json!({"foreignKey": "untouched", "permissions": {"allow": ["Bash(ls)"]}})
                .to_string(),
        )
        .unwrap();

        let codex_config = root.join(".codex").join("config.toml");
        std::fs::create_dir_all(codex_config.parent().unwrap()).unwrap();
        std::fs::write(
            &codex_config,
            "# a user comment\nmodel = \"gpt-5.6\"\n\n[model_providers.openai]\nname = \"OpenAI\"\n",
        )
        .unwrap();

        let registry = uze_integrations::registry::IntegrationRegistry::isolated(&root, &home);
        let (integrations, preference_adapters) = registry.into_parts();
        let app = UzeApplication::new_with_runner_and_preferences(
            home,
            integrations,
            preference_adapters,
            Box::new(SystemProcessRunner),
        );

        app.profiles()
            .create(
                "dev-autonomous",
                Some("test profile".to_owned()),
                Preferences {
                    autonomy: Autonomy::Unattended,
                    sandbox: SandboxScope::FullAccess,
                    model: ModelPreference::Capable,
                },
            )
            .unwrap();

        let results = app
            .profiles()
            .apply(
                "dev-autonomous",
                &["claude-code".to_owned(), "codex".to_owned()],
            )
            .unwrap();
        assert_eq!(results.len(), 2);
        let claude_result = results
            .iter()
            .find(|result| result.integration == "claude-code")
            .unwrap();
        // Manual/Unattended/full-access/opus are all Native for Claude.
        assert!(matches!(
            claude_result.outcome,
            PreferenceApplyOutcome::Applied { .. }
        ));
        let codex_result = results
            .iter()
            .find(|result| result.integration == "codex")
            .unwrap();
        // Codex has no verified "capable" model catalog entry — Unsupported
        // for that one field, hence an approximation overall.
        assert!(matches!(
            codex_result.outcome,
            PreferenceApplyOutcome::AppliedWithApproximation { .. }
        ));

        let claude_written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&claude_settings).unwrap()).unwrap();
        assert_eq!(claude_written["foreignKey"], "untouched");
        assert_eq!(
            claude_written["permissions"]["allow"],
            serde_json::json!(["Bash(ls)"])
        );
        assert_eq!(
            claude_written["permissions"]["defaultMode"],
            "bypassPermissions"
        );
        assert_eq!(claude_written["sandbox"]["enabled"], false);
        assert_eq!(claude_written["model"], "opus");

        let codex_written = std::fs::read_to_string(&codex_config).unwrap();
        assert!(codex_written.contains("# a user comment"));
        assert!(codex_written.contains("model = \"gpt-5.6\""));
        assert!(codex_written.contains("[model_providers.openai]"));
        assert!(codex_written.contains("approval_policy = \"never\""));
        assert!(codex_written.contains("sandbox_mode = \"danger-full-access\""));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn previewing_an_unregistered_harness_answers_for_that_harness_alone() {
        let home = temp_home("preview-unregistered");
        let app = app_with_adapters(
            home.clone(),
            vec![Box::new(FakeAdapter::succeeding("good"))],
        );
        let preview = app.profiles().preview(
            &Preferences::default(),
            &["good".to_owned(), "ghost".to_owned()],
        );
        assert!(preview.harnesses[0].plan.is_ok());
        assert!(preview.harnesses[1].plan.is_err());
        let _ = std::fs::remove_dir_all(home.root());
    }

    /// The failure that started this: a `default` profile had put
    /// `model: "default"` into Claude's settings, which Claude cannot
    /// resolve. The preview has to show it leaving, and after applying,
    /// nothing may be left pending anywhere.
    #[test]
    fn preview_shows_what_apply_changes_and_nothing_is_pending_afterwards() {
        let root = uze_testkit::temp::scratch("profile-preview");
        let home = UzeHome::at(root.join("uze"));
        let claude_settings = root.join("claude").join("settings.json");
        std::fs::create_dir_all(claude_settings.parent().unwrap()).unwrap();
        std::fs::write(
            &claude_settings,
            r#"{"model":"default","permissions":{"defaultMode":"acceptEdits"}}"#,
        )
        .unwrap();

        let registry = uze_integrations::registry::IntegrationRegistry::isolated(&root, &home);
        let (integrations, preference_adapters) = registry.into_parts();
        let app = UzeApplication::new_with_runner_and_preferences(
            home,
            integrations,
            preference_adapters,
            Box::new(SystemProcessRunner),
        );
        app.profiles().list().unwrap();
        let harnesses: Vec<String> = ["claude-code", "codex", "opencode", "antigravity"]
            .map(str::to_owned)
            .to_vec();

        let before = app.profiles().preview(&Preferences::default(), &harnesses);
        let settings_before = std::fs::read_to_string(&claude_settings).unwrap();
        let claude = before.harnesses[0].plan.as_ref().unwrap();
        assert_eq!(claude.config_path, claude_settings);
        let model = claude
            .axes
            .iter()
            .find(|axis| axis.axis == uze_core::preference::PreferenceAxis::Model)
            .unwrap();
        assert_eq!(model.keys[0].current.as_deref(), Some("\"default\""));
        assert_eq!(
            model.keys[0].planned,
            uze_core::preference::PlannedValue::Removed
        );
        assert!(before.pending() > 0);
        assert_eq!(
            std::fs::read_to_string(&claude_settings).unwrap(),
            settings_before,
            "a preview writes nothing"
        );

        app.profiles().apply("default", &harnesses).unwrap();
        let after = app.profiles().preview(&Preferences::default(), &harnesses);
        assert_eq!(after.pending(), 0, "{after:#?}");
        let written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&claude_settings).unwrap()).unwrap();
        assert!(written.get("model").is_none());

        let _ = std::fs::remove_dir_all(&root);
    }
}
