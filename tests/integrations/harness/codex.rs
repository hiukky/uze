//! Codex invocation-policy conformance (ADR-030): honest routing for every
//! combination, the policy sidecar, user-only lifecycle, generated-package
//! model-only exclusion, and the real-Codex zero-model dogfood (L4; skips
//! when `codex` is not on PATH).

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
/// Runs `codex debug prompt-input` against an isolated HOME.
fn run_codex_prompt_input(home: &Path) -> std::result::Result<String, String> {
    let output = std::process::Command::new("codex")
        .env("HOME", home)
        .args(["debug", "prompt-input"])
        .output()
        .map_err(|error| format!("failed to run codex: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "codex debug prompt-input exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Real-Codex deterministic dogfood (zero model calls), driven by UZE's own
/// delivery: a canonical user-only Skill is attached through
/// `CodexIntegration` into an isolated `~/.agents/skills` (with its
/// `agents/openai.yaml` policy), a default Skill sits beside it, and the
/// model-visible prompt is rendered with `codex debug prompt-input`.
/// Expected: the default Skill is offered to the model, the user-only Skill
/// is not. A malformed-metadata control restores the listing, proving the
/// exclusion is caused by the policy file being genuinely read. Skips when
/// `codex` is not on PATH, so CI stays deterministic.
#[test]
fn real_codex_dogfood_user_only_skill_is_hidden_from_the_model() {
    let probe = std::process::Command::new("codex")
        .arg("--version")
        .output();
    if probe.is_err() || probe.as_ref().is_ok_and(|o| !o.status.success()) {
        eprintln!("codex not available on PATH; skipping real-Codex dogfood");
        return;
    }
    let _scope = uze_test_support::env::scope();
    let root = temp("real-codex-dogfood");
    let uze_home = UzeHome::at(root.join("uze"));
    let store = UzeStore::new(uze_home.clone());
    let package = install(&store, workflow_fixture()).unwrap();
    let environment = UzeEngine::new(store)
        .compose(std::slice::from_ref(&package.id))
        .unwrap();
    let resource = &environment.resources[0];
    assert_eq!(
        resource.skill_invocation(),
        SkillInvocationPolicy::USER_ONLY
    );

    let codex_home = root.join("codex-home");
    let agents_home = codex_home.join(".agents");
    let codex = CodexIntegration::new(agents_home.clone(), uze_home.clone());
    mark_setup(&uze_home, &codex);
    let receipt = codex.attach_receipt(resource).unwrap().expect("attaches");
    let ManagedArtifact::SymlinkReference { target, .. } = &receipt.artifact else {
        panic!("expected symlink artifact");
    };
    assert_eq!(
        fs::read_to_string(target.join("agents/openai.yaml")).unwrap(),
        "policy:\n  allow_implicit_invocation: false\n"
    );

    // A default Skill beside it stays implicitly discoverable.
    fs::create_dir_all(agents_home.join("skills/normal")).unwrap();
    fs::write(
        agents_home.join("skills/normal/SKILL.md"),
        "---\nname: normal\ndescription: Run normal tasks N.\n---\n\nNormal body.\n",
    )
    .unwrap();

    let before_store_bytes = fs::read(package.root.join("skills/review/SKILL.md")).unwrap();
    let valid = run_codex_prompt_input(&codex_home).expect("codex prompt-input runs");
    assert!(
        valid.contains("normal: Run normal tasks N"),
        "a default Skill stays implicitly discoverable"
    );
    assert!(
        !valid.contains("workflow:review") && !valid.contains("review: Review code"),
        "the user-only Skill must not be offered to the model"
    );

    // Control: malformed policy restores the listing (the exclusion is
    // caused by the policy file being read).
    fs::write(target.join("agents/openai.yaml"), "policy: [broken yaml\n").unwrap();
    let malformed = run_codex_prompt_input(&codex_home).expect("codex prompt-input runs");
    assert!(
        malformed.contains("workflow:review"),
        "control: malformed policy metadata must not suppress the user-only Skill"
    );

    assert_eq!(
        fs::read(package.root.join("skills/review/SKILL.md")).unwrap(),
        before_store_bytes,
        "the canonical Store bytes are untouched throughout"
    );
    fs::remove_dir_all(root).unwrap();
}
