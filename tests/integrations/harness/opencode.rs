//! OpenCode invocation-policy conformance (ADR-030): what `autoinvoke` and
//! `slash` carry, and what `slash` does not.

use crate::policy::*;

#[test]
fn opencode_routes_each_combination_by_what_the_vendor_carries() {
    let combinations = [
        (
            "default",
            default_body("commit"),
            CompatibilityRoute::Native,
        ),
        // `metadata.opencode/autoinvoke: false` withholds the Skill from
        // model discovery, which is the whole of invoke.model=false.
        (
            "user-only",
            user_only_body("review"),
            CompatibilityRoute::Native,
        ),
        // `slash: false` gates the `/` catalog only: V2 also invokes a
        // Skill by mention (`@id`), and a mentioned Skill's body is
        // expanded whatever its `slash` value. Measured by the Lab's
        // invocation check (`skill-model-only-is-not-invocable`, opencode2
        // beta-19192), which types `@flow:analyze` and finds its body in
        // the request. Half the policy is carried; half is not.
        (
            "model-only",
            model_only_body("legacy"),
            CompatibilityRoute::Adaptable,
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
        let plan = opencode.exposure_plan(&r);
        assert_eq!(
            plan.route, expected,
            "OpenCode routes by what it carries ({label})"
        );
        if label == "model-only" {
            assert!(
                plan.evidence.contains("still invokes it"),
                "the degradation must be stated, never hidden: {}",
                plan.evidence
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn opencode_default_skill_wrapper_carries_the_qualified_label() {
    let (root, home, package, r) =
        make_policy_package("oc-default-label", "commit", &default_body("commit"));
    let store_bytes = fs::read(package.root.join("skills/commit/SKILL.md")).unwrap();
    let opencode = OpenCodeIntegration::new(
        root.join("agents"),
        root.join("config/opencode.json"),
        home.clone(),
    );
    mark_setup(&home, &opencode);

    let receipt = opencode
        .attach_receipt(&r)
        .unwrap()
        .expect("default Skill attaches on OpenCode");
    let ManagedArtifact::SymlinkReference { target, .. } = &receipt.artifact else {
        panic!("expected a managed symlink reference");
    };
    let wrapper = fs::read_to_string(target.join("SKILL.md")).unwrap();
    assert!(
        wrapper.starts_with("---\nname: flow:commit\n"),
        "the OpenCode-visible name stays qualified: {wrapper}"
    );
    assert_eq!(
        fs::read(package.root.join("skills/commit/SKILL.md")).unwrap(),
        store_bytes,
        "the canonical Store bytes stay untouched"
    );
    fs::remove_dir_all(root).unwrap();
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
