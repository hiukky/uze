//! Doctor/status — extracted without semantic change.

#![allow(clippy::empty_line_after_doc_comments)]

use uze_core::{
    Result,
    integration::{AttachmentState, ContextDelivery},
    state,
};

use super::*;

impl UzeApplication {
    /// Full machine diagnostics: Store/state errors, harness detection,
    /// plugin summaries, and per-receipt attachment inspection. The
    /// inspection half is backed by:
    ///
    /// - the in-process + on-disk inspection cache for `Matched` verdicts
    ///   (ADR 024): steady-state runs are milliseconds;
    /// - always-live re-inspection for anomalies, so a warning is never
    ///   stale;
    /// - cache invalidation on every mutation, so a verdict never outlives
    ///   the change that produced the state it describes.
    ///
    /// The only slow path is a cold cache (one vendor-CLI probe per
    /// receipt), which is exactly the honest cost of the first evidence —
    /// paid once per TTL window, not on every screen.
    pub fn doctor(&self) -> DoctorReport {
        let mut report = self.doctor_shell();
        let attachments = report
            .plugins
            .iter()
            .map(|plugin: &PluginSummary| PackageManagedState {
                plugin: plugin.id.clone(),
                state: managed_state(&self.reconcile_cached_report(&plugin.id)),
            })
            .collect();
        report.attachments = attachments;
        report
    }

    /// The cheap half of `doctor` — everything except per-receipt
    /// attachment inspection (`attachments` left empty). Shared by
    /// [`doctor`](Self::doctor), which adds the (cached) inspection layer.
    fn doctor_shell(&self) -> DoctorReport {
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
        let harnesses = self.harness_health();
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
            attachments: Vec::new(),
            ledger_error,
            integration_state_error,
            provisioning_state_error,
        }
    }

    /// Per-harness detection/setup/provisioning detail — shared by
    /// `doctor()` (the full report) and `harness_list`/`harness_inspect`
    /// (the machine-level `harness` namespace's thin read models, which
    /// slice this same computation rather than adding a second one).
    fn harness_health(&self) -> Vec<HarnessHealth> {
        let installed = self.installed_packages();
        self.integrations
            .iter()
            .map(|integration| HarnessHealth {
                integration: integration.id().to_owned(),
                display_name: integration.display_name().to_owned(),
                detection: self.detect_cached(integration.as_ref()),
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
                capabilities: integration.capabilities(),
                native_instructions: matches!(
                    integration.context_delivery(),
                    ContextDelivery::Native { .. }
                ),
            })
            .collect()
    }

    pub fn harness_list(&self) -> Vec<HarnessHealth> {
        self.harness_health()
    }

    /// Matches by either the stable integration id (`claude-code`) or the
    /// display name people actually type (`claude`) — the same two names
    /// `uze doctor`'s own output already shows side by side.
    pub fn harness_inspect(&self, name: &str) -> Result<HarnessHealth> {
        self.harness_health()
            .into_iter()
            .find(|harness| harness.integration == name || harness.display_name == name)
            .ok_or_else(|| {
                uze_core::UzeError::UnknownPackage(format!("harness `{name}` not found"))
            })
    }

    pub fn status(&self, project_root: &std::path::Path) -> Result<StatusReport> {
        let context = self.context_inspect(project_root)?;
        let installed = self.store.package_ids()?.len();
        let contributing = context.contributions.len();
        let issues: Vec<String> = context
            .contributions
            .iter()
            .filter(|contribution| !matches!(contribution.state, AttachmentState::Matched))
            .map(|contribution| format!("{}: {:?}", contribution.package_id, contribution.state))
            .chain(
                context
                    .harnesses
                    .iter()
                    .filter_map(|harness| match &harness.delivery {
                        HarnessContextDelivery::Bridge {
                            needed: true,
                            state,
                        } if *state != AttachmentState::Matched => {
                            Some(format!("{}: bridge {:?}", harness.integration, state))
                        }
                        _ => None,
                    }),
            )
            .chain(
                context
                    .malformed_regions
                    .iter()
                    .map(|region| format!("{region}: malformed")),
            )
            .collect();
        Ok(StatusReport {
            root: context.canonical.clone(),
            portability: context.portability,
            harnesses: context.harnesses,
            packages_installed: installed,
            packages_contributing_here: contributing,
            project_lock: self.project_lock_status(project_root),
            issues,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::{SystemTime, UNIX_EPOCH},
    };

    use uze_core::{
        PackageSource, UzeHome,
        exposure::ExposurePlan,
        integration::{
            AttachmentInspection, AttachmentReceipt, AttachmentState, HarnessDetection,
            ManagedArtifact,
        },
        router::HarnessCapabilities,
        state,
        store::PackageId,
        trust::AlwaysTrust,
    };

    use super::*;

    /// An integration that counts every `inspect_receipt` call and reports
    /// a configurable verdict. The matcher-vs-anomaly distinction is what
    /// the inspection cache depends on (ADR 024): Matched is cached,
    /// anomalies are always re-inspected.
    struct CountingInspection {
        inspected: Arc<AtomicUsize>,
        verdict: AttachmentState,
    }

    impl CountingInspection {
        fn counting(verdict: AttachmentState) -> (Self, Arc<AtomicUsize>) {
            let inspected = Arc::new(AtomicUsize::new(0));
            (
                Self {
                    inspected: inspected.clone(),
                    verdict,
                },
                inspected,
            )
        }
    }
    impl IntegrationPort for CountingInspection {
        fn id(&self) -> &'static str {
            "counting"
        }
        fn capabilities(&self) -> HarnessCapabilities {
            HarnessCapabilities::default()
        }
        fn exposure_plan(&self, _resource: &uze_core::project::Resource) -> ExposurePlan {
            ExposurePlan {
                representation: uze_core::capability::Representation::Standard,
                route: uze_core::router::CompatibilityRoute::Unsupported,
                verification: uze_core::router::VerificationStatus::Unverified,
                mechanism: uze_core::exposure::ExposureMechanism::Unsupported {
                    rationale: "test does not attach".to_owned(),
                },
                evidence: "test".to_owned(),
            }
        }
        fn detect(&self) -> HarnessDetection {
            HarnessDetection::default()
        }
        fn inspect_receipt(&self, _receipt: &AttachmentReceipt) -> AttachmentInspection {
            self.inspected.fetch_add(1, Ordering::SeqCst);
            AttachmentInspection {
                state: self.verdict,
                reason: "test".to_owned(),
            }
        }
    }

    fn temp(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("uze-doctor-{label}-{}-{nonce}", std::process::id()))
    }

    fn write_plugin(root: &Path, name: &str) {
        let dir = root.join(name);
        fs::create_dir_all(dir.join("skills/uze-test")).unwrap();
        fs::write(dir.join("plugin.json"), format!(r#"{{"name": "{name}"}}"#)).unwrap();
        fs::write(dir.join("skills/uze-test/SKILL.md"), "# Test skill\n").unwrap();
    }

    /// An app whose only integration is a fresh counting one sharing the
    /// same `Arc` — so "another invocation" (a new `UzeApplication`) still
    /// observes every live `inspect_receipt` call.
    fn app_with_counter(
        home: &UzeHome,
        inspected: &Arc<AtomicUsize>,
        verdict: AttachmentState,
    ) -> UzeApplication {
        UzeApplication::new(
            home.clone(),
            vec![Box::new(CountingInspection {
                inspected: inspected.clone(),
                verdict,
            })],
        )
    }

    #[test]
    fn matched_inspection_is_cached_across_instances() {
        let base = temp("fast-vs-deep");
        let home = UzeHome::at(base.join("home"));
        let inspected = Arc::new(AtomicUsize::new(0));
        let app = UzeApplication::new(
            home.clone(),
            vec![Box::new(CountingInspection {
                inspected: inspected.clone(),
                verdict: AttachmentState::Matched,
            })],
        );
        let package_root = base.join("flow");
        write_plugin(&base, "flow");
        app.add_plugin(
            PackageSource::Local {
                path: package_root.clone(),
            },
            &AlwaysTrust,
        )
        .unwrap();
        let package_id =
            PackageId::from_plugin_name("flow", &package_root.join("plugin.json")).unwrap();
        state::record_receipt(
            &home,
            "flow:counting:native".to_owned(),
            AttachmentReceipt {
                package_id: package_id.as_str().to_owned(),
                resource_identity: None,
                integration: "counting".to_owned(),
                strategy: "test".to_owned(),
                artifact: ManagedArtifact::IntegrationOwned {
                    kind: "test".to_owned(),
                    selector: "flow".to_owned(),
                    detail: Default::default(),
                },
            },
        )
        .unwrap();

        let first = app.doctor();
        assert_eq!(first.attachments.len(), 1);
        assert_eq!(inspected.load(Ordering::SeqCst), 1, "one cold inspection");

        // Same instance: the in-process tier serves the verdict.
        let _ = app.doctor();
        assert_eq!(inspected.load(Ordering::SeqCst), 1);

        // A fresh instance (e.g. the next TUI refresh): the on-disk tier
        // serves it — no vendor CLI re-spawned.
        let fresh = app_with_counter(&home, &inspected, AttachmentState::Matched);
        let _ = fresh.doctor();
        assert_eq!(
            inspected.load(Ordering::SeqCst),
            1,
            "second invocation must not re-inspect a fresh Matched verdict"
        );
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn anomalies_are_never_cached_and_reinspected_every_time() {
        let base = temp("anomaly");
        let home = UzeHome::at(base.join("home"));
        let (integration, inspected) = CountingInspection::counting(AttachmentState::Drifted);
        let app = UzeApplication::new(home.clone(), vec![Box::new(integration)]);
        let package_root = base.join("flow");
        write_plugin(&base, "flow");
        app.add_plugin(
            PackageSource::Local {
                path: package_root.clone(),
            },
            &AlwaysTrust,
        )
        .unwrap();
        let package_id =
            PackageId::from_plugin_name("flow", &package_root.join("plugin.json")).unwrap();
        state::record_receipt(
            &home,
            "flow:counting:native".to_owned(),
            AttachmentReceipt {
                package_id: package_id.as_str().to_owned(),
                resource_identity: None,
                integration: "counting".to_owned(),
                strategy: "test".to_owned(),
                artifact: ManagedArtifact::IntegrationOwned {
                    kind: "test".to_owned(),
                    selector: "flow".to_owned(),
                    detail: Default::default(),
                },
            },
        )
        .unwrap();

        let report = app.doctor();
        assert_eq!(report.attachments[0].state.drifted, 1);
        assert_eq!(inspected.load(Ordering::SeqCst), 1);
        let _ = app.doctor();
        assert_eq!(
            inspected.load(Ordering::SeqCst),
            2,
            "a drifted verdict must be re-checked live on every read"
        );
        // And it never persisted: a fresh instance re-inspects too.
        let fresh = app_with_counter(&home, &inspected, AttachmentState::Drifted);
        let _ = fresh.doctor();
        assert_eq!(inspected.load(Ordering::SeqCst), 3);
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn installation_invalidates_the_inspection_cache() {
        let base = temp("invalidate-on-install");
        let home = UzeHome::at(base.join("home"));
        let (integration, inspected) = CountingInspection::counting(AttachmentState::Matched);
        let app = UzeApplication::new(home.clone(), vec![Box::new(integration)]);
        let package_root = base.join("flow");
        write_plugin(&base, "flow");
        app.add_plugin(
            PackageSource::Local {
                path: package_root.clone(),
            },
            &AlwaysTrust,
        )
        .unwrap();
        let package_id =
            PackageId::from_plugin_name("flow", &package_root.join("plugin.json")).unwrap();
        state::record_receipt(
            &home,
            "flow:counting:native".to_owned(),
            AttachmentReceipt {
                package_id: package_id.as_str().to_owned(),
                resource_identity: None,
                integration: "counting".to_owned(),
                strategy: "test".to_owned(),
                artifact: ManagedArtifact::IntegrationOwned {
                    kind: "test".to_owned(),
                    selector: "flow".to_owned(),
                    detail: Default::default(),
                },
            },
        )
        .unwrap();

        let _ = app.doctor();
        assert_eq!(inspected.load(Ordering::SeqCst), 1);

        // Installing another package is a mutation: cached verdicts must
        // not outlive it.
        write_plugin(&base, "std");
        app.add_plugin(
            PackageSource::Local {
                path: base.join("std"),
            },
            &AlwaysTrust,
        )
        .unwrap();
        let _ = app.doctor();
        assert_eq!(
            inspected.load(Ordering::SeqCst),
            2,
            "a mutation must invalidate cached inspection verdicts"
        );
        fs::remove_dir_all(&base).ok();
    }
}
