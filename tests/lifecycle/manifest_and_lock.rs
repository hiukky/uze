//! The authored manifest and the derived lock, exercised as a person meets
//! them: a project that has declared nothing, a project set up by an
//! explicit command, and a manifest somebody has since edited by hand.
//!
//! These go through `UzeApplication` rather than `uze_core` directly,
//! because the guarantee being tested is a product one — what the files on
//! disk look like after a command — not the shape of a struct.

use std::fs;

use uze_application::UzeApplication;
use uze_core::{UzeHome, manifest, trust::AlwaysTrust};

/// A real repository, because a manifest declaring links is validated
/// against what the repository ignores — a bare directory would pass tests
/// the product would fail.
fn project(label: &str) -> (UzeApplication, uze_testkit::git::Repository) {
    let repository = uze_testkit::git::Repository::new(label);
    repository.commit_file("README.md", "# p\n");
    let home = uze_testkit::temp::scratch(&format!("{label}-home"));
    (
        UzeApplication::new(UzeHome::at(home), Vec::new()),
        repository,
    )
}

#[test]
fn a_project_that_has_declared_nothing_is_left_untouched_by_reading_it() {
    let (application, repository) = project("manifest-read-only");
    let root = repository.root().to_path_buf();
    let before: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();

    application.context().inspect(&root).unwrap();
    application.project().environment(&root).unwrap();
    application.project().plan(&root).unwrap();

    let after: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(
        before.len(),
        after.len(),
        "inspecting a project must not create anything: {after:?}"
    );
    assert!(!manifest::manifest_path_for(&root).exists());
}

#[test]
fn install_sets_the_project_up_and_writes_no_lock_when_there_is_nothing_to_resolve() {
    let (application, repository) = project("manifest-install-creates");
    let root = repository.root().to_path_buf();

    application.project().install(&root, &AlwaysTrust).unwrap();

    let written = fs::read_to_string(manifest::manifest_path_for(&root)).unwrap();
    assert!(
        written.contains("completion: handoff"),
        "the default must be written out, not implied: {written}"
    );
    assert!(
        written.contains("handoff | merge | pr"),
        "the choices must be discoverable by opening the file: {written}"
    );
    for offered in ["# target:", "# link:", "# setup:", "# gate:", "# slots:"] {
        assert!(
            written.contains(offered),
            "setting a project up must show `{offered}` too, commented: {written}"
        );
    }
    assert_eq!(
        manifest::worktree_policy(&root).unwrap(),
        uze_core::worktree::WorktreePolicy::default(),
        "showing the options must not declare any of them"
    );
    assert!(
        !root.join("agents.lock").exists(),
        "a project with nothing to resolve has no lock"
    );
}

#[test]
fn install_run_twice_leaves_the_manifest_byte_identical() {
    let (application, repository) = project("manifest-install-idempotent");
    let root = repository.root().to_path_buf();
    application.project().install(&root, &AlwaysTrust).unwrap();
    let first = fs::read_to_string(manifest::manifest_path_for(&root)).unwrap();

    application.project().install(&root, &AlwaysTrust).unwrap();
    let second = fs::read_to_string(manifest::manifest_path_for(&root)).unwrap();

    assert_eq!(first, second);
}

#[test]
fn install_never_rewrites_a_manifest_somebody_authored() {
    let (application, repository) = project("manifest-install-preserves");
    let root = repository.root().to_path_buf();
    let authored = "# ours\nworktrees:\n  completion: pr   # decided in the RFC\n";
    fs::write(manifest::manifest_path_for(&root), authored).unwrap();

    application.project().install(&root, &AlwaysTrust).unwrap();

    assert_eq!(
        fs::read_to_string(manifest::manifest_path_for(&root)).unwrap(),
        authored
    );
}

#[test]
fn the_policy_in_force_is_what_the_manifest_says_and_it_reaches_the_projection() {
    let (application, repository) = project("manifest-policy-projected");
    let root = repository.root().to_path_buf();
    fs::write(
        manifest::manifest_path_for(&root),
        "worktrees:\n  completion: pr\n",
    )
    .unwrap();

    application.context().reconcile(&root).unwrap();

    let agents_md = fs::read_to_string(root.join("AGENTS.md")).unwrap();
    assert!(
        agents_md.contains(uze_core::worktree::CompletionBehavior::Pr.instruction_clause()),
        "the declared behavior must be the one an agent reads: {agents_md}"
    );
}

#[test]
fn a_lock_still_carrying_the_policy_is_refused_and_says_where_it_belongs() {
    let (application, repository) = project("manifest-retired-key");
    let root = repository.root().to_path_buf();
    fs::write(
        root.join("agents.lock"),
        "version: 1\nworktrees:\n  completion: pr\n",
    )
    .unwrap();

    let error = application
        .project()
        .environment(&root)
        .expect_err("a lock carrying a declaration must be refused");
    let message = error.to_string();
    assert!(message.contains("worktrees"), "{message}");
    assert!(
        message.contains("agents.yaml"),
        "the operator must be told where it lives now: {message}"
    );
}

#[test]
fn a_typo_in_the_manifest_is_named_rather_than_ignored() {
    let (application, repository) = project("manifest-typo");
    let root = repository.root().to_path_buf();
    fs::write(
        manifest::manifest_path_for(&root),
        "worktrees:\n  completon: pr\n",
    )
    .unwrap();

    let error = application
        .context()
        .inspect(&root)
        .expect_err("a misspelled field must not be silently dropped");
    assert!(error.to_string().contains("completon"), "{error}");
}

/// The pin is the point: a lock that records where bytes came from but not
/// what they were is a description, not a lock. These exercise the guard
/// through the real install path rather than by calling the digest.
mod integrity {
    use super::*;

    /// A marketplace on disk, so the plugin resolves without a network.
    fn marketplace(label: &str, skill: &str) -> std::path::PathBuf {
        let root = uze_testkit::temp::scratch(label);
        fs::create_dir_all(root.join("plugins/flow/skills/demo")).unwrap();
        fs::write(
            root.join("marketplace.json"),
            r#"{"name":"ai","plugins":[{"name":"flow","source":"./plugins/flow"}]}"#,
        )
        .unwrap();
        fs::write(root.join("plugins/flow/plugin.json"), r#"{"name":"flow"}"#).unwrap();
        fs::write(root.join("plugins/flow/skills/demo/SKILL.md"), skill).unwrap();
        root
    }

    fn with_marketplace(label: &str, skill: &str) -> (UzeApplication, std::path::PathBuf) {
        let (application, repository) = project(label);
        let market = marketplace(&format!("{label}-market"), skill);
        application
            .marketplace()
            .add(&market.display().to_string())
            .unwrap();
        (application, repository.root().to_path_buf())
    }

    /// A local path has no stable revision, so it has no honest pin either:
    /// the bytes change whenever their author saves. Recording one would
    /// turn ordinary editing into a mismatch.
    #[test]
    fn a_local_path_records_no_pin_rather_than_a_value_that_would_be_wrong_tomorrow() {
        let (application, root) = with_marketplace("integrity-local", "# demo\n");
        application
            .project()
            .add("flow", "ai", &root, &AlwaysTrust)
            .unwrap();

        let lock = fs::read_to_string(root.join("agents.lock")).unwrap();
        assert!(!lock.contains("integrity:"), "{lock}");
    }

    /// The shape of the lock, which is what a reviewer reads in a diff.
    #[test]
    fn resolution_is_flat_and_the_request_is_echoed() {
        let (application, root) = with_marketplace("integrity-shape", "# demo\n");
        application
            .project()
            .add("flow", "ai", &root, &AlwaysTrust)
            .unwrap();

        let lock = fs::read_to_string(root.join("agents.lock")).unwrap();
        assert!(
            lock.contains("requested:"),
            "the lock must echo what was asked, for offline staleness: {lock}"
        );
        assert!(
            !lock.contains("resolved:"),
            "resolution is the whole file; the nested key names the obvious: {lock}"
        );
    }

    /// The guarantee. A pin that does not match the bytes acquired stops the
    /// install before anything is ingested or delivered.
    #[test]
    fn bytes_that_do_not_match_the_pin_are_refused_and_nothing_is_delivered() {
        let (application, root) = with_marketplace("integrity-mismatch", "# demo\n");
        application
            .project()
            .add("flow", "ai", &root, &AlwaysTrust)
            .unwrap();

        // A second machine: the same declaration, a lock pinning bytes that
        // are not the ones this source now yields.
        let lock = fs::read_to_string(root.join("agents.lock")).unwrap();
        let pinned = lock.replace(
            "  flow:",
            "  flow:\n    integrity: sha256:0000000000000000000000000000000000000000000000000000000000000000",
        );
        fs::write(root.join("agents.lock"), &pinned).unwrap();

        let elsewhere = UzeApplication::new(
            UzeHome::at(uze_testkit::temp::scratch("integrity-mismatch-elsewhere")),
            Vec::new(),
        );
        elsewhere
            .marketplace()
            .add(
                &uze_testkit::temp::scratch("integrity-mismatch-market")
                    .display()
                    .to_string(),
            )
            .ok();

        let error = elsewhere
            .project()
            .install(&root, &AlwaysTrust)
            .expect_err("a pin that does not match must refuse");
        let message = error.to_string();
        assert!(message.contains("flow"), "{message}");
        assert!(
            message.contains("does not match") && message.contains("Nothing was installed"),
            "the operator must be told what happened and that it stopped: {message}"
        );
    }
}

/// Scope: exactly one declaration per repository, in the checkout the
/// worktrees are born from. These hold by construction today — the policy
/// is read from `primary_checkout` — which is precisely why they are worth
/// pinning: nothing stops a future caller passing a worktree's own path.
mod policy_scope {
    use super::*;

    #[test]
    fn an_isolated_checkout_inherits_the_primary_and_cannot_override_it() {
        let (application, repository) = project("policy-scope-inherit");
        let root = repository.root().to_path_buf();
        fs::write(
            manifest::manifest_path_for(&root),
            "worktrees:\n  completion: pr\n",
        )
        .unwrap();

        let checkout = root.join(".worktrees/agent-x");
        repository.git(&[
            "worktree",
            "add",
            "-b",
            "agent/x",
            &checkout.display().to_string(),
            "HEAD",
        ]);
        // A manifest inside the checkout must not be the one that counts.
        fs::write(
            manifest::manifest_path_for(&checkout),
            "worktrees:\n  completion: merge\n",
        )
        .unwrap();

        let policy = application.workspace().delivery_policy(&checkout).unwrap();
        assert_eq!(
            policy.completion, "pr",
            "the primary checkout owns the policy; a worktree inherits it"
        );
    }

    /// The reason no machine-scoped default may resolve an undeclared
    /// policy: two developers must project the same `AGENTS.md`.
    #[test]
    fn an_undeclared_policy_resolves_the_same_for_two_different_homes() {
        let repository = uze_testkit::git::Repository::new("policy-scope-machines");
        repository.commit_file("README.md", "# p\n");
        let root = repository.root().to_path_buf();
        fs::write(manifest::manifest_path_for(&root), "worktrees: {}\n").unwrap();

        let policies: Vec<String> = ["home-a", "home-b"]
            .into_iter()
            .map(|label| {
                let application =
                    UzeApplication::new(UzeHome::at(uze_testkit::temp::scratch(label)), Vec::new());
                application
                    .workspace()
                    .delivery_policy(&root)
                    .unwrap()
                    .completion
                    .to_owned()
            })
            .collect();

        assert_eq!(policies[0], policies[1]);
        assert_eq!(
            policies[0], "handoff",
            "the built-in default, not a machine's taste"
        );
    }
}
