//! Regression coverage for the cross-harness naming collision fixed
//! alongside `IntegrationPort::shared_agent_skill_root`: OpenCode, Codex,
//! and Gemini CLI all discover Agent Skills from the same physical
//! `~/.agents/skills` directory. Before the fix, OpenCode's own
//! `short_then_qualified` naming policy (bare name first) and Codex/Gemini's
//! always-qualified default policy each computed a name independently, so
//! installing a package attached to all three left *two* symlinks for the
//! identical skill sitting in that one shared folder — visible to OpenCode,
//! which scans the whole directory, as a duplicate `/uze` and `/uze-uze`
//! slash command.
//!
//! Deterministic by construction: a `NoopProcessRunner` covers `.provision()`,
//! detection is forced present via an `AlwaysPresent` wrapper, and — since
//! this fixture's one Skill now also qualifies for Generated Native
//! Package/Extension (ADR-020/ADR-021), which shells out to the real
//! `codex`/`gemini` executable directly via `Command::new`, bypassing the
//! injected `ProcessRunner` — a stateful fake `codex`/`gemini` pair
//! (`fake_codex_and_gemini_bin_dir`) is prepended to `PATH` for the
//! duration of the test. No real host binary is ever spawned.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use uze::integrations::{
    codex::CodexIntegration, gemini::GeminiIntegration, opencode::OpenCodeIntegration,
};
use uze::{
    PackageSource, UzeApplication, UzeHome,
    exposure::{ExposurePlan, PackageExposurePlan},
    integration::{
        AttachmentInspection, AttachmentReceipt, HarnessDetection, IntegrationPort,
        IntegrationStatus, PublicationStatus,
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
        "uze-shared-skill-root-{label}-{}-{nonce}",
        std::process::id()
    ))
}

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
    // See `tests/exposure_naming.rs`'s identical override for why this must
    // not delegate to `self.0.provision(runner)`: the wrapped integration's
    // own `provision()` re-probes the real environment via `self.detect()`
    // on itself, bypassing this wrapper's forced-present `detect()` override.
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

/// Writes fake, STATEFUL `codex`/`gemini` executables that understand
/// exactly the subcommands this test's package-level delivery invokes —
/// `plugin marketplace add/list --json`, `plugin add/list --json`,
/// `extensions link --consent`, `extensions list --output-format=json` —
/// well enough for a full attach-then-inspect round trip to report Matched,
/// deterministically and without touching any real harness state.
///
/// Since this fixture's one Skill has no vendor envelope, it now qualifies
/// for Generated Native Package/Extension (ADR-020/ADR-021) —
/// `attach_package` shells out to the real, PATH-resolved executable via
/// `Command::new`, not through the injected `ProcessRunner` this file
/// already uses for `.provision()`, so without this the test would
/// accidentally depend on the developer's real, locally-installed
/// `codex`/`gemini` (see spec §18: "any test that accidentally shells to
/// the developer's real harness is a test bug" — and CI has neither
/// binary installed). Returns a PATH prefix to prepend ahead of the real
/// one.
#[cfg(unix)]
fn fake_codex_and_gemini_bin_dir(root: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let dir = root.join("fake-bin");
    let state = dir.join("state");
    fs::create_dir_all(&state).unwrap();

    // Codex: persists the marketplace root/name (read back out of the real
    // catalogue file this test's own generated-envelope code wrote — the
    // fake never invents a name) and the installed selector/source across
    // calls, then answers `list --json` truthfully from that state.
    let codex_script = format!(
        r#"#!/bin/sh
state="{state}"
case "$1 $2" in
  "plugin marketplace")
    case "$3" in
      add)
        root="$4"
        name=$(sed -n 's/.*"name" *: *"\([^"]*\)".*/\1/p' "$root/.agents/plugins/marketplace.json" | head -1)
        printf '%s' "$name" > "$state/marketplace_name"
        printf '%s' "$root" > "$state/marketplace_root"
        exit 0
        ;;
      list)
        if [ -f "$state/marketplace_root" ]; then
          name=$(cat "$state/marketplace_name")
          root=$(cat "$state/marketplace_root")
          printf '{{"marketplaces":[{{"name":"%s","root":"%s"}}]}}' "$name" "$root"
        else
          printf '{{"marketplaces":[]}}'
        fi
        exit 0
        ;;
    esac
    ;;
  "plugin add")
    selector="$3"
    root=$(cat "$state/marketplace_root" 2>/dev/null)
    id="${{selector%%@*}}"
    printf '%s' "$selector" > "$state/installed_selector"
    printf '%s/%s' "$root" "$id" > "$state/installed_source"
    exit 0
    ;;
  "plugin list")
    if [ -f "$state/installed_selector" ]; then
      selector=$(cat "$state/installed_selector")
      name=$(cat "$state/marketplace_name" 2>/dev/null)
      source=$(cat "$state/installed_source")
      printf '{{"installed":[{{"pluginId":"%s","enabled":true,"installed":true,"marketplaceName":"%s","path":"%s"}}]}}' "$selector" "$name" "$source"
    else
      printf '{{"installed":[]}}'
    fi
    exit 0
    ;;
  "plugin remove")
    rm -f "$state/marketplace_name" "$state/marketplace_root" "$state/installed_selector" "$state/installed_source"
    exit 0
    ;;
esac
exit 0
"#,
        state = state.display(),
    );

    // Gemini: persists the linked extension's name (read from the
    // generated `gemini-extension.json` it was pointed at) and source
    // directory, then answers `extensions list` truthfully from that state.
    let gemini_script = format!(
        r#"#!/bin/sh
state="{state}"
case "$1" in
  extensions)
    case "$2" in
      link)
        dir="$3"
        name=$(sed -n 's/.*"name" *: *"\([^"]*\)".*/\1/p' "$dir/gemini-extension.json" | head -1)
        printf '%s' "$name" > "$state/extension_name"
        printf '%s' "$dir" > "$state/extension_source"
        exit 0
        ;;
      list)
        if [ -f "$state/extension_name" ]; then
          name=$(cat "$state/extension_name")
          source=$(cat "$state/extension_source")
          printf '[{{"name":"%s","installMetadata":{{"source":"%s","type":"link"}},"isActive":true}}]' "$name" "$source"
        else
          printf '[]'
        fi
        exit 0
        ;;
      uninstall)
        rm -f "$state/extension_name" "$state/extension_source"
        exit 0
        ;;
    esac
    ;;
esac
exit 0
"#,
        state = state.display(),
    );

    for (name, script) in [("codex", codex_script), ("gemini", gemini_script)] {
        let path = dir.join(name);
        fs::write(&path, script).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
    }
    dir
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

#[test]
fn opencode_codex_and_gemini_share_exactly_one_symlink_for_the_same_skill() {
    let root = temp("three-harness");
    let agents_home = root.join("agents-home");
    let uze_home = UzeHome::at(root.join("uze-home"));

    // Codex and Gemini registered *before* OpenCode, deliberately: both use
    // the always-qualified default policy in isolation, so if attach order
    // alone decided the group's name, whichever of them resolves first
    // would lock the shared folder onto "acme-review" before OpenCode ever
    // got a chance to try its own preferred bare name. The fix must
    // converge on OpenCode's preference regardless of this order.
    let application = UzeApplication::new_with_runner(
        uze_home.clone(),
        vec![
            Box::new(AlwaysPresent(CodexIntegration::new(
                agents_home.clone(),
                uze_home.clone(),
            ))),
            Box::new(AlwaysPresent(GeminiIntegration::new(
                agents_home.clone(),
                uze_home.clone(),
            ))),
            Box::new(AlwaysPresent(OpenCodeIntegration::new(
                agents_home.clone(),
                root.join("opencode-config.json"),
                uze_home.clone(),
            ))),
        ],
        Box::new(NoopProcessRunner),
    );

    let fake_bin = fake_codex_and_gemini_bin_dir(&root);
    let original_path = std::env::var("PATH").unwrap();
    // SAFETY: this file has exactly one test, so no concurrent access to
    // the process-global `PATH` within this binary; restored immediately
    // after use.
    unsafe {
        std::env::set_var("PATH", format!("{}:{}", fake_bin.display(), original_path));
    }

    let package_dir = skill_fixture(&root.join("fixtures"), "acme", "review");
    application
        .add_plugin(PackageSource::local(package_dir), &uze::trust::AlwaysTrust)
        .unwrap();

    let skills_dir = agents_home.join("skills");
    let entries: Vec<String> = fs::read_dir(&skills_dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        entries,
        vec!["review".to_owned()],
        "exactly one physical entry must exist for the one skill shared by \
         opencode/codex/gemini in ~/.agents/skills, not one per harness, and \
         it must be OpenCode's preferred bare name even though Codex and \
         Gemini attach first: {entries:?}"
    );

    let inspection = application.inspect_plugin("acme").unwrap();
    assert_eq!(
        inspection.managed_state.matched,
        3,
        "all three harnesses must still each have a matched receipt, even \
         though they share one physical artifact: {:?}",
        inspection
            .reconciliation
            .receipts
            .iter()
            .map(|r| (
                r.receipt.integration.clone(),
                r.inspection.state,
                r.inspection.reason.clone()
            ))
            .collect::<Vec<_>>()
    );

    // SAFETY: restoring the process-global PATH this test overrode above.
    unsafe {
        std::env::set_var("PATH", original_path);
    }
    fs::remove_dir_all(root).unwrap();
}
