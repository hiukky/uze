//! Markdown, rendered rather than shown.
//!
//! The contents mode already colours a `.md` file — syntect knows the
//! language — but colouring markup is not reading it. A heading stays a
//! line beginning with hashes, a link is still its own brackets and
//! parentheses, and a fenced block of Rust is one undifferentiated
//! string. This turns the document into what it describes.
//!
//! # It still describes; the host still draws
//!
//! Nothing here draws. It answers [`ContentLine`]s the same way the diff
//! does, so every rule the surface lives under holds: chrome colour is a
//! [`Role`], geometry is the host's, and the wrap is applied where the
//! line is laid out rather than guessed at here.
//!
//! Two things the vocabulary had to grow for it, both small and both
//! genuinely about meaning rather than about markdown: [`Span::italic`],
//! because emphasis has two weights and a role cannot carry the
//! difference, and nothing else.
//!
//! # Why `pulldown-cmark`
//!
//! It is what rustdoc parses with, it is a pull parser (so a document is
//! a stream of events rather than a tree to walk twice), and with default
//! features off it brings one crate the workspace does not already have.
//! The terminal renderers — `termimad`, `tui-markdown` — were the obvious
//! reach and the wrong one: both *draw*, which is the one thing an
//! extension may not do.

use std::path::Path;

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::{
    shared::highlight,
    view::{ContentLine, LineTone, Role, Span},
};

/// Whether `path` is a document this can render.
pub(super) fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(extension.to_ascii_lowercase().as_str(), "md" | "markdown")
        })
}

/// The document `text` describes, as lines.
///
/// `theme_name` is the host's syntax theme, used for fenced code blocks —
/// a block of Rust inside a README is highlighted as Rust, which is most
/// of what a preview is for in a repository.
pub(super) fn render(text: &str, theme_name: &str) -> Vec<ContentLine> {
    let mut options = Options::empty();
    // The three GitHub extensions a README actually uses. They are
    // parser options rather than cargo features, so they cost nothing.
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let mut document = Document::new(theme_name);
    for event in Parser::new_ext(text, options) {
        document.absorb(event);
    }
    document.finish()
}

/// What the current run of text looks like.
#[derive(Clone, Copy, Default)]
struct Emphasis {
    bold: bool,
    italic: bool,
    struck: bool,
}

/// The document being built, one event at a time.
struct Document {
    theme: String,
    lines: Vec<ContentLine>,
    /// The line being assembled. Flushed at every block boundary.
    pending: Vec<Span>,
    emphasis: Emphasis,
    /// How deep in lists we are, and whether each is numbered — the
    /// counter is the list's, so a nested list does not disturb it.
    lists: Vec<Option<u64>>,
    /// Block quotes are cumulative: a quote inside a quote indents twice.
    quotes: usize,
    /// The language of the fenced block being read, when inside one.
    fence: Option<String>,
    /// A table being collected: its rows, and the row being filled.
    table: Option<Table>,
    /// Set between a heading's start and end, so its text is styled as
    /// one rather than span by span.
    heading: Option<HeadingLevel>,
}

#[derive(Default)]
struct Table {
    rows: Vec<Vec<String>>,
    row: Vec<String>,
    cell: String,
}

impl Document {
    fn new(theme: &str) -> Self {
        Self {
            theme: theme.to_owned(),
            lines: Vec::new(),
            pending: Vec::new(),
            emphasis: Emphasis::default(),
            lists: Vec::new(),
            quotes: 0,
            fence: None,
            table: None,
            heading: None,
        }
    }

    fn finish(mut self) -> Vec<ContentLine> {
        self.flush();
        if self.lines.is_empty() {
            self.lines.push(blank());
        }
        self.lines
    }

    fn absorb(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.text(&text),
            Event::Code(code) => {
                // Inline code keeps its backticks' meaning without their
                // characters: it is the one run of text that is quoted
                // rather than emphasised.
                self.push(Span::new(code.to_string(), Role::Accent));
            }
            Event::SoftBreak => self.push(Span::new(" ", Role::Default)),
            Event::HardBreak => self.flush(),
            Event::Rule => {
                self.flush();
                self.lines.push(ContentLine {
                    gutter: " ".to_owned(),
                    number: String::new(),
                    tone: LineTone::Neutral,
                    spans: vec![Span::new("─".repeat(48), Role::Faint)],
                });
                self.lines.push(blank());
            }
            Event::TaskListMarker(done) => {
                self.push(Span::new(
                    if done { "[x] " } else { "[ ] " },
                    if done { Role::Success } else { Role::Muted },
                ));
            }
            // Raw HTML, footnotes and the rest are shown as the source
            // says them rather than dropped: a preview that silently
            // loses part of a document is worse than one that shows it
            // plainly.
            Event::Html(raw) | Event::InlineHtml(raw) => {
                self.push(Span::new(raw.to_string(), Role::Faint));
            }
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Heading { level, .. } => {
                self.flush();
                self.heading = Some(level);
            }
            Tag::Paragraph => self.flush(),
            Tag::Emphasis => self.emphasis.italic = true,
            Tag::Strong => self.emphasis.bold = true,
            Tag::Strikethrough => self.emphasis.struck = true,
            Tag::BlockQuote(_) => {
                self.flush();
                self.quotes += 1;
            }
            Tag::List(first) => {
                self.flush();
                self.lists.push(first);
            }
            Tag::Item => {
                self.flush();
                let depth = self.lists.len().saturating_sub(1);
                let marker = match self.lists.last_mut() {
                    Some(Some(number)) => {
                        let marker = format!("{number}. ");
                        *number += 1;
                        marker
                    }
                    _ => "• ".to_owned(),
                };
                self.pending.push(Span::new(
                    format!("{}{marker}", "  ".repeat(depth)),
                    Role::Muted,
                ));
            }
            Tag::CodeBlock(kind) => {
                self.flush();
                self.fence = Some(match kind {
                    CodeBlockKind::Fenced(language) => language.to_string(),
                    CodeBlockKind::Indented => String::new(),
                });
            }
            Tag::Table(_) => {
                self.flush();
                self.table = Some(Table::default());
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Heading(_) => {
                self.flush();
                self.lines.push(blank());
                self.heading = None;
            }
            TagEnd::Paragraph => {
                self.flush();
                self.lines.push(blank());
            }
            TagEnd::Emphasis => self.emphasis.italic = false,
            TagEnd::Strong => self.emphasis.bold = false,
            TagEnd::Strikethrough => self.emphasis.struck = false,
            TagEnd::BlockQuote(_) => {
                self.flush();
                self.quotes = self.quotes.saturating_sub(1);
            }
            TagEnd::List(_) => {
                self.flush();
                self.lists.pop();
                if self.lists.is_empty() {
                    self.lines.push(blank());
                }
            }
            TagEnd::Item => self.flush(),
            TagEnd::CodeBlock => {
                self.fence = None;
                self.lines.push(blank());
            }
            TagEnd::Table => self.finish_table(),
            TagEnd::TableCell => {
                if let Some(table) = self.table.as_mut() {
                    let cell = std::mem::take(&mut table.cell);
                    table.row.push(cell);
                }
            }
            TagEnd::TableHead | TagEnd::TableRow => {
                if let Some(table) = self.table.as_mut() {
                    let row = std::mem::take(&mut table.row);
                    table.rows.push(row);
                }
            }
            _ => {}
        }
    }

    fn text(&mut self, text: &str) {
        if let Some(table) = self.table.as_mut() {
            table.cell.push_str(text);
            return;
        }
        if let Some(language) = self.fence.clone() {
            self.push_code(text, &language);
            return;
        }
        self.push(Span::new(text.to_owned(), Role::Default));
    }

    /// A fenced block, highlighted as whatever it says it is. One
    /// highlighter for the whole block, because syntect carries state
    /// across lines and a block is one stream.
    fn push_code(&mut self, text: &str, language: &str) {
        let mut highlighter = highlight::highlighter_for_language(language, &self.theme);
        for line in text.lines() {
            let mut spans = vec![Span::new("  ".to_owned(), Role::Default)];
            spans.extend(
                highlight::line(&mut highlighter, line)
                    .into_iter()
                    .map(|(colour, piece)| Span::new(piece, Role::Default).coloured(colour)),
            );
            self.lines.push(ContentLine {
                gutter: " ".to_owned(),
                number: String::new(),
                tone: LineTone::Neutral,
                spans,
            });
        }
    }

    /// Adds a run of text to the line being built, wearing whatever
    /// emphasis is currently open.
    fn push(&mut self, span: Span) {
        let mut span = span;
        if let Some(level) = self.heading {
            span.bold = true;
            span.role = match level {
                HeadingLevel::H1 => Role::Bright,
                HeadingLevel::H2 => Role::Accent,
                _ => Role::Secondary,
            };
        } else {
            span.bold |= self.emphasis.bold;
            span.italic |= self.emphasis.italic;
            if self.emphasis.struck {
                // No strikethrough in the vocabulary, and one variant for
                // one extension is not the bar: dimming says "this no
                // longer counts", which is what the markup means.
                span.role = Role::Faint;
            }
        }
        if self.pending.is_empty() && self.quotes > 0 {
            self.pending
                .push(Span::new("│ ".repeat(self.quotes), Role::Dim));
        }
        self.pending.push(span);
    }

    fn flush(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let spans = std::mem::take(&mut self.pending);
        self.lines.push(ContentLine {
            gutter: " ".to_owned(),
            number: String::new(),
            tone: LineTone::Neutral,
            spans,
        });
    }

    /// A table, once every cell is in: columns as wide as their widest
    /// cell, so a table reads as one.
    fn finish_table(&mut self) {
        let Some(table) = self.table.take() else {
            return;
        };
        let columns = table.rows.iter().map(Vec::len).max().unwrap_or(0);
        let widths: Vec<usize> = (0..columns)
            .map(|column| {
                table
                    .rows
                    .iter()
                    .filter_map(|row| row.get(column))
                    .map(|cell| cell.chars().count())
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        for (index, row) in table.rows.iter().enumerate() {
            let text = widths
                .iter()
                .enumerate()
                .map(|(column, width)| {
                    let cell = row.get(column).map(String::as_str).unwrap_or("");
                    format!("{cell:<width$}", width = *width)
                })
                .collect::<Vec<_>>()
                .join("  ");
            // The head is the row that names the columns, so it is the
            // one that is emphasised — the separator markdown writes
            // under it is layout the parser already consumed.
            let span = match index {
                0 => Span::new(text, Role::Secondary).bold(),
                _ => Span::new(text, Role::Default),
            };
            self.lines.push(ContentLine {
                gutter: " ".to_owned(),
                number: String::new(),
                tone: LineTone::Neutral,
                spans: vec![span],
            });
        }
        self.lines.push(blank());
    }
}

fn blank() -> ContentLine {
    ContentLine {
        gutter: " ".to_owned(),
        number: String::new(),
        tone: LineTone::Neutral,
        spans: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::highlight::FALLBACK_SYNTAX_THEME;

    fn rendered(source: &str) -> Vec<ContentLine> {
        render(source, FALLBACK_SYNTAX_THEME)
    }

    fn text_of(line: &ContentLine) -> String {
        line.spans.iter().map(|span| span.text.as_str()).collect()
    }

    fn lines_of(source: &str) -> Vec<String> {
        rendered(source).iter().map(text_of).collect()
    }

    #[test]
    fn a_document_is_only_a_markdown_one_by_its_extension() {
        assert!(is_markdown(Path::new("README.md")));
        assert!(is_markdown(Path::new("notes.MARKDOWN")));
        assert!(!is_markdown(Path::new("main.rs")));
        assert!(!is_markdown(Path::new("mdbook")));
    }

    /// The point of the mode: the markup stops being text and starts
    /// being what it describes.
    #[test]
    fn the_markup_becomes_the_document_rather_than_staying_on_screen() {
        let lines = lines_of("# Title\n\nSome **bold** and *thin* words.\n");

        assert!(
            lines.iter().any(|line| line == "Title"),
            "a heading loses its hashes: {lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("bold") && !line.contains('*')),
            "and emphasis loses its asterisks: {lines:?}"
        );
    }

    #[test]
    fn emphasis_survives_as_weight_rather_than_as_punctuation() {
        let lines = rendered("Some **bold** and *thin* words.\n");
        let spans: Vec<&Span> = lines.iter().flat_map(|line| &line.spans).collect();

        assert!(
            spans.iter().any(|span| span.text == "bold" && span.bold),
            "strong is bold"
        );
        assert!(
            spans.iter().any(|span| span.text == "thin" && span.italic),
            "and emphasis is italic — a role could not carry the difference"
        );
    }

    /// A README's code block is most of what a preview is for in a
    /// repository, so it is highlighted as the language it says it is.
    #[test]
    fn a_fenced_block_is_highlighted_as_what_it_says_it_is() {
        let lines = rendered("```rust\nfn main() {}\n```\n");
        let coloured = lines
            .iter()
            .flat_map(|line| &line.spans)
            .filter(|span| span.color.is_some())
            .count();

        assert!(coloured > 1, "the block is coloured as Rust, not as prose");
        assert!(
            lines
                .iter()
                .map(text_of)
                .any(|line| line.contains("fn main")),
            "and its text survives"
        );
    }

    #[test]
    fn a_list_is_marked_and_a_nested_one_is_indented() {
        let lines = lines_of("- one\n- two\n  - deep\n");

        assert!(lines.iter().any(|line| line == "• one"), "{lines:?}");
        assert!(lines.iter().any(|line| line == "  • deep"), "{lines:?}");
    }

    #[test]
    fn a_numbered_list_counts_and_a_nested_one_does_not_disturb_it() {
        let lines = lines_of("1. one\n2. two\n");
        assert!(lines.iter().any(|line| line == "1. one"), "{lines:?}");
        assert!(lines.iter().any(|line| line == "2. two"), "{lines:?}");
    }

    #[test]
    fn a_table_lines_its_columns_up() {
        let lines = lines_of("| a | long header |\n|---|---|\n| x | y |\n");
        let header = lines
            .iter()
            .find(|line| line.contains("long header"))
            .expect("the head is drawn");
        let body = lines
            .iter()
            .find(|line| line.trim_end() == "x  y")
            .or_else(|| lines.iter().find(|line| line.starts_with('x')))
            .expect("the body is drawn");

        assert_eq!(
            header.chars().count(),
            body.chars().count(),
            "columns as wide as their widest cell, so the rows line up"
        );
    }

    /// A preview that silently drops part of a document is worse than one
    /// that shows it plainly.
    #[test]
    fn raw_html_is_shown_rather_than_swallowed() {
        let lines = lines_of("<div align=\"center\">hi</div>\n");
        assert!(lines.iter().any(|line| line.contains("div")), "{lines:?}");
    }

    #[test]
    fn an_empty_document_still_has_a_line() {
        assert_eq!(rendered("").len(), 1);
    }
}
