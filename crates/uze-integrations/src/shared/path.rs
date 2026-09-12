//! The one path-safety predicate proven identical across every native
//! package integration that parses a manifest-declared path: reject
//! anything absolute, `..`-escaping, empty, or a bare `.` — never coerce
//! it into something safe. Extracted after fixing a real divergence this
//! predicate itself is the fix for (see this function's own history):
//! Claude's per-entry normalization used to strip a leading `/` before
//! testing `Path::is_absolute()`, so `/etc/passwd` silently became the
//! relative declaration `etc/passwd` instead of being rejected — Codex's
//! independently-written equivalent never did this and was already
//! correct. Once Claude's copy was fixed to match, the two were no longer
//! an ACCIDENTAL_SIMILARITY (the Integration Capability Contracts Audit's
//! classification at the time) but a REAL_SHARED_CONTRACT, so this module
//! is that contract's one home.
//!
//! Beside it live the other path facts every integration states the same
//! way: where a harness's generated artifacts are staged, and the removal
//! of a wrapper nothing references any more.
//!
//! Deliberately narrow: the validation predicate is a predicate only, not
//! a universal path parser. What counts as a declared path field at all
//! (`skills: [...]` vs `skills: "..."`, one array entry vs one whole
//! directory, an inline object vs an external file reference) stays
//! entirely vendor-specific — Claude's and Codex's own manifest schemas
//! differ in that shape, and this function has no opinion on it. Both
//! Antigravity's and Claude's/Codex's coverage functions that read a
//! structural surface rather than a declared path never call this module.

use std::path::{Component, Path, PathBuf};

use uze_core::{Result, UzeError, home::UzeHome};

/// Normalizes one manifest-declared, package-relative path string.
///
/// Accepts a conventional leading `./` and surrounding whitespace; rejects
/// (returns `None` for) anything absolute, empty, a bare `.`, or
/// containing a `..` component. Never widens what it accepts beyond that —
/// in particular, it never strips a leading `/` before deciding whether
/// the declaration is absolute, which is exactly the bug this function
/// replaces.
pub(crate) fn normalize_declared_relative_path(raw: &str) -> Option<PathBuf> {
    let trimmed = raw.trim().trim_start_matches("./").trim_end_matches('/');
    if trimmed.is_empty() || trimmed == "." {
        return None;
    }
    // Asked of the path's own components, not of its text. Both string
    // tests were Unix spellings of the question: `is_absolute` needs a
    // prefix *and* a root on Windows, so `/etc/passwd` is not absolute
    // there — the exact regression this function replaced, returning by
    // another door — and splitting on `/` never sees the `..` in
    // `..\..\Windows`. `Component` answers for the platform the code is
    // running on, and rejecting `Prefix` and `RootDir` alongside
    // `ParentDir` covers `C:\`, `C:foo` and `\\server\share` without
    // naming any of them.
    Path::new(trimmed)
        .components()
        .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
        .then(|| PathBuf::from(trimmed))
}

/// Removes the directory `artifact` was staged in, when detaching `artifact`
/// left it empty — never climbing past `root`.
///
/// Every integration stages a generated artifact two levels deep,
/// `<root>/<package>/<capability>`, so detaching a package's last capability
/// used to leave the package's own directory behind: empty, covered by no
/// receipt, and accumulating one per removed plugin per harness. It is UZE's
/// own state rather than anything a harness reads, which is why it went
/// unnoticed — and why it is worth removing, since "every managed artifact is
/// tracked by a receipt" is either true or it is not.
///
/// `remove_dir` is the guard as much as the action: it refuses a directory
/// that still holds another of the package's capabilities, so no check for
/// siblings is needed and none can go stale.
pub fn prune_empty_package_dir(artifact: &Path, root: &Path) {
    let Some(parent) = artifact.parent() else {
        return;
    };
    if parent == root || !parent.starts_with(root) {
        return;
    }
    let _ = std::fs::remove_dir(parent);
}

/// Everything UZE stages for one harness sits under this root; each kind
/// of generated artifact is a directory inside it (`skills`, `plugins`,
/// `generated`). One shape named in one place, so a caller says which kind
/// it means instead of relying on which sibling module a same-named
/// helper came from.
pub(crate) fn attachment_root(uze_home: &UzeHome, vendor: &str) -> PathBuf {
    uze_home.state_dir().join("attachments").join(vendor)
}

/// Removes a generated wrapper directory once nothing in `skills_dir`
/// links to it any more, then prunes the package directory it leaves
/// empty. Only ever touches UZE-owned directories under `managed_root`.
///
/// `is_uze_wrapper` is the harness's own proof that `target` really is the
/// artifact it generated — the one thing that differs between harnesses
/// here, since only Claude's shim carries a plugin manifest beside its
/// `SKILL.md`.
pub(crate) fn cleanup_unused_wrapper(
    target: &Path,
    managed_root: &Path,
    skills_dir: &Path,
    prune_root: &Path,
    is_uze_wrapper: &dyn Fn(&Path) -> bool,
) -> Result<()> {
    if !target.starts_with(managed_root) || !target.is_dir() {
        return Ok(());
    }
    let referenced = std::fs::read_dir(skills_dir)
        .map_err(|source| UzeError::Read {
            path: skills_dir.to_path_buf(),
            source,
        })?
        .filter_map(std::result::Result::ok)
        .any(|entry| std::fs::read_link(entry.path()).ok().as_deref() == Some(target));
    if referenced || !is_uze_wrapper(target) {
        return Ok(());
    }
    std::fs::remove_dir_all(target).map_err(|source| UzeError::Write {
        path: target.to_path_buf(),
        source,
    })?;
    prune_empty_package_dir(target, prune_root);
    Ok(())
}

#[cfg(test)]
mod normalize_declared_relative_path_tests {
    use super::*;

    /// The exact regression case: a leading `/` must reject, never become
    /// a relative declaration by having the `/` quietly stripped.
    #[test]
    fn leading_slash_is_rejected_as_absolute_not_stripped_into_relative() {
        assert_eq!(normalize_declared_relative_path("/skills/foo"), None);
        assert_eq!(normalize_declared_relative_path("/etc/passwd"), None);
    }

    /// The Windows spellings of the same two attacks. They are strings, so
    /// this test runs everywhere — but it only *fails* where the platform
    /// reads them as paths, which is the point: the predicate has to be
    /// right on the machine it runs on, and a Unix-only reading of "is
    /// this absolute" silently accepts `\etc\passwd` there.
    #[test]
    #[cfg(windows)]
    fn windows_spellings_of_absolute_and_escaping_are_rejected() {
        assert_eq!(normalize_declared_relative_path(r"\etc\passwd"), None);
        assert_eq!(
            normalize_declared_relative_path(r"C:\Windows\System32"),
            None
        );
        assert_eq!(normalize_declared_relative_path(r"C:skills"), None);
        assert_eq!(normalize_declared_relative_path(r"..\..\Windows"), None);
        assert_eq!(normalize_declared_relative_path(r"\\server\share\x"), None);
    }

    /// A backslash is a legal character in a Unix filename, so the same
    /// strings must be accepted here — rejecting them everywhere would be
    /// a different bug, and one a Windows-shaped fix invites.
    #[test]
    #[cfg(unix)]
    fn a_backslash_is_an_ordinary_character_on_unix() {
        assert_eq!(
            normalize_declared_relative_path(r"skills\odd\name"),
            Some(PathBuf::from(r"skills\odd\name"))
        );
    }

    /// Whitespace padding must not defeat the absolute-path check either
    /// (`" /etc"` has no leading `/` at byte 0, so an implementation that
    /// checks `is_absolute()` without trimming first would accept it).
    #[test]
    fn whitespace_padded_absolute_path_is_still_rejected() {
        assert_eq!(normalize_declared_relative_path("  /etc/passwd  "), None);
    }

    #[test]
    fn conventional_relative_forms_are_accepted() {
        assert_eq!(
            normalize_declared_relative_path("skills/foo"),
            Some(PathBuf::from("skills/foo"))
        );
        assert_eq!(
            normalize_declared_relative_path("./skills/foo"),
            Some(PathBuf::from("skills/foo"))
        );
        assert_eq!(
            normalize_declared_relative_path("skills/foo/"),
            Some(PathBuf::from("skills/foo"))
        );
    }

    #[test]
    fn parent_directory_escape_is_rejected_anywhere_in_the_path() {
        assert_eq!(normalize_declared_relative_path("../skills/foo"), None);
        assert_eq!(normalize_declared_relative_path("skills/../foo"), None);
        assert_eq!(normalize_declared_relative_path("../../etc"), None);
    }

    #[test]
    fn empty_and_dot_only_are_rejected() {
        assert_eq!(normalize_declared_relative_path(""), None);
        assert_eq!(normalize_declared_relative_path("."), None);
        assert_eq!(normalize_declared_relative_path("./"), None);
    }

    /// Repeated separators (`skills//foo`) and embedded `.` segments
    /// (`skills/./foo`) are accepted as strings (nothing here claims
    /// they're forbidden syntax) — Rust's own `Path` component parsing
    /// already normalizes both away (a `.` is "skipped, except as the
    /// first component," per `std::path::Components`' own documentation;
    /// repeated separators are documented as collapsed the same way), so
    /// `PathBuf` equality/`starts_with` (Codex's matcher) treats either
    /// form as identical to `skills/foo` — never a widened match, just the
    /// same real directory spelled redundantly. A raw-string matcher
    /// (Claude's) sees them as distinct strings and so never matches them
    /// at all. Neither behavior is a safety gap: at worst a redundant
    /// declaration is either recognized (Codex) or silently ineffective
    /// (Claude) — never a widened or escaped match.
    #[test]
    fn repeated_separators_and_embedded_dot_segments_are_safe_either_way() {
        let real = PathBuf::from("skills/foo");

        let repeated = normalize_declared_relative_path("skills//foo").unwrap();
        assert_ne!(
            repeated.to_string_lossy(),
            real.to_string_lossy(),
            "as a raw string, never literally equal to the real path (Claude's matcher)"
        );
        assert_eq!(
            repeated, real,
            "but as a Path, component-wise equal (Codex's matcher) — Rust's own Path type \
             already collapses redundant separators for comparison"
        );

        let embedded_dot = normalize_declared_relative_path("skills/./foo").unwrap();
        assert_ne!(
            embedded_dot.to_string_lossy(),
            real.to_string_lossy(),
            "as a raw string, never literally equal to the real path (Claude's matcher)"
        );
        assert_eq!(
            embedded_dot, real,
            "but as a Path, component-wise equal (Codex's matcher) — a `.` component is \
             normalized away, not preserved, except as the very first component of a path"
        );
    }
}
