//! Product-facing application boundary.
//!
//! CLI, TUI, and future presentation layers call this facade rather than
//! reaching into Store, integrations, vendor files, or lifecycle mechanics.

use std::{collections::BTreeSet, path::PathBuf};

use serde::Serialize;

use uze_core::{
    PackageSource, Result, UzeEngine, UzeError, UzeHome, UzeStore,
    capability::CapabilityKind,
    exposure::{ExposurePlan, PackageExposurePlan},
    integration::{
        AttachmentState, HarnessDetection, IntegrationPort, IntegrationStatus, PublicationStatus,
        receipt_location,
    },
    provisioning::{ProcessRunner, ProvisionStatus, ProvisioningResult, SystemProcessRunner},
    reconciliation::{PackageRemovalPlan, ReconciliationReport, plan_remove, reconcile_package},
    state,
    store::StoredPackage,
    trust::{self, TrustAuthority, TrustOutcome, TrustRequest},
};
use uze_integrations::{
    claude::ClaudeIntegration, codex::CodexIntegration, gemini::GeminiIntegration,
    opencode::OpenCodeIntegration,
};

pub struct UzeApplication {
    home: UzeHome,
    store: UzeStore,
    integrations: Vec<Box<dyn IntegrationPort>>,
    runner: Box<dyn ProcessRunner>,
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
                Box::new(OpenCodeIntegration::from_env(home.clone())?),
                // EXPERIMENTAL / CONFORMANCE. Registered to exercise the
                // vendor-neutral core against a fourth, differently shaped
                // harness; not a v0 support claim. See integrations/gemini.rs.
                Box::new(GeminiIntegration::from_env(home)?),
            ],
        ))
    }

    /// Dependency-injected constructor for deterministic contract tests or
    /// embedded clients. It has the same application behavior as `from_env`.
    pub fn new(home: UzeHome, integrations: Vec<Box<dyn IntegrationPort>>) -> Self {
        Self::new_with_runner(home, integrations, Box::new(SystemProcessRunner))
    }

    /// Test and embedding composition point for the process runner used only
    /// by explicit harness provisioning. Package lifecycle remains entirely
    /// independent of process execution.
    pub fn new_with_runner(
        home: UzeHome,
        integrations: Vec<Box<dyn IntegrationPort>>,
        runner: Box<dyn ProcessRunner>,
    ) -> Self {
        Self {
            store: UzeStore::new(home.clone()),
            home,
            integrations,
            runner,
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
    pub fn add_plugin(
        &self,
        source: PackageSource,
        authority: &dyn TrustAuthority,
    ) -> Result<AddPluginReport> {
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.home)?;
        // Acquisition brings the bytes to a local directory and owns their
        // cleanup; the Store only ever sees a materialized package.
        let materialized = uze_core::acquisition::acquire(&source)?;
        self.install_materialized(materialized, authority, &[], false)
    }

    /// The half of installation that runs once bytes exist locally.
    ///
    /// Deliberately takes no lock: both public entry points hold one already,
    /// and `MutationLock` is not reentrant. Sharing this body is what lets
    /// `update_plugin` reuse installation without re-entering it.
    fn install_materialized(
        &self,
        materialized: uze_core::MaterializedPackage,
        authority: &dyn TrustAuthority,
        already_trusted: &[trust::ExecutableCapability],
        replacing_installed: bool,
    ) -> Result<AddPluginReport> {
        // Trust is decided here — after the package is materialized and can
        // be inspected honestly, and strictly before anything is written to
        // the Store or shown to a harness. Neither the Store nor any
        // integration knows this question exists.
        self.authorize(
            &materialized,
            authority,
            already_trusted,
            replacing_installed,
        )?;

        // `uze add` is deliberately enough for a harness the user already
        // has.  Preparing a detected integration only creates UZE's own
        // prerequisites (such as a user-scope discovery directory) and
        // records its setup state; it never installs, upgrades, or launches
        // the vendor executable.  Do it before ingesting so a preparation
        // failure cannot leave a newly installed package with no reported
        // delivery attempt.
        self.prepare_detected_integrations(None)?;

        let installed = self.store.ingest(&materialized)?;

        // Derived views refresh before attachment: a native package delivery
        // reads the view it was just given. A failure here is recorded, never
        // propagated — the package is installed, and one integration's view
        // being stale does not make the installation invalid.
        let publications = self.republish_all();
        let unpublished: BTreeSet<&str> = publications
            .iter()
            .filter(|outcome| outcome.error.is_some())
            .map(|outcome| outcome.integration.as_str())
            .collect();

        let environment = self.engine().compose(std::slice::from_ref(&installed.id))?;
        let resources: Vec<_> = environment.resources.iter().collect();
        let mut attachments = Vec::new();
        let mut package_plans = Vec::new();
        for integration in &self.integrations {
            let mut provided = BTreeSet::new();
            // Native delivery reads the view; attempting it against a view
            // that failed to publish would fail for a reason that has
            // nothing to do with this package.
            if let Some(plan) = integration
                .package_exposure_plan(&installed, &resources)
                .filter(|_| !unpublished.contains(integration.id()))
            {
                package_plans.push((integration.id().to_owned(), plan.clone()));
                if let Some(receipt) = integration.attach_package(&installed, &plan)? {
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
            publications,
        })
    }

    /// Re-resolves a package's original request and replaces the installed
    /// copy with the result.
    ///
    /// The order is the whole safety story. Everything that can fail without
    /// consequence happens first — re-resolve, materialize, validate, and ask
    /// about any execution the installed revision did not already have — and
    /// only then is the current package detached. A network failure, an
    /// invalid package or a refused trust question therefore mutates nothing
    /// at all.
    ///
    /// There is deliberately no rollback across integrations. If one fails to
    /// re-attach, the Store stays consistent, the others keep what they got,
    /// and the partial state is reported rather than papered over — `doctor`
    /// reconciles it and a repeat `update` finishes the job. Blind rollback
    /// would mean detaching artifacts UZE had just proven it owns, on the
    /// word of an unrelated failure.
    pub fn update_plugin(
        &self,
        id: &str,
        authority: &dyn TrustAuthority,
    ) -> Result<UpdatePluginReport> {
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.home)?;
        let installed = self.package_by_name(id)?;

        // Re-resolve the *request*, not the resolution: that is what makes a
        // branch move forward while a pinned commit stays put.
        let materialized = uze_core::acquisition::acquire(&installed.provenance.requested)?;

        let previous = {
            let environment = self.engine().compose(std::slice::from_ref(&installed.id))?;
            let resources: Vec<&uze_core::Resource> = environment.resources.iter().collect();
            trust::executable_capabilities(&resources)
        };
        self.authorize(&materialized, authority, &previous, true)?;

        // Nothing destructive has happened yet. From here the current package
        // is removed under the same ownership rules any removal obeys.
        let removal = self.detach_and_remove(id)?;
        if let RemovePluginReport::Blocked { report, plan } = removal {
            return Ok(UpdatePluginReport::Blocked { report, plan });
        }
        // Trust was already settled above against the previous capabilities,
        // so installation must not ask a second time for the same answer.
        let report = self.install_materialized(materialized, &trust::AlwaysTrust, &[], true)?;
        Ok(UpdatePluginReport::Updated {
            plugin: report.plugin,
            attachments: report.attachments,
            publications: report.publications,
        })
    }

    /// Runs only selected, detected setup routines. No integration knowledge
    /// leaks to the caller beyond stable ids and reported facts.
    pub fn setup(&self, requested: Option<&str>) -> Result<Vec<SetupResult>> {
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.home)?;
        self.home.ensure_layout()?;
        let wanted = requested
            .map(|name| self.resolve_integration_id(name))
            .transpose()?;
        let results = self.provision_and_prepare(wanted)?;
        // `setup` is the documented way to repair a derived view that
        // failed to publish, so it always rebuilds them.
        let _ = self.republish_all();
        for result in results.iter().filter(|result| result.configured) {
            if let Some(integration) = self
                .integrations
                .iter()
                .find(|integration| integration.id() == result.integration)
            {
                self.attach_stored_packages_to(integration.as_ref())?;
            }
        }
        Ok(results)
    }

    /// Explicit setup is the only path allowed to provision or update an
    /// executable. `add` deliberately calls only `prepare_detected_*`.
    fn provision_and_prepare(&self, requested: Option<&str>) -> Result<Vec<SetupResult>> {
        self.integrations
            .iter()
            .filter(|integration| requested.is_none_or(|id| integration.id() == id))
            .map(|integration| {
                let provisioning = integration.provision(self.runner.as_ref())?;
                state::record_provisioning(&self.home, integration.id(), &provisioning)?;
                let configured = provisioning.status == ProvisionStatus::Verified;
                if configured {
                    integration.install(&self.home)?;
                }
                Ok(SetupResult {
                    integration: integration.id().to_owned(),
                    detection: provisioning.detection.clone(),
                    configured,
                    provisioning,
                })
            })
            .collect()
    }

    /// Prepares integrations only when their real executable is present.
    /// This is the shared bridge between explicit `setup` and implicit
    /// preparation during `add`; neither presentation layer needs to know
    /// which directories/configuration an integration owns.
    fn prepare_detected_integrations(&self, requested: Option<&str>) -> Result<Vec<SetupResult>> {
        self.integrations
            .iter()
            .filter(|integration| requested.is_none_or(|id| integration.id() == id))
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
                    provisioning: ProvisioningResult::verified(
                        uze_core::provisioning::ProvisionAction::None,
                        "implicit-existing-executable",
                        integration.detect(),
                    ),
                })
            })
            .collect()
    }

    /// Delivers packages which were installed before an explicit setup made
    /// this integration available. This repeats the same package-first plan
    /// as `add`, scoped to one integration, and ledger keys make it
    /// idempotent without inventing a sync subsystem.
    fn attach_stored_packages_to(&self, integration: &dyn IntegrationPort) -> Result<()> {
        for package_id in self.store.package_ids()? {
            let package = self.store.package(&package_id)?;
            let environment = self.engine().compose(std::slice::from_ref(&package.id))?;
            let resources: Vec<_> = environment.resources.iter().collect();
            let mut provided = BTreeSet::new();
            if let Some(plan) = integration.package_exposure_plan(&package, &resources)
                && let Some(receipt) = integration.attach_package(&package, &plan)?
            {
                state::record_receipt(
                    &self.home,
                    package_receipt_key(package.id.as_str(), integration.id()),
                    receipt,
                )?;
                provided = plan.provided_resource_identities;
            }
            for resource in resources {
                if !provided.contains(&resource.identity())
                    && let Some(receipt) = integration.attach_receipt(resource)?
                {
                    state::record_receipt(
                        &self.home,
                        resource_receipt_key(package.id.as_str(), integration.id(), resource),
                        receipt,
                    )?;
                }
            }
        }
        Ok(())
    }

    /// Applies the approved lifecycle contract: reconcile, plan, detach only
    /// matched receipts, re-reconcile, forget resolved ledger records, then
    /// delete UZE-owned package bytes.
    pub fn remove_plugin(&self, id: &str) -> Result<RemovePluginReport> {
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.home)?;
        self.detach_and_remove(id)
    }

    /// Removal without taking the lock; see `install_materialized`.
    fn detach_and_remove(&self, id: &str) -> Result<RemovePluginReport> {
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
        // The package set changed, so every derived view is now stale. A
        // failure to rebuild one does not un-remove the package.
        let _ = self.republish_all();
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
        let installed = self.installed_packages();
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
                provisioning: state::provisioning(&self.home, integration.id())
                    .ok()
                    .flatten(),
                // Observed, not remembered. A package can be installed and
                // reconciled while a harness still cannot see it, and that is
                // exactly the state this field exists to surface.
                publication: integration.publication(&installed),
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
        let provisioning_state_error = self
            .integrations
            .iter()
            .find_map(|integration| state::provisioning(&self.home, integration.id()).err())
            .map(|error| error.to_string());
        DoctorReport {
            uze_home: self.home.root().to_path_buf(),
            store,
            plugins,
            harnesses,
            attachments,
            ledger_error,
            integration_state_error,
            provisioning_state_error,
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
            source: package.provenance.requested.display(),
            store_path: package.root.clone(),
            capability_count: environment.resources.len(),
        })
    }

    /// Refreshes every integration's derived view of the installed package
    /// set. Collects failures instead of propagating them: publication is not
    /// part of package ownership, so one harness failing to rebuild its view
    /// leaves the package installed and the other harnesses unaffected.
    fn republish_all(&self) -> Vec<PublicationOutcome> {
        let packages = self.installed_packages();
        self.integrations
            .iter()
            .map(|integration| PublicationOutcome {
                integration: integration.id().to_owned(),
                error: integration
                    .republish_packages(&packages)
                    .err()
                    .map(|error| error.to_string()),
            })
            .collect()
    }

    fn installed_packages(&self) -> Vec<StoredPackage> {
        self.store
            .package_ids()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|id| self.store.package(&id).ok())
            .collect()
    }

    /// Resolves a requested harness name against the integrations actually
    /// registered in this composition root. There is deliberately no central
    /// list of vendors: an integration declares its own id and aliases, so
    /// registering one is the only step needed to make it selectable.
    fn resolve_integration_id(&self, requested: &str) -> Result<&'static str> {
        self.integrations
            .iter()
            .find(|integration| {
                integration.id() == requested || integration.aliases().contains(&requested)
            })
            .map(|integration| integration.id())
            .ok_or_else(|| {
                let known = self
                    .integrations
                    .iter()
                    .map(|integration| integration.id())
                    .collect::<Vec<_>>()
                    .join(", ");
                UzeError::ExposureUnavailable(format!(
                    "unknown harness `{requested}` (registered: {known})"
                ))
            })
    }

    /// Asks the supplied authority about any capability that would introduce
    /// process execution, and refuses to proceed without a grant.
    ///
    /// Returns `Ok(())` immediately when the package declares nothing
    /// executable: a purely declarative package needs no consent beyond the
    /// decision to install it.
    fn authorize(
        &self,
        materialized: &uze_core::MaterializedPackage,
        authority: &dyn TrustAuthority,
        already_trusted: &[trust::ExecutableCapability],
        replacing_installed: bool,
    ) -> Result<()> {
        let provenance = materialized.provenance();
        if !provenance.requested.crosses_trust_boundary() {
            return Ok(());
        }
        let inspected = uze_core::acquisition::inspect_capabilities(materialized)?;
        let resources: Vec<&uze_core::Resource> = inspected.resources.iter().collect();
        let executable = trust::executable_capabilities(&resources);
        if executable.is_empty() || !trust::introduces_new_execution(already_trusted, &executable) {
            return Ok(());
        }
        let request = TrustRequest {
            package_id: inspected.package_id.clone(),
            requested_source: provenance.requested.display(),
            resolved_source: provenance.resolved.display(),
            executable,
            // The operator is being asked about a *change* to something they
            // already have, not about a first install. Derived from the fact
            // of an existing installation rather than from whether the
            // previous revision happened to execute anything — a declarative
            // package gaining an MCP server is exactly the case that must
            // read as a change.
            previously_trusted: replacing_installed,
        };
        match authority.authorize(&request) {
            TrustOutcome::Granted => Ok(()),
            TrustOutcome::Denied => Err(UzeError::TrustDenied(request.package_id)),
            TrustOutcome::Unavailable => Err(UzeError::TrustRequired {
                package: request.package_id.clone(),
                detail: request
                    .executable
                    .iter()
                    .map(|capability| {
                        format!(
                            "{} -> {} {}",
                            capability.name,
                            capability.command,
                            capability.arguments.join(" ")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; "),
            }),
        }
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
    /// Human-facing description of where this package came from. Display
    /// only: the typed provenance stays in the registry, and nothing parses
    /// this back.
    pub source: String,
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
pub struct PublicationOutcome {
    pub integration: String,
    /// `None` when the derived view refreshed cleanly. A message here is
    /// actionable on its own: the package is installed and only the view
    /// needs rebuilding.
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AddPluginReport {
    pub plugin: PluginSummary,
    pub package_plans: Vec<(String, PackageExposurePlan)>,
    pub attachments: Vec<AttachmentSummary>,
    pub publications: Vec<PublicationOutcome>,
}

#[derive(Clone, Debug, Serialize)]
pub enum UpdatePluginReport {
    Updated {
        plugin: PluginSummary,
        attachments: Vec<AttachmentSummary>,
        publications: Vec<PublicationOutcome>,
    },
    /// The installed package could not be safely detached, so nothing was
    /// replaced. The newly resolved revision is discarded with its scratch
    /// directory.
    Blocked {
        report: ReconciliationReport,
        plan: PackageRemovalPlan,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct SetupResult {
    pub integration: String,
    pub detection: HarnessDetection,
    pub configured: bool,
    pub provisioning: ProvisioningResult,
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
    pub provisioning: Option<state::ProvisioningRecord>,
    pub publication: PublicationStatus,
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
    pub provisioning_state_error: Option<String>,
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

fn package_receipt_key(package: &str, integration: &str) -> String {
    format!("{package}:{integration}:package")
}

fn resource_receipt_key(package: &str, integration: &str, resource: &uze_core::Resource) -> String {
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
    use uze_core::{
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
        fn capabilities(&self) -> uze_core::router::HarnessCapabilities {
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
                    uze_core::ResourceOrigin::Package { id, .. } => id.as_str().to_owned(),
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
                    uze_core::ResourceOrigin::Package { id, .. } => id.as_str().to_owned(),
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
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/packages/agent-plugin-skill")
    }

    fn multi_mcp_fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/packages/multi-mcp-plugin")
    }

    #[test]
    fn list_and_inspect_are_package_centric() {
        let root = temp("inspect");
        let app = UzeApplication::new(UzeHome::at(&root), vec![Box::new(SymlinkIntegration)]);
        app.add_plugin(
            uze_core::PackageSource::local(fixture()),
            &uze_core::trust::AlwaysTrust,
        )
        .unwrap();
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
        let package = app
            .store
            .ingest(
                &uze_core::acquisition::acquire(&uze_core::PackageSource::local(fixture()))
                    .unwrap(),
            )
            .unwrap();
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

        let package = app
            .store
            .ingest(
                &uze_core::acquisition::acquire(&uze_core::PackageSource::local(fixture()))
                    .unwrap(),
            )
            .unwrap();
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
        assert!(
            app.add_plugin(
                uze_core::PackageSource::local(multi_mcp_fixture()),
                &uze_core::trust::AlwaysTrust
            )
            .is_err()
        );
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
        let package = app
            .store
            .ingest(
                &uze_core::acquisition::acquire(&uze_core::PackageSource::local(fixture()))
                    .unwrap(),
            )
            .unwrap();
        assert!(matches!(
            app.remove_plugin(package.id.as_str()).unwrap(),
            RemovePluginReport::Removed { .. }
        ));
        assert!(matches!(
            app.remove_plugin(package.id.as_str()).unwrap(),
            RemovePluginReport::AlreadyAbsent { .. }
        ));
        app.add_plugin(
            uze_core::PackageSource::local(fixture()),
            &uze_core::trust::AlwaysTrust,
        )
        .unwrap();
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
        app.add_plugin(
            uze_core::PackageSource::local(multi_mcp_fixture()),
            &uze_core::trust::AlwaysTrust,
        )
        .unwrap();
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
