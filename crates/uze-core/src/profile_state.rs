//! Persisted Profiles/Preferences (see `preference`). Mirrors `state.rs`'s
//! existing keyed-registry-in-one-JSON-file shape exactly: load whole,
//! mutate, save whole via `persistence::write_atomic`.

use std::{collections::BTreeMap, fs};

use serde::{Deserialize, Serialize};

use crate::{
    error::{Result, UzeError},
    home::UzeHome,
    preference::Preferences,
};

/// A persisted Profile. Contains only preferences (v1 scope) — no policies,
/// packages, skills, MCP config, or org/team scoping.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileRecord {
    /// Slug, doubles as the display name. Validated by `validate_id`.
    pub id: String,
    pub description: Option<String>,
    pub preferences: Preferences,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct ProfileStore {
    profiles: BTreeMap<String, ProfileRecord>,
    active: Option<String>,
}

pub const DEFAULT_PROFILE_ID: &str = "default";

/// Initializes the baseline profile once, so a new installation always has
/// one usable, active profile before the operator creates custom ones.
pub fn ensure_default(home: &UzeHome) -> Result<()> {
    let mut store = load_store(home)?;
    if !store.profiles.is_empty() {
        return Ok(());
    }
    home.ensure_layout()?;
    store.profiles.insert(
        DEFAULT_PROFILE_ID.to_owned(),
        ProfileRecord {
            id: DEFAULT_PROFILE_ID.to_owned(),
            description: None,
            preferences: Preferences::default(),
        },
    );
    store.active = Some(DEFAULT_PROFILE_ID.to_owned());
    save_store(home, &store)
}

/// All persisted profiles, keyed by id.
pub fn load(home: &UzeHome) -> Result<BTreeMap<String, ProfileRecord>> {
    Ok(load_store(home)?.profiles)
}

pub fn get(home: &UzeHome, id: &str) -> Result<Option<ProfileRecord>> {
    Ok(load_store(home)?.profiles.get(id).cloned())
}

pub fn active(home: &UzeHome) -> Result<Option<String>> {
    Ok(load_store(home)?.active)
}

/// Validates a profile id: lowercase ASCII letters, digits, `-`, or `_`,
/// non-empty. Enforced here (not just at the TUI layer) so no caller can
/// persist a slug that would be unsafe as a filename or ambiguous in a
/// harness-native identifier later.
pub fn validate_id(id: &str) -> Result<()> {
    let valid = !id.is_empty()
        && id
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '-' | '_'));
    if valid {
        Ok(())
    } else {
        Err(UzeError::InvalidProfileId(id.to_owned()))
    }
}

/// Creates a new profile. Fails if `id` is already taken — use
/// `update_preferences`/`update_description` to change an existing profile.
pub fn create(
    home: &UzeHome,
    id: &str,
    description: Option<String>,
    preferences: Preferences,
) -> Result<()> {
    validate_id(id)?;
    home.ensure_layout()?;
    let mut store = load_store(home)?;
    if store.profiles.contains_key(id) {
        return Err(UzeError::ProfileAlreadyExists(id.to_owned()));
    }
    store.profiles.insert(
        id.to_owned(),
        ProfileRecord {
            id: id.to_owned(),
            description,
            preferences,
        },
    );
    save_store(home, &store)
}

pub fn update_preferences(home: &UzeHome, id: &str, preferences: Preferences) -> Result<()> {
    let mut store = load_store(home)?;
    let record = store
        .profiles
        .get_mut(id)
        .ok_or_else(|| UzeError::UnknownProfile(id.to_owned()))?;
    record.preferences = preferences;
    save_store(home, &store)
}

pub fn delete(home: &UzeHome, id: &str) -> Result<()> {
    let mut store = load_store(home)?;
    if !store.profiles.contains_key(id) {
        return Err(UzeError::UnknownProfile(id.to_owned()));
    }
    if store.profiles.len() == 1 {
        return Err(UzeError::CannotDeleteOnlyProfile);
    }
    store.profiles.remove(id);
    if store.active.as_deref() == Some(id) {
        store.active = store.profiles.keys().next().cloned();
    }
    save_store(home, &store)
}

/// Marks `id` as the active profile. Idempotent-safe: setting an unknown id
/// active is rejected rather than silently recorded, since the TUI's
/// "(active)" marker would otherwise point at nothing.
pub fn set_active(home: &UzeHome, id: &str) -> Result<()> {
    let mut store = load_store(home)?;
    if !store.profiles.contains_key(id) {
        return Err(UzeError::UnknownProfile(id.to_owned()));
    }
    store.active = Some(id.to_owned());
    save_store(home, &store)
}

fn load_store(home: &UzeHome) -> Result<ProfileStore> {
    let path = home.profiles_path();
    if !path.exists() {
        return Ok(ProfileStore::default());
    }
    let bytes = fs::read(&path).map_err(|source| UzeError::Read {
        path: path.clone(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| UzeError::Json { path, source })
}

fn save_store(home: &UzeHome, store: &ProfileStore) -> Result<()> {
    let path = home.profiles_path();
    let payload =
        serde_json::to_vec_pretty(store).expect("profile store serialization is infallible");
    crate::persistence::write_atomic(&path, &payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preference::{Autonomy, ModelPreference, SandboxScope};

    fn temp_home(label: &str) -> UzeHome {
        UzeHome::at(uze_testkit::temp::scratch(label))
    }

    #[test]
    fn create_persists_and_loads_back() {
        let home = temp_home("create");
        create(
            &home,
            "dev-autonomous",
            Some("desc".to_owned()),
            Preferences::default(),
        )
        .unwrap();
        let loaded = get(&home, "dev-autonomous").unwrap().unwrap();
        assert_eq!(loaded.description.as_deref(), Some("desc"));
        fs::remove_dir_all(home.root()).unwrap();
    }

    #[test]
    fn creating_a_duplicate_id_is_rejected() {
        let home = temp_home("duplicate");
        create(&home, "default", None, Preferences::default()).unwrap();
        assert!(matches!(
            create(&home, "default", None, Preferences::default()),
            Err(UzeError::ProfileAlreadyExists(_))
        ));
        fs::remove_dir_all(home.root()).unwrap();
    }

    #[test]
    fn invalid_id_is_rejected_before_touching_disk() {
        let home = temp_home("invalid-id");
        assert!(matches!(
            create(&home, "Dev Autonomous!", None, Preferences::default()),
            Err(UzeError::InvalidProfileId(_))
        ));
        assert!(!home.profiles_path().exists());
    }

    #[test]
    fn update_preferences_changes_only_the_targeted_profile() {
        let home = temp_home("update");
        create(&home, "a", None, Preferences::default()).unwrap();
        create(&home, "b", None, Preferences::default()).unwrap();
        update_preferences(
            &home,
            "a",
            Preferences {
                autonomy: Autonomy::Unattended,
                sandbox: SandboxScope::FullAccess,
                model: ModelPreference::Capable,
            },
        )
        .unwrap();
        assert_eq!(
            get(&home, "a").unwrap().unwrap().preferences.autonomy,
            Autonomy::Unattended
        );
        assert_eq!(
            get(&home, "b").unwrap().unwrap().preferences.autonomy,
            Autonomy::Balanced
        );
        fs::remove_dir_all(home.root()).unwrap();
    }

    #[test]
    fn deleting_the_only_profile_is_rejected() {
        let home = temp_home("delete-active");
        create(&home, "default", None, Preferences::default()).unwrap();
        set_active(&home, "default").unwrap();
        assert_eq!(active(&home).unwrap().as_deref(), Some("default"));
        assert!(matches!(
            delete(&home, "default"),
            Err(UzeError::CannotDeleteOnlyProfile)
        ));
        assert_eq!(active(&home).unwrap().as_deref(), Some("default"));
        fs::remove_dir_all(home.root()).unwrap();
    }

    #[test]
    fn default_profile_is_created_active_on_initialization() {
        let home = temp_home("default-profile");
        ensure_default(&home).unwrap();
        assert!(get(&home, DEFAULT_PROFILE_ID).unwrap().is_some());
        assert_eq!(active(&home).unwrap().as_deref(), Some(DEFAULT_PROFILE_ID));
        fs::remove_dir_all(home.root()).unwrap();
    }

    #[test]
    fn deleting_an_active_profile_promotes_a_remaining_profile() {
        let home = temp_home("promote-active");
        create(&home, "default", None, Preferences::default()).unwrap();
        create(&home, "coding", None, Preferences::default()).unwrap();
        set_active(&home, "default").unwrap();
        delete(&home, "default").unwrap();
        assert_eq!(active(&home).unwrap().as_deref(), Some("coding"));
        fs::remove_dir_all(home.root()).unwrap();
    }

    #[test]
    fn setting_an_unknown_profile_active_is_an_error() {
        let home = temp_home("unknown-active");
        assert!(matches!(
            set_active(&home, "ghost"),
            Err(UzeError::UnknownProfile(_))
        ));
        let _ = fs::remove_dir_all(home.root());
    }

    #[test]
    fn writes_are_atomic_no_tmp_files_left_behind() {
        let home = temp_home("atomic");
        create(&home, "default", None, Preferences::default()).unwrap();
        let leftovers = fs::read_dir(home.state_dir()).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")
        });
        assert!(!leftovers);
        fs::remove_dir_all(home.root()).unwrap();
    }
}
