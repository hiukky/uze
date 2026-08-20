//! Minimal, secret-free machine integration state. See ADR-006.

use std::{collections::BTreeMap, fs};

use serde::{Deserialize, Serialize};

use crate::{
    error::{Result, UzeError},
    home::UzeHome,
};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct AttachmentLedger {
    receipts: BTreeMap<String, crate::integration::AttachmentReceipt>,
}

pub fn receipts(
    home: &UzeHome,
    package_id: Option<&str>,
) -> Result<Vec<(String, crate::integration::AttachmentReceipt)>> {
    let path = home.state_dir().join("attachments.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let bytes = fs::read(&path).map_err(|source| UzeError::Read {
        path: path.clone(),
        source,
    })?;
    let ledger: AttachmentLedger =
        serde_json::from_slice(&bytes).map_err(|source| UzeError::Json { path, source })?;
    Ok(ledger
        .receipts
        .into_iter()
        .filter(|(_, receipt)| package_id.is_none_or(|id| receipt.package_id == id))
        .collect())
}

pub fn record_receipt(
    home: &UzeHome,
    key: String,
    receipt: crate::integration::AttachmentReceipt,
) -> Result<()> {
    home.ensure_layout()?;
    let path = home.state_dir().join("attachments.json");
    let mut entries = receipts(home, None)?
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    entries.insert(key, receipt);
    crate::persistence::write_atomic(
        &path,
        &serde_json::to_vec_pretty(&AttachmentLedger { receipts: entries })
            .expect("receipt ledger serializable"),
    )
}

pub fn forget_receipt(home: &UzeHome, key: &str) -> Result<()> {
    let path = home.state_dir().join("attachments.json");
    let mut entries = receipts(home, None)?
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    entries.remove(key);
    crate::persistence::write_atomic(
        &path,
        &serde_json::to_vec_pretty(&AttachmentLedger { receipts: entries })
            .expect("receipt ledger serializable"),
    )
}

/// Operational facts about one harness's machine-level UZE integration.
/// Deliberately excludes anything resembling a harness credential.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct IntegrationRecord {
    pub harness: String,
    pub version: Option<String>,
    pub strategy: String,
    pub installed: bool,
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
    crate::persistence::write_atomic(&path, &payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integration::{AttachmentReceipt, ManagedArtifact};
    use std::path::PathBuf;

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

    fn receipt(package: &str, integration: &str, entry: &str) -> AttachmentReceipt {
        AttachmentReceipt {
            package_id: package.to_owned(),
            resource_identity: Some(format!("mcp:{entry}")),
            integration: integration.to_owned(),
            strategy: "managed-vendor-config".to_owned(),
            artifact: ManagedArtifact::VendorConfigEntry {
                entry_name: entry.to_owned(),
                transport: "stdio".to_owned(),
                command: PathBuf::from("/bin/example"),
                args: vec!["--serve".to_owned()],
                cwd: None,
                environment: Vec::new(),
                enabled: None,
            },
        }
    }

    #[test]
    fn receipt_ledger_persists_multiple_receipts_and_idempotent_keys() {
        let home = temp_home("receipt-ledger");
        record_receipt(
            &home,
            "plugin-a:codex:mcp".to_owned(),
            receipt("plugin-a", "codex", "uze-a"),
        )
        .unwrap();
        record_receipt(
            &home,
            "plugin-a:codex:mcp".to_owned(),
            receipt("plugin-a", "codex", "uze-a"),
        )
        .unwrap();
        record_receipt(
            &home,
            "plugin-a:claude:mcp".to_owned(),
            receipt("plugin-a", "claude-code", "uze-a"),
        )
        .unwrap();
        record_receipt(
            &home,
            "plugin-b:codex:mcp".to_owned(),
            receipt("plugin-b", "codex", "uze-b"),
        )
        .unwrap();

        assert_eq!(receipts(&home, None).unwrap().len(), 3);
        let package_a = receipts(&home, Some("plugin-a")).unwrap();
        assert_eq!(package_a.len(), 2);
        assert!(
            package_a
                .iter()
                .all(|(_, receipt)| receipt.package_id == "plugin-a")
        );
        fs::remove_dir_all(home.root()).unwrap();
    }

    #[test]
    fn corrupt_receipt_ledger_is_an_error_not_an_empty_ledger() {
        let home = temp_home("corrupt-ledger");
        home.ensure_layout().unwrap();
        fs::write(home.state_dir().join("attachments.json"), "not json").unwrap();
        assert!(receipts(&home, None).is_err());
        fs::remove_dir_all(home.root()).unwrap();
    }

    #[test]
    fn native_package_receipt_does_not_imply_capability_receipts() {
        let home = temp_home("native-package-only");
        record_receipt(
            &home,
            "plugin-a:codex:package".to_owned(),
            AttachmentReceipt {
                package_id: "plugin-a".to_owned(),
                resource_identity: None,
                integration: "codex".to_owned(),
                strategy: "native-plugin-marketplace".to_owned(),
                artifact: ManagedArtifact::MarketplacePlugin {
                    selector: "plugin-a@uze-local".to_owned(),
                    marketplace_root: PathBuf::from("/uze/store"),
                    package_root: PathBuf::from("/uze/store/packages/plugin-a"),
                },
            },
        )
        .unwrap();
        let stored = receipts(&home, Some("plugin-a")).unwrap();
        assert_eq!(stored.len(), 1);
        assert!(stored[0].1.resource_identity.is_none());
        fs::remove_dir_all(home.root()).unwrap();
    }
}
