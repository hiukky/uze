//! Antigravity invocation-policy conformance (ADR-030): honest routing
//! with stated user-only degradation, the model-only native wrapper, and
//! package exclusion for non-default policies.

use crate::policy::*;

#[test]
fn antigravity_routes_every_combination_honestly() {
    // A. model+user → Native
    let (root, home, _package, r) = make_policy_package("agy-a", "commit", &default_body("commit"));
    let agy = AntigravityIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &agy);
    assert_eq!(agy.exposure_plan(&r).route, CompatibilityRoute::Native);
    fs::remove_dir_all(root).unwrap();

    // B. user-only → Adapted, degradation explicit (no model-hiding exists)
    let (root, home, _package, r) =
        make_policy_package("agy-b", "review", &user_only_body("review"));
    let agy = AntigravityIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &agy);
    let plan = agy.exposure_plan(&r);
    assert_eq!(
        plan.route,
        CompatibilityRoute::Adaptable,
        "Antigravity cannot hide a Skill from the model — Adapted, honestly"
    );
    assert!(
        plan.evidence
            .contains("invoke.model=false cannot be enforced"),
        "the degradation must be stated, never hidden: {}",
        plan.evidence
    );
    fs::remove_dir_all(root).unwrap();

    // C. model-only → Native (`disable-slash-command: true`)
    let (root, home, _package, r) =
        make_policy_package("agy-c", "legacy", &model_only_body("legacy"));
    let agy = AntigravityIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &agy);
    assert_eq!(agy.exposure_plan(&r).route, CompatibilityRoute::Native);
    fs::remove_dir_all(root).unwrap();

    // D. invalid → Unsupported
    let (root, home, _package, r) = make_policy_package("agy-d", "dead", &invalid_body("dead"));
    let agy = AntigravityIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &agy);
    assert_eq!(agy.exposure_plan(&r).route, CompatibilityRoute::Unsupported);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn antigravity_model_only_wrapper_hides_the_slash_command() {
    let (root, home, _package, r) =
        make_policy_package("agy-model-only", "analyze", &model_only_body("analyze"));
    let agy = AntigravityIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &agy);
    let receipt = agy
        .attach_receipt(&r)
        .unwrap()
        .expect("model-only Skill attaches on Antigravity");
    let ManagedArtifact::SymlinkReference { target, .. } = &receipt.artifact else {
        panic!("expected a managed symlink reference");
    };
    let wrapper = fs::read_to_string(target.join("SKILL.md")).unwrap();
    assert!(
        wrapper.contains("disable-slash-command: true"),
        "the current AGY-native model-only control is emitted: {wrapper}"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn antigravity_user_only_wrapper_has_no_forced_policy() {
    let (root, home, _package, r) =
        make_policy_package("agy-physical", "review", &user_only_body("review"));
    let agy = AntigravityIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &agy);
    let plan = agy.exposure_plan(&r);
    let receipt = agy
        .attach_receipt(&r)
        .unwrap()
        .expect("user-only Skill attaches on Antigravity");
    let ManagedArtifact::SymlinkReference { target, .. } = &receipt.artifact else {
        panic!("expected a managed symlink reference");
    };
    let wrapper = fs::read_to_string(target.join("SKILL.md")).unwrap();
    assert!(
        !wrapper.contains("disable-model-invocation") && !target.join("agents").exists(),
        "Antigravity has no explicit-only mechanism — UZE must not invent one"
    );
    assert_eq!(plan.route, CompatibilityRoute::Adaptable);
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn antigravity_generated_package_never_claims_a_user_only_skill() {
    let (root, home, package, r) =
        make_policy_package("agy-envelope", "review", &user_only_body("review"));
    let agy = AntigravityIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &agy);
    let plan = agy.package_exposure_plan(&package, &[&r]).is_none();
    assert!(
        plan,
        "a non-default Skill must not be staged unchanged inside a plugin"
    );
    let fallback = agy.exposure_plan(&r);
    assert_eq!(fallback.route, CompatibilityRoute::Adaptable);
    fs::remove_dir_all(root).unwrap();
}
