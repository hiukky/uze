//! Minimal, secret-free machine integration state. See ADR-006.

use std::{collections::BTreeMap, fs, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::{
    error::{Result, UzeError},
    home::UzeHome,
};

/// Operational facts about one harness's machine-level UZE integration.
/// Deliberately excludes anything resembling a harness credential.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct IntegrationRecord {
    pub harness: String,
    pub version: Option<String>,
    pub strategy: String,
    pub installed: bool,
    pub managed_artifacts: Vec<PathBuf>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct IntegrationRegistry {
    integrations: BTreeMap<String, IntegrationRecord>,
}

/// All recorded integration state, keyed by harness id.
pub fn load(home: &UzeHome) -> Result<BTreeMap<String, IntegrationRecord>> {
    Ok(load_registry(home)?.integrations)
}

pub fn get(home: &UzeHome, harness: &str) -> Result<Option<IntegrationRecord>> {
    Ok(load_registry(home)?.integrations.get(harness).cloned())
}

/// True only when the harness has a recorded, completed installation. Any
/// read/parse failure is treated as "not installed" so exposure planning can
/// safely fall back to a conformance-probe mechanism rather than error.
pub fn is_installed(home: &UzeHome, harness: &str) -> bool {
    get(home, harness)
        .ok()
        .flatten()
        .map(|record| record.installed)
        .unwrap_or(false)
}

/// Idempotently records or refreshes one harness's integration state. A
/// second call with the same harness id replaces, rather than duplicates,
/// its entry.
pub fn record(home: &UzeHome, entry: IntegrationRecord) -> Result<()> {
    home.ensure_layout()?;
    let mut registry = load_registry(home)?;
    registry.integrations.insert(entry.harness.clone(), entry);
    save_registry(home, &registry)
}

fn load_registry(home: &UzeHome) -> Result<IntegrationRegistry> {
    let path = home.integrations_state_path();
    if !path.exists() {
        return Ok(IntegrationRegistry::default());
    }
    let bytes = fs::read(&path).map_err(|source| UzeError::Read {
        path: path.clone(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| UzeError::Json { path, source })
}

fn save_registry(home: &UzeHome, registry: &IntegrationRegistry) -> Result<()> {
    let path = home.integrations_state_path();
    let payload =
        serde_json::to_vec_pretty(registry).expect("integration state serialization is infallible");
    fs::write(&path, payload).map_err(|source| UzeError::Write { path, source })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_home(label: &str) -> UzeHome {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        UzeHome::at(
            std::env::temp_dir().join(format!("uze-state-{label}-{}-{nonce}", std::process::id())),
        )
    }

    fn record_of(harness: &str, version: &str) -> IntegrationRecord {
        IntegrationRecord {
            harness: harness.to_owned(),
            version: Some(version.to_owned()),
            strategy: "managed-user-scope-skills-dir".to_owned(),
            installed: true,
            managed_artifacts: Vec::new(),
        }
    }

    #[test]
    fn recording_twice_refreshes_instead_of_duplicating() {
        let home = temp_home("idempotent");
        record(&home, record_of("claude-code", "2.1.237")).unwrap();
        record(&home, record_of("claude-code", "2.1.238")).unwrap();

        let all = load(&home).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all["claude-code"].version.as_deref(), Some("2.1.238"));
        fs::remove_dir_all(home.root()).unwrap();
    }

    #[test]
    fn unset_harness_is_not_installed() {
        let home = temp_home("absent");
        assert!(!is_installed(&home, "codex"));
    }

    #[test]
    fn one_harness_state_does_not_affect_another() {
        let home = temp_home("independent");
        record(&home, record_of("claude-code", "2.1.237")).unwrap();
        assert!(is_installed(&home, "claude-code"));
        assert!(!is_installed(&home, "codex"));
        fs::remove_dir_all(home.root()).unwrap();
    }
}
