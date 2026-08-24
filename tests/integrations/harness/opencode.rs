//! OpenCode invocation-policy conformance (ADR-030): native routing with
//! `autoinvoke`/`slash: false` metadata and user-only lifecycle.

use crate::policy::*;

#[test]
fn opencode_routes_every_combination_natively() {
    let combinations = [
        (
            "default",
            default_body("commit"),
            CompatibilityRoute::Native,
        ),
        (
            "user-only",
            user_only_body("review"),
            CompatibilityRoute::Native,
        ),
        (
            "model-only",
            model_only_body("legacy"),
            CompatibilityRoute::Native,
        ),
        (
            "invalid",
            invalid_body("dead"),
            CompatibilityRoute::Unsupported,
        ),
    ];
    for (label, body, expected) in combinations {
        let (root, home, _package, r) =
            make_policy_package(&format!("opencode-{label}"), "test", &body);
        let opencode = OpenCodeIntegration::new(
            root.join("agents"),
            root.join("config/opencode.json"),
            home.clone(),
        );
        mark_setup(&home, &opencode);
        assert_eq!(
            opencode.exposure_plan(&r).route,
            expected,
            "OpenCode V2 preserves every combination natively ({label})"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
#[test]
fn opencode_user_only_wrapper_carries_autoinvoke_metadata() {
    let (root, home, package, r) =
        make_policy_package("oc-physical", "review", &user_only_body("review"));
    let store_bytes = fs::read(package.root.join("skills/review/SKILL.md")).unwrap();
    let opencode = OpenCodeIntegration::new(
        root.join("agents"),
        root.join("config/opencode.json"),
        home.clone(),
    );
    mark_setup(&home, &opencode);
    let receipt = opencode
        .attach_receipt(&r)
        .unwrap()
        .expect("user-only Skill attaches on OpenCode");
    let ManagedArtifact::SymlinkReference { target, .. } = &receipt.artifact else {
        panic!("expected a managed symlink reference");
    };
    let wrapper = fs::read_to_string(target.join("SKILL.md")).unwrap();
    assert!(
        wrapper.contains("metadata:\n  opencode/autoinvoke: false\n"),
        "model=false is translated into OpenCode's own control: {wrapper}"
    );
    assert!(
        !wrapper.contains("slash: false"),
        "user invocation stays enabled for a user-only Skill"
    );
    assert_eq!(
        fs::read(package.root.join("skills/review/SKILL.md")).unwrap(),
        store_bytes,
        "the canonical bytes stay untouched"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn opencode_model_only_wrapper_carries_slash_false() {
    let (root, home, _package, r) =
        make_policy_package("oc-model-only", "legacy", &model_only_body("legacy"));
    let opencode = OpenCodeIntegration::new(
        root.join("agents"),
        root.join("config/opencode.json"),
        home.clone(),
    );
    mark_setup(&home, &opencode);
    let receipt = opencode
        .attach_receipt(&r)
        .unwrap()
        .expect("model-only Skill attaches on OpenCode");
    let ManagedArtifact::SymlinkReference { target, .. } = &receipt.artifact else {
        panic!("expected a managed symlink reference");
    };
    let wrapper = fs::read_to_string(target.join("SKILL.md")).unwrap();
    assert!(
        wrapper.contains("slash: false\n"),
        "user=false is translated into OpenCode's catalog-hiding field: {wrapper}"
    );
    assert!(
        !wrapper.contains("opencode/autoinvoke"),
        "model discovery stays enabled for a model-only Skill"
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
