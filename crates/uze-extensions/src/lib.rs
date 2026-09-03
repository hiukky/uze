//! Built-in TUI extensions for the uze workspace client — add-ons that
//! live below `src/ui/` in the dependency graph (`src/` uses this crate,
//! never the reverse), the same relationship `uze-integrations` has to the
//! harness registry: one crate, one module per extension, one registry
//! entry point ([`registry::ExtensionRegistry`]) naming the set.
//!
//! Today there's exactly one extension ([`git`]: the changes overlay and
//! the sidebar's commit timeline); the crate is structured so a second one
//! is another module with its own `CATALOG` entry (see `git::CATALOG`),
//! one registration in `ExtensionRegistry::builtin`, and one more
//! [`ExtensionHit`] variant, not a new crate.
//!
//! # An extension holds no machine access of its own
//!
//! Everything outside the extension's own process memory — running Git,
//! reading a file, knowing where `$HOME` is — arrives through [`Host`].
//! The extension names what it needs; the host decides whether to oblige.
//! Today it always does, in this process, so nothing observable changes.
//!
//! What changes is that an extension is now a pure function of what it is
//! handed. That is what makes the trust question answerable later: code
//! authored elsewhere cannot reach anything it was not given, and the day
//! an extension runs somewhere else, nothing inside it has to change.
//!
//! # An extension describes; the host draws
//!
//! An extension answers with a [`view::View`] — what it has, never how it
//! looks. The host owns rendering, geometry, and the palette, which is why
//! the copy of `src/ui.rs`'s colour table that used to live here is gone
//! along with the two-sided "keep these in sync by eye" it required. See
//! [`view`] for the rest of the reasoning.

pub mod git;
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
    Git(view::ViewHit),
}

/// What an extension may ask the host to do on its behalf.
///
/// Deliberately tiny, and read-only throughout: this is what the one
/// extension actually needs, and a wider surface would be speculation
/// about a second. Nothing here can write.
pub trait Host {
    /// Runs a read-only Git command in `root`, returning its stdout.
    ///
    /// Exit `1` counts as an answer rather than a failure — `git diff`
    /// uses it for "there are differences", which is the ordinary case for
    /// a view whose whole job is showing them.
    fn git(&self, root: &std::path::Path, args: &[&str]) -> Result<String, String>;

    /// A file's contents, or `None` when it cannot be read. Unreadable is
    /// a state a view renders, never an error it propagates.
    fn read_file(&self, path: &std::path::Path) -> Option<String>;

    /// The path as a person would recognise it — `~/relative/path` when it
    /// sits under their home directory.
    fn display_path(&self, path: &std::path::Path) -> String;
}
