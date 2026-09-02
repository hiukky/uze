//! Built-in TUI extensions for the uze workspace client — add-ons that
//! live below `src/ui/` in the dependency graph (`src/` uses this crate,
//! never the reverse), the same relationship `uze-integrations` has to the
//! harness registry: one crate, one module per extension, one registry
//! entry point ([`registry::ExtensionRegistry`]) naming the set.
//!
//! Today there's exactly one extension ([`git_diff`], the Git changes
//! overlay); the crate is structured so a second one is another module with
//! its own `CATALOG` entry (see `git_diff::CATALOG`), one registration in
//! `ExtensionRegistry::builtin`, and one more [`ExtensionHit`] variant, not
//! a new crate.
//!
//! # An extension describes; the host draws
//!
//! An extension answers with a [`view::View`] — what it has, never how it
//! looks. The host owns rendering, geometry, and the palette, which is why
//! the copy of `src/ui.rs`'s colour table that used to live here is gone
//! along with the two-sided "keep these in sync by eye" it required. See
//! [`view`] for the rest of the reasoning.

pub mod git_diff;
pub mod registry;
pub mod view;

/// Something a viewer did inside an extension's own surface, addressed to
/// the extension that owns it.
///
/// The payload is [`view::ViewHit`] for every extension, because the host
/// produces it from what it drew and none of that is extension-specific.
/// The variant is what routes: with two extensions open in one session,
/// "row 3 was clicked" is meaningless without saying whose row 3.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionHit {
    GitChanges(view::ViewHit),
}

/// `~/relative/path` when `root` is under the user's home directory, else
/// the path as-is — mirrors what a shell prompt usually shows.
pub fn display_project_path(root: &std::path::Path) -> String {
    if let Some(home) = std::env::var_os("HOME")
        && let Ok(relative) = root.strip_prefix(&home)
    {
        return format!("~/{}", relative.display());
    }
    root.display().to_string()
}
