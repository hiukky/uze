//! Generic, vendor-neutral infrastructure for launching a harness process
//! through UZE's PATH shim.
//!
//! Nothing in this module knows a specific harness's CLI shape or vendor
//! behavior — that knowledge lives entirely in each integration's own
//! `IntegrationPort::runtime_contribution` (see `integration.rs`). This
//! module answers three harness-agnostic questions instead: which real
//! executable does a shim-invoked name resolve to (skipping UZE's own shim
//! directory, so the shim can never recurse into itself), and what stable,
//! filesystem-safe id identifies a project across repeat runs. Which
//! project a directory belongs to, and what portable context it carries, is
//! `crate::project_context`'s single answer — not this module's.
//!
//! This is `RUNTIME INFRASTRUCTURE`, not `CONTEXT DELIVERY POLICY`: building
//! this does not by itself decide that runtime projection replaces the
//! existing project-root `CLAUDE.md` bridge
//! (`uze context reconcile`'s persistent instruction bridge) — that remains
//! a separate, later decision pending empirical comparison.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

/// What a shim-mediated launch needs to decide, read-only. `cwd` is the
/// directory the user invoked the harness from, not necessarily the project
/// root — see `crate::project_context::resolve`.
pub struct RuntimeContext<'a> {
    pub cwd: &'a Path,
    pub home: &'a crate::home::UzeHome,
}

/// One integration's opt-in contribution to a shim-mediated launch. The
/// default (`passthrough`) is correct for every harness with no runtime
/// projection story yet.
///
/// Deliberately has no `Result`/error variant: an integration that hits
/// trouble building its contribution must decide *inside* its own
/// implementation to fall back to `passthrough_with_note` rather than
/// propagate an error, so a bug in one vendor's runtime logic can never
/// block a launch. Fail-open is structural here, not a convention every
/// call site has to remember.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HarnessRuntimeContribution {
    /// Prepended before the caller's original argv (minus argv[0]).
    pub extra_args: Vec<OsString>,
    /// Added on top of the inherited environment; never clears or replaces
    /// anything the caller already set.
    pub extra_env: Vec<(OsString, OsString)>,
    /// Set only when this contribution fell back to passthrough after a
    /// recoverable problem (e.g. the runtime projection could not be
    /// written). The shim prints this to stderr, never stdout, and never
    /// treats it as fatal.
    pub note: Option<String>,
}

impl HarnessRuntimeContribution {
    pub fn passthrough() -> Self {
        Self::default()
    }

    pub fn passthrough_with_note(note: impl Into<String>) -> Self {
        Self {
            note: Some(note.into()),
            ..Self::default()
        }
    }

    pub fn is_passthrough(&self) -> bool {
        self.extra_args.is_empty() && self.extra_env.is_empty()
    }
}

/// Resolves the real, non-UZE executable for one of `names` (a harness's
/// invoked name plus any aliases), walking `$PATH` in order and skipping
/// any entry that is UZE's own shim directory. Returns the first match,
/// canonicalized, so a persisted result stays meaningful even if a relative
/// `PATH` entry's meaning later changes.
///
/// Never returns a path inside `shims_dir` — that is the entire recursion
/// guard. A caller must not fall back to a bare `Command::new(name)` (or an
/// `exec` of the bare name) when this returns `None`: that would re-enter
/// PATH search and could resolve straight back to the shim.
pub fn resolve_real_executable(names: &[&str], shims_dir: &Path) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    let canonical_shims_dir = shims_dir.canonicalize().ok();
    for dir in std::env::split_paths(&path_var) {
        // Canonicalizing is a filesystem round trip per `PATH` entry, and
        // on a WSL `PATH` carrying Windows directories each one crosses a
        // network filesystem. Only an entry that could *be* the shims
        // directory — same final component — is worth resolving; every
        // other entry is compared as spelled.
        let could_be_shims = dir == shims_dir
            || (dir.file_name().is_some() && dir.file_name() == shims_dir.file_name());
        let is_shims_dir = could_be_shims
            && match (dir.canonicalize().ok(), &canonical_shims_dir) {
                (Some(a), Some(b)) => &a == b,
                _ => dir == shims_dir,
            };
        if is_shims_dir {
            continue;
        }
        for name in names {
            let candidate = dir.join(name);
            if is_executable_file(&candidate) {
                return Some(candidate.canonicalize().unwrap_or(candidate));
            }
        }
    }
    None
}

/// Shared by the PATH walks in this module and in `detection_cache` — the
/// same question, asked for the same reason, so it has one answer.
#[cfg(unix)]
pub(crate) fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
pub(crate) fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

/// Deterministic, filesystem-safe id for a project root. A project's
/// canonical path used directly as a directory name risks length limits,
/// `/`, spaces, and non-UTF-8 segments, so the id is a short hash instead.
///
/// The digest is the workspace's shared, deliberately non-cryptographic one
/// (`crate::digest`): this identifies a project for cache-directory naming
/// and authenticates nothing.
pub fn project_id_for(canonical_project_root: &Path) -> String {
    crate::digest::short_hex(canonical_project_root.to_string_lossy().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[cfg(unix)]
    fn make_executable(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        fs::write(path, b"#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn resolves_real_executable_skipping_shims_dir_even_when_it_is_first_on_path() {
        let mut env = uze_testkit::env::scope();
        let root = uze_testkit::temp::scratch("resolve");
        let shims_dir = root.join("shims");
        let real_bin_dir = root.join("real-bin");
        fs::create_dir_all(&shims_dir).unwrap();
        fs::create_dir_all(&real_bin_dir).unwrap();

        // A shim named `claude` sits in shims_dir too — if the resolver ever
        // matched *any* file named `claude` first, it would find this one
        // before the real binary and recurse.
        make_executable(&shims_dir.join("claude"));
        make_executable(&real_bin_dir.join("claude"));

        env.set(
            "PATH",
            std::env::join_paths([&shims_dir, &real_bin_dir]).unwrap(),
        );

        let resolved = resolve_real_executable(&["claude"], &shims_dir).expect("resolved");
        assert_eq!(
            resolved,
            real_bin_dir.join("claude").canonicalize().unwrap()
        );
    }

    #[test]
    #[cfg(unix)]
    fn no_real_executable_on_path_resolves_to_none_not_the_shim() {
        let mut env = uze_testkit::env::scope();
        let root = uze_testkit::temp::scratch("resolve-none");
        let shims_dir = root.join("shims");
        fs::create_dir_all(&shims_dir).unwrap();
        make_executable(&shims_dir.join("claude"));

        env.set("PATH", std::env::join_paths([&shims_dir]).unwrap());

        assert_eq!(resolve_real_executable(&["claude"], &shims_dir), None);
    }

    #[test]
    fn project_id_is_deterministic_and_distinguishes_projects() {
        let a = project_id_for(Path::new("/tmp/project-a"));
        let a_again = project_id_for(Path::new("/tmp/project-a"));
        let b = project_id_for(Path::new("/tmp/project-b"));
        assert_eq!(a, a_again);
        assert_ne!(a, b);
        assert_eq!(a.len(), 16);
    }

    #[test]
    fn project_id_is_filesystem_safe_for_a_path_containing_spaces() {
        let id = project_id_for(Path::new("/tmp/a project with spaces"));
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
