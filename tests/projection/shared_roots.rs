//! Shared skill-root projection: Codex and OpenCode share exactly one
//! physical entry per default skill; policy-sensitive skills detect
//! cross-integration policy loss; user-only skills still install cleanly.
//! Migrated from the former `tests/skill_invocation_conformance.rs` (groups
//! 6-7).

#![allow(dead_code)] // helper subset reused across the moved tests

use std::{
    fs,
    path::{Path, PathBuf},
};

use uze::{
    PackageSource, UzeApplication, UzeHome,
    exposure::{ExposurePlan, PackageExposurePlan},
    integration::{AttachmentInspection, AttachmentReceipt, HarnessDetection, IntegrationPort},
    project::Resource as ProjectResource,
    provisioning::{ProcessResult, ProcessRunner, ProcessSpec},
    router::HarnessCapabilities,
    store::StoredPackage,
};
use uze_integrations::{codex::CodexIntegration, opencode::OpenCodeIntegration};

use uze_testkit::temp::scratch;

fn temp(label: &str) -> PathBuf {
    scratch(label)
}

fn flow_fixture() -> PathBuf {
    uze_testkit::fixtures::canonical("flow")
}

fn user_only_body(name: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: Explicit user action\ninvoke:\n  model: false\n  user: true\n---\n\nExplicit body.\n"
    )
}

fn model_only_body(name: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: Background knowledge\ninvoke:\n  model: true\n  user: false\n---\n\nBody.\n"
    )
}

fn default_body(name: &str) -> String {
    format!("---\nname: {name}\ndescription: Interactive skill\n---\n\nBody.\n")
}

fn invalid_body(name: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: Uninvokable\ninvoke:\n  model: false\n  user: false\n---\n\nBody.\n"
    )
}

/// Never spawns a process; the fake Codex (below) stands in only when a
/// generated envelope's `attach_package` shells out.
struct NoopProcessRunner;

impl ProcessRunner for NoopProcessRunner {
    fn run(&self, _spec: &ProcessSpec) -> uze::Result<ProcessResult> {
        Ok(ProcessResult {
            success: true,
            timed_out: false,
        })
    }
}

fn always_present<T: IntegrationPort>(integration: T) -> AlwaysPresent<T> {
    AlwaysPresent(integration)
}

struct AlwaysPresent<T: IntegrationPort>(T);

impl<T: IntegrationPort> IntegrationPort for AlwaysPresent<T> {
    fn id(&self) -> &'static str {
        self.0.id()
    }
    fn capabilities(&self) -> HarnessCapabilities {
        self.0.capabilities()
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
    fn status(&self, home: &UzeHome) -> uze::integration::IntegrationStatus {
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

/// A fake `codex` that answers every plugin CLI call successfully.
#[cfg(unix)]
fn fake_codex_bin_dir(root: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let dir = root.join("fake-bin");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("codex");
    fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).unwrap();
    dir
}

#[cfg(unix)]
fn with_fake_codex(root: &Path, f: impl FnOnce()) {
    let fake_bin = fake_codex_bin_dir(root);
    let mut scope = uze_testkit::env::scope();
    scope.set("PATH", uze_testkit::temp::path_prefixed(&fake_bin));
    f();
}

/// A `codex` fake that answers the two inspection commands with caller-built
/// JSON (via env vars) so integration-owned package receipts can be
/// inspected and detached truthfully in tests that exercise per-integration
/// detach/update.
#[cfg(unix)]
fn with_truthful_fake_codex(
    root: &Path,
    marketplace_json: &str,
    plugins_json: &str,
    f: impl FnOnce(),
) {
    use std::os::unix::fs::PermissionsExt;
    let dir = root.join("fake-bin");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("codex");
    fs::write(
        &path,
        "#!/bin/sh\ncase \"$1 $2 $3\" in\n  \"plugin marketplace list\") echo \"$FAKE_CODEX_MARKETPLACES\" ;;\n  \"plugin list --json\") echo \"$FAKE_CODEX_PLUGINS\" ;;\n  *) exit 0 ;;\nesac\n",
    )
    .unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).unwrap();
    let mut scope = uze_testkit::env::scope();
    scope.set("PATH", uze_testkit::temp::path_prefixed(&dir));
    scope.set("FAKE_CODEX_MARKETPLACES", marketplace_json);
    scope.set("FAKE_CODEX_PLUGINS", plugins_json);
    f();
}

/// Builds the Codex+OpenCode shared-root application with a synthetic
/// user-only `flow` package (`flow:review`). `codex_first` only flips the
/// attachment order the application iterates in — the physical result must
/// not depend on it.
#[cfg(unix)]
fn shared_user_only_app(root: &Path, codex_first: bool) -> (UzeApplication, PathBuf, UzeHome) {
    let agents_home = root.join("agents-home");
    let uze_home = UzeHome::at(root.join("uze-home"));
    let codex = Box::new(always_present(CodexIntegration::new(
        agents_home.clone(),
        uze_home.clone(),
    )));
    let opencode = Box::new(always_present(OpenCodeIntegration::new(
        agents_home.clone(),
        root.join("opencode-config.json"),
        uze_home.clone(),
    )));
    let integrations: Vec<Box<dyn IntegrationPort>> = if codex_first {
        vec![codex, opencode]
    } else {
        vec![opencode, codex]
    };
    let application = UzeApplication::new_with_runner(
        uze_home.clone(),
        integrations,
        Box::new(NoopProcessRunner),
    );
    (application, agents_home, uze_home)
}

#[cfg(unix)]
fn user_only_fixture(root: &Path) -> PathBuf {
    let fixture_root = root.join("fixture");
    fs::create_dir_all(fixture_root.join("skills/review")).unwrap();
    fs::write(
        fixture_root.join("skills/review/SKILL.md"),
        user_only_body("review"),
    )
    .unwrap();
    fs::write(
        fixture_root.join("plugin.json"),
        r#"{"name":"flow","version":"1.0.0","description":"user-only fixture"}"#,
    )
    .unwrap();
    fixture_root
}

/// `(SKILL.md bytes, openai.yaml bytes)` of the shared entry's wrapper — the
/// content that must be identical regardless of attach order.
#[cfg(unix)]
fn shared_wrapper_bytes(agents_home: &Path) -> (Vec<u8>, Vec<u8>) {
    let entry = agents_home.join("skills/flow:review");
    assert!(entry.is_symlink(), "one shared physical entry");
    let target = fs::read_link(&entry).unwrap();
    (
        fs::read(target.join("SKILL.md")).unwrap(),
        fs::read(target.join("agents/openai.yaml")).unwrap(),
    )
}

#[test]
#[cfg(unix)]
fn codex_and_opencode_reuse_one_physical_entry_for_a_default_skill() {
    with_fake_codex(&temp("shared-default"), || {
        let root = temp("shared-default");
        let agents_home = root.join("agents-home");
        let uze_home = UzeHome::at(root.join("uze-home"));
        let application = UzeApplication::new_with_runner(
            uze_home.clone(),
            vec![
                Box::new(always_present(CodexIntegration::new(
                    agents_home.clone(),
                    uze_home.clone(),
                ))),
                Box::new(always_present(OpenCodeIntegration::new(
                    agents_home.clone(),
                    root.join("opencode-config.json"),
                    uze_home.clone(),
                ))),
            ],
            Box::new(NoopProcessRunner),
        );
        application
            .plugins()
            .add(
                PackageSource::local(flow_fixture()),
                &uze::trust::AlwaysTrust,
            )
            .expect("a default Skill installs cleanly with both harnesses");
        let entry = agents_home.join("skills/flow:commit");
        assert!(entry.is_symlink());
        let target = fs::read_link(&entry).unwrap();
        assert!(
            target.is_dir() && target.join("SKILL.md").exists(),
            "one physical entry owned by the first attacher, reused by both"
        );
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
#[cfg(unix)]
fn model_only_skill_shared_root_reuse_carries_both_encodings() {
    // A model-only canonical Skill is NOT covered by Codex's generated
    // package envelope (user=false cannot be enforced there), so Codex
    // attaches it capability-level into `~/.agents/skills/flow:legacy`.
    // OpenCode then REUSES that shared entry; the superset representation
    // must carry OpenCode's `slash: false` in the same SKILL.md, so the
    // reuse passes instead of degrading into a conflict. Codex still
    // reports its own user=false limitation honestly (Degraded route);
    // only the shared PHYSICAL representation is unified (ADR-030 §25).
    with_fake_codex(&temp("shared-model-only"), || {
        let root = temp("shared-model-only");
        let agents_home = root.join("agents-home");
        let uze_home = UzeHome::at(root.join("uze-home"));
        let application = UzeApplication::new_with_runner(
            uze_home.clone(),
            vec![
                Box::new(always_present(CodexIntegration::new(
                    agents_home.clone(),
                    uze_home.clone(),
                ))),
                Box::new(always_present(OpenCodeIntegration::new(
                    agents_home.clone(),
                    root.join("opencode-config.json"),
                    uze_home.clone(),
                ))),
            ],
            Box::new(NoopProcessRunner),
        );
        let fixture_root = root.join("fixture");
        fs::create_dir_all(fixture_root.join("skills/legacy")).unwrap();
        fs::write(
            fixture_root.join("skills/legacy/SKILL.md"),
            model_only_body("legacy"),
        )
        .unwrap();
        fs::write(
            fixture_root.join("plugin.json"),
            r#"{"name":"flow","version":"1.0.0","description":"model-only fixture"}"#,
        )
        .unwrap();

        application
            .plugins()
            .add(
                PackageSource::local(&fixture_root),
                &uze::trust::AlwaysTrust,
            )
            .expect("the superset shared entry preserves both encodings");
        let entry = agents_home.join("skills/flow:legacy");
        assert!(entry.is_symlink(), "one shared physical entry");
        let target = fs::read_link(&entry).unwrap();
        let wrapper = fs::read_to_string(target.join("SKILL.md")).unwrap();
        assert!(
            wrapper.contains("slash: false"),
            "OpenCode's encoding must be present on the shared entry: {wrapper}"
        );
        assert!(
            !wrapper.contains("opencode/autoinvoke"),
            "model discovery stays enabled for a model-only Skill: {wrapper}"
        );
        assert!(
            !target.join("agents/openai.yaml").exists(),
            "no Codex policy sidecar for a model=true Skill"
        );
        // Both consumers hold a Matched receipt against the same entry.
        let receipts = uze::state::receipts(&uze_home, Some("flow@local")).unwrap();
        // The shared-entry receipts are the SymlinkReference pair; Codex
        // also holds an integration-owned package receipt (its generated
        // envelope), which is not the surface this scenario asserts.
        let shared_entry_receipt = |integration: &str| {
            receipts
                .iter()
                .find(|(_, r)| {
                    r.integration == integration
                        && matches!(
                            r.artifact,
                            uze::integration::ManagedArtifact::SymlinkReference { .. }
                        )
                })
                .map(|(_, r)| r)
                .unwrap()
        };
        let codex_receipt = shared_entry_receipt("codex");
        let opencode_receipt = shared_entry_receipt("opencode");
        let codex_inspection = CodexIntegration::new(agents_home.clone(), uze_home.clone())
            .inspect_receipt(codex_receipt);
        let opencode_inspection = OpenCodeIntegration::new(
            agents_home.clone(),
            root.join("opencode-config.json"),
            uze_home.clone(),
        )
        .inspect_receipt(opencode_receipt);
        assert_eq!(
            codex_inspection.state,
            uze::integration::AttachmentState::Matched,
            "codex: {} ({:?})",
            codex_inspection.reason,
            codex_receipt.artifact
        );
        assert_eq!(
            opencode_inspection.state,
            uze::integration::AttachmentState::Matched,
            "opencode: {} ({:?})",
            opencode_inspection.reason,
            opencode_receipt.artifact
        );
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
#[cfg(unix)]
fn foreign_shared_entry_without_opencode_encoding_still_conflicts() {
    // The superset makes UZE-generated wrappers compatible in both orders,
    // but the guard stays: a shared entry whose artifact genuinely lacks
    // OpenCode's encoding (e.g. a legacy wrapper or foreign content) must
    // fail deterministically with a ProjectionConflict — never a silent
    // semantic degradation (ADR-030 §25).
    with_fake_codex(&temp("shared-foreign"), || {
        let root = temp("shared-foreign");
        let agents_home = root.join("agents-home");
        let uze_home = UzeHome::at(root.join("uze-home"));
        let application = UzeApplication::new_with_runner(
            uze_home.clone(),
            vec![
                Box::new(always_present(CodexIntegration::new(
                    agents_home.clone(),
                    uze_home.clone(),
                ))),
                Box::new(always_present(OpenCodeIntegration::new(
                    agents_home.clone(),
                    root.join("opencode-config.json"),
                    uze_home.clone(),
                ))),
            ],
            Box::new(NoopProcessRunner),
        );
        let fixture_root = root.join("fixture");
        fs::create_dir_all(fixture_root.join("skills/legacy")).unwrap();
        fs::write(
            fixture_root.join("skills/legacy/SKILL.md"),
            model_only_body("legacy"),
        )
        .unwrap();
        fs::write(
            fixture_root.join("plugin.json"),
            r#"{"name":"flow","version":"1.0.0","description":"model-only fixture"}"#,
        )
        .unwrap();

        // A pre-existing UZE-shaped entry from a previous generation whose
        // wrapper predates the superset: it claims the shared name but
        // carries no vendor encoding at all.
        let legacy_wrapper = uze_home
            .state_dir()
            .join("attachments/codex/skills/flow/legacy");
        fs::create_dir_all(&legacy_wrapper).unwrap();
        fs::write(
            legacy_wrapper.join("SKILL.md"),
            "---\nname: flow:legacy\n---\nbody\n",
        )
        .unwrap();
        fs::create_dir_all(agents_home.join("skills")).unwrap();
        std::os::unix::fs::symlink(&legacy_wrapper, agents_home.join("skills/flow:legacy"))
            .unwrap();
        let resource_identity = uze::project::Resource::from_package(
            uze::store::PackageId::from_plugin_name("flow", &fixture_root.join("plugin.json"))
                .unwrap(),
            fixture_root.clone(),
            uze::capability::Capability {
                kind: uze::capability::CapabilityKind::AgentSkill,
                representation: uze::capability::Representation::Standard,
                path: fixture_root.join("skills/legacy/SKILL.md"),
                payload: Vec::new(),
            },
        )
        .identity();
        uze::state::record_receipt(
            &uze_home,
            "flow/codex/skill:flow:legacy".to_owned(),
            uze::integration::AttachmentReceipt {
                package_id: "flow".to_owned(),
                resource_identity: Some(resource_identity),
                integration: "codex".to_owned(),
                strategy: "managed-user-scope-reference".to_owned(),
                artifact: uze::integration::ManagedArtifact::SymlinkReference {
                    path: agents_home.join("skills/flow:legacy"),
                    target: legacy_wrapper.clone(),
                },
            },
        )
        .unwrap();

        let result = application.plugins().add(
            PackageSource::local(&fixture_root),
            &uze::trust::AlwaysTrust,
        );
        match result {
            Err(uze::UzeError::ProjectionConflict(details)) => {
                assert!(
                    details.entry.ends_with("skills/flow:legacy"),
                    "the conflict is about the shared physical entry: {}",
                    details.entry.display()
                );
                assert!(
                    details.requested_integration == "opencode",
                    "OpenCode is the integration whose invocation encoding is at stake"
                );
            }
            Err(other) => panic!("expected a deterministic ProjectionConflict, got {other:#?}"),
            Ok(_) => panic!(
                "reusing an incompatible wrapper must not silently drop the invocation policy"
            ),
        }
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
#[cfg(unix)]
fn user_only_skill_codex_and_opencode_preserves_codex_policy() {
    // P1 regression (Harness Conformance Lab): Codex and OpenCode share
    // exactly one physical `~/.agents/skills` entry. When both harnesses
    // are installed the entry is created by the second integration
    // (OpenCode, because Codex delivers the user-only Skill through its
    // generated package envelope) — and that shared entry must carry BOTH
    // vendors' encodings: Codex's `agents/openai.yaml` sidecar (without it
    // the real `codex debug prompt-input` lists the user-only Skill as
    // model-visible) and OpenCode's `opencode/autoinvoke: false`.
    with_fake_codex(&temp("shared-user-only-superset"), || {
        let root = temp("shared-user-only-superset");
        let agents_home = root.join("agents-home");
        let uze_home = UzeHome::at(root.join("uze-home"));
        let application = UzeApplication::new_with_runner(
            uze_home.clone(),
            vec![
                Box::new(always_present(CodexIntegration::new(
                    agents_home.clone(),
                    uze_home.clone(),
                ))),
                Box::new(always_present(OpenCodeIntegration::new(
                    agents_home.clone(),
                    root.join("opencode-config.json"),
                    uze_home.clone(),
                ))),
            ],
            Box::new(NoopProcessRunner),
        );
        let fixture_root = root.join("fixture");
        fs::create_dir_all(fixture_root.join("skills/review")).unwrap();
        fs::write(
            fixture_root.join("skills/review/SKILL.md"),
            user_only_body("review"),
        )
        .unwrap();
        fs::write(
            fixture_root.join("plugin.json"),
            r#"{"name":"flow","version":"1.0.0","description":"user-only fixture"}"#,
        )
        .unwrap();
        let store_bytes = fs::read(fixture_root.join("skills/review/SKILL.md")).unwrap();

        application
            .plugins()
            .add(
                PackageSource::local(&fixture_root),
                &uze::trust::AlwaysTrust,
            )
            .expect("Codex + OpenCode must both receive a user-only Skill");
        let entry = agents_home.join("skills/flow:review");
        assert!(entry.is_symlink(), "one shared physical entry");
        let target = fs::read_link(&entry).unwrap();
        assert!(
            target.join("agents/openai.yaml").is_file(),
            "the shared entry must carry Codex's policy sidecar: {}",
            target.display()
        );
        assert_eq!(
            fs::read_to_string(target.join("agents/openai.yaml")).unwrap(),
            "policy:\n  allow_implicit_invocation: false\n"
        );
        let wrapper = fs::read_to_string(target.join("SKILL.md")).unwrap();
        assert!(
            wrapper.contains("opencode/autoinvoke: false"),
            "the shared entry must carry OpenCode's native encoding: {wrapper}"
        );
        assert!(
            wrapper.starts_with("---\nname: flow:review\n"),
            "the shared entry keeps the stable namespaced label: {wrapper}"
        );
        assert_eq!(
            fs::read(fixture_root.join("skills/review/SKILL.md")).unwrap(),
            store_bytes,
            "Store bytes are never rewritten"
        );
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
#[cfg(unix)]
fn user_only_skill_installs_cleanly_with_codex_and_opencode() {
    // A user-only Skill is covered by Codex's generated package envelope
    // (policy sidecar) and attached by OpenCode as a wrapper with
    // `metadata.opencode/autoinvoke: false` — both harnesses get their
    // native encoding, and the shared root carries exactly one entry.
    with_fake_codex(&temp("shared-user-only"), || {
        let root = temp("shared-user-only");
        let agents_home = root.join("agents-home");
        let uze_home = UzeHome::at(root.join("uze-home"));
        let application = UzeApplication::new_with_runner(
            uze_home.clone(),
            vec![
                Box::new(always_present(CodexIntegration::new(
                    agents_home.clone(),
                    uze_home.clone(),
                ))),
                Box::new(always_present(OpenCodeIntegration::new(
                    agents_home.clone(),
                    root.join("opencode-config.json"),
                    uze_home.clone(),
                ))),
            ],
            Box::new(NoopProcessRunner),
        );
        let fixture_root = root.join("fixture");
        fs::create_dir_all(fixture_root.join("skills/review")).unwrap();
        fs::write(
            fixture_root.join("skills/review/SKILL.md"),
            user_only_body("review"),
        )
        .unwrap();
        fs::write(
            fixture_root.join("plugin.json"),
            r#"{"name":"flow","version":"1.0.0","description":"user-only fixture"}"#,
        )
        .unwrap();

        application
            .plugins()
            .add(
                PackageSource::local(&fixture_root),
                &uze::trust::AlwaysTrust,
            )
            .expect("each harness gets its own native encoding for a user-only Skill");
        let entry = agents_home.join("skills/flow:review");
        assert!(entry.is_symlink(), "OpenCode projects the shared entry");
        let target = fs::read_link(&entry).unwrap();
        let wrapper = fs::read_to_string(target.join("SKILL.md")).unwrap();
        assert!(
            wrapper.contains("opencode/autoinvoke: false"),
            "the shared entry carries OpenCode's invocation encoding"
        );
        assert!(
            target.join("agents/openai.yaml").is_file(),
            "the shared entry carries Codex's policy sidecar: {}",
            target.display()
        );
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
#[cfg(unix)]
fn user_only_skill_codex_only_is_model_hidden() {
    // Codex alone: the user-only Skill is delivered through the generated
    // package envelope, whose materialized skill directory must carry the
    // explicit-only policy sidecar — and nothing may leak into the shared
    // `~/.agents/skills` root with a bare name.
    with_fake_codex(&temp("codex-only-hidden"), || {
        let root = temp("codex-only-hidden");
        let agents_home = root.join("agents-home");
        let uze_home = UzeHome::at(root.join("uze-home"));
        let application = UzeApplication::new_with_runner(
            uze_home.clone(),
            vec![Box::new(always_present(CodexIntegration::new(
                agents_home.clone(),
                uze_home.clone(),
            )))],
            Box::new(NoopProcessRunner),
        );
        let fixture_root = user_only_fixture(&root);
        let store_bytes = fs::read(fixture_root.join("skills/review/SKILL.md")).unwrap();
        application
            .plugins()
            .add(
                PackageSource::local(&fixture_root),
                &uze::trust::AlwaysTrust,
            )
            .expect("Codex-only user-only Skill installs");
        assert!(
            !agents_home.join("skills/flow:review").exists(),
            "the envelope covers the skill; no shared-root entry is created"
        );
        let envelope_skill = uze_home
            .state_dir()
            .join("attachments/codex/generated/flow@local/skills/review");
        assert_eq!(
            fs::read_to_string(envelope_skill.join("agents/openai.yaml")).unwrap(),
            "policy:\n  allow_implicit_invocation: false\n",
            "the envelope-delivered Skill keeps the model hidden"
        );
        assert_eq!(
            fs::read(fixture_root.join("skills/review/SKILL.md")).unwrap(),
            store_bytes,
            "Store bytes are never rewritten"
        );
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
#[cfg(unix)]
fn attach_order_is_equivalent_codex_then_opencode_and_reverse() {
    // The superset representation must be invariant under which integration
    // happens to attach first: both orders produce byte-identical wrappers
    // carrying both encodings, under the same stable label.
    with_fake_codex(&temp("attach-order"), || {
        let root_a = temp("attach-order-a");
        let root_b = temp("attach-order-b");
        let (app_a, agents_a, _) = shared_user_only_app(&root_a, true);
        let (app_b, agents_b, _) = shared_user_only_app(&root_b, false);
        for (app, root) in [(&app_a, &root_a), (&app_b, &root_b)] {
            app.plugins()
                .add(
                    PackageSource::local(user_only_fixture(root)),
                    &uze::trust::AlwaysTrust,
                )
                .expect("install succeeds in either order");
        }
        let (skill_a, policy_a) = shared_wrapper_bytes(&agents_a);
        let (skill_b, policy_b) = shared_wrapper_bytes(&agents_b);
        assert_eq!(
            skill_a, skill_b,
            "Codex-then-OpenCode and OpenCode-then-Codex produce the same SKILL.md"
        );
        assert_eq!(
            policy_a, policy_b,
            "both orders carry the same Codex policy sidecar"
        );
        assert!(
            String::from_utf8_lossy(&skill_a).contains("opencode/autoinvoke: false"),
            "the superset wrapper keeps OpenCode valid"
        );
        assert_eq!(
            agents_a.join("skills/flow:review").to_string_lossy(),
            agents_b
                .join("skills/flow:review")
                .to_string_lossy()
                .replace(
                    root_b.to_string_lossy().as_ref(),
                    root_a.to_string_lossy().as_ref()
                ),
            "the stable label stays flow:review in both orders"
        );
        fs::remove_dir_all(&root_a).unwrap();
        fs::remove_dir_all(&root_b).unwrap();
    });
}

#[test]
#[cfg(unix)]
fn repeated_setup_is_idempotent() {
    // Re-running the full install+attach lifecycle (what `uze update` does)
    // must leave the shared projection byte-identical and every receipt
    // still Matched — repeated setup never churns the physical entry.
    let root = temp("idempotent");
    let (app, agents_home, uze_home) = shared_user_only_app(&root, true);
    let generated_root = uze_home.state_dir().join("attachments/codex/generated");
    let marketplaces = format!(
        r#"{{"marketplaces":[{{"name":"uze-store","root":"{}"}}]}}"#,
        generated_root.display()
    );
    let plugins = format!(
        r#"{{"installed":[{{"pluginId":"flow@uze-store","enabled":true,"installed":true,"marketplaceName":"uze-store","path":"{}"}}]}}"#,
        generated_root.join("flow@local").display()
    );
    with_truthful_fake_codex(&root, &marketplaces, &plugins, || {
        app.plugins()
            .add(
                PackageSource::local(user_only_fixture(&root)),
                &uze::trust::AlwaysTrust,
            )
            .expect("initial install");
        let before = shared_wrapper_bytes(&agents_home);
        app.plugins()
            .update("flow", &uze::trust::AlwaysTrust)
            .expect("repeated setup (update) succeeds");
        let after = shared_wrapper_bytes(&agents_home);
        assert_eq!(
            before, after,
            "update leaves the shared wrapper byte-identical"
        );
        let receipt = uze::state::receipts(&uze_home, Some("flow@local"))
            .unwrap()
            .into_iter()
            .find(|(_, r)| r.integration == "opencode")
            .map(|(_, r)| r)
            .unwrap();
        assert_eq!(
            OpenCodeIntegration::new(
                agents_home.clone(),
                root.join("opencode-config.json"),
                uze_home.clone(),
            )
            .inspect_receipt(&receipt)
            .state,
            uze::integration::AttachmentState::Matched
        );
        fs::remove_dir_all(&root).unwrap();
    });
}

#[test]
#[cfg(unix)]
fn detach_codex_preserves_opencode_consumer() {
    // The P1 delivery shape: Codex owns a package receipt (generated
    // envelope), OpenCode owns the shared entry. Detaching Codex must not
    // touch the shared entry OpenCode still consumes.
    let root = temp("detach-codex");
    let (app, agents_home, uze_home) = shared_user_only_app(&root, true);
    let generated_root = uze_home.state_dir().join("attachments/codex/generated");
    let marketplaces = format!(
        r#"{{"marketplaces":[{{"name":"uze-store","root":"{}"}}]}}"#,
        generated_root.display()
    );
    let plugins = format!(
        r#"{{"installed":[{{"pluginId":"flow@uze-store","enabled":true,"installed":true,"marketplaceName":"uze-store","path":"{}"}}]}}"#,
        generated_root.join("flow@local").display()
    );
    with_truthful_fake_codex(&root, &marketplaces, &plugins, || {
        app.plugins()
            .add(
                PackageSource::local(user_only_fixture(&root)),
                &uze::trust::AlwaysTrust,
            )
            .expect("install");
        let codex_receipt = uze::state::receipts(&uze_home, Some("flow@local"))
            .unwrap()
            .into_iter()
            .find(|(_, r)| {
                r.integration == "codex"
                    && matches!(
                        r.artifact,
                        uze::integration::ManagedArtifact::IntegrationOwned { .. }
                    )
            })
            .map(|(_, r)| r)
            .unwrap();
        let detached = CodexIntegration::new(agents_home.clone(), uze_home.clone())
            .detach_receipt(&codex_receipt)
            .unwrap();
        assert_eq!(
            detached.state,
            uze::integration::AttachmentState::Missing,
            "codex: {} ({:?})",
            detached.reason,
            codex_receipt.artifact
        );
        assert!(
            !generated_root.join("flow@local").exists(),
            "the generated envelope is cleaned with Codex's receipt"
        );
        let entry = agents_home.join("skills/flow:review");
        assert!(
            entry.is_symlink(),
            "OpenCode's shared entry survives Codex detach"
        );
        let opencode_receipt = uze::state::receipts(&uze_home, Some("flow@local"))
            .unwrap()
            .into_iter()
            .find(|(_, r)| r.integration == "opencode")
            .map(|(_, r)| r)
            .unwrap();
        assert_eq!(
            OpenCodeIntegration::new(
                agents_home.clone(),
                root.join("opencode-config.json"),
                uze_home.clone(),
            )
            .inspect_receipt(&opencode_receipt)
            .state,
            uze::integration::AttachmentState::Matched
        );
        fs::remove_dir_all(&root).unwrap();
    });
}

#[test]
#[cfg(unix)]
fn detach_opencode_preserves_codex_consumer() {
    // Detaching OpenCode removes the shared entry (it is OpenCode's) but
    // leaves Codex's own representation — the generated envelope — intact.
    let root = temp("detach-opencode");
    let (app, agents_home, uze_home) = shared_user_only_app(&root, true);
    let generated_root = uze_home.state_dir().join("attachments/codex/generated");
    let marketplaces = format!(
        r#"{{"marketplaces":[{{"name":"uze-store","root":"{}"}}]}}"#,
        generated_root.display()
    );
    let plugins = format!(
        r#"{{"installed":[{{"pluginId":"flow@uze-store","enabled":true,"installed":true,"marketplaceName":"uze-store","path":"{}"}}]}}"#,
        generated_root.join("flow@local").display()
    );
    with_truthful_fake_codex(&root, &marketplaces, &plugins, || {
        app.plugins()
            .add(
                PackageSource::local(user_only_fixture(&root)),
                &uze::trust::AlwaysTrust,
            )
            .expect("install");
        let opencode_receipt = uze::state::receipts(&uze_home, Some("flow@local"))
            .unwrap()
            .into_iter()
            .find(|(_, r)| r.integration == "opencode")
            .map(|(_, r)| r)
            .unwrap();
        let default_target = match &opencode_receipt.artifact {
            uze::integration::ManagedArtifact::SymlinkReference { target, .. } => target.clone(),
            other => panic!("expected a symlink receipt, got {other:?}"),
        };
        let detached = OpenCodeIntegration::new(
            agents_home.clone(),
            root.join("opencode-config.json"),
            uze_home.clone(),
        )
        .detach_receipt(&opencode_receipt)
        .unwrap();
        assert_eq!(detached.state, uze::integration::AttachmentState::Missing);
        assert!(
            !agents_home.join("skills/flow:review").exists(),
            "the shared entry is removed with OpenCode's receipt"
        );
        assert!(
            !default_target.exists(),
            "OpenCode's wrapper is cleaned when unreferenced"
        );
        assert!(
            generated_root.join("flow@local").exists(),
            "Codex's generated envelope is untouched by OpenCode detach"
        );
        assert!(
            uze::state::receipts(&uze_home, Some("flow@local"))
                .unwrap()
                .iter()
                .any(|(_, r)| r.integration == "codex"),
            "Codex's own receipt stays recorded"
        );
        fs::remove_dir_all(&root).unwrap();
    });
}

#[test]
#[cfg(unix)]
fn detach_last_consumer_cleans_projection() {
    // After the last consumer detaches, the shared derived projection is
    // fully gone: no entry, no wrapper, no generated envelope.
    let root = temp("detach-last");
    let (app, agents_home, uze_home) = shared_user_only_app(&root, true);
    let generated_root = uze_home.state_dir().join("attachments/codex/generated");
    let marketplaces = format!(
        r#"{{"marketplaces":[{{"name":"uze-store","root":"{}"}}]}}"#,
        generated_root.display()
    );
    let plugins = format!(
        r#"{{"installed":[{{"pluginId":"flow@uze-store","enabled":true,"installed":true,"marketplaceName":"uze-store","path":"{}"}}]}}"#,
        generated_root.join("flow@local").display()
    );
    with_truthful_fake_codex(&root, &marketplaces, &plugins, || {
        app.plugins()
            .add(
                PackageSource::local(user_only_fixture(&root)),
                &uze::trust::AlwaysTrust,
            )
            .expect("install");
        let receipts: Vec<_> = uze::state::receipts(&uze_home, Some("flow@local"))
            .unwrap()
            .into_iter()
            .map(|(_, r)| r)
            .collect();
        let opencode = OpenCodeIntegration::new(
            agents_home.clone(),
            root.join("opencode-config.json"),
            uze_home.clone(),
        );
        let codex = CodexIntegration::new(agents_home.clone(), uze_home.clone());
        for receipt in &receipts {
            let integration: &dyn IntegrationPort = if receipt.integration == "opencode" {
                &opencode
            } else {
                &codex
            };
            integration
                .detach_receipt(receipt)
                .expect("detach always succeeds in the P1 shared shape");
        }
        assert!(
            !agents_home.join("skills/flow:review").exists(),
            "no shared entry remains after the last consumer detaches"
        );
        assert!(
            !generated_root.join("flow@local").exists(),
            "the generated envelope is gone with its receipt"
        );
        let stale_opencode = uze::state::receipts(&uze_home, Some("flow@local"))
            .unwrap()
            .into_iter()
            .find(|(_, r)| r.integration == "opencode")
            .map(|(_, r)| opencode.inspect_receipt(&r).state)
            .unwrap();
        assert_eq!(
            stale_opencode,
            uze::integration::AttachmentState::Missing,
            "the last consumer's artifact is gone; the ledger entry is the app's to forget"
        );
        fs::remove_dir_all(&root).unwrap();
    });
}
