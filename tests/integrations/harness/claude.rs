//! Claude invocation-policy conformance (ADR-030): routing at capability
//! level, the user-only disable-model shim, and generated-package
//! user-only marker materialization.

use crate::policy::*;

#[test]
fn claude_routes_every_combination_at_capability_level() {
    let (root, home, _package, r) =
        make_policy_package("claude-a", "commit", &default_body("commit"));
    let claude = ClaudeIntegration::new(root.join("claude"), home.clone());
    mark_setup(&home, &claude);
    assert_eq!(
        claude.exposure_plan(&r).route,
        CompatibilityRoute::Adaptable
    );
    fs::remove_dir_all(root).unwrap();

    let (root, home, _package, r) =
        make_policy_package("claude-b", "review", &user_only_body("review"));
    let claude = ClaudeIntegration::new(root.join("claude"), home.clone());
    mark_setup(&home, &claude);
    assert_eq!(
        claude.exposure_plan(&r).route,
        CompatibilityRoute::Adaptable
    );
    fs::remove_dir_all(root).unwrap();

    let (root, home, _package, r) = make_policy_package("claude-d", "dead", &invalid_body("dead"));
    let claude = ClaudeIntegration::new(root.join("claude"), home.clone());
    mark_setup(&home, &claude);
    assert_eq!(
        claude.exposure_plan(&r).route,
        CompatibilityRoute::Unsupported
    );
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn claude_user_only_shim_carries_the_disable_model_marker() {
    let (root, home, package, r) =
        make_policy_package("claude-physical", "review", &user_only_body("review"));
    let store_bytes = fs::read(package.root.join("skills/review/SKILL.md")).unwrap();
    let claude = ClaudeIntegration::new(root.join("claude"), home.clone());
    mark_setup(&home, &claude);
    let receipt = claude
        .attach_receipt(&r)
        .unwrap()
        .expect("user-only Skill attaches on Claude");
    let ManagedArtifact::SymlinkReference { target, .. } = &receipt.artifact else {
        panic!("expected a managed symlink reference");
    };
    let shim_skill = fs::read_to_string(target.join("SKILL.md")).unwrap();
    assert!(
        shim_skill.contains("disable-model-invocation: true\n"),
        "model=false is translated into Claude's own marker: {shim_skill}"
    );
    assert_eq!(
        fs::read(package.root.join("skills/review/SKILL.md")).unwrap(),
        store_bytes,
        "the canonical bytes stay untouched"
    );
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn claude_generated_package_covers_a_user_only_skill_and_materializes_the_marker() {
    let (root, home, package, r) =
        make_policy_package("claude-envelope", "review", &user_only_body("review"));
    let claude = ClaudeIntegration::new(root.join("claude"), home.clone());
    let plan = claude
        .package_exposure_plan(&package, &[&r])
        .expect("generated route applies");
    assert!(
        plan.provided_resource_identities.contains(&r.identity()),
        "Claude preserves user-only semantics in the generated envelope (marker injection)"
    );
    // `republish_packages` materializes the generated envelope (the only
    // path that rebuilds derived artifact directories).
    claude.republish_packages(&[package]).unwrap();
    // The generated envelope materializes the marker file, not a symlink of
    // the raw canonical bytes.
    let generated_root = home.state_dir().join("attachments/claude/generated");
    let generated_skill = generated_root.join("flow/skills/review/SKILL.md");
    assert!(
        generated_skill.is_file(),
        "materialized SKILL.md expected at {}",
        generated_skill.display()
    );
    let content = fs::read_to_string(&generated_skill).unwrap();
    assert!(content.contains("disable-model-invocation: true\n"));
    fs::remove_dir_all(root).unwrap();
}
