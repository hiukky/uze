//! Built-in TUI extensions for the uze workspace client — add-ons that
//! live below `src/ui/` in the dependency graph (`src/` uses this crate,
//! never the reverse), the same relationship `uze-integrations` has to the
//! harness registry: one crate, one module per extension, one registry
//! entry point ([`registry::ExtensionRegistry`]) naming the set.
//!
//! One extension ships today — [`code`], which draws three surfaces of
//! the active checkout: what changed in it, what it contains, and its
//! commit timeline. A second is another module with its own `CATALOG`
//! entry, one registration in `ExtensionRegistry::builtin`, and one
//! [`ExtensionHit`] variant per surface it draws, not a new crate.
//!
//! # Where a file goes
//!
//! One directory per extension, named after it, with the extension's own
//! surface — its state, its keys, its registry entry — in the file beside
//! it. What is genuinely shared by more than one lives under [`shared`],
//! and nothing is put there in anticipation of a second reader (see that
//! module for the rule). [`view`] is neither: it is the contract between
//! an extension and whatever draws it, which is why it sits at the root
//! alongside [`Host`], the contract in the other direction.
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

pub mod code;
pub mod registry;
pub mod shared;
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
    /// The code extension's full-frame surface — the changes, the file
    /// tree and a file's contents are one surface in three modes, so
    /// they are one variant.
    Code(view::ViewHit),
    /// The git extension's sidebar section — its commit timeline.
    ///
    /// A second variant for a second *surface* of the one extension, not
    /// a second extension: `SelectItem(3)` means a different thing in a
    /// file list than in a list of commits, and the host is the only side
    /// that knows which of the two it drew.
    CodeTimeline(view::ViewHit),
}

/// One entry of a directory listing, as [`Host::list_dir`] answers it.
///
/// A name and a kind, never a handle: the extension addresses a child by
/// joining the name onto the directory it asked about, so nothing here
/// carries the ability to reach anywhere the host did not already say.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirEntry {
    /// Whether it holds other entries.
    pub directory: bool,
    pub name: String,
}

/// The listing order the contract promises, carried by the type rather
/// than by whoever sorts it: directories first, then each half by name.
///
/// Derived, it would have ordered files first — `false` sorts before
/// `true` — which is the opposite of every file tree a person has used,
/// and a mistake nothing but a test would have caught.
impl Ord for DirEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other
            .directory
            .cmp(&self.directory)
            .then_with(|| self.name.cmp(&other.name))
    }
}

impl PartialOrd for DirEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// What an extension may ask the host to do on its behalf.
///
/// Deliberately tiny: every method here is something an extension that
/// ships today actually needs, and a wider surface would be speculation
/// about one that does not exist yet.
///
/// Three of them write ([`Host::write_file`], [`Host::delete_file`], and
/// the directory listing that makes either reachable), which is a
/// widening of what this trait once granted and the reason it is worth
/// stating plainly: an extension that edits a file needs to be *given*
/// that, and a grant nobody can name is a grant nobody can withhold. The
/// mutating methods answer with a `Result` rather than swallowing the
/// failure the way [`Host::read_file`] does — an unreadable file is a
/// state a view draws, but a save that did not happen is something the
/// person who asked for it has to be told.
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

    /// How many lines a file has, counted the way [`str::lines`] counts
    /// them. Separate from [`Host::read_file`] because the badge asks this
    /// of every untracked file on a timer, and holding each one in memory
    /// to count its newlines costs the size of the file for a number the
    /// caller could have streamed. The default answers from the contents;
    /// a host that can read without materialising them should say so.
    fn count_lines(&self, path: &std::path::Path) -> u32 {
        self.read_file(path)
            .map(|contents| contents.lines().count() as u32)
            .unwrap_or(0)
    }

    /// The path as a person would recognise it — `~/relative/path` when it
    /// sits under their home directory.
    fn display_path(&self, path: &std::path::Path) -> String;

    /// The entries of `path`, directories first and each half sorted by
    /// name — the order every listing arrives in, so the same directory
    /// never reads differently twice.
    fn list_dir(&self, path: &std::path::Path) -> Result<Vec<DirEntry>, String>;

    /// Replaces `path`'s contents. The file must already exist: this
    /// extension edits what a project has, and creating a path from a
    /// typed string is a separate grant nothing asks for yet.
    fn write_file(&self, path: &std::path::Path, contents: &str) -> Result<(), String>;

    /// Removes `path`. Files only — a directory removal is recursive by
    /// nature, and "delete this" meaning "delete these four hundred" is
    /// not a gesture a single keystroke should be able to make.
    fn delete_file(&self, path: &std::path::Path) -> Result<(), String>;

    /// The palette to render syntax-highlighted content with.
    ///
    /// Highlighting is the one thing an extension draws that carries its own
    /// colours rather than the host's roles (see [`view::Rgb`]) — so it is
    /// also the one thing that can end up unreadable against a background it
    /// knows nothing about. Naming the palette is the host's call for the
    /// same reason every other colour is, and it arrives here rather than as
    /// a constant so a light theme does not leave a diff illegible.
    fn syntax_theme(&self) -> String;
}
