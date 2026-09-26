//! The authored manifest and the derived lock, exercised as a person meets
//! them: a project that has declared nothing, a project set up by an
//! explicit command, and a manifest somebody has since edited by hand.
//!
//! These go through `UzeApplication` rather than `uze_core` directly,
//! because the guarantee being tested is a product one — what the files on
//! disk look like after a command — not the shape of a struct.

use std::fs;

use uze_application::UzeApplication;
use uze_core::{UzeHome, manifest, trust::AlwaysTrust, worktree::CompletionBehavior};

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

/// The other act that declares something: the workspace client changing the
/// policy. A project that skipped `uze install` and opened the client
/// straight away has no manifest, and the first declaration is what creates
/// one — the file arrives by intent, still, just not `install`'s.
#[test]
fn declaring_the_policy_from_the_client_creates_the_manifest_and_states_it_first() {
    let (application, repository) = project("manifest-policy-click");
    let root = repository.root().to_path_buf();
    assert!(
        !manifest::manifest_path_for(&root).exists(),
        "the fixture must start with nothing declared"
    );

    let consequence = application
        .workspace()
        .completion_change_consequence(&root)
        .expect("a git repository has a policy to declare");
    assert!(
        consequence.creates_manifest,
        "the caller must be able to say a tracked file is about to appear"
    );
    assert_eq!(consequence.manifest, manifest::manifest_path_for(&root));

    assert!(
        application
            .workspace()
            .set_completion(&root, CompletionBehavior::Pr)
            .unwrap()
    );
    assert_eq!(
        manifest::worktree_policy(&root).unwrap().completion,
        CompletionBehavior::Pr
    );
    assert!(
        !application
            .workspace()
            .completion_change_consequence(&root)
            .unwrap()
            .creates_manifest,
        "the file exists now; a second change edits it"
    );
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
        uze_testkit::git::commit_everything_in(&root);
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

    /// A marketplace on this machine is a clone of a repository, not a
    /// loose directory, so it pins exactly as well as a remote one: the
    /// commit it was read at, and a digest of the bytes that commit holds.
    #[test]
    fn a_marketplace_on_this_machine_pins_like_any_other() {
        let (application, root) = with_marketplace("integrity-local", "# demo\n");
        application
            .project()
            .add(
                "flow",
                "ai",
                &root,
                &AlwaysTrust,
                &uze_application::NoNameCollisionAuthority,
            )
            .unwrap();

        let lock = fs::read_to_string(root.join("agents.lock")).unwrap();
        assert!(lock.contains("integrity: sha256:"), "{lock}");
        assert!(lock.contains("revision:"), "{lock}");
    }

    /// The shape of the lock, which is what a reviewer reads in a diff.
    /// Nothing in it is said twice: no wrapper key names what the whole
    /// file already is, and nothing repeats a map key as a field.
    #[test]
    fn the_lock_repeats_nothing_it_has_already_said() {
        let (application, root) = with_marketplace("integrity-shape", "# demo\n");
        application
            .project()
            .add(
                "flow",
                "ai",
                &root,
                &AlwaysTrust,
                &uze_application::NoNameCollisionAuthority,
            )
            .unwrap();

        let lock = fs::read_to_string(root.join("agents.lock")).unwrap();
        for redundant in [
            "source:",
            "resolved:",
            "requested:",
            "type:",
            "plugin: flow",
        ] {
            assert!(
                !lock.contains(redundant),
                "`{redundant}` says what another line already said: {lock}"
            );
        }
        assert_eq!(
            lock.matches("ai").count(),
            2,
            "the marketplace is named once where it is described and once where \
             it is drawn from: {lock}"
        );
    }

    /// The guarantee. A pin that does not match the bytes acquired stops the
    /// install before anything is ingested or delivered.
    #[test]
    fn bytes_that_do_not_match_the_pin_are_refused_and_nothing_is_delivered() {
        let (application, root) = with_marketplace("integrity-mismatch", "# demo\n");
        application
            .project()
            .add(
                "flow",
                "ai",
                &root,
                &AlwaysTrust,
                &uze_application::NoNameCollisionAuthority,
            )
            .unwrap();

        // A second machine: the same declaration, a lock pinning bytes that
        // are not the ones this source now yields.
        let lock = fs::read_to_string(root.join("agents.lock")).unwrap();
        let pinned = lock
            .lines()
            .map(|line| {
                if line.trim_start().starts_with("integrity:") {
                    "    integrity: sha256:\
                     0000000000000000000000000000000000000000000000000000000000000000"
                } else {
                    line
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(root.join("agents.lock"), format!("{pinned}\n")).unwrap();

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

/// A marketplace repository beside `under`, driven through *its* Git
/// guard.
///
/// Deliberately not a second `uze_testkit::git::Repository`: that takes a
/// process-wide env lock which is not reentrant, so two of them on one
/// thread deadlock. `git_in` runs Git somewhere else under the guard the
/// caller already holds, which is what this needs.
fn marketplace_beside(
    under: &uze_testkit::git::Repository,
    at: &std::path::Path,
    body: &str,
) -> String {
    fs::create_dir_all(at.join("plugins/flow/skills/one")).unwrap();
    fs::write(
        at.join("plugins/flow/plugin.json"),
        r#"{"name":"flow","description":"d"}"#,
    )
    .unwrap();
    write_skill(at, body);
    fs::write(
        at.join("marketplace.json"),
        r#"{"name":"mkt","plugins":[{"name":"flow","source":"./plugins/flow"}]}"#,
    )
    .unwrap();
    under.git_in(at, &["init", "--quiet", "-b", "main", "."]);
    under.git_in(at, &["config", "user.name", "Test"]);
    under.git_in(at, &["config", "user.email", "t@example.invalid"]);
    under.git_in(at, &["add", "-A"]);
    under.git_in(at, &["commit", "-m", "first"]);
    under.git_in(at, &["rev-parse", "HEAD"]).trim().to_owned()
}

fn write_skill(at: &std::path::Path, body: &str) {
    fs::write(
        at.join("plugins/flow/skills/one/SKILL.md"),
        format!("---\nname: one\ndescription: d\n---\n\n{body}\n"),
    )
    .unwrap();
}

fn move_marketplace(
    under: &uze_testkit::git::Repository,
    at: &std::path::Path,
    body: &str,
) -> String {
    write_skill(at, body);
    under.git_in(at, &["add", "-A"]);
    under.git_in(at, &["commit", "-m", "second"]);
    under.git_in(at, &["rev-parse", "HEAD"]).trim().to_owned()
}

/// `install` reproduces what the lock records. That guarantee is what lets
/// a clone of a project reach the bytes the project was locked at, whatever
/// has been pushed since — so a ref that moved must not move it, and
/// `update` is what does.
#[test]
fn install_reproduces_a_pin_the_ref_has_moved_past_and_update_moves_it() {
    let (application, repository) = project("update-project");
    let root = repository.root().to_path_buf();
    let market = root.parent().unwrap().join("market");
    marketplace_beside(&repository, &market, "first body");

    application
        .marketplace()
        .add(&format!("file://{}", market.display()))
        .unwrap();
    application
        .project()
        .add(
            "flow",
            "mkt",
            &root,
            &AlwaysTrust,
            &uze_application::NoNameCollisionAuthority,
        )
        .unwrap();

    let locked_first = fs::read_to_string(root.join("agents.lock")).unwrap();
    let head_after_move = move_marketplace(&repository, &market, "second body");
    assert!(!locked_first.contains(&head_after_move));

    // Install: the pin stands.
    application.project().install(&root, &AlwaysTrust).unwrap();
    assert_eq!(
        fs::read_to_string(root.join("agents.lock")).unwrap(),
        locked_first,
        "install must not move a pin the declared ref has moved past"
    );

    // Update: the pin moves, to exactly where Git says the ref points now.
    let report = application
        .project()
        .update(&root, None, false, &AlwaysTrust)
        .unwrap();
    assert!(report.moved(), "{report:?}");
    let locked_second = fs::read_to_string(root.join("agents.lock")).unwrap();
    assert!(
        locked_second.contains(&head_after_move),
        "the lock records the revision the ref resolves to now: {locked_second}"
    );
}

/// A refused update writes nothing: naming a plugin this project does not
/// declare is a mistake to report, not a lock to rewrite.
#[test]
fn updating_a_plugin_this_project_does_not_declare_writes_nothing() {
    let (application, repository) = project("update-unknown");
    let root = repository.root().to_path_buf();
    let market = root.parent().unwrap().join("market");
    marketplace_beside(&repository, &market, "a first");

    application
        .marketplace()
        .add(&format!("file://{}", market.display()))
        .unwrap();
    application
        .project()
        .add(
            "flow",
            "mkt",
            &root,
            &AlwaysTrust,
            &uze_application::NoNameCollisionAuthority,
        )
        .unwrap();
    let before = fs::read_to_string(root.join("agents.lock")).unwrap();

    let refused = application
        .project()
        .update(&root, Some("not-declared"), false, &AlwaysTrust);

    assert!(refused.is_err(), "{refused:?}");
    assert_eq!(
        fs::read_to_string(root.join("agents.lock")).unwrap(),
        before,
        "a refused update writes nothing"
    );
}

/// The author's loop: a marketplace linked to a checkout follows the
/// working tree, and the project's pin never comes from it.
#[test]
fn a_linked_marketplace_follows_the_checkout_and_pins_nothing() {
    let (application, repository) = project("linked-project");
    let root = repository.root().to_path_buf();
    let market = root.parent().unwrap().join("market");
    marketplace_beside(&repository, &market, "first body");

    application
        .marketplace()
        .add(&format!("file://{}", market.display()))
        .unwrap();
    application
        .project()
        .add(
            "flow",
            "mkt",
            &root,
            &AlwaysTrust,
            &uze_application::NoNameCollisionAuthority,
        )
        .unwrap();
    let pinned = fs::read_to_string(root.join("agents.lock")).unwrap();

    application.marketplace().link("mkt", &market).unwrap();

    // An edit with no commit behind it.
    write_skill(&market, "edited, never committed");
    let report = application
        .project()
        .update(&root, None, false, &AlwaysTrust)
        .unwrap();

    // The Store — what every harness reads — carries the edit.
    let stored = application
        .plugins()
        .list()
        .unwrap()
        .into_iter()
        .find(|plugin| plugin.id == "flow@mkt")
        .expect("the plugin is installed")
        .store_path
        .join("skills/one/SKILL.md");
    assert!(
        fs::read_to_string(&stored)
            .unwrap()
            .contains("edited, never committed"),
        "a linked marketplace follows the working tree"
    );

    // And the lock is untouched, byte for byte.
    assert_eq!(
        fs::read_to_string(root.join("agents.lock")).unwrap(),
        pinned,
        "a pin must never come from unpublished work"
    );
    assert!(
        !report.moved(),
        "nothing was pinned, so nothing moved: {report:?}"
    );

    // UZE performed no Git on the operator's checkout: their edit is still
    // uncommitted, and their branch is untouched.
    let status = repository.git_in(&market, &["status", "--porcelain"]);
    assert!(
        status.contains("SKILL.md"),
        "the edit is still the operator's to commit: {status:?}"
    );
}

/// Linking refuses a checkout holding some other repository: a link says
/// "read this marketplace here", and a directory holding a different
/// project is not that marketplace wherever it sits.
#[test]
fn linking_to_a_foreign_checkout_is_refused() {
    let (application, repository) = project("linked-foreign");
    let root = repository.root().to_path_buf();
    let market = root.parent().unwrap().join("market");
    marketplace_beside(&repository, &market, "a");
    let stranger = root.parent().unwrap().join("stranger");
    marketplace_beside(&repository, &stranger, "b");

    application
        .marketplace()
        .add(&format!("file://{}", market.display()))
        .unwrap();

    let refused = application.marketplace().link("mkt", &stranger);

    assert!(refused.is_err(), "{refused:?}");
}

/// A contributor cloning a project whose author declared a marketplace only
/// they have gets the rest of the environment, and is told what is missing
/// — rather than nothing at all.
#[test]
fn an_unreachable_marketplace_is_skipped_and_named_and_the_rest_installs() {
    let (application, repository) = project("unreachable-market");
    let root = repository.root().to_path_buf();
    let market = root.parent().unwrap().join("market");
    marketplace_beside(&repository, &market, "reachable");

    application
        .marketplace()
        .add(&format!("file://{}", market.display()))
        .unwrap();
    application
        .project()
        .add(
            "flow",
            "mkt",
            &root,
            &AlwaysTrust,
            &uze_application::NoNameCollisionAuthority,
        )
        .unwrap();

    // A second marketplace the author has and nobody else does.
    let manifest = root.join("agents.yaml");
    let declared = fs::read_to_string(&manifest).unwrap();
    fs::write(
        &manifest,
        format!(
            "{declared}  theirs:\n    path: /nonexistent/theirs\n    plugins:\n      - other\n"
        ),
    )
    .unwrap();

    let report = application.project().install(&root, &AlwaysTrust).unwrap();

    match report {
        uze_application::application::InstallReport::Installed { skipped, .. } => {
            assert_eq!(skipped.len(), 1, "{skipped:?}");
            assert_eq!(skipped[0].plugin, "other");
            assert!(
                skipped[0].reason.contains("/nonexistent/theirs"),
                "the skip names what this machine does not have: {}",
                skipped[0].reason
            );
        }
        other => panic!("the reachable half must still install: {other:?}"),
    }

    // The reachable plugin is installed and delivered.
    assert!(
        application
            .plugins()
            .list()
            .unwrap()
            .iter()
            .any(|plugin| plugin.id == "flow@mkt"),
        "the reachable marketplace's plugin is installed"
    );
}

/// A package reproduced from `agents.lock` carries the locked commit *as*
/// its own request, so re-resolving the request can only ever return what
/// is already installed. `uze update` must resolve what the *manifest*
/// declares instead — which is the only thing that can name a newer
/// revision.
///
/// The shape this missed: a marketplace declared by path resolves to a
/// commit from the local checkout, while the lock records the remote
/// identity so the project stays reproducible. A commit never pushed then
/// exists on no remote, and re-resolving the request fails outright rather
/// than merely standing still.
#[test]
fn update_resolves_what_the_manifest_declares_not_what_the_package_requested() {
    let (application, repository) = project("update-declared-ref");
    let root = repository.root().to_path_buf();
    let market = root.parent().unwrap().join("market");
    marketplace_beside(&repository, &market, "first body");

    application
        .marketplace()
        .add(&market.to_string_lossy())
        .unwrap();
    application
        .project()
        .add(
            "flow",
            "mkt",
            &root,
            &AlwaysTrust,
            &uze_application::NoNameCollisionAuthority,
        )
        .unwrap();

    // Reproduce it the way a fresh machine does: the package's own request
    // becomes the locked commit.
    application.plugins().remove("flow@mkt").ok();
    application.project().install(&root, &AlwaysTrust).unwrap();

    let moved_to = move_marketplace(&repository, &market, "second body");

    let report = application
        .project()
        .update(&root, None, false, &AlwaysTrust)
        .unwrap();

    assert!(
        report.moved(),
        "update must follow the declared ref, not the pinned request: {report:?}"
    );
    let locked = fs::read_to_string(root.join("agents.lock")).unwrap();
    assert!(
        locked.contains(&moved_to),
        "the lock records where the declared ref points now: {locked}"
    );
}
