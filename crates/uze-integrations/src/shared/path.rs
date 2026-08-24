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
//! Deliberately narrow: this is the shared VALIDATION predicate only, not
//! a universal path parser. What counts as a declared path field at all
//! (`skills: [...]` vs `skills: "..."`, one array entry vs one whole
//! directory, an inline object vs an external file reference) stays
//! entirely vendor-specific — Claude's and Codex's own manifest schemas
//! differ in that shape, and this function has no opinion on it. Both
//! Antigravity's and Claude's/Codex's coverage functions that read a
//! structural surface rather than a declared path never call this module.

use std::path::{Path, PathBuf};

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
    if trimmed.is_empty() || trimmed == "." || Path::new(trimmed).is_absolute() {
        return None;
    }
    if trimmed.split('/').any(|component| component == "..") {
        return None;
    }
    Some(PathBuf::from(trimmed))
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
