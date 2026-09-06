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
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    error::{Result, UzeError},
    home::UzeHome,
    persistence::write_atomic,
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

/// Names the canonical project root every projection under a project's
/// runtime directory was built for.
///
/// It exists so the sweep below is a `readdir` and a `stat` rather than an
/// exercise in reading back artifacts an integration derived: the root is
/// the one fact deciding whether any of them still has a reason to exist,
/// and no vendor's own output is obliged to state it.
pub const PROJECTION_MARKER: &str = "project.json";

/// The two tenants of `runtime/`. Anything else directly beneath it is
/// unowned and swept — see `prune_projections`.
const RUNTIME_TENANTS: &[&str] = &["projects", "sessions"];

#[derive(Debug, Deserialize, Serialize)]
struct ProjectionMarker {
    root: PathBuf,
}

/// Creates (or refreshes) `integration`'s projection directory for
/// `canonical_project_root`, with the project's marker in place beside it,
/// and returns the directory the integration should write into.
///
/// Every integration goes through here rather than joining the path itself,
/// so the marker cannot be the one thing a new vendor forgets: a project
/// directory without one is indistinguishable from garbage, and the sweep
/// treats it as exactly that.
///
/// The marker is written before the integration's own directory, so a
/// project directory is identifiable for all but the few microseconds
/// between its creation and its marker. A sweep landing inside that window
/// removes a tree this call then recreates — the launch still gets its
/// projection, and the marker-less remains are swept on the next pass and
/// rebuilt on the one after. Nothing is lost either way, because a
/// projection is derived and rebuildable by definition.
pub fn prepare_projection(
    home: &UzeHome,
    integration: &str,
    canonical_project_root: &Path,
) -> Result<PathBuf> {
    let project_dir = home.runtime_project_dir(&project_id_for(canonical_project_root));
    create_dir(&project_dir)?;
    write_marker(&project_dir, canonical_project_root)?;
    let projection = project_dir.join(integration);
    create_dir(&projection)?;
    Ok(projection)
}

/// The canonical project root a project's runtime directory records, when
/// it still records one a reader can understand.
pub fn projection_root(project_dir: &Path) -> Option<PathBuf> {
    let raw = fs::read(project_dir.join(PROJECTION_MARKER)).ok()?;
    serde_json::from_slice::<ProjectionMarker>(&raw)
        .ok()
        .map(|marker| marker.root)
}

/// Removes every runtime projection whose project root is gone, plus
/// anything under `runtime/` that is neither of its two tenants, and
/// returns what it swept. Never fails: a tree it cannot read or delete is
/// left for the next pass rather than turned into an error a caller would
/// have to decide what to do about.
///
/// Existence of the recorded root is the only criterion, deliberately —
/// never mtime. A projection is written once and then skipped on every
/// launch that finds it already current, so its mtime says when the project
/// last *changed*, not when it was last used, and an age rule would collect
/// exactly the projections that work.
///
/// A project directory with no readable marker is swept too. That is the
/// same rule stated from the other side: what cannot be identified cannot
/// be shown to still be needed, and the cost of being wrong is one rebuild
/// on the next launch in that project.
pub fn prune_projections(home: &UzeHome) -> Vec<String> {
    let mut pruned = Vec::new();
    for entry in read_dir(&home.runtime_dir()) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if RUNTIME_TENANTS.contains(&name.as_str()) {
            continue;
        }
        if remove(&entry.path()) {
            pruned.push(name);
        }
    }
    for entry in read_dir(&home.runtime_projects_dir()) {
        let project_dir = entry.path();
        if !project_dir.is_dir() {
            continue;
        }
        if projection_root(&project_dir).is_some_and(|root| root.is_dir()) {
            continue;
        }
        if remove(&project_dir) {
            pruned.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    pruned
}

fn read_dir(path: &Path) -> impl Iterator<Item = fs::DirEntry> {
    fs::read_dir(path).into_iter().flatten().flatten()
}

fn remove(path: &Path) -> bool {
    if path.is_dir() {
        fs::remove_dir_all(path).is_ok()
    } else {
        fs::remove_file(path).is_ok()
    }
}

fn create_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path).map_err(|source| UzeError::Write {
        path: path.to_path_buf(),
        source,
    })
}

/// Idempotent for the same reason the projected instruction file is: two
/// sessions on one project compute the same directory and the same marker,
/// so a same-content write is skipped entirely rather than replacing a file
/// the other one is being identified by.
fn write_marker(project_dir: &Path, canonical_project_root: &Path) -> Result<()> {
    let path = project_dir.join(PROJECTION_MARKER);
    let desired = serde_json::to_vec_pretty(&ProjectionMarker {
        root: canonical_project_root.to_path_buf(),
    })
    .expect("marker serialization is infallible");
    if fs::read(&path).is_ok_and(|current| current == desired) {
        return Ok(());
    }
    write_atomic(&path, &desired)
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

    /// A project root that exists, with a projection already prepared for
    /// one integration. Returns the root and the home it was prepared in.
    fn projected(label: &str, integration: &str) -> (PathBuf, UzeHome) {
        let scratch = uze_testkit::temp::scratch(label);
        let project = scratch.join("project");
        fs::create_dir_all(&project).unwrap();
        let home = UzeHome::at(scratch.join("uze-home"));
        prepare_projection(&home, integration, &project).unwrap();
        (project, home)
    }

    #[test]
    fn a_projection_lives_under_its_project_and_records_the_root_it_was_built_for() {
        let scratch = uze_testkit::temp::scratch("projection-marker");
        let project = scratch.join("project");
        fs::create_dir_all(&project).unwrap();
        let home = UzeHome::at(scratch.join("uze-home"));

        let projection = prepare_projection(&home, "fake-harness", &project).unwrap();

        let project_dir = home.runtime_project_dir(&project_id_for(&project));
        assert_eq!(projection, project_dir.join("fake-harness"));
        assert!(projection.is_dir());
        assert_eq!(projection_root(&project_dir), Some(project.clone()));

        let _ = fs::remove_dir_all(&scratch);
    }

    /// The reason the tree is project-first: one lifetime, one marker to
    /// read, one directory to remove — however many harnesses project into
    /// it.
    #[test]
    fn every_integration_on_one_project_shares_its_directory_and_its_marker() {
        let (project, home) = projected("projection-shared", "first-harness");
        prepare_projection(&home, "second-harness", &project).unwrap();

        let projects: Vec<_> = fs::read_dir(home.runtime_projects_dir())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(projects.len(), 1);

        let project_dir = home.runtime_project_dir(&project_id_for(&project));
        let mut children: Vec<_> = fs::read_dir(&project_dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        children.sort();
        assert_eq!(
            children,
            ["first-harness", PROJECTION_MARKER, "second-harness"]
        );

        let _ = fs::remove_dir_all(project.parent().unwrap());
    }

    #[test]
    fn preparing_the_same_projection_again_changes_nothing() {
        let (project, home) = projected("projection-idempotent", "fake-harness");
        let project_dir = home.runtime_project_dir(&project_id_for(&project));
        let marker = fs::read(project_dir.join(PROJECTION_MARKER)).unwrap();

        prepare_projection(&home, "fake-harness", &project).unwrap();

        assert_eq!(
            fs::read(project_dir.join(PROJECTION_MARKER)).unwrap(),
            marker
        );
        assert_eq!(
            fs::read_dir(home.runtime_projects_dir()).unwrap().count(),
            1
        );

        let _ = fs::remove_dir_all(project.parent().unwrap());
    }

    #[test]
    fn a_projection_outlives_its_project_only_until_the_next_sweep() {
        let (project, home) = projected("projection-swept", "fake-harness");
        let project_dir = home.runtime_project_dir(&project_id_for(&project));

        // The project root goes — an agent's checkout removed on delivery,
        // a worktree deleted by hand, a directory moved.
        fs::remove_dir_all(&project).unwrap();

        assert_eq!(
            prune_projections(&home),
            vec![project_id_for(&project)],
            "a projection whose root is gone must be swept"
        );
        assert!(!project_dir.exists());

        let _ = fs::remove_dir_all(project.parent().unwrap());
    }

    #[test]
    fn a_project_that_still_exists_is_never_swept() {
        let (project, home) = projected("projection-live", "fake-harness");
        let project_dir = home.runtime_project_dir(&project_id_for(&project));

        assert!(prune_projections(&home).is_empty());
        assert!(project_dir.join("fake-harness").is_dir());

        let _ = fs::remove_dir_all(project.parent().unwrap());
    }

    /// What cannot be identified cannot be shown to still be needed. Both
    /// halves of that matter: a directory from before the marker existed,
    /// and one whose marker no reader can make sense of.
    #[test]
    fn a_project_directory_that_names_no_root_is_swept() {
        let scratch = uze_testkit::temp::scratch("projection-unidentifiable");
        let home = UzeHome::at(scratch.join("uze-home"));
        let unmarked = home.runtime_project_dir("unmarked");
        let corrupt = home.runtime_project_dir("corrupt");
        fs::create_dir_all(unmarked.join("fake-harness")).unwrap();
        fs::create_dir_all(&corrupt).unwrap();
        fs::write(corrupt.join(PROJECTION_MARKER), b"not json").unwrap();

        let mut pruned = prune_projections(&home);
        pruned.sort();
        assert_eq!(pruned, ["corrupt", "unmarked"]);
        assert!(!unmarked.exists() && !corrupt.exists());

        let _ = fs::remove_dir_all(&scratch);
    }

    /// The sweep owns `runtime/` itself, not just its projects: anything
    /// beside the two tenants is UZE's own derived output at a path nothing
    /// writes to any more, and rebuildable wherever it does belong.
    #[test]
    fn the_sweep_keeps_both_tenants_and_nothing_else() {
        let (project, home) = projected("projection-tenants", "fake-harness");
        let session = home.runtime_session_dir("fake-harness", "session-1");
        fs::create_dir_all(&session).unwrap();
        let abandoned = home.runtime_dir().join("fake-harness").join("projects");
        fs::create_dir_all(&abandoned).unwrap();
        fs::write(home.runtime_dir().join("stray.json"), b"{}").unwrap();

        let mut pruned = prune_projections(&home);
        pruned.sort();
        assert_eq!(pruned, ["fake-harness", "stray.json"]);

        assert!(!abandoned.exists());
        assert!(
            session.is_dir(),
            "a session receipt is the other tenant, not garbage"
        );
        assert!(
            home.runtime_project_dir(&project_id_for(&project)).is_dir(),
            "a live project must survive its neighbours being swept"
        );

        let _ = fs::remove_dir_all(project.parent().unwrap());
    }

    #[test]
    fn sweeping_a_home_with_no_runtime_tree_is_silent() {
        let scratch = uze_testkit::temp::scratch("projection-absent");
        let home = UzeHome::at(scratch.join("uze-home"));

        assert!(prune_projections(&home).is_empty());
        assert!(prune_projections(&home).is_empty());

        let _ = fs::remove_dir_all(&scratch);
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
