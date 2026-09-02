//! Worktree-policy projection: one declaration in `agents.lock`, rendered
//! into the shared instruction baseline every harness already reads.
//!
//! Isolation itself is not tested here — UZE performs it by choosing an
//! agent's working directory at launch, which is harness-blind. What these
//! cover is the remainder that travels as text: the layout a subagent needs
//! to reproduce, the sentence that stops an already-isolated agent from
//! isolating again, and the completion rule.

use std::{fs, path::Path, path::PathBuf};

use uze_application::UzeApplication;
use uze_core::{
    UzeHome,
    integration::AttachmentState,
    worktree::{self, CompletionBehavior},
};
use uze_integrations::registry::IntegrationRegistry;
use uze_testkit::temp::scratch;

fn temp(label: &str) -> PathBuf {
    scratch(label)
}

/// The real four integrations, against isolated roots.
fn app(root: &Path) -> UzeApplication {
    let home = UzeHome::at(root.join("uze-home"));
    let registry = IntegrationRegistry::isolated(&root.join("harnesses"), &home);
    UzeApplication::new(home, registry.into_parts().0)
}

fn project_with_policy(root: &Path, lock_body: &str) -> PathBuf {
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("agents.lock"), lock_body).unwrap();
    project
}

const POLICY_LOCK: &str = "version: 1\nworktrees: {}\n";

// --- projection into the shared baseline ----------------------------------

#[test]
fn reconcile_projects_the_declaration_into_the_shared_baseline() {
    let root = temp("worktree-projection");
    let application = app(&root);
    let project = project_with_policy(&root, POLICY_LOCK);

    let report = application.context().reconcile(&project).unwrap();
    let region = report
        .worktree_region
        .expect("a declared policy is reconciled");
    assert_eq!(region.state, AttachmentState::Matched);

    let agents_md = fs::read_to_string(project.join("AGENTS.md")).unwrap();
    assert!(
        agents_md.contains(worktree::POLICY_REGION_PREFIX),
        "the region is marker-owned, not free text"
    );
    assert!(agents_md.contains(worktree::WORKTREES_DIRECTORY));
    assert!(agents_md.contains(worktree::BRANCH_PREFIX));

    // Idempotent: a second pass changes nothing and stays matched.
    let before = agents_md.clone();
    application.context().reconcile(&project).unwrap();
    assert_eq!(
        fs::read_to_string(project.join("AGENTS.md")).unwrap(),
        before
    );
    fs::remove_dir_all(root).unwrap();
}

/// The projected text must never ask for a top-level worktree. A harness
/// with its own worktree primitive activates on exactly that instruction and
/// would isolate a second time on top of the checkout UZE already placed the
/// agent in — the nesting this whole design avoids.
#[test]
fn the_projection_never_triggers_a_harnesss_own_isolation() {
    let root = temp("worktree-no-double-isolation");
    let application = app(&root);
    let project = project_with_policy(&root, POLICY_LOCK);
    application.context().reconcile(&project).unwrap();

    let agents_md = fs::read_to_string(project.join("AGENTS.md"))
        .unwrap()
        .to_lowercase();
    assert!(!agents_md.contains("before editing"), "{agents_md}");
    assert!(!agents_md.contains("create or reuse"), "{agents_md}");
    assert!(
        agents_md.contains("already isolated"),
        "an agent UZE placed must be told it is already isolated"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_project_declaring_nothing_gets_no_region() {
    let root = temp("worktree-absent");
    let application = app(&root);
    let project = project_with_policy(&root, "version: 1\n");

    let report = application.context().reconcile(&project).unwrap();
    assert!(report.worktree_region.is_none());
    let status = application.context().inspect(&project).unwrap();
    assert!(status.worktrees.is_none());

    if let Ok(agents_md) = fs::read_to_string(project.join("AGENTS.md")) {
        assert!(!agents_md.contains(worktree::POLICY_REGION_PREFIX));
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_declared_completion_behavior_is_what_reaches_the_baseline() {
    let root = temp("worktree-completion");
    let application = app(&root);
    let project = project_with_policy(&root, "version: 1\nworktrees:\n  completion: merge\n");

    application.context().reconcile(&project).unwrap();
    let agents_md = fs::read_to_string(project.join("AGENTS.md")).unwrap();

    assert!(agents_md.contains(CompletionBehavior::Merge.instruction_clause()));
    assert!(!agents_md.contains(CompletionBehavior::Handoff.instruction_clause()));

    let status = application.context().inspect(&project).unwrap();
    assert_eq!(
        status.worktrees.unwrap().completion,
        CompletionBehavior::Merge
    );
    fs::remove_dir_all(root).unwrap();
}

// --- ownership of the region ----------------------------------------------

/// ADR-009's rule applies to this region like any other: an edited region is
/// drift, and drift is refused rather than silently rewritten.
#[test]
fn an_edited_region_is_blocked_not_overwritten() {
    let root = temp("worktree-drift");
    let application = app(&root);
    let project = project_with_policy(&root, POLICY_LOCK);
    application.context().reconcile(&project).unwrap();

    let agents_md = project.join("AGENTS.md");
    let tampered = fs::read_to_string(&agents_md)
        .unwrap()
        .replace(worktree::WORKTREES_DIRECTORY, "somewhere-else");
    fs::write(&agents_md, &tampered).unwrap();

    let report = application.context().reconcile(&project).unwrap();
    assert_eq!(
        report.worktree_region.unwrap().state,
        AttachmentState::Drifted
    );
    assert_eq!(
        fs::read_to_string(&agents_md).unwrap(),
        tampered,
        "drift is reported, never repaired"
    );
    fs::remove_dir_all(root).unwrap();
}

/// A declaration must stay editable. Before the region identity carried the
/// rendered content's digest, changing the lock produced a permanently
/// drifted region that reconciliation refused to touch — projected once,
/// never updatable.
#[test]
fn editing_the_declaration_replaces_its_region_rather_than_drifting() {
    let root = temp("worktree-edit");
    let application = app(&root);
    let project = project_with_policy(&root, POLICY_LOCK);
    application.context().reconcile(&project).unwrap();

    fs::write(
        project.join("agents.lock"),
        "version: 1\nworktrees:\n  completion: merge\n",
    )
    .unwrap();

    let plan = application.context().plan(&project).unwrap();
    let planned = plan.worktree_region.as_ref().unwrap();
    assert_eq!(planned.superseded.len(), 1, "{planned:?}");
    assert!(plan.has_changes(), "a declaration edit is a pending change");

    let report = application.context().reconcile(&project).unwrap();
    let region = report.worktree_region.unwrap();
    assert_eq!(region.state, AttachmentState::Matched);
    assert_eq!(region.removed_superseded.len(), 1);
    assert!(region.blocked_superseded.is_empty());

    let agents_md = fs::read_to_string(project.join("AGENTS.md")).unwrap();
    assert!(agents_md.contains(CompletionBehavior::Merge.instruction_clause()));
    assert!(
        !agents_md.contains(CompletionBehavior::Handoff.instruction_clause()),
        "the superseded statement must not survive beside the new one"
    );
    assert_eq!(
        agents_md
            .matches("uze:begin project:worktree-policy")
            .count(),
        1,
        "exactly one policy region at a time"
    );
    fs::remove_dir_all(root).unwrap();
}
