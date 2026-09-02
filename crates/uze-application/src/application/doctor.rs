//! Doctor/status — extracted without semantic change.

#![allow(clippy::empty_line_after_doc_comments)]

use uze_core::{Result, integration::AttachmentState, state};

use super::services::Health;
use super::*;

impl Health<'_> {
    /// Full machine diagnostics: Store/state errors, harness detection,
    /// plugin summaries, and per-receipt attachment inspection. The
    /// inspection half is backed by:
    ///
    /// - the in-process + on-disk inspection cache for `Matched` verdicts
    ///   (ADR 018): steady-state runs are milliseconds;
    /// - always-live re-inspection for anomalies, so a warning is never
    ///   stale;
    /// - cache invalidation on every mutation, so a verdict never outlives
    ///   the change that produced the state it describes.
    ///
    /// The only slow path is a cold cache (one vendor-CLI probe per
    /// receipt), which is exactly the honest cost of the first evidence —
    /// paid once per TTL window, not on every screen.
    pub fn report(&self) -> DoctorReport {
        let maintenance = self.maintain();
        let mut report = self.doctor_shell();
        let attachments = report
            .plugins
            .iter()
            .map(|plugin: &PluginSummary| {
                let reconciliation = self.0.reconcile_cached_report(&plugin.id);
                PackageManagedState {
                    plugin: plugin.id.clone(),
                    state: managed_state(&reconciliation),
                    hooks: self.hook_health(&reconciliation),
                }
            })
            .collect();
        report.attachments = attachments;
        report.maintenance = maintenance;
        report
    }

    /// Per-package hook rows for `doctor`: every canonical hook group ×
    /// every harness, with the semantic verdict (native/adapted/degraded/
    /// unsupported), the exact guarantee that is weakened when it is, and
    /// the receipt-owned artifact and its attachment state when the hook is
    /// actually attached (ADR-033 / doctor spec: a degraded hook must be
    /// actionable, never hidden behind a healthy-native row).
    fn hook_health(&self, reconciliation: &ReconciliationReport) -> Vec<HookHealth> {
        use uze_core::{hook::PortableHook, integration::receipt_location, store::PackageId};
        let Ok(id) = PackageId::from_qualified(
            &reconciliation.package_id,
            std::path::Path::new("plugin.json"),
        ) else {
            return Vec::new();
        };
        let Ok(environment) = self.0.engine().compose(std::slice::from_ref(&id)) else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        for resource in environment
            .resources
            .iter()
            .filter(|resource| resource.capability.kind == CapabilityKind::Hook)
        {
            let Ok(hook) = serde_json::from_slice::<PortableHook>(&resource.capability.payload)
            else {
                continue;
            };
            let identity = resource.identity();
            for integration in &self.0.integrations {
                let plan = integration.exposure_plan(resource);
                let attached = reconciliation.receipts.iter().find(|entry| {
                    entry.receipt.integration == integration.id()
                        && entry.receipt.resource_identity.as_deref() == Some(identity.as_str())
                });
                rows.push(HookHealth {
                    hook: hook.id.clone(),
                    event: hook.event.abi_name().to_owned(),
                    harness: integration.id().to_owned(),
                    route: plan.route,
                    // A degraded or unsupported route must state the exact
                    // semantic loss, never hide it behind a healthy verdict.
                    weakened: match plan.route {
                        CompatibilityRoute::Degraded | CompatibilityRoute::Unsupported => {
                            match &plan.mechanism {
                                ExposureMechanism::Unsupported { rationale } => {
                                    Some(rationale.clone())
                                }
                                _ => None,
                            }
                        }
                        _ => None,
                    },
                    artifact: attached.map(|entry| receipt_location(&entry.receipt)),
                    state: attached.map(|entry| entry.inspection.state),
                });
            }
        }
        rows.sort_by(|left, right| (&left.hook, &left.harness).cmp(&(&right.hook, &right.harness)));
        rows
    }

    /// The cheap half of `doctor` — everything except per-receipt
    /// attachment inspection (`attachments` left empty). Shared by
    /// [`doctor`](Self::doctor), which adds the (cached) inspection layer.
    fn doctor_shell(&self) -> DoctorReport {
        let package_ids = self.0.store.package_ids();
        let (store, plugins) = match package_ids {
            Ok(ids) => {
                let packages = ids
                    .into_iter()
                    .filter_map(|id| self.0.store.package(&id).ok())
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
                        .filter_map(|package| self.0.plugin_summary(package).ok())
                        .collect(),
                )
            }
            Err(error) => (StoreHealth::Blocked(error.to_string()), Vec::new()),
        };
        let harnesses = self.harness_health();
        let ledger_error = state::receipts(&self.0.home, None)
            .err()
            .map(|error| error.to_string());
        let integration_state_error = state::load(&self.0.home)
            .err()
            .map(|error| error.to_string());
        let provisioning_state_error = self
            .0
            .integrations
            .iter()
            .find_map(|integration| state::provisioning(&self.0.home, integration.id()).err())
            .map(|error| error.to_string());
        DoctorReport {
            uze_home: self.0.home.root().to_path_buf(),
            store,
            plugins,
            harnesses,
            attachments: Vec::new(),
            ledger_error,
            integration_state_error,
            provisioning_state_error,
            maintenance: MaintenanceReport::default(),
        }
    }

    /// Per-harness detection/setup/provisioning detail — shared by
    /// `doctor()` (the full report) and `harness_list`/`harness_inspect`
    /// (the machine-level `harness` namespace's thin read models, which
    /// slice this same computation rather than adding a second one).
    fn harness_health(&self) -> Vec<HarnessHealth> {
        let installed = self.0.installed_packages();
        self.0
            .integrations
            .iter()
            .map(|integration| HarnessHealth {
                integration: integration.id().to_owned(),
                display_name: integration.display_name().to_owned(),
                description: integration.description().to_owned(),
                detection: self.0.detect_cached(integration.as_ref()),
                setup: integration_status(integration.status(&self.0.home)),
                strategy: state::get(&self.0.home, integration.id())
                    .ok()
                    .flatten()
                    .map(|record| record.strategy),
                provisioning: state::provisioning(&self.0.home, integration.id())
                    .ok()
                    .flatten(),
                // Observed, not remembered. A package can be installed and
                // reconciled while a harness still cannot see it, and that is
                // exactly the state this field exists to surface.
                publication: integration.publication(&installed),
                capabilities: integration.capabilities(),
                runtime_shim_active: self.0.runtime_shim_is_active(integration.as_ref()),
            })
            .collect()
    }

    pub fn harnesses(&self) -> Vec<HarnessHealth> {
        self.harness_health()
    }

    /// Matches by the stable integration id (`claude-code`), any alias
    /// people actually type (`claude`), or the display label doctor shows
    /// back (`Claude Code`) — the same names `uze setup` accepts plus what
    /// `uze doctor`/the TUI print.
    pub fn harness(&self, name: &str) -> Result<HarnessHealth> {
        let id = self
            .0
            .integrations
            .iter()
            .find(|integration| {
                integration.id() == name
                    || integration.aliases().contains(&name)
                    || integration.display_name() == name
            })
            .map(|integration| integration.id());
        self.harness_health()
            .into_iter()
            .find(|harness| Some(harness.integration.as_str()) == id)
            .ok_or_else(|| {
                uze_core::UzeError::UnknownPackage(format!("harness `{name}` not found"))
            })
    }

    /// The human label for an integration id (`claude-code` → `Claude
    /// Code`), for text renders whose read models carry only the stable id.
    /// An id that belongs to no registered integration renders as itself —
    /// a label lookup must never fail a display.
    pub fn integration_label(&self, integration: &str) -> String {
        self.0
            .integrations
            .iter()
            .find(|candidate| candidate.id() == integration)
            .map_or_else(
                || integration.to_owned(),
                |candidate| candidate.display_name().to_owned(),
            )
    }

    pub fn status(&self, project_root: &std::path::Path) -> Result<StatusReport> {
        let context = self.0.context().inspect(project_root)?;
        let installed = self.0.store.package_ids()?.len();
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
                            Some(format!("{}: bridge {:?}", harness.display_name, state))
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
            project_lock: self.0.project().lock_status(project_root),
            issues,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        fs,
        path::Path,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
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
    /// the inspection cache depends on (ADR 018): Matched is cached,
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
        let base = uze_testkit::temp::scratch("fast-vs-deep");
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
                    detail: BTreeMap::default(),
                },
            },
        )
        .unwrap();

        let first = app.health().report();
        assert_eq!(first.attachments.len(), 1);
        assert_eq!(inspected.load(Ordering::SeqCst), 1, "one cold inspection");

        // Same instance: the in-process tier serves the verdict.
        let _ = app.health().report();
        assert_eq!(inspected.load(Ordering::SeqCst), 1);

        // A fresh instance (e.g. the next TUI refresh): the on-disk tier
        // serves it — no vendor CLI re-spawned.
        let fresh = app_with_counter(&home, &inspected, AttachmentState::Matched);
        let _ = fresh.health().report();
        assert_eq!(
            inspected.load(Ordering::SeqCst),
            1,
            "second invocation must not re-inspect a fresh Matched verdict"
        );
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn anomalies_are_never_cached_and_reinspected_every_time() {
        let base = uze_testkit::temp::scratch("anomaly");
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
                    detail: BTreeMap::default(),
                },
            },
        )
        .unwrap();

        let report = app.health().report();
        assert_eq!(report.attachments[0].state.drifted, 1);
        assert_eq!(inspected.load(Ordering::SeqCst), 1);
        let _ = app.health().report();
        assert_eq!(
            inspected.load(Ordering::SeqCst),
            2,
            "a drifted verdict must be re-checked live on every read"
        );
        // And it never persisted: a fresh instance re-inspects too.
        let fresh = app_with_counter(&home, &inspected, AttachmentState::Drifted);
        let _ = fresh.health().report();
        assert_eq!(inspected.load(Ordering::SeqCst), 3);
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn installation_invalidates_the_inspection_cache() {
        let base = uze_testkit::temp::scratch("invalidate-on-install");
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
                    detail: BTreeMap::default(),
                },
            },
        )
        .unwrap();

        let _ = app.health().report();
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
        let _ = app.health().report();
        assert_eq!(
            inspected.load(Ordering::SeqCst),
            2,
            "a mutation must invalidate cached inspection verdicts"
        );
        fs::remove_dir_all(&base).ok();
    }
}

impl UzeApplication {
    pub(super) fn runtime_shim_is_active(&self, integration: &dyn IntegrationPort) -> bool {
        if !integration.supports_runtime_integration() {
            return true;
        }
        let expected = self.home.shims_dir().join(integration.shim_name());
        std::env::var_os("PATH")
            .and_then(|path| {
                std::env::split_paths(&path)
                    .map(|directory| directory.join(integration.shim_name()))
                    .find(|candidate| candidate.is_file())
            })
            // Canonicalized comparison: a PATH entry that reaches the shim
            // through a symlinked directory (or a shim file that is itself
            // a symlink into the UZE install — the normal `~/.uze/shims`
            // case) must count as active, not merely byte-equal paths.
            .is_some_and(|resolved| resolved.canonicalize().ok() == expected.canonicalize().ok())
    }
}
