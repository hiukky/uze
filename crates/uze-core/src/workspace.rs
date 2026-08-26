//! Deterministic workspace detection for `agents.lock` / `marketplace.json`.
//!
//! One predictable rule, no git assumption, no harness assumption: a
//! directory is a workspace when it contains `agents.lock` (consumer), or
//! `marketplace.json` (marketplace), or both (hybrid). The nearest such
//! directory wins over any ancestor.
//!
//! `AGENTS.md` and `.agents/` are explicitly NOT anchors: they are
//! resources *inside* an already-detected workspace, never evidence of
//! one. A directory with only vendor files (`CLAUDE.md`, `.claude/`, …)
//! is therefore simply "no UZE workspace", exactly like a random folder.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::{Result, UzeError, project_lock::LOCK_FILE_NAME};

/// The marketplace manifest name (`marketplace.json`) — the same name
/// `acquisition::marketplace` reads, named here because this module is the
/// one that detects it from a directory rather than parsing it.
pub const MARKETPLACE_MANIFEST_NAME: &str = "marketplace.json";

/// The two UZE workspace anchors, seen from a plain directory.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum WorkspaceKind {
    /// Neither `agents.lock` nor `marketplace.json` on the resolved path.
    NoWorkspace,
    /// `agents.lock` present.
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

/// Walks upward from `cwd` (inclusive) looking for the first directory
/// containing `agents.lock` and/or `marketplace.json`. Nearest ancestor wins —
/// a nested consumer inside a marketplace (or vice versa) is detected as
/// its own workspace, never as the outer one.
pub fn resolve_workspace(cwd: &Path) -> Result<ResolvedWorkspace> {
    let canonical = if cwd.is_dir() {
        cwd.canonicalize()
    } else {
        cwd.parent().unwrap_or(cwd).canonicalize()
    }
    .map_err(|source| UzeError::Read {
        path: cwd.to_path_buf(),
        source,
    })?;

    let mut current = Some(canonical.as_path());
    while let Some(dir) = current {
        let lock = dir.join(LOCK_FILE_NAME).is_file();
        let manifest = dir.join(MARKETPLACE_MANIFEST_NAME).is_file();
        if lock || manifest {
            let kind = match (lock, manifest) {
                (true, true) => WorkspaceKind::Hybrid,
                (true, false) => WorkspaceKind::Consumer,
                (false, true) => WorkspaceKind::Marketplace,
                (false, false) => unreachable!(),
            };
            return Ok(ResolvedWorkspace {
                root: dir.to_path_buf(),
                kind,
            });
        }
        current = dir.parent();
    }

    Ok(ResolvedWorkspace {
        root: canonical,
        kind: WorkspaceKind::NoWorkspace,
    })
}

// Count of a project's own local agent resources is deliberately NOT here:
// `.agents/` contents are resources *inside* a detected workspace, not
// workspace facts, and the Overview now surfaces only semantic states.

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "uze-workspace-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    fn mkdir(path: &Path) {
        fs::create_dir_all(path).unwrap();
    }

    #[test]
    fn no_workspace_when_no_anchors() {
        let root = temp("none");
        mkdir(&root);
        let resolved = resolve_workspace(&root).unwrap();
        assert_eq!(resolved.kind, WorkspaceKind::NoWorkspace);
        assert_eq!(resolved.root, root.canonicalize().unwrap());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn consumer_at_cwd() {
        let root = temp("consumer-root");
        mkdir(&root);
        fs::write(root.join("agents.lock"), "version: 1\n").unwrap();
        let resolved = resolve_workspace(&root).unwrap();
        assert_eq!(resolved.kind, WorkspaceKind::Consumer);
        assert_eq!(resolved.root, root.canonicalize().unwrap());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn consumer_from_subdir_finds_nearest_ancestor() {
        let root = temp("consumer-subdir");
        mkdir(&root);
        fs::write(root.join("agents.lock"), "version: 1\n").unwrap();
        let sub = root.join("src/foo");
        mkdir(&sub);
        let resolved = resolve_workspace(&sub).unwrap();
        assert_eq!(resolved.kind, WorkspaceKind::Consumer);
        assert_eq!(resolved.root, root.canonicalize().unwrap());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn marketplace_manifest_is_an_anchor() {
        let root = temp("marketplace");
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
        let root = temp("agents-json-only");
        mkdir(&root);
        fs::write(root.join("agents.json"), r#"{"name":"m","plugins":[]}"#).unwrap();
        let resolved = resolve_workspace(&root).unwrap();
        assert_eq!(resolved.kind, WorkspaceKind::NoWorkspace);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn both_anchors_are_hybrid() {
        let root = temp("hybrid");
        mkdir(&root);
        fs::write(root.join("agents.lock"), "version: 1\n").unwrap();
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
        let outer = temp("nested-outer");
        let inner = outer.join("packages/foo");
        mkdir(&inner);
        fs::write(outer.join("agents.lock"), "version: 1\n").unwrap();
        fs::write(inner.join("agents.lock"), "version: 1\n").unwrap();
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
        let outer = temp("cross-kind");
        let inner = outer.join("plugins/flow");
        mkdir(&inner);
        fs::write(
            outer.join("marketplace.json"),
            r#"{"name":"m","plugins":[]}"#,
        )
        .unwrap();
        fs::write(inner.join("agents.lock"), "version: 1\n").unwrap();
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
        let root = temp("agents-md-only");
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
        let root = temp("vendors-only");
        mkdir(&root);
        fs::write(root.join("CLAUDE.md"), "# vendor\n").unwrap();
        fs::create_dir_all(root.join(".claude")).unwrap();
        let resolved = resolve_workspace(&root).unwrap();
        assert_eq!(resolved.kind, WorkspaceKind::NoWorkspace);
        fs::remove_dir_all(&root).unwrap();
    }
}
