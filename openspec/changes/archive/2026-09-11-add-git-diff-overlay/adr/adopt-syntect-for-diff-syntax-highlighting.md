# Adopt syntect for diff syntax highlighting

Status: Accepted

## Context

The workspace TUI's new git changes overlay (ADR 038's terminal runtime
gains a client-side, read-only git diff view — see
`openspec/changes/add-git-diff-overlay`) needs to render diff content with
per-language syntax coloring to be legible at the density a terminal diff
view demands, the same bar VS Code's own diff editor sets. Rendering plain
text with only add/remove line coloring is not enough. The workspace TUI
client had no code-highlighting dependency before this.

## Decision

We will use `syntect` (bundled default `SyntaxSet`/`ThemeSet` via
`load_defaults_newlines`/`load_defaults`, no external grammar or theme
files to manage) to produce per-line, per-token foreground-color spans for
diff content, converted to `ratatui::style::Style`/`Color::Rgb` inside the
new `src/ui/git_diff.rs` module.

Alternatives considered:
- **`tree-sitter` + per-language grammar crates.** Rejected for this pass:
  each language needing highlighting is a separate crate dependency to add
  and keep current, versus `syntect`'s single bundled grammar set covering
  a broad range out of the box. More accurate (incremental, real parse
  trees) but that precision buys nothing for coloring diff lines, which
  only need foreground-color spans, not structural analysis.
- **No syntax highlighting (diff-level coloring only: added/removed/
  context, no per-token color).** Rejected: falls visibly short of the
  VS Code diff view this overlay is explicitly replacing the need to
  alt-tab to.

## Consequences

- The `uze` binary crate gains a new external dependency (`syntect`) and
  its bundled default grammar/theme data, compiled into the binary. This
  is scoped to `src/ui/git_diff.rs`; no other module depends on it.
- Highlighting quality for a given file is bounded by `syntect`'s bundled
  Sublime Text/TextMate-style grammar set for that language, not
  `tree-sitter`-grade structural accuracy — acceptable for a read-only
  diff preview, revisit only if a specific language's highlighting proves
  materially wrong in practice.
- If a future need arises for editor-grade highlighting elsewhere in the
  product (not just this diff preview), that would be a separate decision
  to make then, not one this ADR forecloses — `syntect` here is scoped to
  one view.
