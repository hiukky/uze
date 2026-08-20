use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::{
    error::Result,
    exposure::{ExposureMechanism, ExposurePlan, PackageExposureMechanism, PackageExposurePlan},
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
        command: PathBuf,
        args: Vec<String>,
    },
    MarketplacePlugin {
        selector: String,
        marketplace_root: PathBuf,
        package_root: PathBuf,
    },
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
#[derive(Clone, Debug, Default, Serialize)]
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

pub trait IntegrationPort {
    fn id(&self) -> &'static str;
    fn capabilities(&self) -> HarnessCapabilities;

    fn runtime_support(&self) -> RuntimeSupport {
        RuntimeSupport::default()
    }

    /// The integration, not the resource representation, selects how the
    /// harness receives a capability from a composed UZE environment.
    fn exposure_plan(&self, resource: &crate::project::Resource) -> ExposurePlan;

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
            _ => Ok(None),
        }
    }

    fn attach_package(
        &self,
        _package: &StoredPackage,
        _plan: &PackageExposurePlan,
    ) -> Result<Option<PathBuf>> {
        Ok(None)
    }

    /// Returns a receipt for a package-level native delivery. This is
    /// intentionally separate from `attach_receipt`: a native package may
    /// provide several resources and must not manufacture one receipt per
    /// capability.
    fn attach_package_receipt(
        &self,
        package: &StoredPackage,
        plan: &PackageExposurePlan,
    ) -> Result<Option<AttachmentReceipt>> {
        let Some(_location) = self.attach_package(package, plan)? else {
            return Ok(None);
        };
        let PackageExposureMechanism::NativePluginMarketplace {
            marketplace_root,
            marketplace_name,
            plugin_name,
        } = &plan.mechanism
        else {
            return Ok(None);
        };
        Ok(Some(AttachmentReceipt {
            package_id: package.id.as_str().to_owned(),
            resource_identity: None,
            integration: self.id().to_owned(),
            strategy: "native-plugin-marketplace".to_owned(),
            artifact: ManagedArtifact::MarketplacePlugin {
                selector: format!("{plugin_name}@{marketplace_name}"),
                marketplace_root: marketplace_root.clone(),
                package_root: package.root.clone(),
            },
        }))
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
                command,
                args,
            } => ManagedArtifact::VendorConfigEntry {
                entry_name,
                command,
                args,
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
        ManagedArtifact::MarketplacePlugin { selector, .. } => {
            PathBuf::from(format!("plugin:{selector}"))
        }
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
