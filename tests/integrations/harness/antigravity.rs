//! Antigravity invocation-policy conformance (ADR-030): honest routing,
//! both halves of the policy carried by the vendor's own front-matter
//! controls, and package exclusion for non-default policies.
//!
//! The user-only half was Adapted until agy 1.1.27, which had no inverse
//! of `disable-slash-command`. UZE said so rather than inventing the
//! control; the vendor has since shipped `disable-model-invocation`, and
//! these tests moved with it. What is pinned is not the verdict but the
//! rule: a route is Native only when a vendor control carries the
//! semantics, and the wrapper emits nothing the vendor does not read.

use crate::policy::*;

#[test]
fn antigravity_routes_every_combination_honestly() {
    // A. model+user → Native
    let (root, home, _package, r) = make_policy_package("agy-a", "commit", &default_body("commit"));
    let agy = AntigravityIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &agy);
    assert_eq!(agy.exposure_plan(&r).route, CompatibilityRoute::Native);
    fs::remove_dir_all(root).unwrap();

    // B. user-only → Native (`disable-model-invocation: true`)
    let (root, home, _package, r) =
        make_policy_package("agy-b", "review", &user_only_body("review"));
    let agy = AntigravityIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &agy);
    let plan = agy.exposure_plan(&r);
    assert_eq!(
        plan.route,
        CompatibilityRoute::Native,
        "agy 1.1.27 hides a Skill from the model — the evidence must name the control"
    );
    assert!(
        plan.evidence.contains("disable-model-invocation"),
        "a Native claim must name the vendor control that carries it: {}",
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
fn antigravity_user_only_wrapper_carries_the_vendors_own_control() {
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
        wrapper.contains("disable-model-invocation: true"),
        "the control agy reads from front matter is what carries invoke.model=false: {wrapper}"
    );
    assert!(
        !wrapper.contains("disable-slash-command"),
        "a user-only Skill stays slash-invocable: {wrapper}"
    );
    // Still no invented surface: the policy is front matter the vendor
    // parses, never a second artifact UZE made up.
    assert!(!target.join("agents").exists());
    assert_eq!(plan.route, CompatibilityRoute::Native);
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
    assert_eq!(fallback.route, CompatibilityRoute::Native);
    fs::remove_dir_all(root).unwrap();
}
