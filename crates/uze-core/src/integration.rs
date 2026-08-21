//! Harness-agnostic integration and managed-attachment contracts.

use std::{collections::BTreeMap, fs, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::{
    error::Result,
    exposure::{ExposureMechanism, ExposurePlan, McpEnvironmentReference, PackageExposurePlan},
    home::UzeHome,
    project::EffectiveEnvironment,
    router::{HarnessCapabilities, RouteDecision, route},
    runtime::RuntimeSupport,
    state,
    store::StoredPackage,
};

/// Stable receipt for one harness-owned side effect. The ledger persists this
/// intent; every destructive operation still asks the integration to inspect
/// the real harness state first.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AttachmentReceipt {
    pub package_id: String,
    pub resource_identity: Option<String>,
    pub integration: String,
    pub strategy: String,
    pub artifact: ManagedArtifact,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ManagedArtifact {
    SymlinkReference {
        path: PathBuf,
        target: PathBuf,
    },
    VendorConfigEntry {
        entry_name: String,
        transport: String,
        command: PathBuf,
        args: Vec<String>,
        cwd: Option<PathBuf>,
        environment: Vec<McpEnvironmentReference>,
        enabled: Option<bool>,
    },
    /// UZE owns a delimited region inside a shared text file, never the
    /// whole file. See `ExposureMechanism::ManagedTextRegion` and
    /// `crate::text_region`, which every safety rule for this variant lives
    /// in — this receipt shape carries no knowledge of what kind of file
    /// `target_file` is or why.
    ManagedTextRegion {
        target_file: PathBuf,
        region_identity: String,
        expected_content: String,
    },
    /// A delivery whose ownership proof only the owning integration can
    /// interpret. The Core routes it by `receipt.integration`, never reads
    /// `detail`, and refuses to inspect or detach it generically.
    ///
    /// `MARKETPLACE_PLUGIN` is accepted on read so a ledger written before
    /// this variant existed stays interpretable. Its `marketplace_root` and
    /// `package_root` land in `detail` through the flattened capture, and
    /// `kind` falls back to the name that artifact had. Reading a legacy
    /// receipt never rewrites it; only a genuinely new attachment writes,
    /// and a new write always emits this representation.
    #[serde(alias = "MARKETPLACE_PLUGIN")]
    IntegrationOwned {
        #[serde(default = "legacy_artifact_kind")]
        kind: String,
        selector: String,
        #[serde(flatten, default)]
        detail: BTreeMap<String, serde_json::Value>,
    },
}

/// The `kind` a receipt predating [`ManagedArtifact::IntegrationOwned`]
/// implicitly had. Deliberately not a schema version: it is one default for
/// one superseded variant, not a migration framework.
fn legacy_artifact_kind() -> String {
    "marketplace-plugin".to_owned()
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AttachmentState {
    Matched,
    Missing,
    Drifted,
    Conflict,
    Blocked,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AttachmentInspection {
    pub state: AttachmentState,
    pub reason: String,
}

/// Read-only detection of a harness binary. No side effects.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct HarnessDetection {
    pub present: bool,
    pub version: Option<String>,
}

/// Diagnosable, machine-level integration status for `uze doctor`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IntegrationStatus {
    NotConfigured,
    InstalledUnverified,
    InstalledVerified,
}

/// Whether an integration's derived view of the installed package set is
/// currently in place. `NotApplicable` is the honest default: most
/// integrations publish no such view at all.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE", tag = "state", content = "reason")]
pub enum PublicationStatus {
    NotApplicable,
    Published,
    /// The package set is installed but the harness cannot see it. Actionable
    /// by re-running the publication, never by reinstalling the package.
    Unpublished(String),
}

pub trait IntegrationPort {
    fn id(&self) -> &'static str;
    fn capabilities(&self) -> HarnessCapabilities;

    fn runtime_support(&self) -> RuntimeSupport {
        RuntimeSupport::default()
    }

    /// The integration, not the resource representation, selects how the
    /// harness receives a capability from a composed UZE environment.
    fn exposure_plan(&self, resource: &crate::project::Resource) -> ExposurePlan;

    /// Ordered, harness-appropriate candidates for `resource`'s physical
    /// exposure name — most preferred first. This method only *proposes*:
    /// it reads no filesystem, reads no ledger, decides no ownership, and
    /// mutates nothing. Resolving a candidate against what's already
    /// claimed by other UZE-managed resources (via the ledger) and against
    /// what already exists on disk (via `attach`'s own structural checks)
    /// happens entirely outside this method — see
    /// `UzeApplication`'s naming resolution and `ExposureMechanism::attach`.
    ///
    /// The default suits every integration that has no naming policy of
    /// its own: one candidate, fully package-qualified, with **no**
    /// collision-avoidance prefix — the prefix was never part of ownership
    /// (see `AttachmentReceipt`'s doc comment) and this default drops it.
    /// An integration overrides this only when its own harness's UX
    /// genuinely depends on the physical name (Claude Code's decomposed
    /// Skill delivery is the one case today).
    fn exposure_name_candidates(&self, resource: &crate::project::Resource) -> Vec<String> {
        default_exposure_name_candidates(resource)
    }

    /// Optional preferred delivery of an external package as a whole. The
    /// returned plan owns only the listed resources; remaining resources are
    /// still routed capability-by-capability.
    fn package_exposure_plan(
        &self,
        _package: &StoredPackage,
        _resources: &[&crate::project::Resource],
    ) -> Option<PackageExposurePlan> {
        None
    }

    /// Detects whether the harness binary is present and, if cheaply
    /// obtainable, its version. Read-only; performs no filesystem writes.
    fn detect(&self) -> HarnessDetection {
        HarnessDetection::default()
    }

    /// Explicitly provisions or updates the vendor executable through a
    /// route owned by this integration. The default is conservative: an
    /// integration that has not documented a route remains blocked rather
    /// than inheriting another harness's installer.
    fn provision(
        &self,
        _runner: &dyn crate::provisioning::ProcessRunner,
    ) -> Result<crate::provisioning::ProvisioningResult> {
        let detection = self.detect();
        if detection.present {
            return Ok(crate::provisioning::ProvisioningResult::verified(
                crate::provisioning::ProvisionAction::None,
                "existing-executable",
                detection,
            ));
        }
        Ok(crate::provisioning::ProvisioningResult::blocked(
            "this harness has no supported official provisioning route",
        ))
    }

    /// Idempotently ensures this integration's machine-level prerequisites
    /// exist (e.g. its user-scope discovery directory) and records setup
    /// state. Safe to call more than once; a second call refreshes recorded
    /// facts rather than duplicating state or artifacts.
    fn install(&self, home: &UzeHome) -> Result<()> {
        let _ = home;
        Ok(())
    }

    /// Current installed/managed status, for `uze doctor`. The default
    /// reads whatever `install` recorded through the shared `state` module.
    fn status(&self, home: &UzeHome) -> IntegrationStatus {
        match state::get(home, self.id()).ok().flatten() {
            Some(record) if record.installed => IntegrationStatus::InstalledUnverified,
            _ => IntegrationStatus::NotConfigured,
        }
    }

    /// Idempotently creates or refreshes this harness's managed attachment
    /// for one resource. `None` when the currently selected exposure
    /// mechanism does not support persistent attachment (e.g. setup has not
    /// completed yet and the integration is still on a conformance-probe
    /// fallback).
    fn attach(&self, resource: &crate::project::Resource) -> Result<Option<PathBuf>> {
        let plan = self.exposure_plan(resource);
        match &plan.mechanism {
            ExposureMechanism::ManagedUserScopeReference { .. } => {
                Ok(Some(plan.mechanism.attach()?))
            }
            ExposureMechanism::ManagedTextRegion { .. } => {
                Ok(Some(plan.mechanism.attach_text_region()?))
            }
            _ => Ok(None),
        }
    }

    /// Performs a package-level native delivery and returns its own ownership
    /// receipt. Deliberately one method: a native delivery may cover several
    /// resources and must not manufacture one receipt per capability, and
    /// only the integration can describe the artifact it just created. The
    /// Core supplies no default because it has no vocabulary for one.
    fn attach_package(
        &self,
        _package: &StoredPackage,
        _plan: &PackageExposurePlan,
    ) -> Result<Option<AttachmentReceipt>> {
        Ok(None)
    }

    /// Additional names `uze setup <harness>` accepts for this integration.
    /// Kept beside the integration so the Application never holds a manual
    /// catalogue of vendors.
    fn aliases(&self) -> &'static [&'static str] {
        &[]
    }

    /// Rebuilds any derived, harness-owned view of the installed package set
    /// that this integration maintains — for example a catalogue a harness
    /// reads to discover locally installable packages.
    ///
    /// **This is not part of package ownership.** The Store stays the sole
    /// authority for which packages exist; whatever this writes must be
    /// reconstructible from `packages` alone, must hold nothing that exists
    /// only there, and must be safe to delete and regenerate at any moment.
    /// No receipt is produced and nothing here is reconciled: a derived view
    /// cannot drift, it can only be stale, and staleness is repaired by
    /// calling this again.
    ///
    /// A failure here never invalidates an installation. The package stays
    /// installed and the integration reports itself unpublished through
    /// [`IntegrationPort::publication`].
    fn republish_packages(&self, packages: &[StoredPackage]) -> Result<()> {
        let _ = packages;
        Ok(())
    }

    /// Observed health of the derived view [`republish_packages`] maintains.
    ///
    /// Read at diagnosis time rather than recorded at write time, precisely
    /// because the view is derived: remembering its state would create the
    /// second source of truth the derivation exists to avoid.
    fn publication(&self, packages: &[StoredPackage]) -> PublicationStatus {
        let _ = packages;
        PublicationStatus::NotApplicable
    }

    /// Returns a typed ownership receipt after a successful resource attach.
    fn attach_receipt(
        &self,
        resource: &crate::project::Resource,
    ) -> Result<Option<AttachmentReceipt>> {
        let Some(location) = self.attach(resource)? else {
            return Ok(None);
        };
        let package_id = match &resource.origin {
            crate::project::ResourceOrigin::Package { id, .. } => id.as_str().to_owned(),
            _ => return Ok(None),
        };
        let plan = self.exposure_plan(resource);
        let strategy = format!("{:?}", plan.mechanism);
        let artifact = match plan.mechanism {
            ExposureMechanism::ManagedUserScopeReference { source, .. } => {
                ManagedArtifact::SymlinkReference {
                    path: location,
                    target: source,
                }
            }
            ExposureMechanism::ManagedVendorConfig {
                entry_name,
                transport,
                command,
                args,
                cwd,
                environment,
                enabled,
            } => ManagedArtifact::VendorConfigEntry {
                entry_name,
                transport,
                command,
                args,
                cwd,
                environment,
                enabled,
            },
            ExposureMechanism::ManagedTextRegion {
                target_file,
                region_identity,
                expected_content,
            } => ManagedArtifact::ManagedTextRegion {
                target_file,
                region_identity,
                expected_content,
            },
            _ => return Ok(None),
        };
        Ok(Some(AttachmentReceipt {
            package_id,
            resource_identity: Some(resource.identity()),
            integration: self.id().to_owned(),
            strategy,
            artifact,
        }))
    }

    fn inspect_receipt(&self, receipt: &AttachmentReceipt) -> AttachmentInspection {
        inspect_standard_receipt(receipt)
    }

    fn detach_receipt(&self, receipt: &AttachmentReceipt) -> Result<AttachmentInspection> {
        detach_standard_receipt(receipt)
    }
}

/// The naming default every integration inherits unless it overrides
/// `exposure_name_candidates`: one candidate, fully package-qualified,
/// with no collision-avoidance prefix. A free function (not inlined into
/// the trait default) so an integration that overrides the method for one
/// capability kind (e.g. Claude's Skills) can still fall through to this
/// exact same computation for another (e.g. Claude's MCP, which the
/// milestone brief explicitly keeps on the default policy — "não misture
/// Skill e MCP só porque ambos são Resources").
pub fn default_exposure_name_candidates(resource: &crate::project::Resource) -> Vec<String> {
    let crate::project::ResourceOrigin::Package { id, .. } = &resource.origin else {
        return Vec::new();
    };
    let Some(logical) = resource.logical_capability_name() else {
        return Vec::new();
    };
    vec![format!("{}-{}", id.as_str(), logical)]
}

/// Extracts the physical exposure name a receipt's artifact already claims,
/// generically — no artifact variant here knows or cares what kind of
/// capability it represents. `None` for an artifact shape with no single
/// physical name of its own (a text region spans a *portion* of a shared
/// file, not a dedicated entry; an integration-owned artifact's naming is
/// opaque to the Core by design).
pub fn managed_artifact_exposure_name(artifact: &ManagedArtifact) -> Option<String> {
    match artifact {
        ManagedArtifact::SymlinkReference { path, .. } => {
            path.file_name()?.to_str().map(str::to_owned)
        }
        ManagedArtifact::VendorConfigEntry { entry_name, .. } => Some(entry_name.clone()),
        ManagedArtifact::ManagedTextRegion { .. } | ManagedArtifact::IntegrationOwned { .. } => {
            None
        }
    }
}

/// Shared inspection for artifacts whose ownership proof does not depend on a
/// harness schema. Vendor integrations call this explicitly rather than
/// redispatching through `IntegrationPort` from an override.
pub fn inspect_standard_receipt(receipt: &AttachmentReceipt) -> AttachmentInspection {
    match &receipt.artifact {
        ManagedArtifact::SymlinkReference { path, target } => match fs::read_link(path) {
            Ok(actual) if &actual == target => AttachmentInspection {
                state: AttachmentState::Matched,
                reason: "managed symlink target matches receipt".to_owned(),
            },
            Ok(_) => AttachmentInspection {
                state: AttachmentState::Drifted,
                reason: "symlink target differs from receipt".to_owned(),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => AttachmentInspection {
                state: AttachmentState::Missing,
                reason: "managed symlink is missing".to_owned(),
            },
            Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => {
                AttachmentInspection {
                    state: AttachmentState::Conflict,
                    reason: "managed path is occupied by a non-symlink".to_owned(),
                }
            }
            Err(error) => AttachmentInspection {
                state: AttachmentState::Blocked,
                reason: error.to_string(),
            },
        },
        ManagedArtifact::ManagedTextRegion {
            target_file,
            region_identity,
            expected_content,
        } => crate::text_region::inspect(target_file, region_identity, expected_content),
        _ => AttachmentInspection {
            state: AttachmentState::Blocked,
            reason: "integration must inspect this vendor artifact".to_owned(),
        },
    }
}

/// Removes only a currently matched standard artifact. Callers receive any
/// non-matched inspection unchanged, so drift never turns into a destructive
/// operation.
pub fn detach_standard_receipt(receipt: &AttachmentReceipt) -> Result<AttachmentInspection> {
    if let ManagedArtifact::ManagedTextRegion {
        target_file,
        region_identity,
        expected_content,
    } = &receipt.artifact
    {
        // `text_region::detach` already re-inspects immediately before its
        // own destructive write, per the same ADR-009 discipline the rest of
        // this function applies below for a symlink.
        return crate::text_region::detach(target_file, region_identity, expected_content);
    }
    let inspection = inspect_standard_receipt(receipt);
    if inspection.state != AttachmentState::Matched {
        return Ok(inspection);
    }
    // Re-read immediately before unlinking. This protects the normal
    // non-concurrent case where a user changes the reference after a prior
    // doctor/reconcile pass but before detach begins.
    let fresh = inspect_standard_receipt(receipt);
    if fresh.state != AttachmentState::Matched {
        return Ok(fresh);
    }
    if let ManagedArtifact::SymlinkReference { path, .. } = &receipt.artifact {
        fs::remove_file(path).map_err(|source| crate::UzeError::Write {
            path: path.clone(),
            source,
        })?;
    }
    Ok(AttachmentInspection {
        state: AttachmentState::Missing,
        reason: "managed artifact detached".to_owned(),
    })
}

/// A human-readable artifact locator for the existing CLI. The typed receipt
/// remains the source of truth; this intentionally loses no ownership data
/// because it is display-only.
pub fn receipt_location(receipt: &AttachmentReceipt) -> PathBuf {
    match &receipt.artifact {
        ManagedArtifact::SymlinkReference { path, .. } => path.clone(),
        ManagedArtifact::VendorConfigEntry { entry_name, .. } => {
            PathBuf::from(format!("mcp:{entry_name}"))
        }
        ManagedArtifact::ManagedTextRegion {
            target_file,
            region_identity,
            ..
        } => PathBuf::from(format!("{}#{region_identity}", target_file.display())),
        ManagedArtifact::IntegrationOwned { kind, selector, .. } => {
            PathBuf::from(format!("{kind}:{selector}"))
        }
    }
}

#[cfg(test)]
mod artifact_compatibility_tests {
    use super::*;

    /// A ledger written before `IntegrationOwned` existed must stay readable,
    /// because an unreadable receipt blocks removal — safe, but it strands the
    /// user with external state UZE can no longer identify.
    #[test]
    fn legacy_marketplace_receipt_still_deserializes() {
        let legacy = r#"{
            "package_id": "plugin-a",
            "resource_identity": null,
            "integration": "codex",
            "strategy": "native-plugin-marketplace",
            "artifact": {
                "MARKETPLACE_PLUGIN": {
                    "selector": "plugin-a@uze-local",
                    "marketplace_root": "/uze/store",
                    "package_root": "/uze/store/packages/plugin-a"
                }
            }
        }"#;
        let receipt: AttachmentReceipt =
            serde_json::from_str(legacy).expect("legacy receipt reads");
        let ManagedArtifact::IntegrationOwned {
            kind,
            selector,
            detail,
        } = &receipt.artifact
        else {
            panic!("legacy artifact did not map onto the integration-owned variant");
        };
        assert_eq!(kind, "marketplace-plugin");
        assert_eq!(selector, "plugin-a@uze-local");
        // The superseded fields survive as opaque detail, so the owning
        // integration can still prove ownership and detach safely.
        assert_eq!(detail["marketplace_root"], "/uze/store");
        assert_eq!(detail["package_root"], "/uze/store/packages/plugin-a");
    }

    /// Reading a legacy receipt must not silently rewrite the ledger, but any
    /// genuinely new write emits only the current representation.
    #[test]
    fn a_new_write_uses_only_the_current_representation() {
        let receipt = AttachmentReceipt {
            package_id: "plugin-a".to_owned(),
            resource_identity: None,
            integration: "codex".to_owned(),
            strategy: "native-plugin-marketplace".to_owned(),
            artifact: ManagedArtifact::IntegrationOwned {
                kind: "marketplace-plugin".to_owned(),
                selector: "plugin-a@uze-local".to_owned(),
                detail: BTreeMap::new(),
            },
        };
        let encoded = serde_json::to_string(&receipt).unwrap();
        assert!(encoded.contains("INTEGRATION_OWNED"));
        assert!(!encoded.contains("MARKETPLACE_PLUGIN"));
    }

    /// The Core routes an integration-owned artifact by `receipt.integration`
    /// and must never guess at its ownership itself.
    #[test]
    fn an_integration_owned_receipt_is_never_inspected_generically() {
        let receipt = AttachmentReceipt {
            package_id: "plugin-a".to_owned(),
            resource_identity: None,
            integration: "codex".to_owned(),
            strategy: "native-plugin-marketplace".to_owned(),
            artifact: ManagedArtifact::IntegrationOwned {
                kind: "marketplace-plugin".to_owned(),
                selector: "plugin-a@uze-local".to_owned(),
                detail: BTreeMap::new(),
            },
        };
        assert_eq!(
            inspect_standard_receipt(&receipt).state,
            AttachmentState::Blocked
        );
        // And a blocked inspection can never become a destructive operation.
        assert_eq!(
            detach_standard_receipt(&receipt).unwrap().state,
            AttachmentState::Blocked
        );
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;

    fn receipt(path: PathBuf, target: PathBuf) -> AttachmentReceipt {
        AttachmentReceipt {
            package_id: "plugin".to_owned(),
            resource_identity: Some("skill:example".to_owned()),
            integration: "test".to_owned(),
            strategy: "managed-user-scope-reference".to_owned(),
            artifact: ManagedArtifact::SymlinkReference { path, target },
        }
    }

    fn temp(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "uze-receipt-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[cfg(unix)]
    #[test]
    fn symlink_receipt_is_safe_only_when_ownership_still_matches() {
        use std::os::unix::fs::symlink;

        let root = temp("symlink");
        fs::create_dir_all(&root).unwrap();
        let expected = root.join("expected");
        let other = root.join("other");
        fs::create_dir_all(&expected).unwrap();
        fs::create_dir_all(&other).unwrap();
        let path = root.join("managed");
        let receipt = receipt(path.clone(), expected.clone());

        assert_eq!(
            inspect_standard_receipt(&receipt).state,
            AttachmentState::Missing
        );
        symlink(&expected, &path).unwrap();
        assert_eq!(
            inspect_standard_receipt(&receipt).state,
            AttachmentState::Matched
        );
        assert_eq!(
            detach_standard_receipt(&receipt).unwrap().state,
            AttachmentState::Missing
        );
        assert!(!path.exists());

        symlink(&other, &path).unwrap();
        assert_eq!(
            inspect_standard_receipt(&receipt).state,
            AttachmentState::Drifted
        );
        assert_eq!(
            detach_standard_receipt(&receipt).unwrap().state,
            AttachmentState::Drifted
        );
        assert_eq!(fs::read_link(&path).unwrap(), other);

        fs::remove_file(&path).unwrap();
        fs::write(&path, "foreign").unwrap();
        assert_eq!(
            inspect_standard_receipt(&receipt).state,
            AttachmentState::Conflict
        );
        assert_eq!(
            detach_standard_receipt(&receipt).unwrap().state,
            AttachmentState::Conflict
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "foreign");
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_symlink_state_is_blocked() {
        use std::os::unix::fs::PermissionsExt;

        let root = temp("unreadable");
        let locked = root.join("locked");
        fs::create_dir_all(&locked).unwrap();
        let receipt = receipt(locked.join("managed"), root.join("expected"));
        let mut permissions = fs::metadata(&locked).unwrap().permissions();
        permissions.set_mode(0o000);
        fs::set_permissions(&locked, permissions).unwrap();
        assert_eq!(
            inspect_standard_receipt(&receipt).state,
            AttachmentState::Blocked
        );
        let mut permissions = fs::metadata(&locked).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&locked, permissions).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}

#[derive(Clone, Debug)]
pub struct IntegrationAssessment {
    pub integration_id: String,
    pub capability_path: String,
    pub decision: RouteDecision,
    pub exposure_plan: ExposurePlan,
}

pub fn assess_environment(
    environment: &EffectiveEnvironment,
    integration: &dyn IntegrationPort,
) -> Vec<IntegrationAssessment> {
    let capabilities = integration.capabilities();
    environment
        .resources
        .iter()
        .map(|resource| {
            let exposure_plan = integration.exposure_plan(resource);
            let mut decision = route(&resource.capability, &capabilities);
            decision.route = exposure_plan.route;
            decision.verification = exposure_plan.verification.clone();
            decision.rationale = exposure_plan.evidence.clone();
            decision.evidence = exposure_plan.evidence.clone();
            IntegrationAssessment {
                integration_id: integration.id().to_owned(),
                capability_path: resource.display_path(&environment.root),
                decision,
                exposure_plan,
            }
        })
        .collect()
}
