//! Codex invocation-policy conformance (ADR-030): honest routing for every
//! combination, the policy sidecar, user-only lifecycle, and the
//! generated-package model-only exclusion.
//!
//! Deterministic only — every assertion here goes through `CodexIntegration`
//! and the testkit's isolated world; no vendor binary is ever spawned. See
//! the note at the bottom for where the real-Codex evidence lives.

use crate::policy::*;

#[test]
fn codex_routes_every_combination_honestly() {
    // A. model+user → Native
    let (root, home, _package, r) =
        make_policy_package("codex-a", "commit", &default_body("commit"));
    let codex = CodexIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &codex);
    assert_eq!(
        codex.exposure_plan(&r).route,
        CompatibilityRoute::Native,
        "default Skill is a normal model-discoverable Skill on Codex"
    );
    fs::remove_dir_all(root).unwrap();

    // B. user-only → Native (explicit-only policy sidecar)
    let (root, home, _package, r) =
        make_policy_package("codex-b", "review", &user_only_body("review"));
    let codex = CodexIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &codex);
    assert_eq!(codex.exposure_plan(&r).route, CompatibilityRoute::Native);
    fs::remove_dir_all(root).unwrap();

    // C. model-only → Degraded (Codex cannot hide explicit `$skill`)
    let (root, home, _package, r) =
        make_policy_package("codex-c", "legacy", &model_only_body("legacy"));
    let codex = CodexIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &codex);
    assert_eq!(
        codex.exposure_plan(&r).route,
        CompatibilityRoute::Degraded,
        "user=false cannot be enforced on Codex — honest degradation, never invented Native"
    );
    fs::remove_dir_all(root).unwrap();

    // D. invalid → Unsupported, never silently projected
    let (root, home, _package, r) = make_policy_package("codex-d", "dead", &invalid_body("dead"));
    let codex = CodexIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &codex);
    assert_eq!(
        codex.exposure_plan(&r).route,
        CompatibilityRoute::Unsupported
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn codex_user_only_wrapper_carries_the_policy_sidecar_and_never_touches_store() {
    let (root, home, package, r) =
        make_policy_package("codex-physical", "review", &user_only_body("review"));
    let store_bytes = fs::read(package.root.join("skills/review/SKILL.md")).unwrap();
    let codex = CodexIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &codex);
    let receipt = codex
        .attach_receipt(&r)
        .unwrap()
        .expect("user-only Skill attaches on Codex");
    let ManagedArtifact::SymlinkReference { path, target } = &receipt.artifact else {
        panic!("expected a managed symlink reference");
    };
    assert_eq!(path.file_name().unwrap().to_str(), Some("flow:review"));
    assert_eq!(fs::read_link(path).unwrap(), *target);
    let wrapper = fs::read_to_string(target.join("SKILL.md")).unwrap();
    assert!(
        wrapper.starts_with("---\nname: flow:review\n"),
        "the wrapper carries the stable namespaced label: {wrapper}"
    );
    assert_eq!(
        fs::read_to_string(target.join("agents/openai.yaml")).unwrap(),
        "policy:\n  allow_implicit_invocation: false\n",
        "model=false is translated into Codex's own policy sidecar"
    );
    assert_eq!(
        fs::read(package.root.join("skills/review/SKILL.md")).unwrap(),
        store_bytes,
        "Store bytes are never rewritten"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn user_only_skill_lifecycle_attach_matched_detach_missing_on_codex() {
    let (root, home, _package, r) =
        make_policy_package("codex-life", "review", &user_only_body("review"));
    let codex = CodexIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &codex);
    let receipt = codex.attach_receipt(&r).unwrap().expect("attaches");
    assert_eq!(
        receipt.resource_identity.as_deref(),
        Some(r.identity().as_str()),
        "receipt identity stays the canonical Skill resource identity"
    );
    assert_eq!(
        codex.inspect_receipt(&receipt).state,
        AttachmentState::Matched
    );
    let detached = codex.detach_receipt(&receipt).unwrap();
    assert_eq!(detached.state, AttachmentState::Missing);
    assert_eq!(
        codex.inspect_receipt(&receipt).state,
        AttachmentState::Missing
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn user_only_skill_lifecycle_attach_matched_detach_missing_on_opencode() {
    let (root, home, _package, r) =
        make_policy_package("oc-life", "review", &user_only_body("review"));
    let opencode = OpenCodeIntegration::new(
        root.join("agents"),
        root.join("config/opencode.json"),
        home.clone(),
    );
    mark_setup(&home, &opencode);
    let receipt = opencode.attach_receipt(&r).unwrap().expect("attaches");
    assert_eq!(
        opencode.inspect_receipt(&receipt).state,
        AttachmentState::Matched
    );
    assert_eq!(
        opencode.detach_receipt(&receipt).unwrap().state,
        AttachmentState::Missing
    );
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn codex_generated_package_never_claims_a_model_only_skill() {
    let (root, home, package, r) =
        make_policy_package("codex-envelope", "legacy", &model_only_body("legacy"));
    let codex = CodexIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &codex);
    let plan = codex
        .package_exposure_plan(&package, &[&r])
        .expect("generated route applies via the Skill");
    assert!(
        !plan.provided_resource_identities.contains(&r.identity()),
        "Codex cannot preserve user=false in the envelope — never claim it"
    );
    let fallback = codex.exposure_plan(&r);
    assert_eq!(
        fallback.route,
        CompatibilityRoute::Degraded,
        "the capability-level fallback reports the honest degradation"
    );
    fs::remove_dir_all(root).unwrap();
}

// The real-Codex dogfood pair that used to live here (a user-only Skill
// hidden from `codex debug prompt-input`, and the same assertion for an
// OpenCode-owned shared entry) is gone from this suite on purpose: it
// spawned the `codex` on the developer's own `PATH`, which is UZE's runtime
// shim on any dogfooding machine, so the "real harness" it measured was
// whatever the host happened to resolve — it failed here by running `uze`
// instead of Codex. Real-harness evidence belongs to the Harness
// Conformance Lab, where the binary, the HOME, and the network are the
// container's (`conformance/harnesses/codex/scenarios.py`, phase
// `skill-invocation-policy`), driven from this same
// `tests/_fixtures/canonical/workflow` fixture. Everything above stays
// here: it is deterministic delivery logic, proven through UZE's own
// integration port with no vendor process involved.
