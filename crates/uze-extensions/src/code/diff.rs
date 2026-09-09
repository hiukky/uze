//! A unified diff, parsed and paired into the side-by-side shape the
//! overlay draws.
//!
//! Three steps that stay separate on purpose — parse Git's own format,
//! pair the runs of removals against the runs of additions, then colour
//! the result — because only the last of those is expensive, and only the
//! first two are worth testing on their own.

use std::path::Path;

use crate::view::{ContentLine, LineTone, Rgb, Role, Span};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DiffLineKind {
    Context,
    Added,
    Removed,
}

/// One line on one side of the side-by-side diff — see [`DiffRow`].
pub(super) struct DiffCell {
    pub(super) line_no: u32,
    pub(super) kind: DiffLineKind,
    /// Pre-highlighted (see `highlight_diff_rows`) — no syntect types
    /// beyond this module's boundary, and the colour travels as data
    /// because it comes from the syntax theme rather than from the host's
    /// palette (see [`crate::view::Rgb`]).
    pub(super) spans: Vec<(Rgb, String)>,
}

/// One row of a before/after side-by-side diff (see `pair_side_by_side`) —
/// a context line has both `left` and `right`; a pure addition has only
/// `right`; a pure removal has only `left`. Two consecutive runs of
/// removed/added lines pair up row by row, the same visual convention
/// VS Code's own split diff view uses, rather than the unified `+`/`-`
/// stream this is built from (see `parse_unified_diff`).
pub(super) struct DiffRow {
    pub(super) left: Option<DiffCell>,
    pub(super) right: Option<DiffCell>,
}

/// Parses unified diff output (`git diff`'s own format) into line-numbered,
/// classified lines — text only, not yet paired into side-by-side rows
/// (see `pair_side_by_side`) or syntax-highlighted (see
/// `highlight_diff_rows`). Preamble lines (`diff --git`, `index`, `---`,
/// `+++`) are skipped; only content inside a `@@` hunk is kept.
pub(super) fn parse_unified_diff(
    output: &str,
) -> Vec<(DiffLineKind, Option<u32>, Option<u32>, String)> {
    let mut lines = Vec::new();
    let mut old_no = 0u32;
    let mut new_no = 0u32;
    let mut in_hunk = false;
    for line in output.lines() {
        if let Some(header) = line.strip_prefix("@@ ") {
            if let Some(hunk) = parse_hunk_header(header) {
                (old_no, new_no) = hunk;
                in_hunk = true;
            }
            continue;
        }
        if !in_hunk {
            continue;
        }
        if let Some(text) = line.strip_prefix('+') {
            lines.push((DiffLineKind::Added, None, Some(new_no), text.to_owned()));
            new_no += 1;
        } else if let Some(text) = line.strip_prefix('-') {
            lines.push((DiffLineKind::Removed, Some(old_no), None, text.to_owned()));
            old_no += 1;
        } else if let Some(text) = line.strip_prefix(' ') {
            lines.push((
                DiffLineKind::Context,
                Some(old_no),
                Some(new_no),
                text.to_owned(),
            ));
            old_no += 1;
            new_no += 1;
        }
        // Anything else inside a hunk (e.g. "\ No newline at end of file")
        // carries no line of its own — skipped.
    }
    lines
}

/// `header` is everything after the hunk marker's leading `"@@ "`, e.g.
/// `"-12,7 +12,7 @@ fn context_hint"` — returns the hunk's starting
/// `(old_line, new_line)`, or `None` if the header doesn't parse (left as
/// a pre-hunk preamble line rather than guessing).
pub(super) fn parse_hunk_header(header: &str) -> Option<(u32, u32)> {
    let (old_part, rest) = header.split_once(' ')?;
    let (new_part, _) = rest.split_once(" @@")?;
    let old_start = old_part
        .strip_prefix('-')?
        .split(',')
        .next()?
        .parse()
        .ok()?;
    let new_start = new_part
        .strip_prefix('+')?
        .split(',')
        .next()?
        .parse()
        .ok()?;
    Some((old_start, new_start))
}

/// A side-by-side row before syntax highlighting — see [`DiffRow`] for the
/// highlighted, render-ready shape this becomes via `highlight_diff_rows`.
pub(super) struct PairedRow {
    pub(super) left: Option<(u32, DiffLineKind, String)>,
    pub(super) right: Option<(u32, DiffLineKind, String)>,
}

/// Pairs a unified diff's sequential `+`/`-`/context lines into
/// side-by-side rows — a context line appears on both sides at once; a run
/// of removed lines pairs up, row by row, against the run of added lines
/// immediately following it (the same convention VS Code's own split diff
/// view uses), with the longer run's extra rows left blank on the shorter
/// side.
pub(super) fn pair_side_by_side(
    lines: Vec<(DiffLineKind, Option<u32>, Option<u32>, String)>,
) -> Vec<PairedRow> {
    let mut rows = Vec::new();
    let mut removed: Vec<(u32, String)> = Vec::new();
    let mut added: Vec<(u32, String)> = Vec::new();
    for (kind, old_no, new_no, text) in lines {
        match kind {
            DiffLineKind::Removed => removed.push((old_no.unwrap_or_default(), text)),
            DiffLineKind::Added => added.push((new_no.unwrap_or_default(), text)),
            DiffLineKind::Context => {
                flush_pending(&mut rows, &mut removed, &mut added);
                rows.push(PairedRow {
                    left: old_no.map(|no| (no, DiffLineKind::Context, text.clone())),
                    right: new_no.map(|no| (no, DiffLineKind::Context, text)),
                });
            }
        }
    }
    flush_pending(&mut rows, &mut removed, &mut added);
    rows
}

pub(super) fn flush_pending(
    rows: &mut Vec<PairedRow>,
    removed: &mut Vec<(u32, String)>,
    added: &mut Vec<(u32, String)>,
) {
    let paired = removed.len().max(added.len());
    for index in 0..paired {
        rows.push(PairedRow {
            left: removed
                .get(index)
                .map(|(no, text)| (*no, DiffLineKind::Removed, text.clone())),
            right: added
                .get(index)
                .map(|(no, text)| (*no, DiffLineKind::Added, text.clone())),
        });
    }
    removed.clear();
    added.clear();
}

/// Applies syntax highlighting (by `path`'s extension) to `rows` — the one
/// place `crate::highlight`'s output crosses into this module's own
/// `DiffRow`/`DiffCell` shape. The left and right columns are highlighted
/// as two independent line streams, each with its own highlighter —
/// syntect's carries state (an open block comment, for instance) across
/// calls, and "before" and "after" are two separate versions of the file,
/// not one continuous stream.
pub(super) fn highlight_diff_rows(
    rows: Vec<PairedRow>,
    path: &Path,
    theme_name: &str,
) -> Vec<DiffRow> {
    let mut left_highlighter = crate::shared::highlight::highlighter(path, theme_name);
    let mut right_highlighter = crate::shared::highlight::highlighter(path, theme_name);
    rows.into_iter()
        .map(|row| DiffRow {
            left: row.left.map(|(line_no, kind, text)| DiffCell {
                line_no,
                kind,
                spans: crate::shared::highlight::line(&mut left_highlighter, &text),
            }),
            right: row.right.map(|(line_no, kind, text)| DiffCell {
                line_no,
                kind,
                spans: crate::shared::highlight::line(&mut right_highlighter, &text),
            }),
        })
        .collect()
}

pub(super) fn content_line(cell: &DiffCell) -> ContentLine {
    let (gutter, tone) = match cell.kind {
        DiffLineKind::Context => (" ", LineTone::Neutral),
        DiffLineKind::Added => ("+", LineTone::Added),
        DiffLineKind::Removed => ("-", LineTone::Removed),
    };
    ContentLine {
        gutter: gutter.to_owned(),
        number: cell.line_no.to_string(),
        tone,
        spans: cell
            .spans
            .iter()
            .map(|(color, text)| Span {
                text: text.clone(),
                role: Role::Default,
                color: Some(*color),
                bold: false,
                italic: false,
            })
            .collect(),
    }
}

/// Expands paired rows back into their unified representation. Context is
/// shared by both sides and shown once; a replacement retains its natural
/// `-old` then `+new` ordering.
pub(super) fn unified_lines(rows: &[DiffRow]) -> Vec<&DiffCell> {
    let mut lines = Vec::new();
    for row in rows {
        match (&row.left, &row.right) {
            (Some(left), Some(right)) if left.kind == DiffLineKind::Context => {
                lines.push(right);
            }
            (Some(left), Some(right)) => {
                lines.push(left);
                lines.push(right);
            }
            (Some(left), None) => lines.push(left),
            (None, Some(right)) => lines.push(right),
            (None, None) => {}
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::highlight::FALLBACK_SYNTAX_THEME;

    /// Highlighting is the one thing in this crate whose correctness depends
    /// on syntect's regex backend, and the failure mode of changing that
    /// backend is quiet: a syntax whose patterns the engine cannot match
    /// still renders, with every span collapsed into the default colour.
    /// Pinning exact RGB values would break on any syntect or theme update
    /// and prove little, so these assert the two properties that actually
    /// distinguish working highlighting from silently broken highlighting —
    /// the text survives intact, and the line is genuinely coloured in more
    /// than one way.
    mod highlighting {
        use super::*;

        fn spans_for(file: &str, source: &str) -> Vec<(Rgb, String)> {
            let rows = highlight_diff_rows(
                vec![PairedRow {
                    left: None,
                    right: Some((1, DiffLineKind::Added, source.to_owned())),
                }],
                Path::new(file),
                FALLBACK_SYNTAX_THEME,
            );
            rows.into_iter()
                .next()
                .and_then(|row| row.right)
                .expect("the added side is what was asked for")
                .spans
        }

        /// One line per language, each mixing constructs a regex engine has
        /// to get right: a string, a comment, an interpolation, a raw or
        /// template literal.
        const SAMPLES: [(&str, &str); 8] = [
            ("a.rs", "fn main() { let s: &str = \"hi\"; /* c */ }"),
            ("a.py", "def f(a=1): return f\"{a!r}\"  # note"),
            ("a.js", "const re = /ab+c/gi; let o = `t${x}`; // c"),
            ("a.go", "func main() { s := `raw`; _ = s } // c"),
            ("a.md", "# Title with `code` and **bold**"),
            ("a.yaml", "key: \"quoted\"  # trailing comment"),
            ("a.json", "{\"a\": [1, 2.5e3, null, true]}"),
            ("a.sh", "for f in *.rs; do echo \"${f%.rs}\"; done"),
        ];

        #[test]
        fn every_sample_is_coloured_in_more_than_one_way() {
            for (file, source) in SAMPLES {
                let spans = spans_for(file, source);
                let distinct: std::collections::BTreeSet<(u8, u8, u8)> =
                    spans.iter().map(|(Rgb(r, g, b), _)| (*r, *g, *b)).collect();
                assert!(
                    distinct.len() > 1,
                    "`{file}` came back in a single colour, which is what a regex backend that \
                     cannot match this syntax looks like: {spans:?}"
                );
            }
        }

        #[test]
        fn highlighting_never_alters_the_text_it_colours() {
            for (file, source) in SAMPLES {
                let rebuilt: String = spans_for(file, source)
                    .into_iter()
                    .map(|(_, piece)| piece)
                    .collect();
                assert_eq!(rebuilt, source, "`{file}` lost or changed bytes");
            }
        }

        /// The property that made `load_defaults_newlines` necessary: the
        /// highlighter carries state between lines, so a block comment
        /// opened on one line still colours the next.
        #[test]
        fn a_block_comment_stays_open_across_lines() {
            let rows = highlight_diff_rows(
                vec![
                    PairedRow {
                        left: None,
                        right: Some((1, DiffLineKind::Added, "/* open".to_owned())),
                    },
                    PairedRow {
                        left: None,
                        right: Some((2, DiffLineKind::Added, "still inside */".to_owned())),
                    },
                ],
                Path::new("a.rs"),
                FALLBACK_SYNTAX_THEME,
            );
            let colour_of = |row: &DiffRow| row.right.as_ref().unwrap().spans[0].0;
            assert_eq!(
                colour_of(&rows[0]),
                colour_of(&rows[1]),
                "the second line left the comment the first one opened"
            );
        }

        /// An extension syntect does not bundle must fall back to plain
        /// text rather than panic — the same guarantee the theme lookup
        /// above it makes.
        #[test]
        fn an_unknown_extension_falls_back_to_plain_text() {
            let spans = spans_for("a.unknown-to-syntect", "anything at all");
            let rebuilt: String = spans.into_iter().map(|(_, piece)| piece).collect();
            assert_eq!(rebuilt, "anything at all");
        }

        /// The control that gives the assertion above its teeth. Plain text
        /// is what a collapsed highlighter produces — every span in one
        /// colour — so this pins the *other* side of the comparison: if
        /// plain text also came back multi-coloured, `every_sample_…` would
        /// be passing on a property that does not discriminate.
        #[test]
        fn plain_text_is_a_single_colour_so_the_comparison_means_something() {
            let spans = spans_for(
                "a.unknown-to-syntect",
                "fn main() { let s: &str = \"hi\"; /* c */ }",
            );
            let distinct: std::collections::BTreeSet<(u8, u8, u8)> =
                spans.iter().map(|(Rgb(r, g, b), _)| (*r, *g, *b)).collect();
            assert_eq!(
                distinct.len(),
                1,
                "unhighlighted text must be one colour, or the multi-colour \
                 assertion proves nothing: {spans:?}"
            );
        }
    }
    #[test]
    fn parses_a_single_hunk() {
        let diff = "diff --git a/f.rs b/f.rs\nindex abc..def 100644\n--- a/f.rs\n+++ b/f.rs\n@@ -1,3 +1,3 @@\n context\n-old\n+new\n context2\n";
        let lines = parse_unified_diff(diff);
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].0, DiffLineKind::Context);
        assert_eq!(lines[0].1, Some(1));
        assert_eq!(lines[0].2, Some(1));
        assert_eq!(lines[1].0, DiffLineKind::Removed);
        assert_eq!(lines[1].1, Some(2));
        assert_eq!(lines[1].2, None);
        assert_eq!(lines[1].3, "old");
        assert_eq!(lines[2].0, DiffLineKind::Added);
        assert_eq!(lines[2].2, Some(2));
        assert_eq!(lines[3].1, Some(3));
        assert_eq!(lines[3].2, Some(3));
    }
    #[test]
    fn tracks_line_numbers_across_multiple_hunks() {
        let diff = "diff --git a/f.rs b/f.rs\n--- a/f.rs\n+++ b/f.rs\n@@ -1,1 +1,1 @@\n-a\n+b\n@@ -10,1 +10,2 @@\n c\n+d\n";
        let lines = parse_unified_diff(diff);
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].1, Some(1));
        assert_eq!(lines[2].1, Some(10));
        assert_eq!(lines[2].2, Some(10));
        assert_eq!(lines[3].2, Some(11));
    }
    #[test]
    fn preamble_lines_outside_a_hunk_are_skipped() {
        let diff = "diff --git a/f.rs b/f.rs\nindex abc..def 100644\n--- a/f.rs\n+++ b/f.rs\n";
        assert!(parse_unified_diff(diff).is_empty());
    }
    #[test]
    fn a_context_line_pairs_with_itself_on_both_sides() {
        let diff = "diff --git a/f.rs b/f.rs\n--- a/f.rs\n+++ b/f.rs\n@@ -1,1 +1,1 @@\n context\n";
        let rows = pair_side_by_side(parse_unified_diff(diff));
        assert_eq!(rows.len(), 1);
        let left = rows[0].left.as_ref().unwrap();
        let right = rows[0].right.as_ref().unwrap();
        assert_eq!(left.0, 1);
        assert_eq!(left.2, "context");
        assert_eq!(right.0, 1);
        assert_eq!(right.2, "context");
    }
    #[test]
    fn more_removed_than_added_leaves_the_extra_rows_blank_on_the_right() {
        let diff = "diff --git a/f.rs b/f.rs\n--- a/f.rs\n+++ b/f.rs\n@@ -1,3 +1,1 @@\n-one\n-two\n-three\n+only\n";
        let rows = pair_side_by_side(parse_unified_diff(diff));
        assert_eq!(rows.len(), 3);
        assert!(rows[0].left.is_some() && rows[0].right.is_some());
        assert!(rows[1].left.is_some() && rows[1].right.is_none());
        assert!(rows[2].left.is_some() && rows[2].right.is_none());
    }
    #[test]
    fn more_added_than_removed_leaves_the_extra_rows_blank_on_the_left() {
        let diff = "diff --git a/f.rs b/f.rs\n--- a/f.rs\n+++ b/f.rs\n@@ -1,1 +1,3 @@\n-only\n+one\n+two\n+three\n";
        let rows = pair_side_by_side(parse_unified_diff(diff));
        assert_eq!(rows.len(), 3);
        assert!(rows[0].left.is_some() && rows[0].right.is_some());
        assert!(rows[1].left.is_none() && rows[1].right.is_some());
        assert!(rows[2].left.is_none() && rows[2].right.is_some());
    }
    #[test]
    fn equal_removed_and_added_pair_one_to_one() {
        let diff = "diff --git a/f.rs b/f.rs\n--- a/f.rs\n+++ b/f.rs\n@@ -1,2 +1,2 @@\n-old one\n-old two\n+new one\n+new two\n";
        let rows = pair_side_by_side(parse_unified_diff(diff));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].left.as_ref().unwrap().2, "old one");
        assert_eq!(rows[0].right.as_ref().unwrap().2, "new one");
        assert_eq!(rows[1].left.as_ref().unwrap().2, "old two");
        assert_eq!(rows[1].right.as_ref().unwrap().2, "new two");
    }
    #[test]
    fn unified_lines_shows_context_once_and_replacements_in_diff_order() {
        let rows = highlight_diff_rows(
            pair_side_by_side(parse_unified_diff(
                "@@ -1,2 +1,2 @@\n context\n-old\n+new\n",
            )),
            Path::new("example.rs"),
            FALLBACK_SYNTAX_THEME,
        );
        let lines = unified_lines(&rows);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].kind, DiffLineKind::Context);
        assert_eq!(lines[1].kind, DiffLineKind::Removed);
        assert_eq!(lines[2].kind, DiffLineKind::Added);
    }
}
