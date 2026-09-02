//! Profiles/Preferences orchestration: `TUI -> UzeApplication -> Preferences
//! -> PreferencePort -> integration adapters`. Mirrors `harness_health()`'s
//! "iterate `self.integrations`/adapters, no second detection loop" pattern
//! and `setup()`'s per-harness partial-failure isolation.

use serde::Serialize;
use uze_core::{
    Result, UzeError,
    preference::{PreferenceApplyOutcome, Preferences},
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

impl Profiles<'_> {
    pub fn list(&self) -> Result<Vec<ProfileSummary>> {
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.0.home)?;
        profile_state::ensure_default(&self.0.home)?;
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

    pub fn get(&self, id: &str) -> Result<Option<profile_state::ProfileRecord>> {
        profile_state::get(&self.0.home, id)
    }

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

    pub fn update_preferences(&self, id: &str, preferences: Preferences) -> Result<()> {
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.0.home)?;
        profile_state::update_preferences(&self.0.home, id, preferences)
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.0.home)?;
        profile_state::ensure_default(&self.0.home)?;
        profile_state::delete(&self.0.home, id)
    }

    pub fn set_active(&self, id: &str) -> Result<()> {
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.0.home)?;
        profile_state::set_active(&self.0.home, id)
    }

    /// Applies one profile's preferences to exactly the requested harnesses.
    /// A single harness failing (a hard `Err` from its adapter, or no
    /// registered adapter for the id) never aborts the rest — it becomes a
    /// `Failed` result for that harness alone, matching `setup()`'s
    /// per-harness partial-failure isolation.
    pub fn apply(&self, id: &str, harness_ids: &[String]) -> Result<Vec<ProfileApplyResult>> {
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.0.home)?;
        let record = profile_state::get(&self.0.home, id)?
            .ok_or_else(|| UzeError::UnknownProfile(id.to_owned()))?;
        Ok(harness_ids
            .iter()
            .map(|harness_id| {
                let outcome = match self
                    .0
                    .preference_adapters
                    .iter()
                    .find(|adapter| adapter.preference_id() == harness_id.as_str())
                {
                    Some(adapter) => adapter.apply(&record.preferences).unwrap_or_else(|error| {
                        PreferenceApplyOutcome::Failed {
                            reason: error.to_string(),
                        }
                    }),
                    None => PreferenceApplyOutcome::Failed {
                        reason: format!("no preference adapter registered for `{harness_id}`"),
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

#[cfg(test)]
mod tests {
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
}
