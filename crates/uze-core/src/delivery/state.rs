//! Core minimal, secret-free machine integration state. See ADR-006.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{
    error::{Result, UzeError},
    home::UzeHome,
    integration::AttachmentReceipt,
    provisioning::{ProvisionAction, ProvisionStatus, ProvisioningResult},
};

/// Reads one of this module's ledgers, treating an absent file as an empty
/// one. A file that exists but does not read is an error: reading it as
/// empty would invite the next write to overwrite it.
///
/// Everything here is a **record** — ownership and what the operator
/// registered, which nothing else on the machine knows — so it goes
/// through the one rule for reading a record another build wrote. Each
/// ledger names its shape below; a document carrying no shape at all is
/// shape 1, which is what every one of these is today.
fn read_json_or_default<T: uze_document::Shaped + Default>(path: &Path) -> Result<T> {
    Ok(uze_document::read::<T>(path)?.or_default())
}

/// The shapes this module's ledgers are in.
///
/// All shape 1: none has changed since it was first written, and a
/// document with no `schema_version` at all *is* shape 1 — so nothing on
/// any machine has to be touched for these to start obeying the rule. The
/// field appears in the bytes on the release that first needs a rung.
mod shapes {
    use super::{AttachmentLedger, MarketplaceRegistry, ProvisioningRegistry};

    macro_rules! first_shape {
        ($($record:ty => $kind:literal),* $(,)?) => {
            $(impl uze_document::Shaped for $record {
                const SHAPE: u32 = uze_document::FIRST_SHAPE;
                const KIND: &'static str = $kind;
            })*
        };
    }

    first_shape! {
        ProvisioningRegistry => "provisioning",
        MarketplaceRegistry => "marketplaces",
    }

    /// Shape 1 keyed its receipts by a string built from three of their own
    /// fields. Shape 2 drops the key and keeps the values, in the order
    /// the map had them.
    impl uze_document::Shaped for AttachmentLedger {
        const SHAPE: u32 = 2;
        const KIND: &'static str = "attachments";

        fn ladder() -> uze_document::Ladder {
            &[uze_document::Step {
                from: 1,
                to: 2,
                climb: |mut document| {
                    if let Some(receipts) = document.get_mut("receipts")
                        && let Some(keyed) = receipts.as_object()
                    {
                        let values: Vec<serde_json::Value> = keyed.values().cloned().collect();
                        *receipts = serde_json::Value::Array(values);
                    }
                    Ok(document)
                },
            }]
        }
    }
}

/// Reads what UZE last observed, which is never carried across a version.
///
/// Remembered rather than recorded: one this build cannot read is
/// discarded and observed again on the next command, in silence. Refusing
/// it, or setting it aside and saying so, would report an upgrade as a
/// problem when the answer costs one probe.
fn read_cache<T: DeserializeOwned + Default>(path: &Path) -> Result<T> {
    Ok(std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default())
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let payload = serde_json::to_vec_pretty(value).expect("ledger serialization is infallible");
    crate::persistence::write_atomic(path, &payload)
}

/// Who owns what UZE put on a harness's disk.
///
/// A list, because a receipt already says what it is about: its package,
/// its integration and the resource it delivers. It was a map keyed by
/// `"{package}:{integration}:{identity}"` — a string nothing could split
/// back, since a resource identity carries colons of its own
/// (`package:git@ai:skills/commit/SKILL.md` makes five segments), and
/// nothing read: every caller filtered on the fields instead. A key that
/// is a concatenation of the value is the value said twice.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct AttachmentLedger {
    receipts: Vec<AttachmentReceipt>,
}

fn attachments_path(home: &UzeHome) -> PathBuf {
    home.attachments_path()
}

/// Whether two receipts are about the same attachment — which is what a
/// key was for, asked of the fields that answer it.
fn same_attachment(left: &AttachmentReceipt, right: &AttachmentReceipt) -> bool {
    left.package_id == right.package_id
        && left.integration == right.integration
        && left.resource_identity == right.resource_identity
}

pub fn receipts(home: &UzeHome, package_id: Option<&str>) -> Result<Vec<AttachmentReceipt>> {
    let ledger: AttachmentLedger = read_json_or_default(&attachments_path(home))?;
    Ok(ledger
        .receipts
        .into_iter()
        .filter(|receipt| package_id.is_none_or(|id| receipt.package_id == id))
        .collect())
}

/// Records an attachment, replacing whatever was recorded for the same
/// one. Idempotent: attaching twice leaves one receipt, which is what the
/// map's key bought and the fields buy without it.
pub fn record_receipt(home: &UzeHome, receipt: AttachmentReceipt) -> Result<()> {
    home.ensure_layout()?;
    update_receipts(home, |receipts| {
        match receipts
            .iter_mut()
            .find(|existing| same_attachment(existing, &receipt))
        {
            Some(existing) => *existing = receipt,
            None => receipts.push(receipt),
        }
    })
}

pub fn forget_receipt(home: &UzeHome, receipt: &AttachmentReceipt) -> Result<()> {
    update_receipts(home, |receipts| {
        receipts.retain(|existing| !same_attachment(existing, receipt));
    })
}

fn update_receipts(home: &UzeHome, change: impl FnOnce(&mut Vec<AttachmentReceipt>)) -> Result<()> {
    let path = attachments_path(home);
    let mut ledger: AttachmentLedger = read_json_or_default(&path)?;
    change(&mut ledger.receipts);
    write_json(&path, &ledger)
}

/// Operational facts about one harness's machine-level UZE integration.
/// Deliberately excludes anything resembling a harness credential.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct IntegrationRecord {
    pub version: Option<String>,
    pub strategy: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct IntegrationRegistry {
    integrations: BTreeMap<String, IntegrationRecord>,
}

/// All recorded integration state, keyed by harness id.
pub fn load(home: &UzeHome) -> Result<BTreeMap<String, IntegrationRecord>> {
    let registry: IntegrationRegistry = read_cache(&home.harnesses_cache_path())?;
    Ok(registry.integrations)
}

pub fn get(home: &UzeHome, harness: &str) -> Result<Option<IntegrationRecord>> {
    Ok(load(home)?.remove(harness))
}

/// True only when the harness has a recorded installation. Any read/parse
/// failure is treated as "not installed" so exposure planning reports the
/// setup it needs rather than an error.
pub fn is_installed(home: &UzeHome, harness: &str) -> bool {
    get(home, harness).ok().flatten().is_some()
}

/// Idempotently records or refreshes one harness's integration state. A
/// second call with the same harness id replaces, rather than duplicates,
/// its entry.
pub fn record(home: &UzeHome, harness: &str, entry: IntegrationRecord) -> Result<()> {
    home.ensure_layout()?;
    let path = home.harnesses_cache_path();
    let mut registry: IntegrationRegistry = read_cache(&path)?;
    // Every command records each detected harness on its way in; an
    // unchanged record must cost a read, not a synced rewrite of the file.
    if registry.integrations.get(harness) == Some(&entry) {
        return Ok(());
    }
    registry.integrations.insert(harness.to_owned(), entry);
    write_json(&path, &registry)
}

/// Durable evidence of an explicit provisioning attempt. It grants no right
/// to remove a harness executable; it is product history, not ownership.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProvisioningRecord {
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
    let mut registry: ProvisioningRegistry = read_json_or_default(&home.provisioning_state_path())?;
    Ok(registry.harnesses.remove(harness))
}

pub fn record_provisioning(
    home: &UzeHome,
    harness: impl Into<String>,
    result: &ProvisioningResult,
) -> Result<()> {
    home.ensure_layout()?;
    let path = home.provisioning_state_path();
    let mut registry: ProvisioningRegistry = read_json_or_default(&path)?;
    let recorded_at_unix_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    registry.harnesses.insert(
        harness.into(),
        ProvisioningRecord {
            action: result.action,
            status: result.status,
            method: result.method.clone(),
            version: result.detection.version.clone(),
            recorded_at_unix_secs,
        },
    );
    write_json(&path, &registry)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MarketplaceRecord {
    pub source: crate::acquisition::PackageSource,
    /// A checkout on this machine the operator develops this marketplace
    /// in, and which resolution reads instead of `source`.
    ///
    /// A fact about one machine, so it lives here and never in a project's
    /// versioned files — which is the conflation that put an operator's
    /// home directory into a tracked `agents.yaml`. Absent for every
    /// marketplace nobody has linked, which is almost all of them, so it is
    /// optional rather than a shape change: a registry written before this
    /// existed reads back with no link, which is the truth about it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct MarketplaceRegistry {
    marketplaces: BTreeMap<String, MarketplaceRecord>,
}

/// Registers a marketplace under `name`. Idempotent, mirroring
/// `UzeStore::ingest`'s same-origin check: adding a marketplace already
/// registered from the exact same source is a no-op (`Ok(false)`), not an
/// error — a marketplace discovery source has no content of its own to
/// overwrite, so this is safe to repeat. Adding it again from a *different*
/// source is a genuine conflict (`Ok(true)` when newly registered).
pub fn marketplace_add(
    home: &UzeHome,
    name: &str,
    source: crate::acquisition::PackageSource,
) -> Result<bool> {
    home.ensure_layout()?;
    let path = home.marketplaces_path();
    let mut registry: MarketplaceRegistry = read_json_or_default(&path)?;
    if let Some(existing) = registry.marketplaces.get_mut(name) {
        if existing.source == source {
            return Ok(false);
        }
        // The same repository in another spelling is the same marketplace:
        // the entry takes the spelling it is given now, which is the
        // canonical one, and nothing conflicts.
        if existing.source.same_source(&source) {
            existing.source = source;
            write_json(&path, &registry)?;
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
            source,
            // Registering again never silently drops a link: only
            // `marketplace_unlink` removes one.
            link: registry.marketplaces.get(name).and_then(|r| r.link.clone()),
        },
    );
    write_json(&path, &registry)?;
    Ok(true)
}

/// Records that this machine reads `name` from `checkout`.
///
/// Refuses a checkout that is a different repository from the one the
/// marketplace is registered as: a link says "read this marketplace here",
/// and a directory holding some other project is not that marketplace
/// wherever it sits.
pub fn marketplace_link(home: &UzeHome, name: &str, checkout: &std::path::Path) -> Result<()> {
    let path = home.marketplaces_path();
    let mut registry: MarketplaceRegistry = read_json_or_default(&path)?;
    let record = registry
        .marketplaces
        .get_mut(name)
        .ok_or_else(|| crate::UzeError::UnknownMarketplace(name.to_owned()))?;

    let registered = crate::acquisition::marketplace::repository_of(&record.source)?;
    let local = crate::acquisition::marketplace::repository_of(&crate::PackageSource::Local {
        path: checkout.to_path_buf(),
    })?;
    if !crate::acquisition::forge::same_repository(&local.identity, &registered.identity) {
        return Err(crate::UzeError::MarketplaceConflict {
            name: name.to_owned(),
            existing: registered.identity,
            requested: local.identity,
        });
    }

    record.link = Some(local.fetch.into());
    write_json(&path, &registry)
}

/// Stops reading `name` from a checkout. The Store keeps whatever it
/// already holds: unlinking says where to read from next, not that what
/// was read is wrong.
pub fn marketplace_unlink(home: &UzeHome, name: &str) -> Result<bool> {
    let path = home.marketplaces_path();
    let mut registry: MarketplaceRegistry = read_json_or_default(&path)?;
    let record = registry
        .marketplaces
        .get_mut(name)
        .ok_or_else(|| crate::UzeError::UnknownMarketplace(name.to_owned()))?;
    let had = record.link.take().is_some();
    write_json(&path, &registry)?;
    Ok(had)
}

pub fn marketplace_remove(home: &UzeHome, name: &str) -> Result<()> {
    let path = home.marketplaces_path();
    let mut registry: MarketplaceRegistry = read_json_or_default(&path)?;
    if !registry.marketplaces.contains_key(name) {
        return Err(UzeError::UnknownMarketplace(name.to_owned()));
    }
    // Every Store id is marketplace-qualified (ADR-036), so the Store alone
    // answers whether this marketplace still has installed plugins.
    let still_installed = crate::store::UzeStore::new(home.clone())
        .package_ids()?
        .iter()
        .any(|id| id.marketplace() == name);
    if still_installed {
        return Err(UzeError::MarketplaceInUse(name.to_owned()));
    }
    registry.marketplaces.remove(name);
    write_json(&path, &registry)
}

pub fn marketplace_list(home: &UzeHome) -> Result<BTreeMap<String, MarketplaceRecord>> {
    let registry: MarketplaceRegistry = read_json_or_default(&home.marketplaces_path())?;
    Ok(registry.marketplaces)
}

pub fn marketplace_get(home: &UzeHome, name: &str) -> Result<Option<MarketplaceRecord>> {
    Ok(marketplace_list(home)?.remove(name))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::integration::{AttachmentReceipt, ManagedArtifact};
    use std::path::PathBuf;

    fn temp_home(label: &str) -> UzeHome {
        UzeHome::at(uze_testkit::temp::scratch(label))
    }

    fn record_of(version: &str) -> IntegrationRecord {
        IntegrationRecord {
            version: Some(version.to_owned()),
            strategy: "managed-user-scope-skills-dir".to_owned(),
        }
    }

    #[test]
    fn recording_twice_refreshes_instead_of_duplicating() {
        let home = temp_home("idempotent");
        record(&home, "claude-code", record_of("2.1.237")).unwrap();
        record(&home, "claude-code", record_of("2.1.238")).unwrap();

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
        record(&home, "claude-code", record_of("2.1.237")).unwrap();
        assert!(is_installed(&home, "claude-code"));
        assert!(!is_installed(&home, "codex"));
        fs::remove_dir_all(home.root()).unwrap();
    }

    fn receipt(package: &str, integration: &str, entry: &str) -> AttachmentReceipt {
        AttachmentReceipt {
            package_id: package.to_owned(),
            resource_identity: Some(format!("mcp:{entry}")),
            integration: integration.to_owned(),
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

    /// Shape 1 keyed its receipts by a string built from three of their own
    /// fields, and that string could not be split back: a resource identity
    /// carries colons of its own. Shape 2 drops the key and keeps the
    /// values, and a machine coming from the previous release loses nothing
    /// by the change.
    #[test]
    fn a_ledger_keyed_by_a_string_is_carried_across_into_a_list() {
        let home = temp_home("receipt-ledger-shape-1");
        home.ensure_layout().unwrap();
        fs::write(
            home.attachments_path(),
            br#"{"receipts":{
              "git@ai:opencode:package:git@ai:skills/commit/SKILL.md":{
                "package_id":"git@ai","resource_identity":"package:git@ai:skills/commit/SKILL.md",
                "integration":"opencode","artifact":{"INTEGRATION_OWNED":{"kind":"k","selector":"s",
                "origin":"generated","detail":{}}}},
              "git@ai:codex:package":{
                "package_id":"git@ai","resource_identity":null,"integration":"codex",
                "artifact":{"INTEGRATION_OWNED":{"kind":"k","selector":"s","origin":"generated",
                "detail":{}}}}}}"#,
        )
        .unwrap();

        let carried = receipts(&home, Some("git@ai")).unwrap();
        assert_eq!(carried.len(), 2, "both receipts survive: {carried:?}");
        assert!(
            carried
                .iter()
                .any(|receipt| receipt.resource_identity.as_deref()
                    == Some("package:git@ai:skills/commit/SKILL.md")),
            "including the one whose identity made the old key unsplittable"
        );
    }

    #[test]
    fn receipt_ledger_persists_multiple_receipts_and_idempotent_keys() {
        let home = temp_home("receipt-ledger");
        record_receipt(&home, receipt("plugin-a", "codex", "uze-a")).unwrap();
        record_receipt(&home, receipt("plugin-a", "codex", "uze-a")).unwrap();
        record_receipt(&home, receipt("plugin-a", "claude-code", "uze-a")).unwrap();
        record_receipt(&home, receipt("plugin-b", "codex", "uze-b")).unwrap();

        assert_eq!(receipts(&home, None).unwrap().len(), 3);
        let package_a = receipts(&home, Some("plugin-a")).unwrap();
        assert_eq!(package_a.len(), 2);
        assert!(
            package_a
                .iter()
                .all(|receipt| receipt.package_id == "plugin-a")
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
            AttachmentReceipt {
                package_id: "plugin-a".to_owned(),
                resource_identity: None,
                integration: "codex".to_owned(),
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
        assert!(stored[0].resource_identity.is_none());
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
