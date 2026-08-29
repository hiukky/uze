//! Safe machine-level desired-state convergence for `doctor` and the TUI.
//!
//! This is deliberately narrower than install/update: it neither acquires
//! bytes nor expands trust. A receipt must first inspect as `Missing`, then
//! the owning integration must prove it can rebuild that exact artifact.

use serde::Serialize;

use uze_core::{
    integration::{AttachmentState, ManagedArtifact, PublicationStatus},
    persistence::MutationLock,
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
            .filter(|outcome| matches!(outcome, MaintenanceOutcome::Repaired { .. }))
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
