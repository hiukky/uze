//! Removing a plugin, and tearing down the harness artifacts its receipts
//! own.

use uze_core::{
    PackageSource, Result, UzeError,
    integration::AttachmentState,
    manifest::BUILT_IN_MARKETPLACE,
    reconciliation::{PackageRemovalPlan, plan_remove},
    state,
    store::StoredPackage,
};

use crate::bootstrap;

use super::super::services::Plugins;
use super::super::*;

impl Plugins<'_> {
    #[tracing::instrument(name = "plugins.remove", skip_all, fields(id = %id), err)]
    pub fn remove(&self, id: &str) -> Result<RemovePluginReport> {
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.0.home)?;
        // Removal changes vendor-visible state; cached inspection verdicts
        // must not outlive it (ADR 018).
        self.0.inspection_cache.invalidate();
        self.detach_and_remove(id, false)
    }

    /// Whether an installed package is protected from removal. Only the
    /// verified official origin is: an embedded provenance, not a
    /// marketplace name — a local/Git package that merely shares the name
    /// `uze` remains removable, so protection cannot be spoofed by naming.
    pub(crate) fn is_protected_package(package: &StoredPackage) -> bool {
        matches!(
            &package.provenance.requested,
            PackageSource::Embedded { id } if is_protected_plugin(BUILT_IN_MARKETPLACE, id)
        )
    }

    pub(crate) fn detach_and_remove(
        &self,
        id: &str,
        allow_protected: bool,
    ) -> Result<RemovePluginReport> {
        let package = match self.0.package_by_name(id) {
            Ok(package) => package,
            Err(UzeError::UnknownPackage(_)) => {
                // There is no tombstone, so UZE cannot claim this package was
                // previously installed. It can still make repeated remove a
                // safe no-op when no ownership evidence remains.
                if state::receipts(&self.0.home, Some(id))?.is_empty() {
                    return Ok(RemovePluginReport::AlreadyAbsent {
                        plugin: id.to_owned(),
                    });
                }
                return Ok(RemovePluginReport::Blocked {
                    report: self.0.reconcile(id),
                    plan: PackageRemovalPlan::BlockedByInspection,
                });
            }
            Err(error) => return Err(error),
        };
        if !allow_protected && Self::is_protected_package(&package) {
            return Err(UzeError::ProtectedPackage(package.id.as_str().to_owned()));
        }
        let (detached_receipts, already_missing_receipts, final_report) =
            match self.0.detach_owned_receipts(package.id.as_str())? {
                ReceiptTeardown::Refused { report, plan }
                | ReceiptTeardown::Incomplete { report, plan } => {
                    return Ok(RemovePluginReport::Blocked { report, plan });
                }
                ReceiptTeardown::Detached {
                    detached_receipts,
                    already_missing_receipts,
                    final_report,
                } => (detached_receipts, already_missing_receipts, final_report),
            };
        for reconciled in &final_report.receipts {
            state::forget_receipt(&self.0.home, &reconciled.receipt)?;
        }
        self.0.store.remove_package(&package.id)?;
        // The package set changed, so every derived view is now stale. A
        // failure to rebuild one does not un-remove the package.
        let _ = self.0.republish_all();
        Ok(RemovePluginReport::Removed {
            plugin: package.id.as_str().to_owned(),
            detached_receipts,
            already_missing_receipts,
        })
    }
}

/// Whether `plugin`, offered by `marketplace`, belongs to the official set
/// compiled into this binary — which re-seeds itself, so removing a member
/// of it is not an operation that means anything.
pub(crate) fn is_protected_plugin(marketplace: &str, plugin: &str) -> bool {
    marketplace == BUILT_IN_MARKETPLACE
        && bootstrap::entries()
            .is_ok_and(|official| official.plugins.iter().any(|entry| entry.name == plugin))
}

/// Where detaching a package's receipts got to.
pub(crate) enum ReceiptTeardown {
    /// Nothing was touched: the receipts do not all reconcile as safe to
    /// remove.
    Refused {
        report: ReconciliationReport,
        plan: PackageRemovalPlan,
    },
    /// Detaching began and did not finish; `report` was taken afterwards.
    Incomplete {
        report: ReconciliationReport,
        plan: PackageRemovalPlan,
    },
    /// Every artifact the receipts own is gone, verified by `final_report` —
    /// which is what the ledger may be forgotten against, never the snapshot
    /// taken before detaching.
    Detached {
        detached_receipts: Vec<String>,
        already_missing_receipts: Vec<String>,
        final_report: ReconciliationReport,
    },
}

impl UzeApplication {
    /// Detaches every artifact `package_id`'s receipts own, only when all of
    /// them reconcile as safe to remove. Leaves the ledger alone: whether a
    /// receipt that failed to be forgotten is an error is the caller's call.
    ///
    /// `Err` only when an integration fails to detach.
    pub(crate) fn detach_owned_receipts(&self, package_id: &str) -> Result<ReceiptTeardown> {
        let report = self.reconcile(package_id);
        let (detachable_receipts, already_missing_receipts) = match plan_remove(&report) {
            PackageRemovalPlan::Safe {
                detachable_receipts,
                already_missing_receipts,
            } => (detachable_receipts, already_missing_receipts),
            plan => return Ok(ReceiptTeardown::Refused { report, plan }),
        };
        for reconciled in &report.receipts {
            if reconciled.inspection.state != AttachmentState::Matched {
                continue;
            }
            let Some(integration) = self
                .integrations
                .iter()
                .find(|integration| integration.id() == reconciled.receipt.integration)
            else {
                return Ok(ReceiptTeardown::Incomplete {
                    report: self.reconcile(package_id),
                    plan: PackageRemovalPlan::BlockedByInspection,
                });
            };
            tracing::info!(
                target: uze_core::acquisition::git::STEP,
                step = "detach",
                harness = integration.id()
            );
            if integration.detach_receipt(&reconciled.receipt)?.state != AttachmentState::Missing {
                let report = self.reconcile(package_id);
                let plan = plan_remove(&report);
                return Ok(ReceiptTeardown::Incomplete { report, plan });
            }
        }
        let final_report = self.reconcile(package_id);
        let final_plan = plan_remove(&final_report);
        if !matches!(final_plan, PackageRemovalPlan::Safe { .. }) {
            return Ok(ReceiptTeardown::Incomplete {
                report: final_report,
                plan: final_plan,
            });
        }
        Ok(ReceiptTeardown::Detached {
            detached_receipts: detachable_receipts,
            already_missing_receipts,
            final_report,
        })
    }
}
