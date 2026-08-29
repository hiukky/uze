//! Safe machine-level desired-state convergence for `doctor` and the TUI.
//!
//! This is deliberately narrower than install/update: it neither acquires
//! bytes nor expands trust. A receipt must first inspect as `Missing`, then
//! the owning integration must prove it can rebuild that exact artifact.

use std::collections::BTreeSet;

use serde::Serialize;

use uze_core::{
    integration::{AttachmentState, ManagedArtifact, PublicationStatus},
    persistence::MutationLock,
    reconciliation::{PackageRemovalPlan, plan_remove},
    state,
};

use super::*;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "outcome", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MaintenanceOutcome {
    Repaired {
        plugin: String,
        integration: String,
        receipt: String,
    },
    /// Receipts found under a package id nothing in the Store answers to
    /// any more — not "missing an artifact" (that is `Repaired`'s job),
    /// but evidence for a package that is simply gone (removed, or
    /// renamed out from under them, e.g. this project's own
    /// marketplace-qualification: `flow` became `flow@ai`, orphaning
    /// every `flow`-keyed receipt). Detached and forgotten the same way
    /// `remove_plugin` would, and only when that path is fully `Safe`
    /// (see `reconcile_orphaned_receipts`) — never for a foreign or
    /// ambiguous state.
    OrphanCleaned {
        plugin: String,
        ledger_keys: Vec<String>,
    },
    UpdateAvailable {
        plugin: String,
    },
    NeedsHumanAction {
        plugin: String,
        integration: Option<String>,
        receipt: Option<String>,
        state: Option<AttachmentState>,
        reason: String,
    },
    Unavailable {
        integration: String,
        reason: String,
    },
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct MaintenanceReport {
    pub outcomes: Vec<MaintenanceOutcome>,
}

impl MaintenanceReport {
    pub fn repaired_count(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|outcome| {
                matches!(
                    outcome,
                    MaintenanceOutcome::Repaired { .. } | MaintenanceOutcome::OrphanCleaned { .. }
                )
            })
            .count()
    }
}

impl UzeApplication {
    /// Bounded, local maintenance used by health presenters. It only repairs
    /// receipt-proven missing artifacts and stale derived views. Every other
    /// state remains evidence for a person to decide on.
    pub fn maintain_environment(&self) -> MaintenanceReport {
        let Ok(_mutation) = MutationLock::acquire(&self.home) else {
            return MaintenanceReport {
                outcomes: vec![MaintenanceOutcome::Unavailable {
                    integration: "environment".to_owned(),
                    reason: "another UZE mutation is in progress".to_owned(),
                }],
            };
        };

        let mut report = MaintenanceReport::default();
        // Prunes any Store registration whose directory is gone and cleans
        // every receipt left behind under an id nothing answers to any
        // more — first, so `installed_packages` below never treats a
        // ghost registration as real.
        report.outcomes.extend(self.reconcile_orphaned_receipts());
        let packages = self.installed_packages();

        for integration in &self.integrations {
            if let PublicationStatus::Unpublished(_) = integration.publication(&packages)
                && let Err(error) = integration.republish_packages(&packages)
            {
                report.outcomes.push(MaintenanceOutcome::Unavailable {
                    integration: integration.id().to_owned(),
                    reason: error.to_string(),
                });
            }
        }

        for package in &packages {
            if self
                .plugin_summary(package)
                .ok()
                .and_then(|summary| summary.update_available)
                == Some(true)
            {
                report.outcomes.push(MaintenanceOutcome::UpdateAvailable {
                    plugin: package.id.as_str().to_owned(),
                });
            }

            let entries = match state::receipts(&self.home, Some(package.id.as_str())) {
                Ok(entries) => entries,
                Err(error) => {
                    report.outcomes.push(MaintenanceOutcome::NeedsHumanAction {
                        plugin: package.id.as_str().to_owned(),
                        integration: None,
                        receipt: None,
                        state: Some(AttachmentState::Blocked),
                        reason: error.to_string(),
                    });
                    continue;
                }
            };

            // Maintenance only probes receipt kinds it knows how to restore
            // exactly. Other attachments are still inspected once by the
            // subsequent doctor report, avoiding a second vendor CLI call on
            // every anomalous report.
            for (ledger_key, receipt) in entries.into_iter().filter(|(_, receipt)| {
                matches!(
                    receipt.artifact,
                    ManagedArtifact::SymlinkReference { .. }
                        | ManagedArtifact::ManagedTextRegion { .. }
                )
            }) {
                let Some(integration) = self
                    .integrations
                    .iter()
                    .find(|candidate| candidate.id() == receipt.integration)
                else {
                    report.outcomes.push(MaintenanceOutcome::NeedsHumanAction {
                        plugin: package.id.as_str().to_owned(),
                        integration: Some(receipt.integration.clone()),
                        receipt: Some(ledger_key),
                        state: Some(AttachmentState::Blocked),
                        reason: "the receipt's integration is unavailable".to_owned(),
                    });
                    continue;
                };
                let inspection = integration.inspect_receipt(&receipt);
                match inspection.state {
                    AttachmentState::Matched => {}
                    AttachmentState::Missing => match integration.repair_missing_receipt(&receipt) {
                        Ok(true) => {
                            let after = integration.inspect_receipt(&receipt);
                            if after.state == AttachmentState::Matched {
                                report.outcomes.push(MaintenanceOutcome::Repaired {
                                    plugin: package.id.as_str().to_owned(),
                                    integration: integration.id().to_owned(),
                                    receipt: ledger_key,
                                });
                            } else {
                                report.outcomes.push(MaintenanceOutcome::Unavailable {
                                    integration: integration.id().to_owned(),
                                    reason: format!(
                                        "repair of `{}` did not converge: {}",
                                        ledger_key, after.reason
                                    ),
                                });
                            }
                        }
                        Ok(false) => report.outcomes.push(MaintenanceOutcome::NeedsHumanAction {
                            plugin: package.id.as_str().to_owned(),
                            integration: Some(integration.id().to_owned()),
                            receipt: Some(ledger_key),
                            state: Some(AttachmentState::Missing),
                            reason: format!(
                                "{}; this attachment cannot be safely rebuilt from its receipt",
                                inspection.reason
                            ),
                        }),
                        Err(error) => report.outcomes.push(MaintenanceOutcome::Unavailable {
                            integration: integration.id().to_owned(),
                            reason: format!("repair of `{ledger_key}` failed: {error}"),
                        }),
                    },
                    state @ (AttachmentState::Drifted
                    | AttachmentState::Conflict
                    | AttachmentState::Blocked) => {
                        report.outcomes.push(MaintenanceOutcome::NeedsHumanAction {
                            plugin: package.id.as_str().to_owned(),
                            integration: Some(receipt.integration),
                            receipt: Some(ledger_key),
                            state: Some(state),
                            reason: inspection.reason,
                        });
                    }
                }
            }
        }

        if report.repaired_count() > 0 {
            self.inspection_cache.invalidate();
        }
        report
    }

    /// First prunes any Store registration whose directory is gone
    /// (`UzeStore::prune_ghost_registrations`), then detaches and forgets
    /// every receipt keyed by a package id nothing currently installed
    /// answers to — evidence for a package that no longer exists, most
    /// often left behind by a rename (a store id format change, or a
    /// plugin reinstalled under a different marketplace) rather than an
    /// explicit remove. Left alone, a physical slot an orphan still
    /// occupies (e.g. a shared-root Skill entry, keyed by stable label
    /// rather than the full qualified id) permanently blocks the live
    /// package's own attach with a projection conflict — this is what
    /// lets that resolve itself instead of requiring a person to notice
    /// and hand-clean the ledger.
    ///
    /// Reuses exactly the safety rule `remove_plugin` enforces: an orphan
    /// is only ever touched when every one of its receipts reconciles as
    /// cleanly `Safe` to remove (see `plan_remove`) — `Drifted`,
    /// `Conflict`, `Blocked`, or an unrecoverable ledger all fall through
    /// to `NeedsHumanAction` untouched, same as a manual remove would.
    pub(crate) fn reconcile_orphaned_receipts(&self) -> Vec<MaintenanceOutcome> {
        let mut outcomes = Vec::new();
        // A registry entry is the Store's sole claim that a package is
        // installed; prune false claims before anything below asks it.
        if let Ok(pruned) = self.store.prune_ghost_registrations()
            && !pruned.is_empty()
        {
            self.inspection_cache.invalidate();
        }
        let known_ids: BTreeSet<String> = self
            .installed_packages()
            .into_iter()
            .map(|package| package.id.as_str().to_owned())
            .collect();

        let Ok(all_receipts) = state::receipts(&self.home, None) else {
            // The ledger-read failure is already surfaced elsewhere in the
            // report this feeds into; nothing new to say about it here.
            return outcomes;
        };
        let orphan_ids: BTreeSet<String> = all_receipts
            .into_iter()
            .map(|(_, receipt)| receipt.package_id)
            .filter(|id| !known_ids.contains(id.as_str()))
            .collect();

        for package_id in orphan_ids {
            let report = self.reconcile(&package_id);
            if !matches!(plan_remove(&report), PackageRemovalPlan::Safe { .. }) {
                outcomes.push(MaintenanceOutcome::NeedsHumanAction {
                    plugin: package_id,
                    integration: None,
                    receipt: None,
                    state: None,
                    reason: "orphaned receipts exist for a package no longer installed, \
                             and are not all safely removable automatically"
                        .to_owned(),
                });
                continue;
            }
            let mut blocked = false;
            for reconciled in &report.receipts {
                if reconciled.inspection.state != AttachmentState::Matched {
                    continue;
                }
                let Some(integration) = self
                    .integrations
                    .iter()
                    .find(|integration| integration.id() == reconciled.receipt.integration)
                else {
                    blocked = true;
                    break;
                };
                match integration.detach_receipt(&reconciled.receipt) {
                    Ok(inspection) if inspection.state == AttachmentState::Missing => {}
                    _ => {
                        blocked = true;
                        break;
                    }
                }
            }
            // Re-reconcile after detaching, exactly like `detach_and_remove`:
            // the ledger is only ever forgotten against a fresh, live-verified
            // Missing state, never the pre-detach snapshot.
            let final_report = self.reconcile(&package_id);
            if blocked || !matches!(plan_remove(&final_report), PackageRemovalPlan::Safe { .. }) {
                outcomes.push(MaintenanceOutcome::NeedsHumanAction {
                    plugin: package_id,
                    integration: None,
                    receipt: None,
                    state: None,
                    reason: "orphaned receipts did not fully detach".to_owned(),
                });
                continue;
            }
            let ledger_keys: Vec<String> = final_report
                .receipts
                .iter()
                .filter(|reconciled| {
                    state::forget_receipt(&self.home, &reconciled.ledger_key).is_ok()
                })
                .map(|reconciled| reconciled.ledger_key.clone())
                .collect();
            if !ledger_keys.is_empty() {
                outcomes.push(MaintenanceOutcome::OrphanCleaned {
                    plugin: package_id,
                    ledger_keys,
                });
            }
        }
        outcomes
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use uze_core::{
        PackageSource,
        integration::{AttachmentReceipt, ManagedArtifact},
        state,
        trust::AlwaysTrust,
    };

    use super::*;

    #[cfg(unix)]
    #[test]
    fn restores_a_missing_receipt_owned_symlink_without_touching_plugin_bytes() {
        let root = std::env::temp_dir().join(format!(
            "uze-maintenance-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let home = UzeHome::at(&root);
        let target = root.join("store-target");
        let link = root.join("harness/skill");
        fs::create_dir_all(&target).unwrap();
        let app = UzeApplication::new(home.clone(), vec![Box::new(TestIntegration)]);
        app.add_plugin(
            PackageSource::local(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../tests/_fixtures/canonical/skill-plugin"),
            ),
            &AlwaysTrust,
        )
        .unwrap();
        let package_id = app.list_plugins().unwrap().remove(0).id;
        state::record_receipt(
            &home,
            "fixture:receipt".to_owned(),
            AttachmentReceipt {
                package_id,
                resource_identity: Some("skill:fixture".to_owned()),
                integration: "fixture".to_owned(),
                strategy: "symlink".to_owned(),
                artifact: ManagedArtifact::SymlinkReference {
                    path: link.clone(),
                    target: target.clone(),
                },
            },
        )
        .unwrap();
        let report = app.maintain_environment();
        assert!(report.outcomes.iter().any(|outcome| matches!(
            outcome,
            MaintenanceOutcome::Repaired { receipt, .. } if receipt == "fixture:receipt"
        )));
        assert_eq!(fs::read_link(link).unwrap(), target);
    }

    #[cfg(unix)]
    #[test]
    fn preserves_a_drifted_receipt_owned_symlink() {
        let root = std::env::temp_dir().join(format!(
            "uze-maintenance-drift-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let home = UzeHome::at(&root);
        let expected = root.join("expected");
        let foreign = root.join("foreign");
        let link = root.join("harness/skill");
        fs::create_dir_all(&expected).unwrap();
        fs::create_dir_all(&foreign).unwrap();
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&foreign, &link).unwrap();
        let app = UzeApplication::new(home.clone(), vec![Box::new(TestIntegration)]);
        app.add_plugin(
            PackageSource::local(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../tests/_fixtures/canonical/skill-plugin"),
            ),
            &AlwaysTrust,
        )
        .unwrap();
        let package_id = app.list_plugins().unwrap().remove(0).id;
        state::record_receipt(
            &home,
            "fixture:drifted".to_owned(),
            AttachmentReceipt {
                package_id,
                resource_identity: Some("skill:fixture".to_owned()),
                integration: "fixture".to_owned(),
                strategy: "symlink".to_owned(),
                artifact: ManagedArtifact::SymlinkReference {
                    path: link.clone(),
                    target: expected,
                },
            },
        )
        .unwrap();

        let report = app.maintain_environment();
        assert!(report.outcomes.iter().any(|outcome| matches!(
            outcome,
            MaintenanceOutcome::NeedsHumanAction {
                state: Some(AttachmentState::Drifted),
                ..
            }
        )));
        assert_eq!(fs::read_link(link).unwrap(), foreign);
    }

    #[cfg(unix)]
    #[test]
    fn an_orphaned_receipt_is_cleaned_and_frees_the_slot_it_occupied() {
        // Reproduces the real failure a store id format change (this
        // project's own marketplace-qualification: `flow` -> `flow@ai`)
        // leaves behind: an old receipt still recorded under a package id
        // nothing installs under any more, still physically occupying a
        // shared slot a live install now needs.
        let root = std::env::temp_dir().join(format!(
            "uze-maintenance-orphan-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let home = UzeHome::at(&root);
        let target = root.join("store-target");
        let link = root.join("harness/skill");
        fs::create_dir_all(&target).unwrap();
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let app = UzeApplication::new(home.clone(), vec![Box::new(TestIntegration)]);

        // No package named "old-git" is installed — this receipt is
        // already an orphan the moment it is recorded.
        state::record_receipt(
            &home,
            "old-git:fixture:package".to_owned(),
            AttachmentReceipt {
                package_id: "old-git".to_owned(),
                resource_identity: None,
                integration: "fixture".to_owned(),
                strategy: "symlink".to_owned(),
                artifact: ManagedArtifact::SymlinkReference {
                    path: link.clone(),
                    target: target.clone(),
                },
            },
        )
        .unwrap();

        let report = app.maintain_environment();
        assert!(
            report.outcomes.iter().any(|outcome| matches!(
                outcome,
                MaintenanceOutcome::OrphanCleaned { plugin, .. } if plugin == "old-git"
            )),
            "expected an OrphanCleaned outcome for old-git, got {:?}",
            report.outcomes
        );
        assert!(
            state::receipts(&home, Some("old-git")).unwrap().is_empty(),
            "the orphan's ledger entry must be forgotten"
        );
        assert!(
            !link.exists(),
            "the orphan's physical slot must be freed, not just forgotten in the ledger"
        );

        let _ = fs::remove_dir_all(root);
    }

    struct TestIntegration;

    impl uze_core::integration::IntegrationPort for TestIntegration {
        fn id(&self) -> &'static str {
            "fixture"
        }

        fn capabilities(&self) -> uze_core::router::HarnessCapabilities {
            uze_core::router::HarnessCapabilities::default()
        }

        fn exposure_plan(
            &self,
            _resource: &uze_core::Resource,
        ) -> uze_core::exposure::ExposurePlan {
            panic!("not used")
        }
    }
}
