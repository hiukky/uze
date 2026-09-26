//! A unified diff, parsed and coloured in the order Git wrote it.
//!
//! Two steps that stay separate on purpose — parse Git's own format, then
//! colour the result — because only the second is expensive, and only the
//! first is worth testing on its own.

use std::path::Path;

use crate::view::{ContentLine, LineTone, Rgb, Role, Span};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DiffLineKind {
    Context,
    Added,
    Removed,
}

/// One line of a diff before it is coloured.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct DiffLine {
    pub(super) kind: DiffLineKind,
    /// The line's number on the side it belongs to: the old file's for a
    /// removal, the new file's for an addition and for context.
    pub(super) line_no: u32,
    pub(super) text: String,
}

/// One line of the diff, ready to draw.
pub(super) struct DiffCell {
    pub(super) line_no: u32,
    pub(super) kind: DiffLineKind,
    /// Pre-highlighted (see [`highlight`]) — no syntect types beyond this
    /// module's boundary, and the colour travels as data because it comes
    /// from the syntax theme rather than from the host's palette (see
    /// [`crate::view::Rgb`]).
    pub(super) spans: Vec<(Rgb, String)>,
}

/// Parses and colours `git diff` output for the file at `path`.
pub(super) fn read(output: &str, path: &Path, theme_name: &str) -> Vec<DiffCell> {
    highlight(parse_unified_diff(output), path, theme_name)
}

/// Parses unified diff output (`git diff`'s own format) into line-numbered,
/// classified lines in the order Git wrote them. Preamble lines
/// (`diff --git`, `index`, `---`, `+++`) are skipped; only content inside a
/// `@@` hunk is kept.
pub(super) fn parse_unified_diff(output: &str) -> Vec<DiffLine> {
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
        let (kind, line_no, text) = if let Some(text) = line.strip_prefix('+') {
            (DiffLineKind::Added, new_no, text)
        } else if let Some(text) = line.strip_prefix('-') {
            (DiffLineKind::Removed, old_no, text)
        } else if let Some(text) = line.strip_prefix(' ') {
            (DiffLineKind::Context, new_no, text)
        } else {
            // Anything else inside a hunk (e.g. "\ No newline at end of
            // file") carries no line of its own.
            continue;
        };
        if kind != DiffLineKind::Added {
            old_no += 1;
        }
        if kind != DiffLineKind::Removed {
            new_no += 1;
        }
        lines.push(DiffLine {
            kind,
            line_no,
            text: text.to_owned(),
        });
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

/// Applies syntax highlighting (by `path`'s extension) to `lines`.
///
/// The old and the new file are highlighted as two line streams, each with
/// its own highlighter: syntect's carries state (an open block comment, for
/// instance) across calls, and "before" and "after" are two versions of the
/// file, not one continuous stream. A removal feeds only the old one, an
/// addition only the new, and context both — so each stream sees exactly
/// the file it stands for.
/// How many lines of a diff are coloured. Colouring is per line and
/// sequential — the highlighter's state is why a comment opened above
/// stays one below — so a generated file or a lockfile rewritten whole
/// was tens of seconds of it before its first line could be drawn. Past
/// this many lines, forty screens or so, the rest is drawn as the text it
/// is, in the theme's own ink.
const COLOURED_DIFF_LINES: usize = 2000;

pub(super) fn highlight(lines: Vec<DiffLine>, path: &Path, theme_name: &str) -> Vec<DiffCell> {
    let mut old = super::highlight::highlighter(path, theme_name);
    let mut new = super::highlight::highlighter(path, theme_name);
    lines
        .into_iter()
        .enumerate()
        .map(
            |(
                at,
                DiffLine {
                    kind,
                    line_no,
                    text,
                },
            )| {
                if at >= COLOURED_DIFF_LINES {
                    return DiffCell {
                        line_no,
                        kind,
                        spans: super::highlight::plain(theme_name, &text),
                    };
                }
                let spans = match kind {
                    DiffLineKind::Removed => super::highlight::line(&mut old, &text),
                    DiffLineKind::Added => super::highlight::line(&mut new, &text),
                    DiffLineKind::Context => {
                        super::highlight::line(&mut old, &text);
                        super::highlight::line(&mut new, &text)
                    }
                };
                DiffCell {
                    line_no,
                    kind,
                    spans,
                }
            },
        )
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
                ground: None,
                bold: false,
                italic: false,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code::highlight::FALLBACK_SYNTAX_THEME;

    /// A diff longer than colouring pays for keeps every line and every
    /// byte; only the lines past the bound are drawn in one ink.
    #[test]
    fn a_diff_past_the_colouring_bound_keeps_every_line_uncoloured() {
        let total = COLOURED_DIFF_LINES + 50;
        let mut output = format!("@@ -0,0 +1,{total} @@\n");
        for line in 0..total {
            output.push_str(&format!("+let value_{line} = {line}; // line\n"));
        }
        let cells = read(&output, Path::new("big.rs"), FALLBACK_SYNTAX_THEME);

        assert_eq!(cells.len(), total);
        assert!(cells[0].spans.len() > 1, "the head is coloured");
        let tail = &cells[total - 1];
        assert_eq!(tail.spans.len(), 1, "the tail is one span");
        assert_eq!(
            tail.spans[0].1,
            format!("let value_{} = {}; // line", total - 1, total - 1)
        );
        assert_eq!(tail.line_no, total as u32);
    }

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

        fn added(line_no: u32, text: &str) -> DiffLine {
            DiffLine {
                kind: DiffLineKind::Added,
                line_no,
                text: text.to_owned(),
            }
        }

        fn spans_for(file: &str, source: &str) -> Vec<(Rgb, String)> {
            highlight(
                vec![added(1, source)],
                Path::new(file),
                FALLBACK_SYNTAX_THEME,
            )
            .into_iter()
            .next()
            .expect("the line that was asked for")
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
            let cells = highlight(
                vec![added(1, "/* open"), added(2, "still inside */")],
                Path::new("a.rs"),
                FALLBACK_SYNTAX_THEME,
            );
            assert_eq!(
                cells[0].spans[0].0, cells[1].spans[0].0,
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
    fn line(kind: DiffLineKind, line_no: u32, text: &str) -> DiffLine {
        DiffLine {
            kind,
            line_no,
            text: text.to_owned(),
        }
    }

    #[test]
    fn parses_a_single_hunk() {
        let diff = "diff --git a/f.rs b/f.rs\nindex abc..def 100644\n--- a/f.rs\n+++ b/f.rs\n@@ -1,3 +1,3 @@\n context\n-old\n+new\n context2\n";
        assert_eq!(
            parse_unified_diff(diff),
            [
                line(DiffLineKind::Context, 1, "context"),
                line(DiffLineKind::Removed, 2, "old"),
                line(DiffLineKind::Added, 2, "new"),
                line(DiffLineKind::Context, 3, "context2"),
            ]
        );
    }

    #[test]
    fn tracks_line_numbers_across_multiple_hunks() {
        let diff = "diff --git a/f.rs b/f.rs\n--- a/f.rs\n+++ b/f.rs\n@@ -1,1 +1,1 @@\n-a\n+b\n@@ -10,1 +10,2 @@\n c\n+d\n";
        let numbers: Vec<u32> = parse_unified_diff(diff)
            .iter()
            .map(|line| line.line_no)
            .collect();
        assert_eq!(numbers, [1, 1, 10, 11]);
    }

    #[test]
    fn preamble_lines_outside_a_hunk_are_skipped() {
        let diff = "diff --git a/f.rs b/f.rs\nindex abc..def 100644\n--- a/f.rs\n+++ b/f.rs\n";
        assert!(parse_unified_diff(diff).is_empty());
    }

    /// A replacement of several lines reads the way Git wrote it: every
    /// removal, then every addition, never interleaved.
    #[test]
    fn a_multi_line_replacement_keeps_gits_order() {
        let diff = "@@ -1,4 +1,4 @@\n one\n-a\n-b\n+c\n+d\n four\n";
        assert_eq!(
            parse_unified_diff(diff),
            [
                line(DiffLineKind::Context, 1, "one"),
                line(DiffLineKind::Removed, 2, "a"),
                line(DiffLineKind::Removed, 3, "b"),
                line(DiffLineKind::Added, 2, "c"),
                line(DiffLineKind::Added, 3, "d"),
                line(DiffLineKind::Context, 4, "four"),
            ]
        );
    }
}
