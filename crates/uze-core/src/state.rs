//! Core minimal, secret-free machine integration state. See ADR-006.

use std::{collections::BTreeMap, fs};

use serde::{Deserialize, Serialize};

use crate::{
    error::{Result, UzeError},
    home::UzeHome,
    provisioning::{ProvisionAction, ProvisionStatus, ProvisioningResult},
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
        .is_some_and(|record| record.installed)
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

/// Durable evidence of an explicit provisioning attempt. It grants no right
/// to remove a harness executable; it is product history, not ownership.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProvisioningRecord {
    pub harness: String,
    pub action: ProvisionAction,
    pub status: ProvisionStatus,
    pub method: String,
    pub version: Option<String>,
    pub recorded_at_unix_secs: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct ProvisioningRegistry {
    harnesses: BTreeMap<String, ProvisioningRecord>,
}

pub fn provisioning(home: &UzeHome, harness: &str) -> Result<Option<ProvisioningRecord>> {
    Ok(load_provisioning_registry(home)?
        .harnesses
        .get(harness)
        .cloned())
}

pub fn record_provisioning(
    home: &UzeHome,
    harness: impl Into<String>,
    result: &ProvisioningResult,
) -> Result<()> {
    home.ensure_layout()?;
    let harness = harness.into();
    let mut registry = load_provisioning_registry(home)?;
    let recorded_at_unix_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    registry.harnesses.insert(
        harness.clone(),
        ProvisioningRecord {
            harness,
            action: result.action,
            status: result.status,
            method: result.method.clone(),
            version: result.detection.version.clone(),
            recorded_at_unix_secs,
        },
    );
    let path = home.provisioning_state_path();
    let payload = serde_json::to_vec_pretty(&registry)
        .expect("provisioning state serialization is infallible");
    crate::persistence::write_atomic(&path, &payload)
}

fn load_provisioning_registry(home: &UzeHome) -> Result<ProvisioningRegistry> {
    let path = home.provisioning_state_path();
    if !path.exists() {
        return Ok(ProvisioningRegistry::default());
    }
    let bytes = fs::read(&path).map_err(|source| UzeError::Read {
        path: path.clone(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| UzeError::Json { path, source })
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MarketplaceRecord {
    pub name: String,
    pub source: crate::acquisition::PackageSource,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct MarketplaceRegistry {
    marketplaces: BTreeMap<String, MarketplaceRecord>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct PluginMarketplaceRegistry {
    plugins: BTreeMap<String, String>,
}

/// Registers a marketplace under `name`. Idempotent, mirroring
/// `UzeStore::install_materialized`'s same-origin check: adding a
/// marketplace already registered from the exact same source is a no-op
/// (`Ok(false)`), not an error — a marketplace discovery source has no
/// content of its own to overwrite, so this is safe to repeat. Adding it
/// again from a *different* source is a genuine conflict (`Ok(true)` when
/// newly registered).
pub fn marketplace_add(
    home: &UzeHome,
    name: &str,
    source: crate::acquisition::PackageSource,
) -> Result<bool> {
    home.ensure_layout()?;
    let mut registry = load_marketplace_registry(home)?;
    if let Some(existing) = registry.marketplaces.get(name) {
        if existing.source == source {
            return Ok(false);
        }
        return Err(UzeError::MarketplaceConflict {
            name: name.to_owned(),
            existing: format!("{:?}", existing.source),
            requested: format!("{source:?}"),
        });
    }
    registry.marketplaces.insert(
        name.to_owned(),
        MarketplaceRecord {
            name: name.to_owned(),
            source,
        },
    );
    save_marketplace_registry(home, &registry)?;
    Ok(true)
}

pub fn marketplace_remove(home: &UzeHome, name: &str) -> Result<()> {
    let mut registry = load_marketplace_registry(home)?;
    if !registry.marketplaces.contains_key(name) {
        return Err(UzeError::UnknownPackage(format!(
            "marketplace `{name}` not found"
        )));
    }
    let plugin_map = load_plugin_marketplace_registry(home)?;
    if plugin_map.plugins.values().any(|m| m == name) {
        return Err(UzeError::ExposureUnavailable(format!(
            "marketplace `{name}` still has installed plugins; remove them first"
        )));
    }
    registry.marketplaces.remove(name);
    save_marketplace_registry(home, &registry)
}

pub fn marketplace_list(home: &UzeHome) -> Result<BTreeMap<String, MarketplaceRecord>> {
    Ok(load_marketplace_registry(home)?.marketplaces)
}

pub fn marketplace_get(home: &UzeHome, name: &str) -> Result<Option<MarketplaceRecord>> {
    Ok(load_marketplace_registry(home)?
        .marketplaces
        .get(name)
        .cloned())
}

pub fn plugin_marketplace_record(home: &UzeHome, plugin_id: &str, marketplace: &str) -> Result<()> {
    home.ensure_layout()?;
    let mut registry = load_plugin_marketplace_registry(home)?;
    registry
        .plugins
        .insert(plugin_id.to_owned(), marketplace.to_owned());
    save_plugin_marketplace_registry(home, &registry)
}

pub fn plugin_marketplace_get(home: &UzeHome, plugin_id: &str) -> Result<Option<String>> {
    Ok(load_plugin_marketplace_registry(home)?
        .plugins
        .get(plugin_id)
        .cloned())
}

pub fn plugin_marketplace_remove(home: &UzeHome, plugin_id: &str) -> Result<()> {
    let mut registry = load_plugin_marketplace_registry(home)?;
    registry.plugins.remove(plugin_id);
    save_plugin_marketplace_registry(home, &registry)
}

fn load_marketplace_registry(home: &UzeHome) -> Result<MarketplaceRegistry> {
    let path = home.marketplaces_path();
    if !path.exists() {
        return Ok(MarketplaceRegistry::default());
    }
    let bytes = fs::read(&path).map_err(|source| UzeError::Read {
        path: path.clone(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| UzeError::Json { path, source })
}

fn save_marketplace_registry(home: &UzeHome, registry: &MarketplaceRegistry) -> Result<()> {
    let path = home.marketplaces_path();
    let payload = serde_json::to_vec_pretty(registry).expect("marketplace registry serializable");
    crate::persistence::write_atomic(&path, &payload)
}

fn load_plugin_marketplace_registry(home: &UzeHome) -> Result<PluginMarketplaceRegistry> {
    let path = home.plugin_marketplaces_path();
    if !path.exists() {
        return Ok(PluginMarketplaceRegistry::default());
    }
    let bytes = fs::read(&path).map_err(|source| UzeError::Read {
        path: path.clone(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| UzeError::Json { path, source })
}

fn save_plugin_marketplace_registry(
    home: &UzeHome,
    registry: &PluginMarketplaceRegistry,
) -> Result<()> {
    let path = home.plugin_marketplaces_path();
    let payload =
        serde_json::to_vec_pretty(registry).expect("plugin marketplace registry serializable");
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
                artifact: ManagedArtifact::IntegrationOwned {
                    kind: "marketplace-plugin".to_owned(),
                    selector: "plugin-a@uze-local".to_owned(),
                    detail: [(
                        "marketplace_root".to_owned(),
                        serde_json::json!("/uze/store"),
                    )]
                    .into_iter()
                    .collect(),
                },
            },
        )
        .unwrap();
        let stored = receipts(&home, Some("plugin-a")).unwrap();
        assert_eq!(stored.len(), 1);
        assert!(stored[0].1.resource_identity.is_none());
        fs::remove_dir_all(home.root()).unwrap();
    }

    #[test]
    fn provisioning_state_is_secret_free_and_separate_from_attachment_ownership() {
        let home = temp_home("provisioning");
        let result = ProvisioningResult::verified(
            ProvisionAction::Install,
            "official-install-script",
            crate::integration::HarnessDetection {
                present: true,
                version: Some("1.2.3".to_owned()),
            },
        );
        record_provisioning(&home, "opencode", &result).unwrap();
        let record = provisioning(&home, "opencode").unwrap().unwrap();
        assert_eq!(record.action, ProvisionAction::Install);
        assert_eq!(record.version.as_deref(), Some("1.2.3"));
        assert!(!home.state_dir().join("attachments.json").exists());
        let raw = fs::read_to_string(home.provisioning_state_path()).unwrap();
        assert!(!raw.contains("command"));
        fs::remove_dir_all(home.root()).unwrap();
    }
}
