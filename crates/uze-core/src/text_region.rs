//! Managed Text Region — ownership of a delimited slice of a shared text
//! file, never the whole file.
//!
//! This module knows nothing about any harness, any package format, or
//! Instructions specifically. It answers exactly one question: "does the
//! region this caller identified by `region_identity` inside `target_file`
//! currently hold `expected_content`, and can it be safely
//! created/inspected/removed without touching anything else in that file?"
//!
//! A region is delimited by two whole-line HTML comment markers derived
//! deterministically from `region_identity`:
//!
//! ```text
//! <!-- uze:begin <region_identity> -->
//! ...content...
//! <!-- uze:end <region_identity> -->
//! ```
//!
//! Ownership is proven structurally (exactly one matching begin marker line,
//! exactly one matching end marker line, end after begin), never by
//! comparing free-form text. Any other marker shape for a given identity —
//! duplicated, out of order, or only half present — is a parsing-safety
//! failure, reported as `Blocked`, never guessed at.

use std::{fs, path::Path};

use crate::{
    error::{Result, UzeError},
    integration::{AttachmentInspection, AttachmentState},
    persistence::write_atomic,
};

/// Characters a region identity may contain. Deliberately narrow: an
/// identity built from these characters can never itself be mistaken for
/// marker syntax, so a whole-line-equality parser is sufficient without
/// escaping.
fn identity_is_valid(identity: &str) -> bool {
    !identity.is_empty()
        && identity
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '_' | '-' | '.' | '/'))
}

fn markers(region_identity: &str) -> (String, String) {
    (
        format!("<!-- uze:begin {region_identity} -->"),
        format!("<!-- uze:end {region_identity} -->"),
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Newline {
    Lf,
    Crlf,
}

impl Newline {
    fn separator(self) -> &'static str {
        match self {
            Newline::Lf => "\n",
            Newline::Crlf => "\r\n",
        }
    }
}

/// Reads `path` into logical lines (no trailing-newline sentinel, no `\r`)
/// plus the newline style to write back. `Ok(None)` means the file does not
/// exist yet — a legitimate, common state, not an error.
fn read_lines(path: &Path) -> Result<Option<(Vec<String>, Newline)>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(UzeError::Read {
                path: path.to_path_buf(),
                source: error,
            });
        }
    };
    let content =
        String::from_utf8(bytes).map_err(|_| UzeError::InvalidTextEncoding(path.to_path_buf()))?;
    let style = if content.contains("\r\n") {
        Newline::Crlf
    } else {
        Newline::Lf
    };
    let normalized = content.replace("\r\n", "\n");
    let body = normalized.strip_suffix('\n').unwrap_or(&normalized);
    let lines = if body.is_empty() {
        Vec::new()
    } else {
        body.split('\n').map(str::to_owned).collect()
    };
    Ok(Some((lines, style)))
}

fn write_lines(path: &Path, lines: &[String], style: Newline) -> Result<()> {
    let sep = style.separator();
    let mut out = lines.join(sep);
    if !lines.is_empty() {
        out.push_str(sep);
    }
    write_atomic(path, out.as_bytes())
}

/// Normalizes caller-supplied content into the exact physical lines that
/// belong between the markers: internal CRLF collapsed to LF (the file's own
/// style governs what is written back), and at most one trailing newline
/// stripped so content passed straight from a file's own bytes does not
/// produce a spurious blank line before the end marker.
fn content_lines(expected_content: &str) -> Vec<String> {
    let normalized = expected_content.replace("\r\n", "\n");
    let body = normalized.strip_suffix('\n').unwrap_or(&normalized);
    if body.is_empty() {
        Vec::new()
    } else {
        body.split('\n').map(str::to_owned).collect()
    }
}

enum Scan {
    Missing,
    WellFormed { begin: usize, end: usize },
    Malformed,
}

fn scan(lines: &[String], begin_marker: &str, end_marker: &str) -> Scan {
    let begins: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.as_str() == begin_marker)
        .map(|(index, _)| index)
        .collect();
    let ends: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.as_str() == end_marker)
        .map(|(index, _)| index)
        .collect();
    match (begins.as_slice(), ends.as_slice()) {
        ([], []) => Scan::Missing,
        (&[begin], &[end]) if end > begin => Scan::WellFormed { begin, end },
        _ => Scan::Malformed,
    }
}

fn blocked(reason: impl Into<String>) -> AttachmentInspection {
    AttachmentInspection {
        state: AttachmentState::Blocked,
        reason: reason.into(),
    }
}

/// Ownership state of one region, scoped to exactly that region — never the
/// rest of the file.
pub fn inspect(
    target_file: &Path,
    region_identity: &str,
    expected_content: &str,
) -> AttachmentInspection {
    if !identity_is_valid(region_identity) {
        return blocked(format!(
            "invalid managed-region identity `{region_identity}`"
        ));
    }
    let (begin_marker, end_marker) = markers(region_identity);
    let lines = match read_lines(target_file) {
        Ok(Some((lines, _style))) => lines,
        Ok(None) => {
            return AttachmentInspection {
                state: AttachmentState::Missing,
                reason: "managed text region's target file does not exist".to_owned(),
            };
        }
        Err(error) => return blocked(error.to_string()),
    };
    match scan(&lines, &begin_marker, &end_marker) {
        Scan::Missing => AttachmentInspection {
            state: AttachmentState::Missing,
            reason: "managed text region markers are absent".to_owned(),
        },
        Scan::Malformed => blocked(
            "managed text region markers are duplicated, out of order, or only half present"
                .to_owned(),
        ),
        Scan::WellFormed { begin, end } => {
            let current = lines[begin + 1..end].join("\n");
            let expected = content_lines(expected_content).join("\n");
            if current == expected {
                AttachmentInspection {
                    state: AttachmentState::Matched,
                    reason: "managed text region matches expected content".to_owned(),
                }
            } else {
                AttachmentInspection {
                    state: AttachmentState::Drifted,
                    reason: "managed text region content differs from expected".to_owned(),
                }
            }
        }
    }
}

/// Idempotently creates the region if it is currently `Missing`. Never
/// touches the file when the region is already `Matched`. Refuses — never
/// overwrites — when it is `Drifted`, `Blocked`, or `Conflict`: this
/// function's only safe outcome besides "already correct" is "correctly
/// created," never "silently repaired."
pub fn attach(target_file: &Path, region_identity: &str, expected_content: &str) -> Result<()> {
    if !identity_is_valid(region_identity) {
        return Err(UzeError::InvalidRegionIdentity(region_identity.to_owned()));
    }
    let (begin_marker, end_marker) = markers(region_identity);
    let (mut lines, style) = read_lines(target_file)?.unwrap_or((Vec::new(), Newline::Lf));
    match scan(&lines, &begin_marker, &end_marker) {
        Scan::WellFormed { begin, end } => {
            let current = lines[begin + 1..end].join("\n");
            let expected = content_lines(expected_content).join("\n");
            if current == expected {
                Ok(())
            } else {
                Err(UzeError::ManagedRegionDrift(target_file.to_path_buf()))
            }
        }
        Scan::Malformed => Err(UzeError::ManagedRegionConflict(target_file.to_path_buf())),
        Scan::Missing => {
            lines.push(begin_marker);
            lines.extend(content_lines(expected_content));
            lines.push(end_marker);
            write_lines(target_file, &lines, style)
        }
    }
}

/// Removes only the region's own marker lines and content lines. Every byte
/// outside them is preserved untouched, including line-ending style. Refuses
/// destructively when the freshly re-inspected state is not `Matched` —
/// `Missing` is a safe no-op (already gone), `Drifted`/`Blocked` refuse per
/// ADR-009.
pub fn detach(
    target_file: &Path,
    region_identity: &str,
    expected_content: &str,
) -> Result<AttachmentInspection> {
    let inspection = inspect(target_file, region_identity, expected_content);
    if inspection.state != AttachmentState::Matched {
        return Ok(inspection);
    }
    // Re-read immediately before the destructive write, mirroring
    // `detach_standard_receipt`'s symlink re-check: this protects the normal
    // non-concurrent case where state changed between a prior inspect and
    // this detach call.
    let fresh = inspect(target_file, region_identity, expected_content);
    if fresh.state != AttachmentState::Matched {
        return Ok(fresh);
    }
    let (begin_marker, end_marker) = markers(region_identity);
    let (mut lines, style) = read_lines(target_file)?
        .ok_or_else(|| UzeError::ManagedRegionConflict(target_file.to_path_buf()))?;
    let Scan::WellFormed { begin, end } = scan(&lines, &begin_marker, &end_marker) else {
        return Ok(blocked(
            "managed text region changed shape between inspection and detach".to_owned(),
        ));
    };
    lines.drain(begin..=end);
    write_lines(target_file, &lines, style)?;
    Ok(AttachmentInspection {
        state: AttachmentState::Missing,
        reason: "managed text region detached".to_owned(),
    })
}

/// Ensures a region exists exactly when `should_exist` is true, and is
/// absent otherwise — both directions going through the same safety rules
/// as `attach`/`detach` (idempotent, never overwrites drift, never
/// destructively removes drift). Convenience for a caller whose own state
/// (not this module's business) decides whether a region is currently
/// wanted.
pub fn reconcile(
    target_file: &Path,
    region_identity: &str,
    expected_content: &str,
    should_exist: bool,
) -> AttachmentInspection {
    if should_exist {
        let _ = attach(target_file, region_identity, expected_content);
    } else {
        let _ = detach(target_file, region_identity, expected_content);
    }
    inspect(target_file, region_identity, expected_content)
}

/// Structural well-formedness of a region's markers, independent of any
/// content comparison. This is the same proof `remove_unconditionally`
/// relies on to act, exposed here so a caller can **preview** a structural-
/// only removal — e.g. for a read-only report — without performing it and
/// without needing `expected_content`, which content-verified `inspect`
/// requires.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegionShape {
    /// The target file does not exist, or has no marker lines for this
    /// identity at all.
    Absent,
    /// Exactly one well-formed begin/end pair — removable by
    /// `remove_unconditionally`, inspectable by `inspect` if a caller has
    /// `expected_content`.
    WellFormed,
    /// Duplicated, out of order, or only half present — refuses any
    /// destructive operation, the same way `inspect`/`detach` do.
    Malformed,
}

/// Previews what `remove_unconditionally` would find, without touching the
/// file. See `RegionShape` for what each outcome means.
pub fn region_shape(target_file: &Path, region_identity: &str) -> RegionShape {
    if !identity_is_valid(region_identity) {
        return RegionShape::Malformed;
    }
    let (begin_marker, end_marker) = markers(region_identity);
    let Ok(Some((lines, _style))) = read_lines(target_file) else {
        return RegionShape::Absent;
    };
    match scan(&lines, &begin_marker, &end_marker) {
        Scan::Missing => RegionShape::Absent,
        Scan::WellFormed { .. } => RegionShape::WellFormed,
        Scan::Malformed => RegionShape::Malformed,
    }
}

/// Removes a region identified by structure alone — exactly one well-formed
/// begin/end pair for `region_identity` — without comparing its content to
/// anything. This is deliberately **weaker** than `detach`: it exists only
/// for a caller that has already determined, by its own means, that no
/// current source owns this identity any more (an "orphaned" region), and
/// therefore has no `expected_content` left to verify drift against.
/// Ownership is proven the same way `attach`/`inspect` always prove it —
/// exact marker match, never a guess — but content drift *inside* an
/// already-orphaned region cannot be detected here, because there is no
/// longer anything to compare it to. Callers should prefer `detach` whenever
/// `expected_content` is available. See `region_shape` to preview this
/// function's outcome read-only.
pub fn remove_unconditionally(
    target_file: &Path,
    region_identity: &str,
) -> Result<AttachmentInspection> {
    match region_shape(target_file, region_identity) {
        RegionShape::Absent => Ok(AttachmentInspection {
            state: AttachmentState::Missing,
            reason: "managed text region markers are absent".to_owned(),
        }),
        RegionShape::Malformed => Ok(blocked(
            "managed text region markers are duplicated, out of order, or only half present"
                .to_owned(),
        )),
        RegionShape::WellFormed => {
            let (begin_marker, end_marker) = markers(region_identity);
            // Absence and validity were already established by
            // `region_shape`; re-reading is cheap and keeps this function
            // free of unsafe unwraps on that already-proven state.
            let (mut lines, style) =
                read_lines(target_file)?.expect("region_shape proved the file exists");
            let Scan::WellFormed { begin, end } = scan(&lines, &begin_marker, &end_marker) else {
                return Ok(blocked(
                    "managed text region changed shape between preview and removal".to_owned(),
                ));
            };
            lines.drain(begin..=end);
            write_lines(target_file, &lines, style)?;
            Ok(AttachmentInspection {
                state: AttachmentState::Missing,
                reason: "orphaned managed text region removed (no current source owns it)"
                    .to_owned(),
            })
        }
    }
}

/// The `region_identity` of every well-formed managed region currently in
/// `target_file`, in file order. A missing or unreadable file yields an
/// empty list.
pub fn region_identities_present(target_file: &Path) -> Vec<String> {
    let Ok(Some((lines, _style))) = read_lines(target_file) else {
        return Vec::new();
    };
    lines
        .iter()
        .filter_map(|line| {
            line.strip_prefix("<!-- uze:begin ")
                .and_then(|rest| rest.strip_suffix(" -->"))
        })
        .map(str::to_owned)
        .collect()
}

/// Whether `target_file` holds any content that is not part of a
/// well-formed UZE-managed region — i.e. whether a user (or anything other
/// than UZE) wrote something into it independently. A missing file, or one
/// containing only managed regions and blank lines, reports `false`. A
/// malformed region's lines count as content *outside* any region, since
/// ownership of them cannot be proven — the same fail-closed default every
/// other function in this module applies.
pub fn has_content_outside_managed_regions(target_file: &Path) -> bool {
    let Ok(Some((lines, _style))) = read_lines(target_file) else {
        return false;
    };
    let mut owned = std::collections::BTreeSet::new();
    for identity in region_identities_present(target_file) {
        let (begin_marker, end_marker) = markers(&identity);
        if let Scan::WellFormed { begin, end } = scan(&lines, &begin_marker, &end_marker) {
            owned.extend(begin..=end);
        }
    }
    lines
        .iter()
        .enumerate()
        .any(|(index, line)| !owned.contains(&index) && !line.trim().is_empty())
}

/// Whether `target_file` currently holds *any* UZE-managed region at all,
/// regardless of identity. Generic infrastructure-lifecycle check: a caller
/// that manages a second, resource-independent artifact conditioned on "is
/// this file still needed by at least one region" uses this rather than
/// tracking specific identities itself. A missing or unreadable file
/// conservatively reports `false` — no content means no dependent region.
pub fn any_region_present(target_file: &Path) -> bool {
    let Ok(Some((lines, _style))) = read_lines(target_file) else {
        return false;
    };
    lines
        .iter()
        .any(|line| line.starts_with("<!-- uze:begin ") && line.ends_with(" -->"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "uze-text-region-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    // --- attach --------------------------------------------------------

    #[test]
    fn attach_creates_the_file_when_it_does_not_exist() {
        let root = temp("create");
        let file = root.join("NOTES.md");
        attach(&file, "pkg-a/instructions", "hello\nworld").unwrap();
        let content = fs::read_to_string(&file).unwrap();
        assert_eq!(
            content,
            "<!-- uze:begin pkg-a/instructions -->\nhello\nworld\n<!-- uze:end pkg-a/instructions -->\n"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn attach_preserves_existing_user_content_and_appends() {
        let root = temp("preserve");
        fs::create_dir_all(&root).unwrap();
        let file = root.join("NOTES.md");
        fs::write(&file, "user text A\nuser text B\n").unwrap();
        attach(&file, "pkg-a/instructions", "managed content").unwrap();
        let content = fs::read_to_string(&file).unwrap();
        assert_eq!(
            content,
            "user text A\nuser text B\n<!-- uze:begin pkg-a/instructions -->\nmanaged content\n<!-- uze:end pkg-a/instructions -->\n"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn attach_is_idempotent_when_already_matched() {
        let root = temp("idempotent");
        let file = root.join("project-context.md");
        attach(&file, "id", "content").unwrap();
        let before = fs::read_to_string(&file).unwrap();
        attach(&file, "id", "content").unwrap();
        let after = fs::read_to_string(&file).unwrap();
        assert_eq!(before, after, "a matched region is never rewritten");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn attach_refuses_to_overwrite_drifted_content() {
        let root = temp("drift-refuse");
        let file = root.join("NOTES.md");
        attach(&file, "id", "original").unwrap();
        let path = file.clone();
        let mut content = fs::read_to_string(&path).unwrap();
        content = content.replace("original", "user-edited");
        fs::write(&path, content).unwrap();
        let error = attach(&file, "id", "original").unwrap_err();
        assert!(matches!(error, UzeError::ManagedRegionDrift(_)));
        assert!(fs::read_to_string(&file).unwrap().contains("user-edited"));
        fs::remove_dir_all(root).unwrap();
    }

    // --- inspect ---------------------------------------------------------

    #[test]
    fn inspect_scopes_to_the_declared_region_only() {
        let root = temp("scoped-inspect");
        let file = root.join("NOTES.md");
        attach(&file, "id", "managed").unwrap();

        // Editing text OUTSIDE the region leaves it MATCHED.
        let mut content = fs::read_to_string(&file).unwrap();
        content = format!("user note\n{content}user note after\n");
        fs::write(&file, &content).unwrap();
        assert_eq!(
            inspect(&file, "id", "managed").state,
            AttachmentState::Matched
        );

        // Editing text INSIDE the region becomes DRIFTED.
        let drifted = content.replace("managed", "tampered");
        fs::write(&file, drifted).unwrap();
        assert_eq!(
            inspect(&file, "id", "managed").state,
            AttachmentState::Drifted
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn inspect_reports_missing_for_absent_file_and_absent_region() {
        let root = temp("missing");
        let file = root.join("NOTES.md");
        assert_eq!(
            inspect(&file, "id", "x").state,
            AttachmentState::Missing,
            "no file at all"
        );
        fs::create_dir_all(&root).unwrap();
        fs::write(&file, "just user content\n").unwrap();
        assert_eq!(
            inspect(&file, "id", "x").state,
            AttachmentState::Missing,
            "file exists but has no markers"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn duplicate_or_malformed_markers_are_blocked_never_guessed() {
        let root = temp("malformed");
        fs::create_dir_all(&root).unwrap();
        let file = root.join("NOTES.md");

        fs::write(
            &file,
            "<!-- uze:begin id -->\na\n<!-- uze:end id -->\n<!-- uze:begin id -->\nb\n<!-- uze:end id -->\n",
        )
        .unwrap();
        assert_eq!(inspect(&file, "id", "a").state, AttachmentState::Blocked);

        fs::write(&file, "<!-- uze:begin id -->\nno end marker\n").unwrap();
        assert_eq!(inspect(&file, "id", "x").state, AttachmentState::Blocked);

        fs::write(&file, "<!-- uze:end id -->\nno begin marker\n").unwrap();
        assert_eq!(inspect(&file, "id", "x").state, AttachmentState::Blocked);

        // End marker appears before begin marker: still malformed, not a
        // reversed-but-valid region.
        fs::write(&file, "<!-- uze:end id -->\nstuff\n<!-- uze:begin id -->\n").unwrap();
        assert_eq!(inspect(&file, "id", "x").state, AttachmentState::Blocked);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_literal_marker_line_inside_user_content_for_the_same_identity_is_blocked_not_guessed() {
        let root = temp("literal-marker");
        fs::create_dir_all(&root).unwrap();
        let file = root.join("NOTES.md");
        // Two begin markers for "id": one real, one that just happens to be
        // literal package content. The parser cannot know which is
        // authoritative, so it must refuse rather than pick one.
        fs::write(
            &file,
            "<!-- uze:begin id -->\n<!-- uze:begin id -->\n<!-- uze:end id -->\n",
        )
        .unwrap();
        assert_eq!(inspect(&file, "id", "x").state, AttachmentState::Blocked);
        fs::remove_dir_all(root).unwrap();
    }

    // --- detach ------------------------------------------------------------

    #[test]
    fn detach_removes_only_the_region_between_two_pieces_of_user_content() {
        let root = temp("detach-between");
        let file = root.join("NOTES.md");
        fs::create_dir_all(&root).unwrap();
        fs::write(&file, "user text A\n").unwrap();
        attach(&file, "pkg/resource", "managed content").unwrap();
        let mut content = fs::read_to_string(&file).unwrap();
        content.push_str("user text B\n");
        fs::write(&file, &content).unwrap();

        let result = detach(&file, "pkg/resource", "managed content").unwrap();
        assert_eq!(result.state, AttachmentState::Missing);
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "user text A\nuser text B\n"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn detach_on_missing_region_is_a_safe_no_op() {
        let root = temp("detach-missing");
        fs::create_dir_all(&root).unwrap();
        let file = root.join("NOTES.md");
        fs::write(&file, "just user content\n").unwrap();
        let result = detach(&file, "id", "x").unwrap();
        assert_eq!(result.state, AttachmentState::Missing);
        assert_eq!(fs::read_to_string(&file).unwrap(), "just user content\n");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn drifted_region_blocks_destructive_detach_per_adr_009() {
        let root = temp("detach-drifted");
        let file = root.join("NOTES.md");
        attach(&file, "id", "original").unwrap();
        let tampered = fs::read_to_string(&file)
            .unwrap()
            .replace("original", "user-edited");
        fs::write(&file, &tampered).unwrap();

        let result = detach(&file, "id", "original").unwrap();
        assert_eq!(result.state, AttachmentState::Drifted);
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            tampered,
            "a drifted region must never be destructively removed"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn detach_leaves_an_empty_file_rather_than_deleting_a_preexisting_file() {
        let root = temp("detach-empty");
        fs::create_dir_all(&root).unwrap();
        let file = root.join("NOTES.md");
        fs::write(&file, "").unwrap(); // pre-existing, empty, user-owned file
        attach(&file, "id", "content").unwrap();
        detach(&file, "id", "content").unwrap();
        assert!(
            file.exists(),
            "detach never deletes a file it found pre-existing"
        );
        assert_eq!(fs::read_to_string(&file).unwrap(), "");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn multiple_regions_from_different_identities_coexist_and_detach_independently() {
        let root = temp("multi-region");
        let file = root.join("NOTES.md");
        attach(&file, "pkg-a/instructions", "content A").unwrap();
        attach(&file, "pkg-b/instructions", "content B").unwrap();
        assert_eq!(
            inspect(&file, "pkg-a/instructions", "content A").state,
            AttachmentState::Matched
        );
        assert_eq!(
            inspect(&file, "pkg-b/instructions", "content B").state,
            AttachmentState::Matched
        );

        detach(&file, "pkg-a/instructions", "content A").unwrap();
        assert_eq!(
            inspect(&file, "pkg-a/instructions", "content A").state,
            AttachmentState::Missing
        );
        assert_eq!(
            inspect(&file, "pkg-b/instructions", "content B").state,
            AttachmentState::Matched,
            "detaching one package's region must not disturb another's"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn crlf_files_are_preserved_through_attach_and_detach() {
        let root = temp("crlf");
        fs::create_dir_all(&root).unwrap();
        let file = root.join("NOTES.md");
        fs::write(&file, "user text A\r\nuser text B\r\n").unwrap();
        attach(&file, "id", "managed").unwrap();
        let raw = fs::read(&file).unwrap();
        let content = String::from_utf8(raw).unwrap();
        assert!(content.contains("\r\n"), "CRLF style must be preserved");
        assert!(!content.replace("\r\n", "").contains('\r'), "no stray CR");

        detach(&file, "id", "managed").unwrap();
        let restored = fs::read(&file).unwrap();
        assert_eq!(restored, b"user text A\r\nuser text B\r\n");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_file_missing_a_trailing_newline_is_normalized_on_attach() {
        let root = temp("no-trailing-newline");
        fs::create_dir_all(&root).unwrap();
        let file = root.join("NOTES.md");
        fs::write(&file, "user text with no trailing newline").unwrap();
        attach(&file, "id", "managed").unwrap();
        let content = fs::read_to_string(&file).unwrap();
        assert!(content.starts_with("user text with no trailing newline\n<!-- uze:begin id -->"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unicode_content_round_trips_untouched() {
        let root = temp("unicode");
        let file = root.join("NOTES.md");
        attach(&file, "id", "café — 日本語 — emoji 🎉").unwrap();
        assert_eq!(
            inspect(&file, "id", "café — 日本語 — emoji 🎉").state,
            AttachmentState::Matched
        );
        detach(&file, "id", "café — 日本語 — emoji 🎉").unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn region_shape_previews_remove_unconditionally_without_writing() {
        let root = temp("shape-preview");
        fs::create_dir_all(&root).unwrap();
        let file = root.join("NOTES.md");
        assert_eq!(region_shape(&file, "id"), RegionShape::Absent);

        attach(&file, "id", "content").unwrap();
        let before = fs::read_to_string(&file).unwrap();
        assert_eq!(region_shape(&file, "id"), RegionShape::WellFormed);
        let after = fs::read_to_string(&file).unwrap();
        assert_eq!(before, after, "region_shape must never write");

        fs::write(
            &file,
            "<!-- uze:begin dup -->\na\n<!-- uze:end dup -->\n<!-- uze:begin dup -->\nb\n<!-- uze:end dup -->\n",
        )
        .unwrap();
        assert_eq!(region_shape(&file, "dup"), RegionShape::Malformed);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn has_content_outside_managed_regions_distinguishes_user_content_from_pure_regions() {
        let root = temp("outside-content");
        let file = root.join("NOTES.md");
        assert!(
            !has_content_outside_managed_regions(&file),
            "missing file has no content"
        );

        attach(&file, "id", "managed only").unwrap();
        assert!(
            !has_content_outside_managed_regions(&file),
            "a file holding only a well-formed region has no *outside* content"
        );

        let mut content = fs::read_to_string(&file).unwrap();
        content = format!("user note\n{content}");
        fs::write(&file, &content).unwrap();
        assert!(has_content_outside_managed_regions(&file));

        // A malformed region's own lines count as unattributable content.
        fs::write(
            &file,
            "<!-- uze:begin dup -->\na\n<!-- uze:end dup -->\n<!-- uze:begin dup -->\nb\n<!-- uze:end dup -->\n",
        )
        .unwrap();
        assert!(has_content_outside_managed_regions(&file));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn any_region_present_detects_regardless_of_identity() {
        let root = temp("any-present");
        let file = root.join("NOTES.md");
        assert!(!any_region_present(&file), "missing file has no regions");
        attach(&file, "id", "content").unwrap();
        assert!(any_region_present(&file));
        detach(&file, "id", "content").unwrap();
        assert!(!any_region_present(&file));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reconcile_creates_when_wanted_and_removes_when_not() {
        let root = temp("reconcile");
        let file = root.join("NOTES.md");
        assert_eq!(
            reconcile(&file, "id", "content", true).state,
            AttachmentState::Matched
        );
        assert_eq!(
            reconcile(&file, "id", "content", true).state,
            AttachmentState::Matched,
            "reconciling an already-wanted region is a no-op, not a rewrite"
        );
        assert_eq!(
            reconcile(&file, "id", "content", false).state,
            AttachmentState::Missing
        );
        assert_eq!(
            reconcile(&file, "id", "content", false).state,
            AttachmentState::Missing,
            "reconciling an already-absent, unwanted region is a safe no-op"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reconcile_refuses_to_remove_a_drifted_but_unwanted_region() {
        let root = temp("reconcile-drift");
        let file = root.join("NOTES.md");
        attach(&file, "id", "original").unwrap();
        let tampered = fs::read_to_string(&file)
            .unwrap()
            .replace("original", "user-edited");
        fs::write(&file, &tampered).unwrap();
        let result = reconcile(&file, "id", "original", false);
        assert_eq!(result.state, AttachmentState::Drifted);
        assert_eq!(fs::read_to_string(&file).unwrap(), tampered);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn region_identities_present_lists_every_well_formed_region() {
        let root = temp("identities");
        let file = root.join("NOTES.md");
        assert!(region_identities_present(&file).is_empty());
        attach(&file, "package:a:instructions", "A").unwrap();
        attach(&file, "package:b:instructions", "B").unwrap();
        assert_eq!(
            region_identities_present(&file),
            vec![
                "package:a:instructions".to_owned(),
                "package:b:instructions".to_owned()
            ]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn remove_unconditionally_removes_a_well_formed_orphaned_region_but_refuses_malformed_ones() {
        let root = temp("orphan");
        let file = root.join("NOTES.md");
        fs::create_dir_all(&root).unwrap();
        fs::write(&file, "user text A\n").unwrap();
        attach(&file, "package:gone:instructions", "old content").unwrap();
        let mut content = fs::read_to_string(&file).unwrap();
        content.push_str("user text B\n");
        fs::write(&file, &content).unwrap();

        let result = remove_unconditionally(&file, "package:gone:instructions").unwrap();
        assert_eq!(result.state, AttachmentState::Missing);
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "user text A\nuser text B\n"
        );

        // A malformed/duplicated shape still refuses, exactly like detach.
        fs::write(
            &file,
            "<!-- uze:begin dup -->\na\n<!-- uze:end dup -->\n<!-- uze:begin dup -->\nb\n<!-- uze:end dup -->\n",
        )
        .unwrap();
        assert_eq!(
            remove_unconditionally(&file, "dup").unwrap().state,
            AttachmentState::Blocked
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_region_identity_is_rejected_not_sanitized() {
        let root = temp("invalid-id");
        let file = root.join("NOTES.md");
        assert!(attach(&file, "has spaces", "x").is_err());
        assert!(attach(&file, "has\nnewline", "x").is_err());
        assert_eq!(
            inspect(&file, "has spaces", "x").state,
            AttachmentState::Blocked
        );
    }
}
