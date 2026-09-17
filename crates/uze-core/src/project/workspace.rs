//! Deterministic workspace detection for `agents.yaml` / `marketplace.json`.
//!
//! One predictable rule, no git assumption, no harness assumption: a
//! directory is a workspace when it contains `agents.yaml` (consumer), or
//! `marketplace.json` (marketplace), or both (hybrid). The nearest such
//! directory wins over any ancestor.
//!
//! The consumer anchor is the *manifest*, not the lock: a project that has
//! declared an environment but never resolved one has no lock yet and is
//! still a workspace. The lock is derived, and a derived file cannot be
//! what identifies a project.
//!
//! `AGENTS.md` and `.agents/` are explicitly NOT anchors: they are
//! resources *inside* an already-detected workspace, never evidence of
//! one. A directory with only vendor files (`CLAUDE.md`, `.claude/`, …)
//! is therefore simply "no UZE workspace", exactly like a random folder.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::{
    Result,
    manifest::MANIFEST_FILE_NAME,
    project_root::{find_upward, is_repository_root},
};

/// The marketplace manifest name (`marketplace.json`) — the same name
/// `acquisition::marketplace` reads, named here because this module is the
/// one that detects it from a directory rather than parsing it.
pub const MARKETPLACE_MANIFEST_NAME: &str = "marketplace.json";

/// The two UZE workspace anchors, seen from a plain directory.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum WorkspaceKind {
    /// Neither `agents.yaml` nor `marketplace.json` on the resolved path.
    NoWorkspace,
    /// `agents.yaml` present.
    Consumer,
    /// `marketplace.json` present.
    Marketplace,
    /// Both present in the same directory.
    Hybrid,
}

/// What directory detection found, and why.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedWorkspace {
    /// The workspace root: the nearest ancestor (or the cwd itself) that
    /// carries an anchor. For `NoWorkspace`, the canonicalized cwd.
    pub root: PathBuf,
    pub kind: WorkspaceKind,
}

/// The root runtime identities are keyed on for `cwd`: its workspace when
/// one is anchored, otherwise the Git repository it sits in, otherwise `cwd`
/// itself.
///
/// Every runtime-scoped identity keyed on "which workspace is this" must go
/// through here rather than through the raw launch directory. The terminal
/// runtime keys a server — and therefore a whole set of agent panes — on
/// this answer: resolving it differently in two places means launching UZE
/// from a repository and from a subdirectory of it produces two independent
/// servers over one repository, each believing it is alone.
pub fn workspace_root_or_self(cwd: &Path) -> PathBuf {
    let Ok(workspace) = resolve_workspace(cwd) else {
        return cwd.to_path_buf();
    };
    if workspace.kind != WorkspaceKind::NoWorkspace {
        return workspace.root;
    }
    find_upward(&workspace.root, |dir| {
        is_repository_root(dir).then(|| dir.to_path_buf())
    })
    .ok()
    .and_then(|(_, repository)| repository)
    .unwrap_or(workspace.root)
}

/// The nearest directory, from `cwd` upward, anchoring a workspace — an
/// `agents.yaml`, a `marketplace.json`, or both — and which kind it is. A
/// nested workspace is detected as its own, never as the outer one. With no
/// anchor anywhere, the canonical `cwd` with [`WorkspaceKind::NoWorkspace`].
pub fn resolve_workspace(cwd: &Path) -> Result<ResolvedWorkspace> {
    let (start, anchored) = find_upward(cwd, |dir| {
        anchor_kind(dir).map(|kind| ResolvedWorkspace {
            root: dir.to_path_buf(),
            kind,
        })
    })?;
    Ok(anchored.unwrap_or(ResolvedWorkspace {
        root: start,
        kind: WorkspaceKind::NoWorkspace,
    }))
}

fn anchor_kind(dir: &Path) -> Option<WorkspaceKind> {
    let consumer = dir.join(MANIFEST_FILE_NAME).is_file();
    let marketplace = dir.join(MARKETPLACE_MANIFEST_NAME).is_file();
    match (consumer, marketplace) {
        (true, true) => Some(WorkspaceKind::Hybrid),
        (true, false) => Some(WorkspaceKind::Consumer),
        (false, true) => Some(WorkspaceKind::Marketplace),
        (false, false) => None,
    }
}

// Count of a project's own local agent resources is deliberately NOT here:
// `.agents/` contents are resources *inside* a detected workspace, not
// workspace facts, and the Overview now surfaces only semantic states.

#[cfg(test)]
mod workspace_root_tests {
    use super::*;

    #[test]
    fn a_subdirectory_and_its_workspace_root_resolve_to_one_answer() {
        let root = uze_testkit::temp::scratch("workspace-root");
        let nested = root.join("crates").join("inner");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(root.join(MANIFEST_FILE_NAME), "worktrees: {}\n").unwrap();

        // The property the terminal server is keyed on: launching from the
        // root and from a subdirectory must not produce two identities.
        assert_eq!(
            workspace_root_or_self(&nested),
            workspace_root_or_self(&root)
        );
    }

    #[test]
    fn a_subdirectory_of_a_repository_without_a_manifest_resolves_to_the_repository() {
        let repository = uze_testkit::temp::scratch("workspace-repository");
        let nested = repository.join("crates").join("inner");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(repository.join(".git")).unwrap();

        assert_eq!(
            workspace_root_or_self(&nested),
            repository.canonicalize().unwrap()
        );
        assert_eq!(
            workspace_root_or_self(&nested),
            workspace_root_or_self(&repository)
        );
    }

    #[test]
    fn a_directory_marking_no_workspace_answers_itself() {
        let root = uze_testkit::temp::scratch("workspace-none");
        assert_eq!(
            workspace_root_or_self(&root),
            root.canonicalize().unwrap_or(root)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn mkdir(path: &Path) {
        fs::create_dir_all(path).unwrap();
    }

    #[test]
    fn no_workspace_when_no_anchors() {
        let root = uze_testkit::temp::scratch("none");
        mkdir(&root);
        let resolved = resolve_workspace(&root).unwrap();
        assert_eq!(resolved.kind, WorkspaceKind::NoWorkspace);
        assert_eq!(resolved.root, root.canonicalize().unwrap());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn consumer_at_cwd() {
        let root = uze_testkit::temp::scratch("consumer-root");
        mkdir(&root);
        fs::write(root.join(MANIFEST_FILE_NAME), "worktrees: {}\n").unwrap();
        let resolved = resolve_workspace(&root).unwrap();
        assert_eq!(resolved.kind, WorkspaceKind::Consumer);
        assert_eq!(resolved.root, root.canonicalize().unwrap());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn consumer_from_subdir_finds_nearest_ancestor() {
        let root = uze_testkit::temp::scratch("consumer-subdir");
        mkdir(&root);
        fs::write(root.join(MANIFEST_FILE_NAME), "worktrees: {}\n").unwrap();
        let sub = root.join("src/foo");
        mkdir(&sub);
        let resolved = resolve_workspace(&sub).unwrap();
        assert_eq!(resolved.kind, WorkspaceKind::Consumer);
        assert_eq!(resolved.root, root.canonicalize().unwrap());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn marketplace_manifest_is_an_anchor() {
        let root = uze_testkit::temp::scratch("marketplace");
        mkdir(&root);
        fs::write(
            root.join("marketplace.json"),
            r#"{"name":"m","plugins":[]}"#,
        )
        .unwrap();
        let resolved = resolve_workspace(&root).unwrap();
        assert_eq!(resolved.kind, WorkspaceKind::Marketplace);
        assert_eq!(resolved.root, root.canonicalize().unwrap());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn agents_json_alone_is_not_a_marketplace_anchor() {
        let root = uze_testkit::temp::scratch("agents-json-only");
        mkdir(&root);
        fs::write(root.join("agents.json"), r#"{"name":"m","plugins":[]}"#).unwrap();
        let resolved = resolve_workspace(&root).unwrap();
        assert_eq!(resolved.kind, WorkspaceKind::NoWorkspace);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn both_anchors_are_hybrid() {
        let root = uze_testkit::temp::scratch("hybrid");
        mkdir(&root);
        fs::write(root.join(MANIFEST_FILE_NAME), "worktrees: {}\n").unwrap();
        fs::write(
            root.join("marketplace.json"),
            r#"{"name":"m","plugins":[]}"#,
        )
        .unwrap();
        let resolved = resolve_workspace(&root).unwrap();
        assert_eq!(resolved.kind, WorkspaceKind::Hybrid);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn nested_workspace_nearest_wins() {
        let outer = uze_testkit::temp::scratch("nested-outer");
        let inner = outer.join("packages/foo");
        mkdir(&inner);
        fs::write(outer.join(MANIFEST_FILE_NAME), "worktrees: {}\n").unwrap();
        fs::write(inner.join(MANIFEST_FILE_NAME), "worktrees: {}\n").unwrap();
        let deep = inner.join("src");
        mkdir(&deep);
        let resolved = resolve_workspace(&deep).unwrap();
        assert_eq!(resolved.kind, WorkspaceKind::Consumer);
        assert_eq!(resolved.root, inner.canonicalize().unwrap());
        fs::remove_dir_all(&outer).unwrap();
    }

    #[test]
    fn nearest_anchor_wins_across_kinds() {
        // A marketplace at the outer level, a consumer nested inside it:
        // running inside the nested consumer must see the consumer.
        let outer = uze_testkit::temp::scratch("cross-kind");
        let inner = outer.join("plugins/flow");
        mkdir(&inner);
        fs::write(
            outer.join("marketplace.json"),
            r#"{"name":"m","plugins":[]}"#,
        )
        .unwrap();
        fs::write(inner.join(MANIFEST_FILE_NAME), "worktrees: {}\n").unwrap();
        let resolved = resolve_workspace(&inner).unwrap();
        assert_eq!(
            resolved.kind,
            WorkspaceKind::Consumer,
            "the nearest anchor (the nested agents.lock) must win"
        );
        assert_eq!(resolved.root, inner.canonicalize().unwrap());
        fs::remove_dir_all(&outer).unwrap();
    }

    #[test]
    fn agents_md_alone_is_not_a_workspace() {
        let root = uze_testkit::temp::scratch("agents-md-only");
        mkdir(&root);
        fs::write(root.join("AGENTS.md"), "# hi\n").unwrap();
        let resolved = resolve_workspace(&root).unwrap();
        assert_eq!(
            resolved.kind,
            WorkspaceKind::NoWorkspace,
            "AGENTS.md is a resource, not an anchor"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn vendors_without_anchors_are_no_workspace() {
        let root = uze_testkit::temp::scratch("vendors-only");
        mkdir(&root);
        fs::write(root.join("CLAUDE.md"), "# vendor\n").unwrap();
        fs::create_dir_all(root.join(".claude")).unwrap();
        let resolved = resolve_workspace(&root).unwrap();
        assert_eq!(resolved.kind, WorkspaceKind::NoWorkspace);
        fs::remove_dir_all(&root).unwrap();
    }
}
