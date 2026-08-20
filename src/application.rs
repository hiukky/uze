//! Product-facing application boundary.
//!
//! CLI, TUI, and future presentation layers call this facade rather than
//! reaching into Store, integrations, vendor files, or lifecycle mechanics.

use std::{collections::BTreeSet, path::PathBuf};

use serde::Serialize;

use crate::{
    Result, UzeEngine, UzeError, UzeHome, UzeStore,
    capability::CapabilityKind,
    exposure::{ExposurePlan, PackageExposurePlan},
    integration::{
        AttachmentState, HarnessDetection, IntegrationPort, IntegrationStatus, receipt_location,
    },
    integrations::{
        claude::ClaudeIntegration, codex::CodexIntegration, opencode::OpenCodeIntegration,
    },
    reconciliation::{PackageRemovalPlan, ReconciliationReport, plan_remove, reconcile_package},
    state,
    store::StoredPackage,
};

pub struct UzeApplication {
    home: UzeHome,
    store: UzeStore,
    integrations: Vec<Box<dyn IntegrationPort>>,
}

impl UzeApplication {
    /// Production composition root. Concrete harness details remain inside
    /// this layer, not in CLI or TUI code.
    pub fn from_env(home: UzeHome) -> Result<Self> {
        Ok(Self::new(
            home.clone(),
            vec![
                Box::new(ClaudeIntegration::from_env(home.clone())?),
                Box::new(CodexIntegration::from_env(home.clone())?),
                Box::new(OpenCodeIntegration::from_env(home)?),
            ],
        ))
    }

    /// Dependency-injected constructor for deterministic contract tests or
    /// embedded clients. It has the same application behavior as `from_env`.
    pub fn new(home: UzeHome, integrations: Vec<Box<dyn IntegrationPort>>) -> Self {
        Self {
            store: UzeStore::new(home.clone()),
            home,
            integrations,
        }
    }

    pub fn list_plugins(&self) -> Result<Vec<PluginSummary>> {
        self.store
            .package_ids()?
            .into_iter()
            .map(|id| self.plugin_summary(&self.store.package(&id)?))
            .collect()
    }

    pub fn inspect_plugin(&self, id: &str) -> Result<PluginInspection> {
        let package = self.package_by_name(id)?;
        let environment = self.engine().compose(std::slice::from_ref(&package.id))?;
        let resources: Vec<_> = environment.resources.iter().collect();
        let deliveries = self
            .integrations
            .iter()
            .map(|integration| {
                let package_plan = integration.package_exposure_plan(&package, &resources);
                let provided = package_plan
                    .as_ref()
                    .map(|plan| plan.provided_resource_identities.clone())
                    .unwrap_or_default();
                let capabilities = resources
                    .iter()
                    .map(|resource| CapabilityDelivery {
                        identity: resource.identity(),
                        kind: resource.capability.kind,
                        plan: (!provided.contains(&resource.identity()))
                            .then(|| integration.exposure_plan(resource)),
                        provided_by_package: provided.contains(&resource.identity()),
                    })
                    .collect();
                HarnessDelivery {
                    integration: integration.id().to_owned(),
                    package_plan,
                    capabilities,
                }
            })
            .collect();
        let reconciliation = self.reconcile(package.id.as_str());
        Ok(PluginInspection {
            plugin: self.plugin_summary(&package)?,
            capabilities: resources
                .iter()
                .map(|resource| PluginCapability {
                    identity: resource.identity(),
                    name: resource.name(),
                    kind: resource.capability.kind,
                })
                .collect(),
            deliveries,
            managed_state: managed_state(&reconciliation),
            reconciliation,
        })
    }

    /// Installs once, chooses package-native delivery first, attaches only
    /// remaining resources, and records every persistent side effect.
    pub fn add_plugin(&self, source: impl Into<PathBuf>) -> Result<AddPluginReport> {
        let _mutation = crate::persistence::MutationLock::acquire(&self.home)?;
        let installed = self.store.install_agent_plugin(source.into())?;
        let environment = self.engine().compose(std::slice::from_ref(&installed.id))?;
        let resources: Vec<_> = environment.resources.iter().collect();
        let mut attachments = Vec::new();
        let mut package_plans = Vec::new();
        for integration in &self.integrations {
            let mut provided = BTreeSet::new();
            if let Some(plan) = integration.package_exposure_plan(&installed, &resources) {
                package_plans.push((integration.id().to_owned(), plan.clone()));
                if let Some(receipt) = integration.attach_package_receipt(&installed, &plan)? {
                    let location = receipt_location(&receipt);
                    state::record_receipt(
                        &self.home,
                        package_receipt_key(installed.id.as_str(), integration.id()),
                        receipt,
                    )?;
                    attachments.push(AttachmentSummary {
                        integration: integration.id().to_owned(),
                        location,
                    });
                    provided = plan.provided_resource_identities;
                }
            }
            for resource in &resources {
                if !provided.contains(&resource.identity())
                    && let Some(receipt) = integration.attach_receipt(resource)?
                {
                    let location = receipt_location(&receipt);
                    state::record_receipt(
                        &self.home,
                        resource_receipt_key(installed.id.as_str(), integration.id(), resource),
                        receipt,
                    )?;
                    attachments.push(AttachmentSummary {
                        integration: integration.id().to_owned(),
                        location,
                    });
                }
            }
        }
        Ok(AddPluginReport {
            plugin: self.plugin_summary(&installed)?,
            package_plans,
            attachments,
        })
    }

    /// Runs only selected, detected setup routines. No integration knowledge
    /// leaks to the caller beyond stable ids and reported facts.
    pub fn setup(&self, requested: Option<&str>) -> Result<Vec<SetupResult>> {
        let _mutation = crate::persistence::MutationLock::acquire(&self.home)?;
        self.home.ensure_layout()?;
        let wanted = requested.map(normalize_harness_name).transpose()?;
        self.integrations
            .iter()
            .filter(|integration| wanted.is_none_or(|id| integration.id() == id))
            .map(|integration| {
                let detection = integration.detect();
                let configured = detection.present;
                if detection.present {
                    integration.install(&self.home)?;
                }
                Ok(SetupResult {
                    integration: integration.id().to_owned(),
                    detection,
                    configured,
                })
            })
            .collect()
    }

    /// Applies the approved lifecycle contract: reconcile, plan, detach only
    /// matched receipts, re-reconcile, forget resolved ledger records, then
    /// delete UZE-owned package bytes.
    pub fn remove_plugin(&self, id: &str) -> Result<RemovePluginReport> {
        let _mutation = crate::persistence::MutationLock::acquire(&self.home)?;
        let package = match self.package_by_name(id) {
            Ok(package) => package,
            Err(UzeError::UnknownPackage(_)) => {
                // There is no tombstone, so UZE cannot claim this package was
                // previously installed. It can still make repeated remove a
                // safe no-op when no ownership evidence remains.
                if state::receipts(&self.home, Some(id))?.is_empty() {
                    return Ok(RemovePluginReport::AlreadyAbsent {
                        plugin: id.to_owned(),
                    });
                }
                return Ok(RemovePluginReport::Blocked {
                    report: self.reconcile(id),
                    plan: PackageRemovalPlan::BlockedByInspection,
                });
            }
            Err(error) => return Err(error),
        };
        let report = self.reconcile(package.id.as_str());
        let plan = plan_remove(&report);
        let (detached_receipts, already_missing_receipts) = match &plan {
            PackageRemovalPlan::Safe {
                detachable_receipts,
                already_missing_receipts,
            } => (
                detachable_receipts.clone(),
                already_missing_receipts.clone(),
            ),
            _ => (Vec::new(), Vec::new()),
        };
        if !matches!(plan, PackageRemovalPlan::Safe { .. }) {
            return Ok(RemovePluginReport::Blocked { report, plan });
        }
        for reconciled in &report.receipts {
            if reconciled.inspection.state != AttachmentState::Matched {
                continue;
            }
            let Some(integration) = self
                .integrations
                .iter()
                .find(|integration| integration.id() == reconciled.receipt.integration)
            else {
                return Ok(RemovePluginReport::Blocked {
                    report: self.reconcile(package.id.as_str()),
                    plan: PackageRemovalPlan::BlockedByInspection,
                });
            };
            let detached = integration.detach_receipt(&reconciled.receipt)?;
            if detached.state != AttachmentState::Missing {
                return Ok(RemovePluginReport::Blocked {
                    report: self.reconcile(package.id.as_str()),
                    plan: plan_remove(&self.reconcile(package.id.as_str())),
                });
            }
        }
        let final_report = self.reconcile(package.id.as_str());
        let final_plan = plan_remove(&final_report);
        if !matches!(final_plan, PackageRemovalPlan::Safe { .. }) {
            return Ok(RemovePluginReport::Blocked {
                report: final_report,
                plan: final_plan,
            });
        }
        for reconciled in &final_report.receipts {
            state::forget_receipt(&self.home, &reconciled.ledger_key)?;
        }
        self.store.remove_package(&package.id)?;
        Ok(RemovePluginReport::Removed {
            plugin: package.id.as_str().to_owned(),
            detached_receipts,
            already_missing_receipts,
        })
    }

    /// Deterministic environment diagnostics. Attachment facts are always
    /// obtained through the same receipt reconciliation used by removal.
    pub fn doctor(&self) -> DoctorReport {
        let package_ids = self.store.package_ids();
        let (store, plugins) = match package_ids {
            Ok(ids) => {
                let packages = ids
                    .into_iter()
                    .filter_map(|id| self.store.package(&id).ok())
                    .collect::<Vec<_>>();
                let inconsistencies = packages
                    .iter()
                    .filter_map(package_store_inconsistency)
                    .collect::<Vec<_>>();
                let health = if inconsistencies.is_empty() {
                    StoreHealth::Ready
                } else {
                    StoreHealth::Blocked(inconsistencies.join("; "))
                };
                (
                    health,
                    packages
                        .iter()
                        .filter_map(|package| self.plugin_summary(package).ok())
                        .collect(),
                )
            }
            Err(error) => (StoreHealth::Blocked(error.to_string()), Vec::new()),
        };
        let harnesses = self
            .integrations
            .iter()
            .map(|integration| HarnessHealth {
                integration: integration.id().to_owned(),
                detection: integration.detect(),
                setup: integration_status(integration.status(&self.home)),
                strategy: state::get(&self.home, integration.id())
                    .ok()
                    .flatten()
                    .map(|record| record.strategy),
            })
            .collect();
        let attachments = plugins
            .iter()
            .map(|plugin: &PluginSummary| PackageManagedState {
                plugin: plugin.id.clone(),
                state: managed_state(&self.reconcile(&plugin.id)),
            })
            .collect();
        let ledger_error = state::receipts(&self.home, None)
            .err()
            .map(|error| error.to_string());
        let integration_state_error = state::load(&self.home).err().map(|error| error.to_string());
        DoctorReport {
            uze_home: self.home.root().to_path_buf(),
            store,
            plugins,
            harnesses,
            attachments,
            ledger_error,
            integration_state_error,
        }
    }

    fn package_by_name(&self, name: &str) -> Result<StoredPackage> {
        self.store
            .package_ids()?
            .into_iter()
            .find(|id| id.as_str() == name)
            .map(|id| self.store.package(&id))
            .transpose()?
            .ok_or_else(|| UzeError::UnknownPackage(name.to_owned()))
    }

    fn plugin_summary(&self, package: &StoredPackage) -> Result<PluginSummary> {
        let environment = self.engine().compose(std::slice::from_ref(&package.id))?;
        Ok(PluginSummary {
            id: package.id.as_str().to_owned(),
            source: package.source.clone(),
            store_path: package.root.clone(),
            capability_count: environment.resources.len(),
        })
    }

    fn engine(&self) -> UzeEngine {
        UzeEngine::new(self.store.clone())
    }

    fn reconcile(&self, package_id: &str) -> ReconciliationReport {
        let integrations = self
            .integrations
            .iter()
            .map(|integration| integration.as_ref() as &dyn IntegrationPort)
            .collect::<Vec<_>>();
        reconcile_package(&self.home, package_id, &integrations)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct PluginSummary {
    pub id: String,
    pub source: PathBuf,
    pub store_path: PathBuf,
    pub capability_count: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct PluginCapability {
    pub identity: String,
    pub name: String,
    pub kind: CapabilityKind,
}

#[derive(Clone, Debug, Serialize)]
pub struct CapabilityDelivery {
    pub identity: String,
    pub kind: CapabilityKind,
    pub provided_by_package: bool,
    pub plan: Option<ExposurePlan>,
}

#[derive(Clone, Debug, Serialize)]
pub struct HarnessDelivery {
    pub integration: String,
    pub package_plan: Option<PackageExposurePlan>,
    pub capabilities: Vec<CapabilityDelivery>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PluginInspection {
    pub plugin: PluginSummary,
    pub capabilities: Vec<PluginCapability>,
    pub deliveries: Vec<HarnessDelivery>,
    pub managed_state: ManagedStateSummary,
    pub reconciliation: ReconciliationReport,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ManagedStateSummary {
    pub matched: usize,
    pub missing: usize,
    pub drifted: usize,
    pub conflicts: usize,
    pub blocked: usize,
    pub ledger_error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AttachmentSummary {
    pub integration: String,
    pub location: PathBuf,
}

#[derive(Clone, Debug, Serialize)]
pub struct AddPluginReport {
    pub plugin: PluginSummary,
    pub package_plans: Vec<(String, PackageExposurePlan)>,
    pub attachments: Vec<AttachmentSummary>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SetupResult {
    pub integration: String,
    pub detection: HarnessDetection,
    pub configured: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "outcome", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RemovePluginReport {
    /// No Store registration or attachment receipt remained. This is a safe
    /// idempotent outcome, not historical evidence that the package existed.
    AlreadyAbsent { plugin: String },
    Removed {
        plugin: String,
        detached_receipts: Vec<String>,
        already_missing_receipts: Vec<String>,
    },
    Blocked {
        report: ReconciliationReport,
        plan: PackageRemovalPlan,
    },
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StoreHealth {
    Ready,
    Blocked(String),
}

#[derive(Clone, Debug, Serialize)]
pub struct HarnessHealth {
    pub integration: String,
    pub detection: HarnessDetection,
    pub setup: String,
    pub strategy: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PackageManagedState {
    pub plugin: String,
    pub state: ManagedStateSummary,
}

#[derive(Clone, Debug, Serialize)]
pub struct DoctorReport {
    pub uze_home: PathBuf,
    pub store: StoreHealth,
    pub plugins: Vec<PluginSummary>,
    pub harnesses: Vec<HarnessHealth>,
    pub attachments: Vec<PackageManagedState>,
    pub ledger_error: Option<String>,
    pub integration_state_error: Option<String>,
}

fn managed_state(report: &ReconciliationReport) -> ManagedStateSummary {
    let mut summary = ManagedStateSummary {
        ledger_error: report.ledger_error.clone(),
        ..ManagedStateSummary::default()
    };
    for receipt in &report.receipts {
        match receipt.inspection.state {
            AttachmentState::Matched => summary.matched += 1,
            AttachmentState::Missing => summary.missing += 1,
            AttachmentState::Drifted => summary.drifted += 1,
            AttachmentState::Conflict => summary.conflicts += 1,
            AttachmentState::Blocked => summary.blocked += 1,
        }
    }
    summary
}

fn integration_status(status: IntegrationStatus) -> String {
    match status {
        IntegrationStatus::NotConfigured => "not configured",
        IntegrationStatus::InstalledUnverified => "installed / unverified",
        IntegrationStatus::InstalledVerified => "installed / verified",
    }
    .to_owned()
}

fn normalize_harness_name(value: &str) -> Result<&'static str> {
    match value {
        "claude" | "claude-code" => Ok("claude-code"),
        "codex" => Ok("codex"),
        "opencode" => Ok("opencode"),
        other => Err(UzeError::ExposureUnavailable(format!(
            "unknown harness `{other}` (expected `claude`, `codex`, or `opencode`)"
        ))),
    }
}

fn package_receipt_key(package: &str, integration: &str) -> String {
    format!("{package}:{integration}:package")
}

fn resource_receipt_key(package: &str, integration: &str, resource: &crate::Resource) -> String {
    format!("{package}:{integration}:{}", resource.identity())
}

fn package_store_inconsistency(package: &StoredPackage) -> Option<String> {
    if !package.root.is_dir() {
        return Some(format!(
            "package `{}` store directory is missing",
            package.id.as_str()
        ));
    }
    if !package.manifest.is_file() {
        return Some(format!(
            "package `{}` plugin.json is missing",
            package.id.as_str()
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;
    use crate::{
        capability::CapabilityKind,
        exposure::{ExposureMechanism, ExposurePlan},
        integration::{AttachmentReceipt, ManagedArtifact},
        project::Resource,
        router::{CompatibilityRoute, HarnessCapabilities, VerificationStatus},
    };

    struct SymlinkIntegration;
    impl IntegrationPort for SymlinkIntegration {
        fn id(&self) -> &'static str {
            "test"
        }
        fn capabilities(&self) -> crate::router::HarnessCapabilities {
            HarnessCapabilities::default()
        }
        fn exposure_plan(&self, resource: &Resource) -> ExposurePlan {
            ExposurePlan {
                representation: resource.capability.representation,
                route: CompatibilityRoute::Adaptable,
                verification: VerificationStatus::Unverified,
                mechanism: ExposureMechanism::Unsupported {
                    rationale: "test does not attach".to_owned(),
                },
                evidence: "test".to_owned(),
            }
        }
    }

    struct PartialIntegration {
        root: PathBuf,
        attached: Cell<bool>,
    }

    struct AllResourceSymlinkIntegration {
        root: PathBuf,
    }

    impl IntegrationPort for AllResourceSymlinkIntegration {
        fn id(&self) -> &'static str {
            "all-resources"
        }

        fn capabilities(&self) -> HarnessCapabilities {
            HarnessCapabilities::default()
        }

        fn exposure_plan(&self, resource: &Resource) -> ExposurePlan {
            ExposurePlan {
                representation: resource.capability.representation,
                route: CompatibilityRoute::Adaptable,
                verification: VerificationStatus::Unverified,
                mechanism: ExposureMechanism::Unsupported {
                    rationale: "test attachment is implemented directly".to_owned(),
                },
                evidence: "test".to_owned(),
            }
        }

        fn attach_receipt(&self, resource: &Resource) -> Result<Option<AttachmentReceipt>> {
            let path = self.root.join(resource.name());
            #[cfg(unix)]
            std::os::unix::fs::symlink(&resource.capability.path, &path).map_err(|source| {
                UzeError::Write {
                    path: path.clone(),
                    source,
                }
            })?;
            Ok(Some(AttachmentReceipt {
                package_id: match &resource.origin {
                    crate::ResourceOrigin::Package { id, .. } => id.as_str().to_owned(),
                    _ => unreachable!(),
                },
                resource_identity: Some(resource.identity()),
                integration: self.id().to_owned(),
                strategy: "test".to_owned(),
                artifact: ManagedArtifact::SymlinkReference {
                    path,
                    target: resource.capability.path.clone(),
                },
            }))
        }
    }

    impl IntegrationPort for PartialIntegration {
        fn id(&self) -> &'static str {
            "partial"
        }

        fn capabilities(&self) -> HarnessCapabilities {
            HarnessCapabilities::default()
        }

        fn exposure_plan(&self, resource: &Resource) -> ExposurePlan {
            ExposurePlan {
                representation: resource.capability.representation,
                route: CompatibilityRoute::Adaptable,
                verification: VerificationStatus::Unverified,
                mechanism: ExposureMechanism::Unsupported {
                    rationale: "test attachment is implemented directly".to_owned(),
                },
                evidence: "test".to_owned(),
            }
        }

        fn attach_receipt(&self, resource: &Resource) -> Result<Option<AttachmentReceipt>> {
            if resource.name() == "github" {
                return Err(UzeError::ExposureUnavailable(
                    "simulated second attachment failure".to_owned(),
                ));
            }
            let path = self.root.join("first-managed-resource");
            #[cfg(unix)]
            std::os::unix::fs::symlink(&resource.capability.path, &path).map_err(|source| {
                UzeError::Write {
                    path: path.clone(),
                    source,
                }
            })?;
            self.attached.set(true);
            Ok(Some(AttachmentReceipt {
                package_id: match &resource.origin {
                    crate::ResourceOrigin::Package { id, .. } => id.as_str().to_owned(),
                    _ => unreachable!(),
                },
                resource_identity: Some(resource.identity()),
                integration: self.id().to_owned(),
                strategy: "test".to_owned(),
                artifact: ManagedArtifact::SymlinkReference {
                    path,
                    target: resource.capability.path.clone(),
                },
            }))
        }
    }

    fn temp(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "uze-application-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    fn fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/packages/agent-plugin-skill")
    }

    fn multi_mcp_fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/packages/multi-mcp-plugin")
    }

    #[test]
    fn list_and_inspect_are_package_centric() {
        let root = temp("inspect");
        let app = UzeApplication::new(UzeHome::at(&root), vec![Box::new(SymlinkIntegration)]);
        app.add_plugin(fixture()).unwrap();
        let listed = app.list_plugins().unwrap();
        assert_eq!(listed.len(), 1);
        let inspection = app.inspect_plugin(&listed[0].id).unwrap();
        assert_eq!(inspection.plugin.id, listed[0].id);
        assert_eq!(inspection.capabilities[0].kind, CapabilityKind::AgentSkill);
        assert_eq!(inspection.deliveries.len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn removal_uses_reconciliation_and_preserves_drift() {
        use std::os::unix::fs::symlink;
        let root = temp("remove");
        let home = UzeHome::at(&root);
        let app = UzeApplication::new(home.clone(), vec![Box::new(SymlinkIntegration)]);
        let package = app.store.install_agent_plugin(fixture()).unwrap();
        let expected = package.root.join("skills/uze-e2e");
        let managed = root.join("managed");
        symlink(&expected, &managed).unwrap();
        state::record_receipt(
            &home,
            "receipt".to_owned(),
            AttachmentReceipt {
                package_id: package.id.as_str().to_owned(),
                resource_identity: None,
                integration: "test".to_owned(),
                strategy: "symlink".to_owned(),
                artifact: ManagedArtifact::SymlinkReference {
                    path: managed.clone(),
                    target: expected.clone(),
                },
            },
        )
        .unwrap();
        assert!(matches!(
            app.remove_plugin(package.id.as_str()).unwrap(),
            RemovePluginReport::Removed { .. }
        ));
        assert!(!managed.exists());
        assert!(app.store.package(&package.id).is_err());

        let package = app.store.install_agent_plugin(fixture()).unwrap();
        let foreign = root.join("foreign");
        fs::create_dir_all(&foreign).unwrap();
        symlink(&foreign, &managed).unwrap();
        state::record_receipt(
            &home,
            "receipt".to_owned(),
            AttachmentReceipt {
                package_id: package.id.as_str().to_owned(),
                resource_identity: None,
                integration: "test".to_owned(),
                strategy: "symlink".to_owned(),
                artifact: ManagedArtifact::SymlinkReference {
                    path: managed.clone(),
                    target: package.root.clone(),
                },
            },
        )
        .unwrap();
        assert!(matches!(
            app.remove_plugin(package.id.as_str()).unwrap(),
            RemovePluginReport::Blocked {
                plan: PackageRemovalPlan::BlockedByDrift,
                ..
            }
        ));
        assert!(app.store.package(&package.id).is_ok());
        assert_eq!(fs::read_link(&managed).unwrap(), foreign);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn doctor_reports_corrupt_ledger_without_destructive_work() {
        let root = temp("doctor");
        let home = UzeHome::at(&root);
        home.ensure_layout().unwrap();
        fs::write(home.state_dir().join("attachments.json"), "bad").unwrap();
        fs::write(home.integrations_state_path(), "bad").unwrap();
        let app = UzeApplication::new(home.clone(), vec![Box::new(SymlinkIntegration)]);
        let report = app.doctor();
        assert!(report.ledger_error.is_some());
        assert!(report.integration_state_error.is_some());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn add_failure_after_a_confirmed_attachment_leaves_reconcilable_ledger_evidence() {
        let root = temp("partial-add");
        let home = UzeHome::at(&root);
        let integration = PartialIntegration {
            root: root.clone(),
            attached: Cell::new(false),
        };
        let app = UzeApplication::new(home.clone(), vec![Box::new(integration)]);
        assert!(app.add_plugin(multi_mcp_fixture()).is_err());
        assert!(
            app.store
                .package_ids()
                .unwrap()
                .iter()
                .any(|id| id.as_str() == "multi-mcp-plugin")
        );
        let receipts = state::receipts(&home, Some("multi-mcp-plugin")).unwrap();
        assert_eq!(receipts.len(), 1);
        assert_eq!(app.doctor().attachments[0].state.matched, 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn remove_is_idempotent_without_claiming_history_for_absent_state() {
        let root = temp("remove-twice");
        let app = UzeApplication::new(UzeHome::at(&root), vec![Box::new(SymlinkIntegration)]);
        let package = app.store.install_agent_plugin(fixture()).unwrap();
        assert!(matches!(
            app.remove_plugin(package.id.as_str()).unwrap(),
            RemovePluginReport::Removed { .. }
        ));
        assert!(matches!(
            app.remove_plugin(package.id.as_str()).unwrap(),
            RemovePluginReport::AlreadyAbsent { .. }
        ));
        app.add_plugin(fixture()).unwrap();
        assert_eq!(app.list_plugins().unwrap().len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn multi_mcp_package_has_independent_receipts_through_safe_removal() {
        let root = temp("multi-mcp-lifecycle");
        let home = UzeHome::at(&root);
        let app = UzeApplication::new(
            home.clone(),
            vec![Box::new(AllResourceSymlinkIntegration {
                root: root.clone(),
            })],
        );
        app.add_plugin(multi_mcp_fixture()).unwrap();
        let receipts = state::receipts(&home, Some("multi-mcp-plugin")).unwrap();
        assert_eq!(receipts.len(), 2);
        assert_ne!(
            receipts[0].1.resource_identity,
            receipts[1].1.resource_identity
        );
        assert!(matches!(
            app.remove_plugin("multi-mcp-plugin").unwrap(),
            RemovePluginReport::Removed { .. }
        ));
        assert!(
            state::receipts(&home, Some("multi-mcp-plugin"))
                .unwrap()
                .is_empty()
        );
        fs::remove_dir_all(root).unwrap();
    }
}
