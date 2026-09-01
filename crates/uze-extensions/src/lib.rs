//! Built-in TUI extensions for the uze workspace client — presentation
//! add-ons that live below `src/ui/` in the dependency graph (`src/` uses
//! this crate, never the reverse), the same relationship `uze-integrations`
//! has to the harness registry: one crate, one module per extension, one
//! registry entry point ([`registry::ExtensionRegistry`]) naming the set.
//!
//! Today there's exactly one extension ([`git_diff`], the Git changes
//! overlay); the crate is structured so a second one is another module with
//! its own `CATALOG` entry (see `git_diff::CATALOG`), one registration in
//! `ExtensionRegistry::builtin`, and another [`ExtensionHit`] variant, not
//! a new crate. [`palette`] and the
//! small text helpers below are duplicated from `src/ui.rs` rather than
//! imported from it — `src/` depends on this crate, so this crate can't
//! depend back on `src/` for its own design system without a cycle. Keep
//! the two in sync by eye until enough extensions exist to justify hoisting
//! the palette into its own crate both sides depend on.

pub mod git_diff;
pub mod palette;
pub mod registry;

use ratatui::{
    style::{Modifier, Style},
    text::Span,
};

/// A hit an extension's own render pass produced, wrapped so the host's
/// hit-testing vector never has to widen for it — same principle as one
/// `WorkspaceHit::Extension` variant on the host side instead of one
/// variant per extension. Flat for now, since there's only one extension;
/// once a second exists, namespace this into
/// `ExtensionHit::GitChanges(git_diff::Hit)` and so on rather than
/// letting two extensions' hit kinds collide in one flat enum.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionHit {
    SelectFile(usize),
    ResizeTree,
    Close,
}

/// `~/relative/path` when `root` is under the user's home directory, else
/// the path as-is — mirrors what a shell prompt usually shows. Duplicated
/// from `src/ui.rs`'s private helper of the same name/behavior; see this
/// crate's own doc comment for why it's a copy, not an import.
pub fn display_project_path(root: &std::path::Path) -> String {
    if let Some(home) = std::env::var_os("HOME")
        && let Ok(relative) = root.strip_prefix(&home)
    {
        return format!("~/{}", relative.display());
    }
    root.display().to_string()
}

/// Styled spans for one footer hint: `key action · key action …` chunks are
/// split so the command/key part carries the accent (and bold) and the
/// description stays muted — the shortcut bar reads as "keys + what they
/// do" instead of one uniform wall of gray text. Chunks without a verb
/// (e.g. `y/n`) render as a command alone. Duplicated from `src/ui.rs`; see
/// this crate's own doc comment for why.
pub fn hint_spans(hint: &str) -> Vec<Span<'static>> {
    let command = Style::default()
        .fg(palette::ACCENT)
        .add_modifier(Modifier::BOLD);
    let muted = Style::default().fg(palette::MUTED);
    let mut spans = Vec::new();
    for (i, chunk) in hint.split(" · ").enumerate() {
        if i > 0 {
            spans.push(Span::raw(" · "));
        }
        match chunk.split_once(' ') {
            Some((key, action)) => {
                spans.push(Span::styled(key.to_owned(), command));
                spans.push(Span::styled(format!(" {action}"), muted));
            }
            None => spans.push(Span::styled(chunk.to_owned(), command)),
        }
    }
    spans
}
