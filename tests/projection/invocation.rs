//! Stable Namespaced Invocation Labels (ADR-026) — conformance contract.
//!
//! Every UZE-projected Skill gets a stable, plugin-qualified invocation
//! label (`<plugin>:<capability>`, e.g. `flow:review`) as its single
//! candidate — deterministic, independent of installation order and of
//! which other plugins are installed, with no bare aliases in v0. The label
//! is a *presentation* concern: canonical Resource identity, Store bytes,
//! package layout, coverage identities and capability bodies are never
//! touched. Each harness encodes the label in its own physical
//! representation (vendor owns physical syntax): Claude via its native
//! plugin namespace, Codex verbatim (`flow:review` — verified against
//! codex-cli 0.149.0), OpenCode verbatim as the physical directory name
//! (its skill ID comes from the path), Antigravity verbatim
//! (`flow:review` — `agy plugin validate` accepts `:` in skill names).
//!
//! The canonical model has ONE Skill kind (ADR-030); the label is the
//! same `<plugin>:<skill>` for every invocation policy.

use std::{
    fs,
    path::{Path, PathBuf},
};

use uze::integrations::{
    antigravity::AntigravityIntegration, claude::ClaudeIntegration, codex::CodexIntegration,
    opencode::OpenCodeIntegration,
};
use uze::{
    PackageSource, Resource, UzeApplication, UzeEngine, UzeHome, UzeStore,
    capability::CapabilityKind,
    exposure::{ExposurePlan, PackageExposurePlan},
    integration::{
        AttachmentInspection, AttachmentReceipt, HarnessDetection, IntegrationPort,
        ManagedArtifact, qualified_capability_name,
    },
    project::Resource as ProjectResource,
    provisioning::{ProcessResult, ProcessRunner, ProcessSpec},
    router::{CompatibilityRoute, HarnessCapabilities},
    state,
    store::StoredPackage,
};

fn temp(label: &str) -> PathBuf {
    uze_testkit::temp::scratch(label)
}

fn workflow_fixture() -> PathBuf {
    uze_testkit::fixtures::canonical("workflow")
}

fn install(store: &UzeStore, path: impl Into<PathBuf>) -> uze::Result<StoredPackage> {
    store.ingest(&uze::acquisition::acquire(&PackageSource::local(path))?)
}

fn mark_setup(home: &UzeHome, integration: &dyn IntegrationPort) {
    state::record(
        home,
        state::IntegrationRecord {
            harness: integration.id().to_owned(),
            version: Some("test".to_owned()),
            strategy: "test".to_owned(),
            installed: true,
        },
    )
    .unwrap();
}

fn stored_workflow(label: &str) -> (PathBuf, UzeHome, StoredPackage, Vec<Resource>) {
    let root = temp(label);
    let home = UzeHome::at(&root);
    let store = UzeStore::new(home.clone());
    let package = install(&store, workflow_fixture()).unwrap();
    let environment = UzeEngine::new(store)
        .compose(std::slice::from_ref(&package.id))
        .unwrap();
    (root, home, package, environment.resources)
}

fn skills_of(resources: &[Resource]) -> &Resource {
    resources
        .iter()
        .find(|r| r.capability.kind == CapabilityKind::AgentSkill)
        .unwrap()
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

// --- 1-2. Stable label derived from plugin id --------------------------------

#[test]
fn label_is_plugin_namespace_plus_capability_name_and_stable() {
    assert_eq!(qualified_capability_name("flow", "review"), "flow:review");
    assert_eq!(
        qualified_capability_name("openspec", "proposal"),
        "openspec:proposal"
    );
    assert_eq!(
        qualified_capability_name("security", "audit"),
        "security:audit"
    );

    let (root, _home, _package, resources) = stored_workflow("label-stable");
    let skill = skills_of(&resources);
    for integration in [
        Box::new(ClaudeIntegration::new(
            root.join("claude"),
            UzeHome::at(&root),
        )) as Box<dyn IntegrationPort>,
        Box::new(CodexIntegration::new(
            root.join("agents"),
            UzeHome::at(&root),
        )) as Box<dyn IntegrationPort>,
        Box::new(OpenCodeIntegration::new(
            root.join("agents"),
            root.join("config/opencode/opencode.json"),
            UzeHome::at(&root),
        )) as Box<dyn IntegrationPort>,
        Box::new(AntigravityIntegration::new(
            root.join("agents"),
            UzeHome::at(&root),
        )) as Box<dyn IntegrationPort>,
    ] {
        let first = integration.exposure_name_candidates(skill);
        let second = integration.exposure_name_candidates(skill);
        assert_eq!(first, second, "the label must be stable per integration");
        assert_eq!(
            first.len(),
            1,
            "exactly one candidate, never bare+qualified"
        );
    }
    let opencode = OpenCodeIntegration::new(
        root.join("agents"),
        root.join("config/opencode/opencode.json"),
        UzeHome::at(&root),
    );
    assert_eq!(
        opencode.exposure_name_candidates(skill),
        vec!["workflow:review"]
    );
    fs::remove_dir_all(root).unwrap();
}

// --- 3-5. Installation order and coexisting same-named capabilities ---------

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
        format!("---\nname: {skill_name}\ndescription: fixture.\n---\n\nBody.\n"),
    )
    .unwrap();
    dir
}

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

#[test]
fn installing_another_plugin_never_renames_an_existing_one() {
    let root = temp("order-stability");
    let (application, agents_home) = app_with_opencode(&root);

    application
        .add_plugin(
            PackageSource::local(skill_fixture(&root.join("f"), "alpha", "review")),
            &uze::trust::AlwaysTrust,
        )
        .unwrap();
    let skills_dir = agents_home.join("skills");
    let alpha_before = skills_dir.join("alpha:review");
    assert!(alpha_before.is_symlink());

    // Installing a second plugin with the SAME logical name must not rename
    // or disturb the first.
    application
        .add_plugin(
            PackageSource::local(skill_fixture(&root.join("f"), "beta", "review")),
            &uze::trust::AlwaysTrust,
        )
        .unwrap();
    assert!(alpha_before.is_symlink(), "alpha:review is untouched");
    assert!(skills_dir.join("beta:review").is_symlink());

    // Reverse order yields the exact same labels per plugin.
    let root2 = temp("order-reverse");
    let (application2, agents2) = app_with_opencode(&root2);
    application2
        .add_plugin(
            PackageSource::local(skill_fixture(&root2.join("f"), "beta", "review")),
            &uze::trust::AlwaysTrust,
        )
        .unwrap();
    application2
        .add_plugin(
            PackageSource::local(skill_fixture(&root2.join("f"), "alpha", "review")),
            &uze::trust::AlwaysTrust,
        )
        .unwrap();
    let skills2 = agents2.join("skills");
    assert!(skills2.join("alpha:review").is_symlink());
    assert!(skills2.join("beta:review").is_symlink());
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(root2).unwrap();
}

#[test]
fn same_named_skills_from_two_packages_are_independently_addressable() {
    let root = temp("skill-independence");
    let (application, agents_home) = app_with_opencode(&root);
    application
        .add_plugin(
            PackageSource::local(skill_fixture(&root.join("f"), "alpha", "review")),
            &uze::trust::AlwaysTrust,
        )
        .unwrap();
    application
        .add_plugin(
            PackageSource::local(skill_fixture(&root.join("f"), "beta", "review")),
            &uze::trust::AlwaysTrust,
        )
        .unwrap();
    let skills_dir = agents_home.join("skills");
    assert!(skills_dir.join("alpha:review").is_symlink());
    assert!(skills_dir.join("beta:review").is_symlink());
    assert!(
        !skills_dir.join("review").exists(),
        "no bare alias is created"
    );
    fs::remove_dir_all(root).unwrap();
}

// --- 6-8. Identity/Store/receipt invariants ---------------------------------

#[test]
fn labels_never_touch_canonical_identity_store_or_receipts() {
    let (root, home, package, resources) = stored_workflow("invariants");
    let skill = skills_of(&resources);
    let canonical_identity = skill.identity();
    assert!(canonical_identity.contains("skills/review/SKILL.md"));
    assert_eq!(skill.logical_capability_name().as_deref(), Some("review"));

    let codex = CodexIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &codex);
    let receipt = codex.attach_receipt(skill).unwrap().expect("attaches");
    // Receipts keep the canonical identity; the label lives only in the
    // physical artifact name.
    assert_eq!(
        receipt.resource_identity.as_deref().unwrap(),
        canonical_identity
    );
    let ManagedArtifact::SymlinkReference { path, .. } = &receipt.artifact else {
        panic!("expected symlink artifact");
    };
    assert_eq!(path.file_name().unwrap(), "workflow:review");
    // Store bytes stay byte-identical.
    assert_eq!(
        fs::read(package.root.join("skills/review/SKILL.md")).unwrap(),
        fs::read(workflow_fixture().join("skills/review/SKILL.md")).unwrap()
    );
    assert!(
        !package.root.join("state").exists() && !package.root.join("attachments").exists(),
        "derived artifacts never land in the Store"
    );
    fs::remove_dir_all(root).unwrap();
}

// --- 9. Claude does not double-namespace ------------------------------------

#[test]
fn claude_declares_plain_and_namespaces_natively_without_double_prefix() {
    let (root, home, package, resources) = stored_workflow("claude-plain");
    let claude = ClaudeIntegration::new(root.join("claude"), home.clone());
    let resources: Vec<_> = resources.iter().collect();
    let plan = claude
        .package_exposure_plan(&package, &resources)
        .expect("generatable");
    assert_eq!(plan.route, CompatibilityRoute::Native);
    // UZE never materializes the namespace into the plugin: coverage
    // identities stay canonical and the generated manifest keeps the plain
    // `skills` surface (Claude itself produces `/workflow:review`).
    for identity in &plan.provided_resource_identities {
        assert!(
            identity.contains("skills/review/SKILL.md"),
            "coverage stays on canonical identities: {identity}"
        );
        assert!(
            !identity.contains("workflow:review"),
            "invocation labels are never coverage identities"
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn claude_shim_namespace_matches_plugin_and_never_double_prefixes() {
    #[cfg(unix)]
    {
        let root = temp("claude-shim");
        let home = UzeHome::at(&root);
        let store = UzeStore::new(home.clone());
        let package = install(&store, workflow_fixture()).unwrap();
        let environment = UzeEngine::new(store)
            .compose(std::slice::from_ref(&package.id))
            .unwrap();
        let claude = ClaudeIntegration::new(root.join("claude"), home.clone());
        mark_setup(&home, &claude);
        let skill = skills_of(&environment.resources);
        let receipt = claude.attach_receipt(skill).unwrap().expect("attaches");
        let ManagedArtifact::SymlinkReference { path, .. } = &receipt.artifact else {
            panic!("expected symlink artifact");
        };
        assert_eq!(path.file_name().unwrap(), "workflow:review");
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(path.join(".claude-plugin/plugin.json")).unwrap())
                .unwrap();
        assert_eq!(
            manifest["name"], "workflow",
            "the shim's plugin name is the namespace, so Claude exposes \
             `/workflow:review`, never `/workflow:workflow:review`"
        );
        assert_ne!(
            manifest["name"].as_str().unwrap(),
            "workflow:review",
            "the label is the directory name, never the plugin name"
        );
        fs::remove_dir_all(root).unwrap();
    }
}

// --- 10-12. Per-harness physical representation -----------------------------

#[test]
fn physical_representations_preserve_the_semantic_label() {
    let (root, home, _package, resources) = stored_workflow("physical");
    let skill = skills_of(&resources);

    // OpenCode: the physical directory name IS the label (skill ID comes
    // from the path, not the frontmatter).
    let opencode = OpenCodeIntegration::new(
        root.join("agents"),
        root.join("config/opencode/opencode.json"),
        home.clone(),
    );
    mark_setup(&home, &opencode);
    assert_eq!(
        opencode.exposure_name_candidates(skill),
        vec!["workflow:review"]
    );

    // Codex: verbatim colon label.
    let codex = CodexIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &codex);
    assert_eq!(
        codex.exposure_name_candidates(skill),
        vec!["workflow:review"]
    );

    // Antigravity: verbatim colon label (vendor name pattern accepts `:` in
    // skill names — verified against agy's own `plugin validate`).
    let antigravity = AntigravityIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &antigravity);
    assert_eq!(
        antigravity.exposure_name_candidates(skill),
        vec!["workflow:review"]
    );
    fs::remove_dir_all(root).unwrap();
}

// --- 13-14. Coverage identities unchanged -----------------------------------

#[test]
fn package_coverage_keeps_canonical_identities() {
    let (root, home, package, resources) = stored_workflow("coverage-generated");
    let resources: Vec<_> = resources.iter().collect();
    // Claude's generated envelope preserves the user-only policy (marker
    // injection) and covers exactly the one Skill, on its canonical identity.
    let claude = ClaudeIntegration::new(root.join("claude"), home.clone());
    let plan = claude
        .package_exposure_plan(&package, &resources)
        .expect("generatable");
    assert_eq!(plan.provided_resource_identities.len(), 1);
    // Antigravity cannot preserve the user-only policy in a native plugin,
    // so it covers NOTHING at package level (honest semantic coverage).
    let antigravity = AntigravityIntegration::new(root.join("agents"), home.clone());
    let aplan = antigravity
        .package_exposure_plan(&package, &resources)
        .expect("natively expressible");
    assert_eq!(
        aplan.provided_resource_identities.len(),
        0,
        "user-only semantics degrade on Antigravity; never claimed as covered"
    );
    for identity in plan.provided_resource_identities.iter() {
        assert!(
            identity.contains("skills/review/SKILL.md"),
            "coverage identity stays canonical: {identity}"
        );
    }
    fs::remove_dir_all(root).unwrap();
}

// --- 15-16. MCP unchanged ---------------------------------------------------

#[test]
fn mcp_naming_is_unchanged() {
    let root = temp("mcp-unchanged");
    let store = UzeStore::new(UzeHome::at(&root));
    let mcp_fixture = uze_testkit::fixtures::canonical("mcp-plugin");
    let package = install(&store, mcp_fixture).unwrap();
    let environment = UzeEngine::new(store)
        .compose(std::slice::from_ref(&package.id))
        .unwrap();
    let mcp = &environment.resources[0];
    assert_eq!(mcp.capability.kind, CapabilityKind::Mcp);
    // MCP keeps the legacy fully-qualified dash form, untouched.
    let opencode = OpenCodeIntegration::new(
        root.join("agents"),
        root.join("config/opencode/opencode.json"),
        UzeHome::at(&root),
    );
    assert_eq!(
        opencode.exposure_name_candidates(mcp),
        vec!["uze-mcp-conformance-uze-conformance".to_owned()]
    );

    let (root2, _home, _package, resources) = stored_workflow("skill-label");
    assert_eq!(
        skills_of(&resources).logical_capability_name().as_deref(),
        Some("review")
    );
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(root2).unwrap();
}
