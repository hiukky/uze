//! Behavioural tests for `UzeApplication`.
//!
//! Moved out of `application.rs` verbatim: at 1.6k lines they were half
//! that file, and none of them are read while working on the production
//! surface they cover.

use std::{
    cell::Cell,
    fs,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use super::*;
use uze_core::{
    capability::CapabilityKind,
    exposure::{ExposureMechanism, ExposurePlan},
    integration::{AttachmentReceipt, HarnessDetection, ManagedArtifact},
    project::Resource,
    router::{CompatibilityRoute, HarnessCapabilities, VerificationStatus},
};

/// `setup` probes `$SHELL` (`shell_path::detect_shell_rc`) to decide
/// whether to append a PATH line to the *operator's real* shell rc
/// file — by design, never mocked, since it edits the interactive
/// shell the developer actually uses (see `shell_path`'s own module
/// doc: "never invoked implicitly"). Calling `UzeApplication::setup`
/// in-process, as these tests do, is exactly the invocation shape
/// that check can't tell apart from a real `uze setup` run — it would
/// otherwise edit the real `~/.zshrc`/`~/.bashrc` on whatever machine
/// runs this test. Blanking `$SHELL` to an unrecognized value makes
/// `detect_shell_rc` return `None`, so `setup` falls back to its
/// manual-instruction path and never opens any file outside `home`.
fn setup_without_touching_the_real_shell_rc(
    app: &UzeApplication,
    requested: Option<&str>,
) -> Result<Vec<SetupResult>> {
    uze_testkit::env::with_env_var("SHELL", "uze-test-no-recognized-shell", || {
        app.setup(requested)
    })
}

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

struct AbsentIntegration {
    attach_attempted: Cell<bool>,
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

    fn detect(&self) -> HarnessDetection {
        HarnessDetection {
            present: true,
            version: None,
        }
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
        {
            let already_correct = fs::read_link(&path)
                .map(|target| target == resource.capability.path)
                .unwrap_or(false);
            if !already_correct {
                if path.symlink_metadata().is_ok() {
                    fs::remove_file(&path).map_err(|source| UzeError::Write {
                        path: path.clone(),
                        source,
                    })?;
                }
                std::os::unix::fs::symlink(&resource.capability.path, &path).map_err(|source| {
                    UzeError::Write {
                        path: path.clone(),
                        source,
                    }
                })?;
            }
        }
        Ok(Some(AttachmentReceipt {
            package_id: match &resource.origin {
                uze_core::ResourceOrigin::Package { id, .. } => id.as_str().to_owned(),
                uze_core::ResourceOrigin::Project { .. } => unreachable!(),
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

    fn detect(&self) -> HarnessDetection {
        HarnessDetection {
            present: true,
            version: None,
        }
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
                uze_core::ResourceOrigin::Project { .. } => unreachable!(),
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

impl IntegrationPort for AbsentIntegration {
    fn id(&self) -> &'static str {
        "absent"
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
                rationale: "an absent integration must not attach".to_owned(),
            },
            evidence: "test".to_owned(),
        }
    }

    fn attach_receipt(&self, _resource: &Resource) -> Result<Option<AttachmentReceipt>> {
        self.attach_attempted.set(true);
        Err(UzeError::ExposureUnavailable(
            "absent integration was invoked".to_owned(),
        ))
    }
}

pub(crate) fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/_fixtures/canonical/skill-plugin")
}

pub(crate) fn multi_mcp_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/_fixtures/canonical/multi-mcp-plugin")
}

#[test]
pub(crate) fn list_and_inspect_are_package_centric() {
    let root = uze_testkit::temp::scratch("inspect");
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

#[test]
pub(crate) fn add_installs_portable_package_without_invoking_absent_harnesses() {
    let root = uze_testkit::temp::scratch("absent-harness");
    let absent = AbsentIntegration {
        attach_attempted: Cell::new(false),
    };
    let app = UzeApplication::new(UzeHome::at(&root), vec![Box::new(absent)]);
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
pub(crate) fn removal_uses_reconciliation_and_preserves_drift() {
    use std::os::unix::fs::symlink;
    let root = uze_testkit::temp::scratch("remove");
    let home = UzeHome::at(&root);
    let app = UzeApplication::new(home.clone(), vec![Box::new(SymlinkIntegration)]);
    let package = app
        .store
        .ingest(
            &uze_core::acquisition::acquire(&uze_core::PackageSource::local(fixture())).unwrap(),
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
                target: expected,
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
            &uze_core::acquisition::acquire(&uze_core::PackageSource::local(fixture())).unwrap(),
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

/// ADR-038 `replace`: the existing active plugin is fully removed and
/// the new install claims the bare name it freed — the happy path with
/// no receipts to make removal unsafe.
#[test]
pub(crate) fn replace_resolution_removes_the_existing_active_plugin_and_installs_the_new_one() {
    let root = uze_testkit::temp::scratch("replace-happy");
    let home = UzeHome::at(&root);
    let app = UzeApplication::new(home.clone(), vec![Box::new(SymlinkIntegration)]);
    let acquired =
        uze_core::acquisition::acquire(&uze_core::PackageSource::local(fixture())).unwrap();
    let alpha = app
        .install_materialized_from_marketplace(
            acquired,
            "alpha",
            &uze_core::trust::AlwaysTrust,
            &[],
            false,
            &uze_core::naming::NoNameCollisionAuthority,
        )
        .unwrap();
    assert_eq!(alpha.plugin.active_name, "uze-agent-skill-conformance");

    let acquired =
        uze_core::acquisition::acquire(&uze_core::PackageSource::local(fixture())).unwrap();
    let beta = app
        .install_materialized_from_marketplace(
            acquired,
            "beta",
            &uze_core::trust::AlwaysTrust,
            &[],
            false,
            &uze_core::naming::FixedResolution(uze_core::naming::NameCollisionResolution::Replace),
        )
        .unwrap();

    assert_eq!(beta.plugin.id, "uze-agent-skill-conformance@beta");
    assert_eq!(beta.plugin.active_name, "uze-agent-skill-conformance");
    assert!(
        app.store
            .package(
                &uze_core::store::PackageId::from_qualified(
                    &alpha.plugin.id,
                    std::path::Path::new("plugin.json"),
                )
                .unwrap()
            )
            .is_err()
    );
    let ids: Vec<_> = app
        .list_plugins()
        .unwrap()
        .into_iter()
        .map(|plugin| plugin.id)
        .collect();
    assert_eq!(ids, vec!["uze-agent-skill-conformance@beta".to_owned()]);
    fs::remove_dir_all(root).unwrap();
}

/// ADR-038 `replace`, unsafe case: the existing active plugin has a
/// drifted receipt, so removing it is not `Safe` — the whole replace
/// aborts with the structured collision error, and the existing plugin
/// is left exactly as it was (never partially detached, never removed).
#[test]
pub(crate) fn replace_resolution_aborts_and_preserves_the_existing_plugin_when_removal_is_blocked()
{
    use std::os::unix::fs::symlink;
    let root = uze_testkit::temp::scratch("replace-blocked");
    let home = UzeHome::at(&root);
    let app = UzeApplication::new(home.clone(), vec![Box::new(SymlinkIntegration)]);
    let acquired =
        uze_core::acquisition::acquire(&uze_core::PackageSource::local(fixture())).unwrap();
    let alpha = app
        .install_materialized_from_marketplace(
            acquired,
            "alpha",
            &uze_core::trust::AlwaysTrust,
            &[],
            false,
            &uze_core::naming::NoNameCollisionAuthority,
        )
        .unwrap();
    let alpha_id = alpha.plugin.id.clone();

    // Foreign content now occupies the managed slot: the receipt inspects
    // Drifted, so `plan_remove` refuses to touch it.
    let managed = root.join("managed");
    let foreign = root.join("foreign");
    fs::create_dir_all(&foreign).unwrap();
    symlink(&foreign, &managed).unwrap();
    state::record_receipt(
        &home,
        "receipt".to_owned(),
        AttachmentReceipt {
            package_id: alpha_id.clone(),
            resource_identity: None,
            integration: "test".to_owned(),
            strategy: "symlink".to_owned(),
            artifact: ManagedArtifact::SymlinkReference {
                path: managed.clone(),
                target: app
                    .store
                    .package(
                        &uze_core::store::PackageId::from_qualified(
                            &alpha_id,
                            std::path::Path::new("plugin.json"),
                        )
                        .unwrap(),
                    )
                    .unwrap()
                    .root,
            },
        },
    )
    .unwrap();

    let acquired =
        uze_core::acquisition::acquire(&uze_core::PackageSource::local(fixture())).unwrap();
    let result = app.install_materialized_from_marketplace(
        acquired,
        "beta",
        &uze_core::trust::AlwaysTrust,
        &[],
        false,
        &uze_core::naming::FixedResolution(uze_core::naming::NameCollisionResolution::Replace),
    );
    assert!(matches!(
        result,
        Err(UzeError::PluginNameCollision { existing, requested, .. })
            if existing == alpha_id && requested == "uze-agent-skill-conformance@beta"
    ));
    // The existing plugin is untouched: still registered, receipt intact,
    // foreign symlink never disturbed.
    assert!(
        app.store
            .package(
                &uze_core::store::PackageId::from_qualified(
                    &alpha_id,
                    std::path::Path::new("plugin.json"),
                )
                .unwrap()
            )
            .is_ok()
    );
    assert_eq!(fs::read_link(&managed).unwrap(), foreign);
    assert!(
        app.store
            .package(
                &uze_core::store::PackageId::from_qualified(
                    "uze-agent-skill-conformance@beta",
                    std::path::Path::new("plugin.json"),
                )
                .unwrap()
            )
            .is_err(),
        "the beta install must never have been ingested"
    );
    fs::remove_dir_all(root).unwrap();
}

/// ADR-038: `update_plugin` re-resolves the source and reinstalls under
/// the same marketplace-qualified id, but must never silently revert an
/// aliased plugin back to its bare plugin name — the alias is a fact
/// about *this* installation, not something an update should erase.
#[test]
pub(crate) fn update_preserves_an_aliased_plugins_active_name() {
    let root = uze_testkit::temp::scratch("update-alias");
    let home = UzeHome::at(&root);
    let app = UzeApplication::new(home.clone(), vec![Box::new(SymlinkIntegration)]);
    let acquired =
        uze_core::acquisition::acquire(&uze_core::PackageSource::local(fixture())).unwrap();
    app.install_materialized_from_marketplace(
        acquired,
        "alpha",
        &uze_core::trust::AlwaysTrust,
        &[],
        false,
        &uze_core::naming::NoNameCollisionAuthority,
    )
    .unwrap();

    let acquired =
        uze_core::acquisition::acquire(&uze_core::PackageSource::local(fixture())).unwrap();
    let beta = app
        .install_materialized_from_marketplace(
            acquired,
            "beta",
            &uze_core::trust::AlwaysTrust,
            &[],
            false,
            &uze_core::naming::FixedResolution(uze_core::naming::NameCollisionResolution::Alias(
                "conformance-beta".to_owned(),
            )),
        )
        .unwrap();
    assert_eq!(beta.plugin.active_name, "conformance-beta");

    let updated = app
        .update_plugin("conformance-beta", &uze_core::trust::AlwaysTrust)
        .unwrap();
    let UpdatePluginReport::Updated { plugin, .. } = updated else {
        panic!("expected Updated, got {updated:?}");
    };
    assert_eq!(plugin.id, "uze-agent-skill-conformance@beta");
    assert_eq!(
        plugin.active_name, "conformance-beta",
        "update must not revert the alias to the bare plugin name"
    );
    // Still resolvable by its alias, and the other package's own bare
    // name is unaffected.
    assert_eq!(
        app.package_by_name("conformance-beta").unwrap().id.as_str(),
        "uze-agent-skill-conformance@beta"
    );
    assert_eq!(
        app.package_by_name("uze-agent-skill-conformance")
            .unwrap()
            .id
            .as_str(),
        "uze-agent-skill-conformance@alpha"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
pub(crate) fn doctor_reports_corrupt_ledger_without_destructive_work() {
    let root = uze_testkit::temp::scratch("doctor");
    let home = UzeHome::at(&root);
    home.ensure_layout().unwrap();
    fs::write(home.state_dir().join("attachments.json"), "bad").unwrap();
    fs::write(home.integrations_state_path(), "bad").unwrap();
    let app = UzeApplication::new(home, vec![Box::new(SymlinkIntegration)]);
    let report = app.doctor();
    assert!(report.ledger_error.is_some());
    assert!(report.integration_state_error.is_some());
    fs::remove_dir_all(root).unwrap();
}

struct NamedIntegration;
impl IntegrationPort for NamedIntegration {
    fn id(&self) -> &'static str {
        "named"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["n"]
    }
    fn display_name(&self) -> &'static str {
        "Named Tool"
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

#[test]
pub(crate) fn harness_inspect_finds_by_id_or_display_name_and_errors_on_unknown() {
    let root = uze_testkit::temp::scratch("harness-inspect");
    // `SymlinkIntegration::id()` is "test"; it declares no `display_name`
    // override, so both default to the same string here — the point is
    // that lookup succeeds through the id path at all.
    let app = UzeApplication::new(
        UzeHome::at(&root),
        vec![Box::new(SymlinkIntegration), Box::new(NamedIntegration)],
    );
    let by_id = app.harness_inspect("test").unwrap();
    assert_eq!(by_id.integration, "test");
    assert!(app.harness_inspect("does-not-exist").is_err());
    // Aliases (what `uze setup` accepts) and the display label (what
    // doctor shows back) both resolve to the same harness as the id.
    let by_alias = app.harness_inspect("n").unwrap();
    assert_eq!(by_alias.integration, "named");
    let by_label = app.harness_inspect("Named Tool").unwrap();
    assert_eq!(by_label.integration, "named");
    // `harness_list` must return exactly the same data `harness_inspect`
    // filters down to one entry from — same underlying computation.
    let listed = app.harness_list();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].integration, by_id.integration);
    fs::remove_dir_all(root).unwrap();
}

#[test]
pub(crate) fn market_inspect_errors_on_an_unregistered_marketplace() {
    let root = uze_testkit::temp::scratch("market-inspect-unknown");
    let app = UzeApplication::new(UzeHome::at(&root), Vec::new());
    assert!(app.market_inspect("does-not-exist").is_err());
    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
pub(crate) fn add_failure_after_a_confirmed_attachment_leaves_reconcilable_ledger_evidence() {
    let root = uze_testkit::temp::scratch("partial-add");
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
            .any(|id| id.as_str() == "multi-mcp-plugin@local")
    );
    let receipts = state::receipts(&home, Some("multi-mcp-plugin@local")).unwrap();
    assert_eq!(receipts.len(), 1);
    assert_eq!(app.doctor().attachments[0].state.matched, 1);
    fs::remove_dir_all(root).unwrap();
}

#[test]
pub(crate) fn remove_is_idempotent_without_claiming_history_for_absent_state() {
    let root = uze_testkit::temp::scratch("remove-twice");
    let app = UzeApplication::new(UzeHome::at(&root), vec![Box::new(SymlinkIntegration)]);
    let package = app
        .store
        .ingest(
            &uze_core::acquisition::acquire(&uze_core::PackageSource::local(fixture())).unwrap(),
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
pub(crate) fn multi_mcp_package_has_independent_receipts_through_safe_removal() {
    let root = uze_testkit::temp::scratch("multi-mcp-lifecycle");
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
    let receipts = state::receipts(&home, Some("multi-mcp-plugin@local")).unwrap();
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
        state::receipts(&home, Some("multi-mcp-plugin@local"))
            .unwrap()
            .is_empty()
    );
    fs::remove_dir_all(root).unwrap();
}

pub(crate) fn mcp_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/_fixtures/canonical/mcp-plugin")
}

// --- Marketplace bootstrap: install-only, never silent-update --------

#[test]
pub(crate) fn bootstrap_installs_exactly_the_default_policy_and_is_idempotent() {
    let root = uze_testkit::temp::scratch("bootstrap-default");
    let app = UzeApplication::new(UzeHome::at(&root), Vec::new());

    assert!(app.ensure_default_plugins().unwrap(), "first call installs");
    let installed: Vec<String> = app
        .list_plugins()
        .unwrap()
        .into_iter()
        .map(|p| p.id)
        .collect();
    let expected: Vec<String> = bootstrap::DEFAULT_PLUGIN_IDS
        .iter()
        .map(|name| format!("{name}@uze-official"))
        .collect();
    assert_eq!(installed, expected);

    assert!(
        !app.ensure_default_plugins().unwrap(),
        "second call installs nothing new"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
pub(crate) fn bootstrap_never_mutates_an_already_installed_default_plugin() {
    let root = uze_testkit::temp::scratch("bootstrap-no-silent-update");
    let app = UzeApplication::new(UzeHome::at(&root), Vec::new());
    app.ensure_default_plugins().unwrap();

    let package = app.package_by_name("uze").unwrap();
    let manifest_path = package.root.join("plugin.json");
    fs::write(&manifest_path, "{\"name\":\"uze\",\"tampered\":true}").unwrap();
    let tampered = fs::read_to_string(&manifest_path).unwrap();

    // A read-only-shaped call (every CLI command runs this) must not
    // touch the tampered content, even though it clearly differs from
    // the embedded snapshot.
    assert!(!app.ensure_default_plugins().unwrap());
    assert_eq!(fs::read_to_string(&manifest_path).unwrap(), tampered);

    // The drift is still visible — read-only, informational.
    let summary = app
        .plugin_summary(&app.package_by_name("uze").unwrap())
        .unwrap();
    assert_eq!(summary.update_available, Some(true));
    fs::remove_dir_all(root).unwrap();
}

#[test]
pub(crate) fn read_only_bootstrap_leaves_store_state_byte_identical_on_repeat() {
    let root = uze_testkit::temp::scratch("bootstrap-snapshot");
    let home = UzeHome::at(&root);
    let app = UzeApplication::new(home.clone(), Vec::new());
    app.ensure_default_plugins().unwrap();

    let packages_before = fs::read(home.registry_path()).unwrap();
    let attachments_path = home.state_dir().join("attachments.json");
    let attachments_before = fs::read(&attachments_path).ok();

    app.ensure_default_plugins().unwrap();
    app.list_plugins().unwrap();
    app.doctor();

    assert_eq!(fs::read(home.registry_path()).unwrap(), packages_before);
    assert_eq!(fs::read(&attachments_path).ok(), attachments_before);
    fs::remove_dir_all(root).unwrap();
}

#[test]
pub(crate) fn existing_receipts_survive_repeated_bootstrap_unchanged() {
    let root = uze_testkit::temp::scratch("bootstrap-receipts");
    let home = UzeHome::at(&root);
    let app = UzeApplication::new(
        home.clone(),
        vec![Box::new(AllResourceSymlinkIntegration {
            root: root.clone(),
        })],
    );
    app.ensure_default_plugins().unwrap();
    let before = state::receipts(&home, Some("uze@uze-official")).unwrap();
    assert!(!before.is_empty());

    app.ensure_default_plugins().unwrap();
    let after = state::receipts(&home, Some("uze@uze-official")).unwrap();
    assert_eq!(before, after);
    fs::remove_dir_all(root).unwrap();
}

#[test]
pub(crate) fn a_default_plugin_that_would_cross_the_trust_boundary_is_not_installed_silently() {
    let root = uze_testkit::temp::scratch("bootstrap-trust");
    let app = UzeApplication::new(UzeHome::at(&root), Vec::new());

    // Simulate a hypothetical embedded snapshot revision that declares
    // an MCP server — exactly the scenario Fase I describes. `Embedded`
    // sources cross the trust boundary (see `crosses_trust_boundary`),
    // so a non-interactive authority must refuse, not install.
    let mut materialized =
        uze_core::acquisition::acquire(&uze_core::PackageSource::local(mcp_fixture())).unwrap();
    let fixture_root = materialized.root().to_path_buf();
    materialized.retarget(
        fixture_root,
        uze_core::Provenance {
            requested: uze_core::PackageSource::Embedded {
                id: "uze-mcp-conformance".to_owned(),
            },
            resolved: uze_core::ResolvedSource::Embedded {
                id: "uze-mcp-conformance".to_owned(),
            },
        },
    );

    let result = app.install_materialized_from_marketplace(
        materialized,
        "local",
        &uze_core::trust::NoTrustAuthority,
        &[],
        false,
        &uze_core::naming::NoNameCollisionAuthority,
    );
    assert!(matches!(result, Err(UzeError::TrustRequired { .. })));
    assert!(
        app.list_plugins().unwrap().is_empty(),
        "nothing was installed"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
pub(crate) fn a_corrupted_stored_copy_reports_unknown_update_status_without_panicking() {
    let root = uze_testkit::temp::scratch("bootstrap-corrupt");
    let app = UzeApplication::new(UzeHome::at(&root), Vec::new());
    app.ensure_default_plugins().unwrap();

    let package = app.package_by_name("uze").unwrap();
    fs::remove_file(package.root.join("plugin.json")).unwrap();

    let summary = app
        .plugin_summary(&app.package_by_name("uze").unwrap())
        .unwrap();
    assert_eq!(summary.update_available, Some(true));

    fs::remove_dir_all(&package.root).unwrap();
    let summary = app
        .plugin_summary(&app.package_by_name("uze").unwrap())
        .unwrap();
    assert_eq!(summary.update_available, None);
    fs::remove_dir_all(root).unwrap();
}

#[test]
pub(crate) fn official_embedded_plugin_is_protected_from_remove_but_allows_update() {
    let root = uze_testkit::temp::scratch("protected-update");
    let home = UzeHome::at(&root);
    let app = UzeApplication::new(home, Vec::new());
    app.add_plugin(
        uze_core::PackageSource::Embedded {
            id: "uze".to_owned(),
        },
        &uze_core::trust::AlwaysTrust,
    )
    .unwrap();

    let err = app.remove_plugin("uze").unwrap_err();
    assert!(
        err.to_string()
            .contains("official marketplace plugin `uze@uze-official` is protected"),
        "expected protected error, got: {err}"
    );

    let report = app
        .update_plugin("uze", &uze_core::trust::AlwaysTrust)
        .unwrap();
    assert!(
        matches!(report, UpdatePluginReport::Updated { .. }),
        "expected Updated, got: {report:?}"
    );
    assert!(app.package_by_name("uze").is_ok());
    fs::remove_dir_all(root).unwrap();
}

#[test]
pub(crate) fn local_spoof_named_uze_is_not_protected() {
    let root = uze_testkit::temp::scratch("spoof-not-protected");
    let spoof_src = uze_testkit::temp::scratch("spoof-src-uze");
    fs::create_dir_all(spoof_src.join("skills/spoof")).unwrap();
    fs::write(
        spoof_src.join("plugin.json"),
        r#"{"name":"uze","description":"spoof","version":"0.1.0"}"#,
    )
    .unwrap();
    fs::write(spoof_src.join("skills/spoof/SKILL.md"), "# Spoof\n").unwrap();

    let home = UzeHome::at(&root);
    let app = UzeApplication::new(home, Vec::new());
    app.add_plugin(
        uze_core::PackageSource::Local {
            path: spoof_src.clone(),
        },
        &uze_core::trust::AlwaysTrust,
    )
    .unwrap();

    let package = app.package_by_name("uze").unwrap();
    assert!(!UzeApplication::is_protected_package(&package));

    let report = app.remove_plugin("uze").unwrap();
    assert!(matches!(report, RemovePluginReport::Removed { .. }));
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(spoof_src).unwrap();
}

// --- cli-performance: detect_cached / DetectionCache integration ---
// See ADR 018 and specs/cli-performance/spec.md. `FakeIntegration`
// stands in for a slow harness (a real vendor `--version` probe costs
// seconds) without spawning a real subprocess, and its
// shared `Arc<AtomicUsize>` counter is what these tests assert
// against: the whole point of `detect_cached` is that this counter
// stays at 1 no matter how many call sites, command executions, or
// (simulated) CLI invocations ask for the same integration's
// detection result.

struct FakeIntegration {
    id: &'static str,
    detection: HarnessDetection,
    delay: Duration,
    calls: Arc<AtomicUsize>,
}

impl FakeIntegration {
    fn new(id: &'static str, present: bool, calls: Arc<AtomicUsize>) -> Self {
        Self {
            id,
            detection: HarnessDetection {
                present,
                version: present.then(|| "1.0.0".to_owned()),
            },
            delay: Duration::ZERO,
            calls,
        }
    }

    fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }
}

impl IntegrationPort for FakeIntegration {
    fn id(&self) -> &'static str {
        self.id
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities::default()
    }

    fn detect(&self) -> HarnessDetection {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if !self.delay.is_zero() {
            std::thread::sleep(self.delay);
        }
        self.detection.clone()
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

#[test]
fn detect_cached_calls_detect_at_most_once_per_command() {
    let root = uze_testkit::temp::scratch("detect-cached-once-per-command");
    let calls = Arc::new(AtomicUsize::new(0));
    let fake = FakeIntegration::new("fake-a", true, calls.clone());
    let app = UzeApplication::new(UzeHome::at(&root), vec![Box::new(fake)]);
    let integration = app.integrations[0].as_ref();

    let _ = app.detect_cached(integration);
    let _ = app.detect_cached(integration);
    let _ = app.detect_cached(integration);

    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "three calls within one UzeApplication (one command) must probe only once"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn detect_cached_reuses_the_on_disk_result_across_separate_uze_application_instances() {
    // A fresh `UzeApplication` instance stands in for a separate CLI
    // invocation: it shares nothing in-process with the first, only
    // the on-disk cache file under the same `UzeHome`.
    let root = uze_testkit::temp::scratch("detect-cached-cross-invocation");
    let calls = Arc::new(AtomicUsize::new(0));

    let first = UzeApplication::new(
        UzeHome::at(&root),
        vec![Box::new(FakeIntegration::new(
            "fake-b",
            true,
            calls.clone(),
        ))],
    );
    let _ = first.detect_cached(first.integrations[0].as_ref());

    let second = UzeApplication::new(
        UzeHome::at(&root),
        vec![Box::new(FakeIntegration::new(
            "fake-b",
            true,
            calls.clone(),
        ))],
    );
    let _ = second.detect_cached(second.integrations[0].as_ref());

    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "the second UzeApplication instance must reuse the first's on-disk result"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn prepare_detected_integrations_probes_each_integration_at_most_once() {
    // Regression test for the bug found while measuring end-to-end
    // timing (design.md decision 7): `install()` used to call
    // `self.detect()` again internally, on top of the one
    // `detect_cached` call `prepare_detected_integrations` already
    // made — two live probes per integration instead of one.
    let root = uze_testkit::temp::scratch("prepare-detected-once");
    let calls = Arc::new(AtomicUsize::new(0));
    let fake = FakeIntegration::new("fake-c", true, calls.clone());
    let app = UzeApplication::new(UzeHome::at(&root), vec![Box::new(fake)]);

    let results = app.prepare_detected_integrations(None).unwrap();

    assert!(results[0].configured);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "prepare_detected_integrations must probe each integration exactly once, \
             including the install() step it triggers"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn provision_and_prepare_writes_through_the_cache_on_success() {
    let root = uze_testkit::temp::scratch("write-through-on-provision");
    let calls = Arc::new(AtomicUsize::new(0));
    let fake = FakeIntegration::new("fake-d", true, calls.clone());
    let app = UzeApplication::new(UzeHome::at(&root), vec![Box::new(fake)]);

    let results = app.provision_and_prepare(None).unwrap();
    assert!(results[0].configured);
    let calls_after_provision = calls.load(Ordering::SeqCst);
    assert!(calls_after_provision >= 1);

    // The write-through (ADR 018 decision 3) means a `detect_cached`
    // call right after observes the fresh result without an extra
    // live probe.
    let _ = app.detect_cached(app.integrations[0].as_ref());
    assert_eq!(
        calls.load(Ordering::SeqCst),
        calls_after_provision,
        "detect_cached after a successful provision must not re-probe"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn cache_warm_detect_cached_meets_the_performance_budget() {
    // Stands in for a real vendor `--version` probe's second-scale cost
    // without spawning a subprocess (see proposal.md's measurements).
    const SLOW_HARNESS_DELAY: Duration = Duration::from_millis(500);
    const BUDGET: Duration = Duration::from_millis(50);

    let root = uze_testkit::temp::scratch("perf-budget");
    let calls = Arc::new(AtomicUsize::new(0));

    {
        let fake = FakeIntegration::new("slow-harness", true, calls.clone())
            .with_delay(SLOW_HARNESS_DELAY);
        let app = UzeApplication::new(UzeHome::at(&root), vec![Box::new(fake)]);
        let integration = app.integrations[0].as_ref();

        // Cold: pays the simulated delay once, populates both cache
        // tiers.
        let _ = app.detect_cached(integration);

        // Warm, same command: in-process memoization tier.
        let started = Instant::now();
        let _ = app.detect_cached(integration);
        let elapsed = started.elapsed();
        assert!(
            elapsed < BUDGET,
            "warm in-process detect_cached took {elapsed:?}, budget is {BUDGET:?}"
        );
    }

    {
        // A fresh UzeApplication simulates a separate CLI invocation:
        // only the on-disk tier is available, no in-process memo.
        let fake = FakeIntegration::new("slow-harness", true, calls.clone())
            .with_delay(SLOW_HARNESS_DELAY);
        let app = UzeApplication::new(UzeHome::at(&root), vec![Box::new(fake)]);
        let integration = app.integrations[0].as_ref();

        let started = Instant::now();
        let _ = app.detect_cached(integration);
        let elapsed = started.elapsed();
        assert!(
            elapsed < BUDGET,
            "warm cross-invocation detect_cached took {elapsed:?}, budget is {BUDGET:?}"
        );
    }

    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "the simulated slow probe must be paid exactly once across the cold call \
             and both warm reads (in-process and cross-invocation)"
    );
    let _ = fs::remove_dir_all(&root);
}

// --- Production resilience: `setup` never aborts the whole run on one harness ---
//
// Real user machines are not fresh temp dirs: a same-name Antigravity
// plugin imported outside UZE, a drifted symlink, or a shim conflict must
// surface as a per-harness warning, not as a fatal `uze: ...` that aborts
// the entire `setup` and leaves other harnesses half-configured.
// These tests replicate those production anomalies deterministically
// without a real `agy` binary, via minimal fake integrations.

struct HealthySymlinkIntegration {
    root: PathBuf,
}

impl IntegrationPort for HealthySymlinkIntegration {
    fn id(&self) -> &'static str {
        "healthy-harness"
    }
    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities::default()
    }
    fn detect(&self) -> HarnessDetection {
        HarnessDetection {
            present: true,
            version: Some("9.9.9".to_owned()),
        }
    }
    fn exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        ExposurePlan {
            representation: resource.capability.representation,
            route: CompatibilityRoute::Adaptable,
            verification: VerificationStatus::Unverified,
            mechanism: ExposureMechanism::Unsupported {
                rationale: "healthy test does not use exposure_plan".to_owned(),
            },
            evidence: "test".to_owned(),
        }
    }
    fn attach_receipt(&self, resource: &Resource) -> Result<Option<AttachmentReceipt>> {
        let path = self.root.join(resource.name());
        #[cfg(unix)]
        {
            let already_correct = fs::read_link(&path)
                .map(|target| target == resource.capability.path)
                .unwrap_or(false);
            if !already_correct {
                if path.symlink_metadata().is_ok() {
                    fs::remove_file(&path).map_err(|source| UzeError::Write {
                        path: path.clone(),
                        source,
                    })?;
                }
                std::os::unix::fs::symlink(&resource.capability.path, &path).map_err(|source| {
                    UzeError::Write {
                        path: path.clone(),
                        source,
                    }
                })?;
            }
        }
        Ok(Some(AttachmentReceipt {
            package_id: match &resource.origin {
                uze_core::ResourceOrigin::Package { id, .. } => id.as_str().to_owned(),
                uze_core::ResourceOrigin::Project { .. } => unreachable!(),
            },
            resource_identity: Some(resource.identity()),
            integration: self.id().to_owned(),
            strategy: "test-healthy".to_owned(),
            artifact: ManagedArtifact::SymlinkReference {
                path,
                target: resource.capability.path.clone(),
            },
        }))
    }
}

struct ForeignFailingIntegration {
    root: PathBuf,
}

impl IntegrationPort for ForeignFailingIntegration {
    fn id(&self) -> &'static str {
        "antigravity"
    }
    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities::default()
    }
    fn detect(&self) -> HarnessDetection {
        HarnessDetection {
            present: true,
            version: Some("1.1.19".to_owned()),
        }
    }
    fn exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        ExposurePlan {
            representation: resource.capability.representation,
            route: CompatibilityRoute::Adaptable,
            verification: VerificationStatus::Unverified,
            mechanism: ExposureMechanism::Unsupported {
                rationale: "foreign test".to_owned(),
            },
            evidence: "test".to_owned(),
        }
    }
    fn attach_receipt(&self, resource: &Resource) -> Result<Option<AttachmentReceipt>> {
        // Only the default `uze` package is treated as foreign-occupied;
        // any other package should succeed so per-package resilience can be
        // observed (the same shape as the real Antigravity preflight which
        // only blocks the conflicting name).
        if let uze_core::ResourceOrigin::Package { id, .. } = &resource.origin
            && id.as_str().eq("uze")
        {
            return Ok(None);
        }
        let path = self.root.join(resource.name());
        #[cfg(unix)]
        {
            if path.symlink_metadata().is_ok() {
                fs::remove_file(&path).map_err(|source| UzeError::Write {
                    path: path.clone(),
                    source,
                })?;
            }
            std::os::unix::fs::symlink(&resource.capability.path, &path).map_err(|source| {
                UzeError::Write {
                    path: path.clone(),
                    source,
                }
            })?;
        }
        Ok(Some(AttachmentReceipt {
            package_id: match &resource.origin {
                uze_core::ResourceOrigin::Package { id, .. } => id.as_str().to_owned(),
                uze_core::ResourceOrigin::Project { .. } => unreachable!(),
            },
            resource_identity: Some(resource.identity()),
            integration: self.id().to_owned(),
            strategy: "test-foreign".to_owned(),
            artifact: ManagedArtifact::SymlinkReference {
                path,
                target: resource.capability.path.clone(),
            },
        }))
    }
    fn attach_package(
        &self,
        _package: &StoredPackage,
        _plan: &PackageExposurePlan,
    ) -> Result<Option<AttachmentReceipt>> {
        Ok(None)
    }
}

struct ShimConflictingIntegration {}

impl IntegrationPort for ShimConflictingIntegration {
    fn id(&self) -> &'static str {
        "shim-test"
    }
    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities::default()
    }
    fn detect(&self) -> HarnessDetection {
        HarnessDetection {
            present: true,
            version: Some("1.0.0".to_owned()),
        }
    }
    fn exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        ExposurePlan {
            representation: resource.capability.representation,
            route: CompatibilityRoute::Adaptable,
            verification: VerificationStatus::Unverified,
            mechanism: ExposureMechanism::Unsupported {
                rationale: "shim test".to_owned(),
            },
            evidence: "test".to_owned(),
        }
    }
    fn supports_runtime_integration(&self) -> bool {
        true
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["shim-test"]
    }
}

#[test]
fn runtime_shim_repairs_an_rc_file_when_the_shims_dir_is_already_shadowed() {
    let root = uze_testkit::temp::scratch("runtime-shim-shadowed");
    let home = UzeHome::at(root.join("uze-home"));
    let shims_dir = home.shims_dir();
    let real_bin_dir = root.join(".local/bin");
    fs::create_dir_all(&shims_dir).unwrap();
    fs::create_dir_all(&real_bin_dir).unwrap();
    let real_executable = real_bin_dir.join("shim-test");
    fs::write(&real_executable, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(&real_executable, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let rc_file = root.join(".zshrc");
    fs::write(
        &rc_file,
        format!(
            concat!(
                "# >>> uze shims path >>>\n",
                "export PATH=\"{}:$PATH\"\n",
                "# <<< uze shims path <<<\n",
                "export PATH=\"{}:$PATH\"\n",
            ),
            shims_dir.display(),
            real_bin_dir.display(),
        ),
    )
    .unwrap();
    let path = std::env::join_paths([real_bin_dir.as_path(), shims_dir.as_path()]).unwrap();
    let mut environment = uze_testkit::env::scope();
    environment
        .set("HOME", &root)
        .set("SHELL", "/bin/zsh")
        .set("PATH", path);
    assert_eq!(
        uze_core::shell_path::detect_shell_rc(&root)
            .expect("zsh rc is detected")
            .rc_file,
        rc_file
    );

    let app = UzeApplication::new(home, Vec::new());
    let setup = app
        .ensure_runtime_shim(&ShimConflictingIntegration {})
        .unwrap()
        .expect("runtime-enabled integration creates a shim");
    assert_eq!(setup.rc_file_updated, Some(rc_file.clone()));
    assert!(setup.path_hint.is_some(), "current shell remains shadowed");
    let rc = fs::read_to_string(&rc_file).unwrap();
    assert!(rc.starts_with(&format!(
        "export PATH=\"{}:$PATH\"\n",
        real_bin_dir.display()
    )));
    assert!(rc.ends_with(&format!(
        "# >>> uze shims path >>>\nexport PATH=\"{}:$PATH\"\n# <<< uze shims path <<<\n",
        shims_dir.display()
    )));

    drop(environment);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn setup_continues_when_one_harness_has_foreign_state_and_other_succeeds() {
    let root = uze_testkit::temp::scratch("setup-resilience-foreign-one-harness");
    let home = UzeHome::at(&root);
    let healthy_root = root.join("healthy");
    let foreign_root = root.join("foreign");
    fs::create_dir_all(&healthy_root).unwrap();
    fs::create_dir_all(&foreign_root).unwrap();

    let app = UzeApplication::new(
        home.clone(),
        vec![
            Box::new(HealthySymlinkIntegration { root: healthy_root }),
            Box::new(ForeignFailingIntegration { root: foreign_root }),
        ],
    );
    // Seed store with the default `uze` package (the one Antigravity
    // would see as foreign) plus one additional fixture package.
    app.ensure_default_plugins().unwrap();
    app.add_plugin(
        uze_core::PackageSource::local(fixture()),
        &uze_core::trust::AlwaysTrust,
    )
    .unwrap();

    // Mock the real binaries for shim resolution: create fake executables
    // for both harnesses so `ensure_runtime_shim` for the shim-less ones
    // just returns Ok(None) instead of being the reason for a warning.
    let fake_bin = root.join("bin");
    fs::create_dir_all(&fake_bin).unwrap();

    let results = setup_without_touching_the_real_shell_rc(&app, None).unwrap();
    assert_eq!(results.len(), 2, "both harnesses must be reported");

    let healthy = results
        .iter()
        .find(|r| r.integration == "healthy-harness")
        .expect("healthy harness missing");
    let foreign = results
        .iter()
        .find(|r| r.integration == "antigravity")
        .expect("foreign harness missing");

    assert!(healthy.configured, "healthy harness stays configured");
    assert!(
        healthy.attach_error.is_none(),
        "healthy harness must have no attach_error, got {:?}",
        healthy.attach_error
    );
    assert!(
        foreign.configured,
        "externally present harness stays configured"
    );
    assert!(
        foreign.attach_error.is_none(),
        "external native delivery is a successful no-op: {:?}",
        foreign.attach_error
    );

    // Healthy harness actually attached at least one package; foreign's
    // failure for the `uze` package does not erase that.
    let healthy_receipts = state::receipts(&home, None)
        .unwrap()
        .into_iter()
        .filter(|(_, r)| r.integration == "healthy-harness")
        .count();
    assert!(
        healthy_receipts >= 1,
        "healthy harness must have recorded receipts despite sibling failure"
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn attach_stored_packages_to_is_per_package_resilient() {
    let root = uze_testkit::temp::scratch("attach-per-package-resilience");
    let home = UzeHome::at(&root);
    let foreign_root = root.join("foreign2");
    fs::create_dir_all(&foreign_root).unwrap();

    let app = UzeApplication::new(
        home.clone(),
        vec![Box::new(ForeignFailingIntegration { root: foreign_root })],
    );
    // Two packages: default `uze` is externally available and the
    // canonical skill fixture still attaches.
    app.ensure_default_plugins().unwrap();
    app.add_plugin(
        uze_core::PackageSource::local(fixture()),
        &uze_core::trust::AlwaysTrust,
    )
    .unwrap();

    let foreign: &dyn IntegrationPort = app.integrations[0].as_ref();
    let result = app.attach_stored_packages_to(foreign);
    assert!(result.is_ok(), "external native delivery is not an error");

    // But the non-conflicting package must still have been attempted and
    // recorded — per-package resilience, not abort-on-first.
    let receipts = state::receipts(&home, None).unwrap();
    let has_fixture_receipt = receipts.iter().any(|(_, r)| {
        r.integration == "antigravity"
            && r.package_id == "uze-agent-skill-conformance@local"
            && r.resource_identity.is_some()
    });
    assert!(
        has_fixture_receipt,
        "fixture package must have been attached despite `uze` package failing: {receipts:?}"
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn setup_is_idempotent_with_foreign_state_present() {
    let root = uze_testkit::temp::scratch("setup-idempotent-foreign");
    let home = UzeHome::at(&root);
    let foreign_root = root.join("foreign3");
    fs::create_dir_all(&foreign_root).unwrap();

    let app = UzeApplication::new(
        home,
        vec![Box::new(ForeignFailingIntegration { root: foreign_root })],
    );
    app.ensure_default_plugins().unwrap();

    let first = setup_without_touching_the_real_shell_rc(&app, None).unwrap();
    let foreign_first = first
        .iter()
        .find(|r| r.integration == "antigravity")
        .unwrap()
        .attach_error
        .clone();
    assert!(foreign_first.is_none());

    let second = setup_without_touching_the_real_shell_rc(&app, None).unwrap();
    let foreign_second = second
        .iter()
        .find(|r| r.integration == "antigravity")
        .unwrap()
        .attach_error
        .clone();
    assert_eq!(
        foreign_first, foreign_second,
        "repeated setup must keep the external native no-op silent"
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
#[cfg(unix)]
fn shim_failure_is_reported_but_does_not_abort_setup() {
    let root = uze_testkit::temp::scratch("setup-shim-resilience");
    let home = UzeHome::at(&root);
    let shim_root = root.join("shim-data");
    fs::create_dir_all(&shim_root).unwrap();

    // Pre-create a conflicting regular file where the shim symlink would go,
    // so `refresh_shim_symlink` returns `ManagedEntryConflict`.
    let shims_dir = home.shims_dir();
    fs::create_dir_all(&shims_dir).unwrap();
    let shim_path = shims_dir.join("shim-test");
    fs::write(&shim_path, "foreign file, not a symlink").unwrap();

    // Provide a fake real executable so resolution succeeds up to the
    // symlink step — a temp dir on PATH containing `shim-test`.
    let fake_bin = root.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let fake_exe = fake_bin.join("shim-test");
    fs::write(&fake_exe, "#!/bin/sh\necho 1.0.0\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&fake_exe).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&fake_exe, perms).unwrap();
    }
    let mut env_scope = uze_testkit::env::scope();
    env_scope.set(
        "PATH",
        format!(
            "{}:{}",
            fake_bin.display(),
            std::env::var_os("PATH")
                .unwrap_or_default()
                .to_string_lossy()
        ),
    );
    // `SHELL` is set on the SAME guard: the setup path must not edit any
    // real rc file, and a second `env::scope()` here would deadlock on
    // the process-env lock (Mutex is not reentrant).
    env_scope.set("SHELL", "uze-test-no-recognized-shell");

    let app = UzeApplication::new(home, vec![Box::new(ShimConflictingIntegration {})]);

    let results = app.setup(None).unwrap();
    let shim_result = results
        .iter()
        .find(|r| r.integration == "shim-test")
        .expect("shim harness missing");
    assert!(shim_result.configured);
    assert!(
        shim_result.shim_error.is_some(),
        "shim conflict must be surfaced as shim_error, not fatal"
    );
    assert!(
        shim_result.attach_error.is_none(),
        "attach itself should not have failed"
    );

    let _ = fs::remove_dir_all(root);
}
