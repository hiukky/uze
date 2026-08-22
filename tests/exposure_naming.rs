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
//! so the suite does not depend on whether `claude` is installed on the
//! runner. Provisioning is wired to a `NoopProcessRunner` so no real
//! installer/updater is invoked.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use uze::integrations::claude::ClaudeIntegration;
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
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "uze-exposure-naming-{label}-{}-{nonce}",
        std::process::id()
    ))
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
    fn provision(
        &self,
        runner: &dyn ProcessRunner,
    ) -> uze::Result<uze::provisioning::ProvisioningResult> {
        self.0.provision(runner)
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

fn app_with_claude(root: &Path) -> (UzeApplication, PathBuf) {
    let claude_home = root.join("claude-home");
    let uze_home = UzeHome::at(root.join("uze-home"));
    let application = UzeApplication::new_with_runner(
        uze_home.clone(),
        vec![Box::new(AlwaysPresent(ClaudeIntegration::new(
            claude_home.clone(),
            uze_home,
        )))],
        Box::new(NoopProcessRunner),
    );
    (application, claude_home)
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
fn package_uze_plus_skill_uze_naturally_gets_the_bare_name_uze_no_special_case() {
    let root = temp("uze-natural");
    let (application, claude_home) = app_with_claude(&root);
    install(&application, official_package());

    let mut names: Vec<String> = fs::read_dir(claude_home.join("skills"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_str().unwrap().to_owned())
        .collect();
    names.sort();
    assert_eq!(names, vec!["uze".to_owned()]);
    fs::remove_dir_all(root).unwrap();
}

fn official_package() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/uze")
}

// --- Legacy receipt reuse ----------------------------------------------------

#[test]
fn a_legacy_uze_prefixed_receipt_is_reused_verbatim_never_recomputed_or_duplicated() {
    let root = temp("legacy-reuse");
    let (application, claude_home) = app_with_claude(&root);
    let package_dir = skill_fixture(&root.join("fixtures"), "legacy-pkg", "review");

    let summary = install(&application, package_dir);
    let skills_dir = claude_home.join("skills");
    let fresh_path = skills_dir.join("review");
    assert!(
        fresh_path.is_symlink(),
        "sanity: a fresh install with no collision claims the short name"
    );

    // Simulate a pre-refactor installation: physically rename the artifact
    // to the legacy "uze-<package>-<skill>" shape and point the ledger
    // receipt's path at it, exactly what an install performed before this
    // milestone would have left on disk.
    let legacy_name = "uze-legacy-pkg-review";
    let legacy_path = skills_dir.join(legacy_name);
    let target = fs::read_link(&fresh_path).unwrap();
    fs::remove_file(&fresh_path).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &legacy_path).unwrap();
    rewrite_receipt_path(&root, &summary.id, &fresh_path, &legacy_path);

    let mut before_listing: Vec<String> = fs::read_dir(&skills_dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_str().unwrap().to_owned())
        .collect();
    before_listing.sort();
    // Filter out the default `uze` skill which is now seeded on setup; the
    // test isolates the legacy receipt reuse, not the presence of the default plugin.
    let before_filtered: Vec<String> = before_listing
        .iter()
        .filter(|name| *name != "uze")
        .cloned()
        .collect();
    assert_eq!(before_filtered, vec![legacy_name.to_owned()]);

    // Re-run setup/attach — must reuse the legacy receipt's name exactly,
    // never compute the new short name and create a second artifact. The
    // the default `uze` plugin may appear alongside it after setup seeds the default plugin.
    application.setup(None).unwrap();

    let mut after_listing: Vec<String> = fs::read_dir(&skills_dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_str().unwrap().to_owned())
        .collect();
    after_listing.sort();
    let after_filtered: Vec<String> = after_listing
        .iter()
        .filter(|name| *name != "uze")
        .cloned()
        .collect();
    assert_eq!(
        after_filtered,
        vec![legacy_name.to_owned()],
        "exactly one non-default artifact must exist, and it must still be the legacy one"
    );
    // The default `uze` is expected alongside the legacy artifact after setup.
    assert!(
        after_listing.contains(&"uze".to_owned()),
        "default uze skill should be present alongside the legacy artifact"
    );
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
    let (application, claude_home) = app_with_claude(&root);
    let fixture_root = root.join("fixtures");

    install(
        &application,
        skill_fixture(&fixture_root, "frontend", "review"),
    );
    install(
        &application,
        skill_fixture(&fixture_root, "security", "review"),
    );

    let mut names: Vec<String> = fs::read_dir(claude_home.join("skills"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_str().unwrap().to_owned())
        .collect();
    names.sort();
    // Deterministic: the first safely-attached resource claims the short
    // name; a later one wanting the same name falls back to its
    // package-qualified form. Not an ordering guarantee beyond that.
    assert_eq!(
        names,
        vec!["review".to_owned(), "security-review".to_owned()]
    );

    // Removing one must not disturb the other.
    application.remove_plugin("frontend").unwrap();
    let remaining: Vec<String> = fs::read_dir(claude_home.join("skills"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_str().unwrap().to_owned())
        .collect();
    assert_eq!(remaining, vec!["security-review".to_owned()]);
    fs::remove_dir_all(root).unwrap();
}

// --- Foreign collision: never overwritten, explicit conflict ---------------

#[test]
fn a_foreign_artifact_occupying_the_short_name_is_never_overwritten() {
    let root = temp("foreign-collision");
    let (application, claude_home) = app_with_claude(&root);
    let package_dir = skill_fixture(&root.join("fixtures"), "security", "review");

    // A foreign, non-UZE directory already occupies the exact short name
    // UZE would otherwise claim.
    fs::create_dir_all(claude_home.join("skills/review")).unwrap();
    fs::write(claude_home.join("skills/review/SKILL.md"), "not ours").unwrap();

    let result =
        application.add_plugin(PackageSource::local(package_dir), &uze::trust::AlwaysTrust);
    assert!(
        matches!(result, Err(uze::UzeError::ManagedEntryConflict(_))),
        "a foreign occupant of the desired name must surface as an explicit conflict, \
         not a silent skip or an automatic fallback retry: {result:?}"
    );
    assert_eq!(
        fs::read_to_string(claude_home.join("skills/review/SKILL.md")).unwrap(),
        "not ours",
        "the foreign artifact must be completely untouched"
    );
    fs::remove_dir_all(root).unwrap();
}

// --- Lifecycle ---------------------------------------------------------------

#[test]
fn inspect_matched_missing_drifted_and_detach_all_still_work_under_new_naming() {
    let root = temp("lifecycle");
    let (application, claude_home) = app_with_claude(&root);
    install(
        &application,
        skill_fixture(&root.join("fixtures"), "acme", "review"),
    );

    let inspection = application.inspect_plugin("acme").unwrap();
    assert_eq!(inspection.managed_state.matched, 1);

    // MISSING: remove the physical artifact by hand.
    fs::remove_file(claude_home.join("skills/review")).unwrap();
    let inspection = application.inspect_plugin("acme").unwrap();
    assert_eq!(inspection.managed_state.missing, 1);

    // Re-add is idempotent: recreates exactly the same (existing-receipt)
    // artifact name.
    application.setup(None).unwrap();
    assert!(claude_home.join("skills/review").is_symlink());
    let inspection = application.inspect_plugin("acme").unwrap();
    assert_eq!(inspection.managed_state.matched, 1);

    // DRIFTED: repoint the symlink elsewhere.
    let elsewhere = root.join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    fs::remove_file(claude_home.join("skills/review")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&elsewhere, claude_home.join("skills/review")).unwrap();
    let inspection = application.inspect_plugin("acme").unwrap();
    assert_eq!(inspection.managed_state.drifted, 1);

    // Drift blocks destructive removal.
    assert!(matches!(
        application.remove_plugin("acme").unwrap(),
        uze::application::RemovePluginReport::Blocked { .. }
    ));
    assert_eq!(
        fs::read_link(claude_home.join("skills/review")).unwrap(),
        elsewhere
    );

    // Fix it back, then remove cleanly; remove twice is a safe no-op.
    fs::remove_file(claude_home.join("skills/review")).unwrap();
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
