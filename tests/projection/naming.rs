//! L1 contract for the Identity/Exposure Naming refactor (Checkpoint 2).
//!
//! Central claims under test:
//!   - `resource.identity()` (ownership) never changes when physical
//!     exposure naming changes.
//!   - New exposures get short, unqualified-or-package-qualified names —
//!     never a `"uze-"` collision-avoidance prefix, which never
//!     participated in ownership.
//!   - "Existing receipt wins": an already-attached resource, on *any*
//!     naming scheme including the legacy one, is never recomputed,
//!     renamed, or duplicated.
//!   - Managed collisions (two UZE-managed resources wanting the same
//!     short name) resolve deterministically without either one
//!     overwriting the other.
//!   - A foreign artifact occupying the desired name is never overwritten;
//!     UZE surfaces an explicit conflict instead of silently falling back
//!     (this milestone's accepted first-slice scope).
//!
//! Deterministic by construction: no vendor binary is ever spawned.
//! Harness detection is forced to `present` via an `AlwaysPresent` wrapper
//! so the suite does not depend on whether the harness binary is installed
//! on the runner. Provisioning is wired to a `NoopProcessRunner` so no real
//! installer/updater is invoked.
//!
//! Naming/collision tests run against `OpenCodeIntegration` (see
//! `app_with_opencode`), not Claude: since Generated Native Package
//! (ADR-020) made every skill/MCP fixture in this file eligible for
//! whole-package native Claude delivery, only an integration with no
//! package-level native concept at all — OpenCode's
//! `package_exposure_plan` stays unconditionally `None` — still exercises
//! this file's actual subject, per-resource naming resolution
//! (`short_then_qualified_exposure_name_candidates`), through the full
//! `add_plugin` path.

use std::{
    fs,
    path::{Path, PathBuf},
};

use uze::integrations::opencode::OpenCodeIntegration;
use uze::{
    PackageSource, Resource, UzeApplication, UzeEngine, UzeHome, UzeStore,
    exposure::{ExposurePlan, PackageExposurePlan},
    integration::{
        AttachmentInspection, AttachmentReceipt, HarnessDetection, IntegrationPort,
        IntegrationStatus, PublicationStatus, default_exposure_name_candidates,
    },
    project::Resource as ProjectResource,
    provisioning::{ProcessResult, ProcessRunner, ProcessSpec},
    router::HarnessCapabilities,
    store::StoredPackage,
};

fn temp(label: &str) -> PathBuf {
    uze_test_support::temp::scratch(label)
}

/// Never spawns a process. Provisioning only needs *a* verified outcome to
/// unlock `setup`'s attach step — the real vendor installer/updater must
/// never run against the developer's actual `claude` CLI just because a
/// test happened to call `setup()`.
struct NoopProcessRunner;

impl ProcessRunner for NoopProcessRunner {
    fn run(&self, _spec: &ProcessSpec) -> uze::Result<ProcessResult> {
        Ok(ProcessResult {
            success: true,
            timed_out: false,
        })
    }
}

struct AlwaysPresent<T: IntegrationPort>(T);

impl<T: IntegrationPort> IntegrationPort for AlwaysPresent<T> {
    fn id(&self) -> &'static str {
        self.0.id()
    }
    fn capabilities(&self) -> HarnessCapabilities {
        self.0.capabilities()
    }
    fn runtime_support(&self) -> uze::runtime::RuntimeSupport {
        self.0.runtime_support()
    }
    fn exposure_plan(&self, resource: &ProjectResource) -> ExposurePlan {
        self.0.exposure_plan(resource)
    }
    fn exposure_name_candidates(&self, resource: &ProjectResource) -> Vec<String> {
        self.0.exposure_name_candidates(resource)
    }
    fn shared_agent_skill_root(&self) -> Option<PathBuf> {
        self.0.shared_agent_skill_root()
    }
    fn package_exposure_plan(
        &self,
        package: &StoredPackage,
        resources: &[&ProjectResource],
    ) -> Option<PackageExposurePlan> {
        self.0.package_exposure_plan(package, resources)
    }
    fn detect(&self) -> HarnessDetection {
        HarnessDetection {
            present: true,
            version: Some("9.9.9".to_owned()),
        }
    }
    // Deliberately does NOT delegate to `self.0.provision(runner)`: every
    // real integration's default/override `provision()` calls `self.detect()`
    // on itself internally (see `IntegrationPort::provision`'s default body
    // and `OpenCodeIntegration::provision`) — a call that resolves against
    // the wrapped concrete type, not this wrapper's `detect()` override, and
    // so re-probes the real environment regardless of the forced-present
    // detection above. Returning a verified result built from `self.detect()`
    // here keeps `provision()` and `detect()` consistent under the wrapper.
    fn provision(
        &self,
        _runner: &dyn ProcessRunner,
    ) -> uze::Result<uze::provisioning::ProvisioningResult> {
        Ok(uze::provisioning::ProvisioningResult::verified(
            uze::provisioning::ProvisionAction::None,
            "test-always-present",
            self.detect(),
        ))
    }
    fn install(&self, home: &UzeHome, detection: &HarnessDetection) -> uze::Result<()> {
        self.0.install(home, detection)
    }
    fn status(&self, home: &UzeHome) -> IntegrationStatus {
        self.0.status(home)
    }
    fn attach(&self, resource: &ProjectResource) -> uze::Result<Option<PathBuf>> {
        self.0.attach(resource)
    }
    fn attach_package(
        &self,
        package: &StoredPackage,
        plan: &PackageExposurePlan,
    ) -> uze::Result<Option<AttachmentReceipt>> {
        self.0.attach_package(package, plan)
    }
    fn aliases(&self) -> &'static [&'static str] {
        self.0.aliases()
    }
    fn republish_packages(&self, packages: &[StoredPackage]) -> uze::Result<()> {
        self.0.republish_packages(packages)
    }
    fn publication(&self, packages: &[StoredPackage]) -> PublicationStatus {
        self.0.publication(packages)
    }
    fn attach_receipt(&self, resource: &ProjectResource) -> uze::Result<Option<AttachmentReceipt>> {
        self.0.attach_receipt(resource)
    }
    fn inspect_receipt(&self, receipt: &AttachmentReceipt) -> AttachmentInspection {
        self.0.inspect_receipt(receipt)
    }
    fn detach_receipt(&self, receipt: &AttachmentReceipt) -> uze::Result<AttachmentInspection> {
        self.0.detach_receipt(receipt)
    }
}

/// Capability-level naming/collision tests moved to OpenCode (see the
/// individual test comments below): under Generated Native Package
/// (ADR-020), any Claude-bound package with a conventional `skills/`
/// directory or `mcp.json` — which every fixture in this file has — now
/// qualifies for whole-package native delivery rather than per-Skill
/// decomposition, so it can no longer exercise
/// `ManagedUserScopeReference` naming resolution through the full
/// `add_plugin` path. OpenCode has no package-level native delivery
/// concept at all (`package_exposure_plan` stays at Core's `None`
/// default for every package, unconditionally) and uses the same shared
/// `short_then_qualified_exposure_name_candidates` naming primitive
/// Claude does, so it exercises the identical naming/collision logic
/// this file's central claims are about, without incidentally also
/// asserting anything about package-level generation — matching this
/// milestone's guidance to adjust the test's level rather than add a
/// product-level escape hatch.
fn app_with_opencode(root: &Path) -> (UzeApplication, PathBuf) {
    let agents_home = root.join("opencode-agents");
    let uze_home = UzeHome::at(root.join("uze-home"));
    let application = UzeApplication::new_with_runner(
        uze_home.clone(),
        vec![Box::new(AlwaysPresent(OpenCodeIntegration::new(
            agents_home.clone(),
            root.join("opencode-config/opencode.json"),
            uze_home,
        )))],
        Box::new(NoopProcessRunner),
    );
    (application, agents_home)
}

fn install(app: &UzeApplication, path: PathBuf) -> uze::application::PluginSummary {
    app.add_plugin(PackageSource::local(path), &uze::trust::AlwaysTrust)
        .unwrap()
        .plugin
}

fn skill_fixture(root: &Path, package_id: &str, skill_name: &str) -> PathBuf {
    let dir = root.join(package_id);
    fs::create_dir_all(dir.join("skills").join(skill_name)).unwrap();
    fs::write(
        dir.join("plugin.json"),
        format!(
            r#"{{"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"{package_id}"}}"#
        ),
    )
    .unwrap();
    fs::write(
        dir.join("skills").join(skill_name).join("SKILL.md"),
        format!("---\nname: {skill_name}\ndescription: test fixture.\n---\n\nBody.\n"),
    )
    .unwrap();
    dir
}

/// The one AgentSkill resource a Store installation of `package_dir`
/// contributes, used to test pure functions (`identity`,
/// `default_exposure_name_candidates`) directly, independent of any
/// `UzeApplication`/harness wiring.
fn store_resource(root: &Path, package_dir: PathBuf) -> Resource {
    let home = UzeHome::at(root.join("uze-home"));
    let store = UzeStore::new(home.clone());
    let installed = store
        .ingest(&uze::acquisition::acquire(&PackageSource::local(package_dir)).unwrap())
        .unwrap();
    let environment = UzeEngine::new(store)
        .compose(std::slice::from_ref(&installed.id))
        .unwrap();
    environment
        .resources
        .into_iter()
        .find(|resource| resource.capability.kind == uze::capability::CapabilityKind::AgentSkill)
        .unwrap()
}

// --- Identity invariance ----------------------------------------------------

#[test]
fn resource_identity_is_unaffected_by_resolved_exposure_name() {
    let root = temp("identity-invariance");
    let resource = store_resource(
        &root,
        skill_fixture(&root.join("fixtures"), "acme", "review"),
    );
    let before = resource.identity();

    let mut resolved = resource.clone();
    resolved.resolved_exposure_name = Some("anything-at-all".to_owned());
    assert_eq!(
        before,
        resolved.identity(),
        "identity must not depend on exposure naming"
    );
    fs::remove_dir_all(root).unwrap();
}

// --- New default naming: no "uze-" prefix -----------------------------------

#[test]
fn default_candidates_carry_no_uze_collision_prefix() {
    let root = temp("default-candidates");
    let resource = store_resource(
        &root,
        skill_fixture(&root.join("fixtures"), "security-tools", "review"),
    );
    let candidates = default_exposure_name_candidates(&resource);
    assert_eq!(candidates, vec!["security-tools-review".to_owned()]);
    assert!(!candidates[0].starts_with("uze-"));
    fs::remove_dir_all(root).unwrap();
}

// --- Official /uze package: no special-case ---------------------------------

#[test]
fn package_uze_plus_skill_uze_naturally_gets_the_stable_label_no_special_case() {
    let root = temp("uze-natural");
    let (application, agents_home) = app_with_opencode(&root);
    install(&application, official_package());

    let mut names: Vec<String> = fs::read_dir(agents_home.join("skills"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_str().unwrap().to_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec!["uze:uze".to_owned()],
        "the official package gets the same stable namespaced label as any other plugin (ADR-026)"
    );
    fs::remove_dir_all(root).unwrap();
}

fn official_package() -> PathBuf {
    uze_test_support::fixtures::official_plugin()
}

// --- Legacy receipt migration (ADR-026) -------------------------------------

#[test]
fn a_legacy_named_receipt_is_migrated_to_the_stable_label_not_frozen_forever() {
    let root = temp("legacy-migration");
    let (application, agents_home) = app_with_opencode(&root);
    let package_dir = skill_fixture(&root.join("fixtures"), "legacy-pkg", "review");

    let summary = install(&application, package_dir);
    let skills_dir = agents_home.join("skills");
    let fresh_path = skills_dir.join("legacy-pkg:review");
    assert!(
        fresh_path.is_symlink(),
        "sanity: a fresh install claims its stable namespaced label"
    );

    // Simulate an install performed under the previous naming policy:
    // physically rename the artifact to the legacy "uze-<package>-<skill>"
    // shape and point the ledger receipt's path at it, exactly what a
    // pre-ADR-026 install would have left on disk.
    let legacy_name = "uze-legacy-pkg-review";
    let legacy_path = skills_dir.join(legacy_name);
    let target = fs::read_link(&fresh_path).unwrap();
    fs::remove_file(&fresh_path).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &legacy_path).unwrap();
    rewrite_receipt_path(&root, &summary.id, &fresh_path, &legacy_path);

    // Re-run setup/attach — the legacy artifact is exactly UZE-owned
    // (Matched), so it migrates to the stable label: the legacy entry is
    // removed, one labeled entry exists, and no second artifact is created.
    application.setup(None).unwrap();

    let mut after_listing: Vec<String> = fs::read_dir(&skills_dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_str().unwrap().to_owned())
        .collect();
    after_listing.sort();
    let after_filtered: Vec<String> = after_listing
        .iter()
        .filter(|name| *name != "uze:uze")
        .cloned()
        .collect();
    assert_eq!(
        after_filtered,
        vec!["legacy-pkg:review".to_owned()],
        "the legacy name migrates to the stable label; no duplicate artifact"
    );
    assert!(
        !legacy_path.exists(),
        "the UZE-owned legacy symlink is removed by the migration"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_legacy_name_taken_over_by_foreign_content_is_surrendered_and_relabeled() {
    let root = temp("legacy-conflict");
    let (application, agents_home) = app_with_opencode(&root);
    let package_dir = skill_fixture(&root.join("fixtures"), "legacy-pkg", "review");

    let summary = install(&application, package_dir);
    let skills_dir = agents_home.join("skills");
    let fresh_path = skills_dir.join("legacy-pkg:review");
    assert!(fresh_path.is_symlink());

    // Simulate the shape seen on real machines: a pre-ADR-026 bare entry
    // whose name was later taken over by a real, user-owned skill directory.
    let legacy_name = "review";
    let legacy_path = skills_dir.join(legacy_name);
    let target = fs::read_link(&fresh_path).unwrap();
    fs::remove_file(&fresh_path).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &legacy_path).unwrap();
    rewrite_receipt_path(&root, &summary.id, &fresh_path, &legacy_path);
    fs::remove_file(&legacy_path).unwrap();
    fs::create_dir_all(&legacy_path).unwrap();
    fs::write(legacy_path.join("SKILL.md"), "user's own skill").unwrap();

    // Re-run setup — the foreign occupant keeps its content untouched; UZE
    // attaches under its stable label and the stale receipt is forgotten.
    application.setup(None).unwrap();
    assert_eq!(
        fs::read_to_string(legacy_path.join("SKILL.md")).unwrap(),
        "user's own skill",
        "foreign content is never touched"
    );
    assert!(
        legacy_path.is_dir() && !legacy_path.is_symlink(),
        "the user-owned directory remains exactly as it was"
    );
    assert!(
        skills_dir.join("legacy-pkg:review").is_symlink(),
        "UZE delivers under its stable labeled name"
    );
    let inspection = application.inspect_plugin(&summary.id).unwrap();
    assert_eq!(inspection.managed_state.matched, 1);
    assert_eq!(inspection.managed_state.conflicts, 0);
    fs::remove_dir_all(root).unwrap();
}

/// Test-only ledger surgery: rewrites the one receipt for `package_id` so
/// its `SymlinkReference.path` points at `new_path` instead of `old_path` —
/// simulating what a pre-refactor install already left on disk, not
/// something any production code path does.
fn rewrite_receipt_path(root: &Path, package_id: &str, old_path: &Path, new_path: &Path) {
    let ledger_path = root.join("uze-home/state/attachments.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(&ledger_path).unwrap()).unwrap();
    let receipts = value["receipts"].as_object_mut().unwrap();
    for receipt in receipts.values_mut() {
        if receipt["package_id"] == package_id
            && let Some(path) = receipt.pointer_mut("/artifact/SYMLINK_REFERENCE/path")
        {
            assert_eq!(path.as_str().unwrap(), old_path.to_str().unwrap());
            *path = serde_json::json!(new_path.to_str().unwrap());
        }
    }
    fs::write(&ledger_path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}

// --- Managed collision: two packages, same skill name -----------------------

#[test]
fn two_packages_with_the_same_skill_name_coexist_deterministically() {
    let root = temp("managed-collision");
    let (application, agents_home) = app_with_opencode(&root);
    let fixture_root = root.join("fixtures");

    install(
        &application,
        skill_fixture(&fixture_root, "frontend", "review"),
    );
    install(
        &application,
        skill_fixture(&fixture_root, "security", "review"),
    );

    let mut names: Vec<String> = fs::read_dir(agents_home.join("skills"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_str().unwrap().to_owned())
        .collect();
    names.sort();
    // Stable namespaced labels: each package's skill keeps its own
    // `name:label` regardless of installation order and of the other
    // package's presence (ADR-026). Installing `security` after `frontend`
    // never renames `frontend:review`.
    assert_eq!(
        names,
        vec!["frontend:review".to_owned(), "security:review".to_owned()]
    );

    // Removing one must not disturb the other.
    application.remove_plugin("frontend").unwrap();
    let remaining: Vec<String> = fs::read_dir(agents_home.join("skills"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_str().unwrap().to_owned())
        .collect();
    assert_eq!(remaining, vec!["security:review".to_owned()]);
    fs::remove_dir_all(root).unwrap();
}

// --- Foreign collision: never overwritten, explicit conflict ---------------

#[test]
fn a_foreign_artifact_occupying_the_short_name_is_never_overwritten() {
    let root = temp("foreign-collision");
    let (application, agents_home) = app_with_opencode(&root);
    let package_dir = skill_fixture(&root.join("fixtures"), "security", "review");

    // A foreign, non-UZE directory already occupies the exact namespaced
    // label UZE would claim.
    fs::create_dir_all(agents_home.join("skills/security:review")).unwrap();
    fs::write(
        agents_home.join("skills/security:review/SKILL.md"),
        "not ours",
    )
    .unwrap();

    let result =
        application.add_plugin(PackageSource::local(package_dir), &uze::trust::AlwaysTrust);
    assert!(
        matches!(result, Err(uze::UzeError::ManagedEntryConflict(_))),
        "a foreign occupant of the desired name must surface as an explicit conflict, \
         not a silent skip or an automatic fallback retry: {result:?}"
    );
    assert_eq!(
        fs::read_to_string(agents_home.join("skills/security:review/SKILL.md")).unwrap(),
        "not ours",
        "the foreign artifact must be completely untouched"
    );
    fs::remove_dir_all(root).unwrap();
}

// --- Lifecycle ---------------------------------------------------------------

#[test]
fn inspect_matched_missing_drifted_and_detach_all_still_work_under_new_naming() {
    let root = temp("lifecycle");
    let (application, agents_home) = app_with_opencode(&root);
    install(
        &application,
        skill_fixture(&root.join("fixtures"), "acme", "review"),
    );

    let inspection = application.inspect_plugin("acme").unwrap();
    assert_eq!(inspection.managed_state.matched, 1);

    // MISSING: remove the physical artifact by hand.
    fs::remove_file(agents_home.join("skills/acme:review")).unwrap();
    let inspection = application.inspect_plugin("acme").unwrap();
    assert_eq!(inspection.managed_state.missing, 1);

    // Re-add is idempotent: recreates exactly the same (existing-receipt)
    // artifact name.
    application.setup(None).unwrap();
    assert!(agents_home.join("skills/acme:review").is_symlink());
    let inspection = application.inspect_plugin("acme").unwrap();
    assert_eq!(inspection.managed_state.matched, 1);

    // DRIFTED: repoint the symlink elsewhere.
    let elsewhere = root.join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    fs::remove_file(agents_home.join("skills/acme:review")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&elsewhere, agents_home.join("skills/acme:review")).unwrap();
    let inspection = application.inspect_plugin("acme").unwrap();
    assert_eq!(inspection.managed_state.drifted, 1);

    // Drift blocks destructive removal.
    assert!(matches!(
        application.remove_plugin("acme").unwrap(),
        uze::application::RemovePluginReport::Blocked { .. }
    ));
    assert_eq!(
        fs::read_link(agents_home.join("skills/acme:review")).unwrap(),
        elsewhere
    );

    // Fix it back, then remove cleanly; remove twice is a safe no-op.
    fs::remove_file(agents_home.join("skills/acme:review")).unwrap();
    application.setup(None).unwrap();
    assert!(matches!(
        application.remove_plugin("acme").unwrap(),
        uze::application::RemovePluginReport::Removed { .. }
    ));
    assert!(matches!(
        application.remove_plugin("acme").unwrap(),
        uze::application::RemovePluginReport::AlreadyAbsent { .. }
    ));
    fs::remove_dir_all(root).unwrap();
}

// --- Structural: ownership never inferred from a "uze-" name pattern -------

#[test]
fn ownership_logic_never_pattern_matches_on_a_uze_prefix() {
    let crates_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("crates");
    let forbidden = ["starts_with(\"uze-", "strip_prefix(\"uze-"];
    let mut offenders = Vec::new();
    for entry in walk_rs_files(&crates_root) {
        let content = fs::read_to_string(&entry).unwrap();
        for pattern in forbidden {
            if content.contains(pattern) {
                offenders.push(format!("{}: {pattern}", entry.display()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "ownership must never be inferred from a \"uze-\" filename pattern: {offenders:?}"
    );
}

#[test]
fn no_source_special_cases_the_official_uze_package_or_skill_name() {
    let scan_roots = [
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("crates/uze-core/src"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("crates/uze-integrations/src"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("crates/uze-application/src"),
    ];
    let forbidden = ["== \"uze\""];
    // The default marketplace bootstrap
    // (`crates/uze-application/src/bootstrap.rs`) is the sole exception: the
    // binary must know which package ids its embedded snapshot carries
    // without requiring `uze add`. Every other hardcoding of the literal
    // remains forbidden — notably, `application.rs` itself is no longer
    // whitelisted: `ensure_default_plugins` reaches the default id only
    // through `bootstrap::DEFAULT_PLUGIN_IDS`, never a literal comparison.
    let allowed_files = ["bootstrap.rs"];
    let mut offenders = Vec::new();
    for root in scan_roots {
        for entry in walk_rs_files(&root) {
            if allowed_files
                .iter()
                .any(|name| entry.file_name().and_then(|f| f.to_str()) == Some(name))
            {
                continue;
            }
            let content = fs::read_to_string(&entry).unwrap();
            for pattern in forbidden {
                if content.contains(pattern) {
                    offenders.push(format!("{}: {pattern}", entry.display()));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "no production code may special-case the literal package/skill name \"uze\": {offenders:?}"
    );
}

/// Store, Engine, Router and every `IntegrationPort` must stay unaware the
/// marketplace/bootstrap primitive exists — matching the same rule the M1/M2
/// invariants already hold acquisition and vendor identity to. Checks the
/// specific type/module paths (`acquisition::marketplace`,
/// `MarketplaceManifest`, `MarketplacePluginEntry`), not the bare English
/// word "marketplace" — Codex's own, unrelated, pre-existing local plugin
/// catalogue (`codex.rs`'s `MARKETPLACE_NAME`, `plugin marketplace add`)
/// legitimately uses that word for a different concept and must not trip
/// this check.
#[test]
fn store_engine_router_and_integrations_stay_marketplace_neutral() {
    let scan_roots = [
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("crates/uze-core/src/store.rs"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("crates/uze-core/src/engine.rs"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("crates/uze-core/src/router.rs"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("crates/uze-core/src/integration.rs"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("crates/uze-integrations/src"),
    ];
    let forbidden = [
        "acquisition::marketplace",
        "MarketplaceManifest",
        "MarketplacePluginEntry",
    ];
    let mut offenders = Vec::new();
    for root in scan_roots {
        let files = if root.is_dir() {
            walk_rs_files(&root)
        } else {
            vec![root]
        };
        for entry in files {
            let Ok(content) = fs::read_to_string(&entry) else {
                continue;
            };
            for pattern in forbidden {
                if content.contains(pattern) {
                    offenders.push(format!("{}: {pattern}", entry.display()));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "Store/Engine/Router/Integration must never reference the marketplace primitive: {offenders:?}"
    );
}

fn walk_rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.filter_map(std::result::Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk_rs_files(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
    out
}
