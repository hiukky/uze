//! Where a project begins, seen from any directory inside it.
//!
//! The nearest directory declaring `agents.yaml` is the project. Without
//! one, the nearest `AGENTS.md` is, and without that, the repository: the
//! first `.git` met ends the walk, so a repository never inherits a manifest
//! or `AGENTS.md` from a directory above it that happens to be a parent on
//! this machine (a dotfiles repository in `$HOME`, a checkout under another
//! checkout). Outside any repository, the directory itself is the project.

use std::path::{Path, PathBuf};

use crate::{Result, UzeError, manifest::MANIFEST_FILE_NAME, project_context::AGENTS_MD_FILE_NAME};

/// The file or directory marking a Git repository's root.
const GIT_MARKER: &str = ".git";

pub fn resolve_project_root(cwd: &Path) -> Result<PathBuf> {
    if !cwd.exists() {
        return Err(UzeError::MissingPath(cwd.to_path_buf()));
    }
    let mut nearest_agents_md = None;
    let (start, root) = find_upward(cwd, |dir| {
        if dir.join(MANIFEST_FILE_NAME).is_file() {
            return Some(dir.to_path_buf());
        }
        if nearest_agents_md.is_none() && dir.join(AGENTS_MD_FILE_NAME).is_file() {
            nearest_agents_md = Some(dir.to_path_buf());
        }
        is_repository_root(dir).then(|| {
            nearest_agents_md
                .clone()
                .unwrap_or_else(|| dir.to_path_buf())
        })
    })?;
    Ok(root.or(nearest_agents_md).unwrap_or(start))
}

/// Walks from the directory `path` names — its parent when `path` is a file
/// — up through every ancestor, nearest first, and returns the canonical
/// starting directory with the first answer `found` gives.
pub(crate) fn find_upward<T>(
    path: &Path,
    found: impl FnMut(&Path) -> Option<T>,
) -> Result<(PathBuf, Option<T>)> {
    let directory = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    };
    let start = directory.canonicalize().map_err(|source| UzeError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let answer = start.ancestors().find_map(found);
    Ok((start, answer))
}

pub(crate) fn is_repository_root(directory: &Path) -> bool {
    directory.join(GIT_MARKER).exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn cwd_with_a_manifest_is_root() {
        let root = uze_testkit::temp::scratch("manifest-root");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(MANIFEST_FILE_NAME), "worktrees: {}\n").unwrap();
        let sub = root.join("sub");
        fs::create_dir_all(&sub).unwrap();
        // cwd is sub, which declares nothing; the walk finds the parent's
        // manifest, and the parent is the project root
        let resolved = resolve_project_root(&sub).unwrap();
        assert_eq!(resolved, root.canonicalize().unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn fallback_is_cwd_when_no_markers() {
        let root = uze_testkit::temp::scratch("fallback");
        fs::create_dir_all(&root).unwrap();
        let resolved = resolve_project_root(&root).unwrap();
        assert_eq!(resolved, root.canonicalize().unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_repository_never_inherits_an_agents_md_from_above_it() {
        // A repo with no portable context of its own resolves to itself,
        // even when an ancestor directory carries an AGENTS.md — otherwise
        // the same project resolves differently depending on where it was
        // cloned.
        let outer = uze_testkit::temp::scratch("git-boundary");
        let repo = outer.join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::write(outer.join(AGENTS_MD_FILE_NAME), "# not this one\n").unwrap();
        let sub = repo.join("src");
        fs::create_dir_all(&sub).unwrap();
        let resolved = resolve_project_root(&sub).unwrap();
        assert_eq!(resolved, repo.canonicalize().unwrap());
        fs::remove_dir_all(outer).unwrap();
    }

    #[test]
    fn prefers_agents_md_over_git() {
        let root = uze_testkit::temp::scratch("agents-vs-git");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join(AGENTS_MD_FILE_NAME), "# hi\n").unwrap();
        let sub = root.join("sub");
        fs::create_dir_all(&sub).unwrap();
        let resolved = resolve_project_root(&sub).unwrap();
        assert_eq!(resolved, root.canonicalize().unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_repository_never_inherits_a_manifest_from_above_it() {
        let outer = uze_testkit::temp::scratch("git-boundary-manifest");
        let repo = outer.join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::write(outer.join(MANIFEST_FILE_NAME), "worktrees: {}\n").unwrap();
        let sub = repo.join("src");
        fs::create_dir_all(&sub).unwrap();
        let resolved = resolve_project_root(&sub).unwrap();
        assert_eq!(resolved, repo.canonicalize().unwrap());
        fs::remove_dir_all(outer).unwrap();
    }
}
