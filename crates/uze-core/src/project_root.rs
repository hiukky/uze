//! Deterministic project root resolution for `agents.lock`.
//!
//! One predictable rule, no git assumption.

use std::path::{Path, PathBuf};

use crate::{Result, UzeError, project_lock::LOCK_FILE_NAME};

pub fn resolve_project_root(cwd: &Path) -> Result<PathBuf> {
    if !cwd.exists() {
        return Err(UzeError::MissingPath(cwd.to_path_buf()));
    }
    let cwd = if cwd.is_dir() {
        cwd.canonicalize().map_err(|source| UzeError::Read {
            path: cwd.to_path_buf(),
            source,
        })?
    } else {
        cwd.parent()
            .unwrap_or(cwd)
            .canonicalize()
            .map_err(|source| UzeError::Read {
                path: cwd.to_path_buf(),
                source,
            })?
    };

    // Walk upward (starting at cwd itself) looking for agents.lock,
    // AGENTS.md, or .git (priority in that order per directory).
    //
    // The first `.git` met also *ends* the walk, after that directory has
    // been examined: a repository is a project boundary, so a repo without
    // its own AGENTS.md must resolve to the repo — never inherit an
    // AGENTS.md from some directory above it that happens to be a parent on
    // this machine (a dotfiles repo in `$HOME`, a checkout under another
    // checkout). Without the boundary the answer depended on where the tree
    // happened to be cloned, which is exactly the kind of environment
    // sensitivity this resolution exists to remove.
    let mut current = Some(cwd.as_path());
    let mut best_agents: Option<PathBuf> = None;
    let mut best_git: Option<PathBuf> = None;
    while let Some(dir) = current {
        if dir.join(LOCK_FILE_NAME).is_file() {
            return Ok(dir.to_path_buf());
        }
        if best_agents.is_none() && dir.join("AGENTS.md").is_file() {
            best_agents = Some(dir.to_path_buf());
        }
        if dir.join(".git").exists() {
            best_git = Some(dir.to_path_buf());
            break;
        }
        current = dir.parent();
    }

    if let Some(p) = best_agents {
        return Ok(p);
    }
    if let Some(p) = best_git {
        return Ok(p);
    }

    // 3. Fallback: cwd itself.
    Ok(cwd)
}

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
            "uze-projroot-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn cwd_with_lock_is_root() {
        let root = temp("lock-root");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("agents.lock"), "version: 1\n").unwrap();
        let sub = root.join("sub");
        fs::create_dir_all(&sub).unwrap();
        // cwd is sub but sub has no lock; walk finds parent lock → parent is root
        let resolved = resolve_project_root(&sub).unwrap();
        assert_eq!(resolved, root.canonicalize().unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn fallback_is_cwd_when_no_markers() {
        let root = temp("fallback");
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
        let outer = temp("git-boundary");
        let repo = outer.join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::write(outer.join("AGENTS.md"), "# not this one\n").unwrap();
        let sub = repo.join("src");
        fs::create_dir_all(&sub).unwrap();
        let resolved = resolve_project_root(&sub).unwrap();
        assert_eq!(resolved, repo.canonicalize().unwrap());
        fs::remove_dir_all(outer).unwrap();
    }

    #[test]
    fn prefers_agents_md_over_git() {
        let root = temp("agents-vs-git");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("AGENTS.md"), "# hi\n").unwrap();
        let sub = root.join("sub");
        fs::create_dir_all(&sub).unwrap();
        let resolved = resolve_project_root(&sub).unwrap();
        assert_eq!(resolved, root.canonicalize().unwrap());
        fs::remove_dir_all(root).unwrap();
    }
}
