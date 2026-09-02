//! The single answer to "what project context exists here, and where is
//! its root".
//!
//! Before this module the question had three different answers living in
//! three places — `workspace::resolve_workspace` (lock/manifest only),
//! `project_root::resolve_project_root` (lock > AGENTS.md > .git), and a
//! bespoke upward walk in the Claude runtime shim that stopped at `.git`.
//! They disagreed, so whether a harness "saw" a project's `AGENTS.md`
//! depended on which of the three a given call site happened to use, and
//! the runtime projection additionally refused to deliver a project's
//! `.agents/` unless an `AGENTS.md` happened to sit beside it.
//!
//! One rule now: the root comes from `project_root::resolve_project_root`
//! (the same rule every project-scoped CLI command already resolves with),
//! and the two portable context resources — `AGENTS.md` and `.agents/` —
//! are observed independently at that root. Neither gates the other.

use std::path::{Path, PathBuf};

/// The portable project-context resources, as they exist on disk right
/// now. Purely observational: resolving never creates or moves anything.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectContext {
    /// The resolved project root — `resolve_project_root`'s answer, which
    /// falls back to `cwd` itself when nothing marks a root.
    pub root: PathBuf,
    /// The shared instruction baseline, when the project has one.
    pub agents_md: Option<PathBuf>,
    /// The portable resource directory, when the project has one.
    pub agents_directory: Option<PathBuf>,
}

/// The resource kinds `.agents/` carries. Named here rather than at each
/// call site so a harness projecting the directory and a status view
/// describing it can never drift apart on which subdirectories count.
pub const AGENTS_DIRECTORY_RESOURCES: &[&str] = &["skills", "agents"];

pub const AGENTS_MD_FILE_NAME: &str = "AGENTS.md";
pub const AGENTS_DIRECTORY_NAME: &str = ".agents";

impl ProjectContext {
    /// Whether this project has anything portable to deliver at all. The
    /// activation condition for any delivery strategy: a project with only
    /// `.agents/skills/` and no `AGENTS.md` is still a project with
    /// context, and vice versa.
    pub fn has_any(&self) -> bool {
        self.agents_md.is_some() || self.agents_directory.is_some()
    }

    /// The `.agents/<resource>` directories that actually exist, in the
    /// canonical order of [`AGENTS_DIRECTORY_RESOURCES`].
    pub fn agents_directory_resources(&self) -> Vec<(&'static str, PathBuf)> {
        let Some(directory) = self.agents_directory.as_ref() else {
            return Vec::new();
        };
        AGENTS_DIRECTORY_RESOURCES
            .iter()
            .filter_map(|resource| {
                let path = directory.join(resource);
                path.is_dir().then_some((*resource, path))
            })
            .collect()
    }
}

/// Resolves the project context containing `cwd`. Infallible by design:
/// an unresolvable or nonexistent `cwd` yields a context rooted at `cwd`
/// with no resources, never an error — every caller (a status popup, a
/// shim about to exec a harness) needs an answer it can render or ignore,
/// not a failure mode.
pub fn resolve(cwd: &Path) -> ProjectContext {
    let root = crate::project_root::resolve_project_root(cwd)
        .unwrap_or_else(|_| cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf()));
    at_root(root)
}

/// Observes the resources at an already-resolved root, skipping the
/// upward walk. For a caller that resolved the root through
/// `resolve_project_root` itself and must not risk landing somewhere else.
pub fn at_root(root: PathBuf) -> ProjectContext {
    let agents_md = root.join(AGENTS_MD_FILE_NAME);
    let agents_directory = root.join(AGENTS_DIRECTORY_NAME);
    ProjectContext {
        agents_md: agents_md.is_file().then_some(agents_md),
        agents_directory: agents_directory.is_dir().then_some(agents_directory),
        root,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn agents_directory_alone_is_still_project_context() {
        // The regression this module exists for: the runtime projection
        // used to abort before looking at `.agents/` whenever `AGENTS.md`
        // was absent, so a project delivering only Skills delivered
        // nothing and the UI reported the harness as "not supported".
        let root = uze_testkit::temp::scratch("agents-dir-only");
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(root.join(".agents/skills")).unwrap();
        let context = resolve(&root);
        assert!(context.agents_md.is_none());
        assert!(context.agents_directory.is_some());
        assert!(context.has_any());
        assert_eq!(
            context
                .agents_directory_resources()
                .iter()
                .map(|(resource, _)| *resource)
                .collect::<Vec<_>>(),
            vec!["skills"]
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_nested_directory_resolves_to_the_project_root_not_itself() {
        let root = uze_testkit::temp::scratch("nested");
        fs::write(root.join("AGENTS.md"), "x\n").unwrap();
        let nested = root.join("crates/deep");
        fs::create_dir_all(&nested).unwrap();
        let context = resolve(&nested);
        assert_eq!(context.root, root.canonicalize().unwrap());
        assert!(context.agents_md.is_some());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_git_repository_without_portable_context_has_none() {
        let root = uze_testkit::temp::scratch("bare-git");
        fs::create_dir_all(root.join(".git")).unwrap();
        let context = resolve(&root);
        assert_eq!(context.root, root.canonicalize().unwrap());
        assert!(!context.has_any());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_nonexistent_directory_resolves_to_itself_with_no_context() {
        let root = uze_testkit::temp::scratch("absent");
        let missing = root.join("nowhere");
        let context = resolve(&missing);
        assert!(!context.has_any());
        assert_eq!(context.root, missing);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_subdirectory_of_a_repository_resolves_to_the_repository() {
        let root = uze_testkit::temp::scratch("repo-subdir");
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("AGENTS.md"), "x\n").unwrap();
        fs::create_dir_all(root.join(".agents/skills")).unwrap();
        let nested = root.join("crates/deep/src");
        fs::create_dir_all(&nested).unwrap();
        let context = resolve(&nested);
        assert_eq!(context.root, root.canonicalize().unwrap());
        assert!(context.agents_md.is_some());
        assert!(context.agents_directory.is_some());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_nested_repository_is_its_own_project_and_inherits_nothing() {
        // A checkout inside another checkout is a project boundary: it must
        // resolve to itself and see none of the outer repository's context,
        // otherwise the same vendored tree carries different instructions
        // depending on where it happens to be nested.
        let outer = uze_testkit::temp::scratch("nested-repo");
        fs::create_dir_all(outer.join(".git")).unwrap();
        fs::write(outer.join("AGENTS.md"), "# outer\n").unwrap();
        let inner = outer.join("vendor");
        fs::create_dir_all(inner.join(".git")).unwrap();
        let context = resolve(&inner);
        assert_eq!(context.root, inner.canonicalize().unwrap());
        assert!(context.agents_md.is_none());
        assert!(!context.has_any());
        let _ = fs::remove_dir_all(&outer);
    }
}
