//! Choosing the directory a new space is born from.
//!
//! A space is born from a root, and that root is a directory that already
//! exists — so the prompt is a filter over real directories rather than a
//! free-text path. What the user types names a directory to list and a
//! segment to match inside it: the list narrows on every keystroke, `Tab`
//! descends into the row it lands on, and `Enter` creates the space there.
//!
//! The choice is always a listed row, never the directory being listed —
//! which is why deleting the trailing separator is how you pick that
//! directory itself: it becomes a segment matched inside its own parent,
//! and lands on the row bearing its name.

use std::path::{Path, PathBuf};

/// One directory the prompt can land on.
pub(super) struct Candidate {
    pub(super) name: String,
    pub(super) path: PathBuf,
}

/// Open state of the sidebar's "+ new" prompt; `None` on the model when
/// closed. Holds its own listing so a keystroke only re-reads the
/// filesystem when it changes which directory is being listed — typing a
/// name filters what was already read.
pub(super) struct RootPicker {
    input: String,
    /// The directory `listing` was read from, so a keystroke that only
    /// narrows the filter does not touch the filesystem again.
    listed: PathBuf,
    listing: Vec<Candidate>,
    /// Indices into `listing` matching the input's trailing segment, best
    /// match first.
    matched: Vec<usize>,
    selected: usize,
}

impl RootPicker {
    /// Opens the prompt inside `prefill` — the selected space's root, the
    /// directory a sibling space is most likely to be found next to.
    pub(super) fn opened_in(prefill: &str) -> Self {
        let mut input = prefill.trim_end_matches('/').to_owned();
        input.push('/');
        let mut picker = Self {
            input,
            listed: PathBuf::new(),
            listing: Vec::new(),
            matched: Vec::new(),
            selected: 0,
        };
        picker.refresh();
        picker
    }

    pub(super) fn input(&self) -> &str {
        &self.input
    }

    pub(super) fn selected(&self) -> usize {
        self.selected
    }

    pub(super) fn match_count(&self) -> usize {
        self.matched.len()
    }

    pub(super) fn matches(&self) -> impl Iterator<Item = &Candidate> {
        self.matched
            .iter()
            .filter_map(|index| self.listing.get(*index))
    }

    pub(super) fn candidate(&self, index: usize) -> Option<&Candidate> {
        self.matched
            .get(index)
            .and_then(|index| self.listing.get(*index))
    }

    /// First match to draw when only `height` rows are available — the
    /// list scrolls just enough to keep the selection on screen.
    pub(super) fn window_start(&self, height: usize) -> usize {
        (self.selected + 1).saturating_sub(height.max(1))
    }

    pub(super) fn select(&mut self, index: usize) {
        if index < self.matched.len() {
            self.selected = index;
        }
    }

    pub(super) fn move_selection(&mut self, delta: isize) {
        let last = self.matched.len().saturating_sub(1);
        self.selected = self.selected.saturating_add_signed(delta).min(last);
    }

    pub(super) fn typed(&mut self, character: char) {
        self.input.push(character);
        self.refresh();
    }

    pub(super) fn pasted(&mut self, text: &str) {
        self.input.push_str(text);
        self.refresh();
    }

    pub(super) fn backspace(&mut self) {
        self.input.pop();
        self.refresh();
    }

    /// Descends into the selected directory: the input becomes that
    /// directory, and the list becomes its children.
    pub(super) fn descend(&mut self) {
        let Some(candidate) = self.candidate(self.selected) else {
            return;
        };
        self.input = format!("{}/", crate::ui::display_project_path(&candidate.path));
        self.refresh();
    }

    /// The root a space would be created at right now: the selected
    /// directory, or — with nothing matching — the typed path itself when
    /// it already names a directory. `None` means there is nothing to
    /// create yet, and the prompt stays open.
    pub(super) fn chosen(&self) -> Option<PathBuf> {
        if let Some(candidate) = self.candidate(self.selected) {
            return Some(candidate.path.clone());
        }
        let typed = expand_home(&self.input);
        typed.is_dir().then_some(typed)
    }

    fn refresh(&mut self) {
        let (directory, needle) = split(&self.input);
        if directory != self.listed {
            self.listing = read_directories(&directory);
            self.listed = directory;
        }
        let needle = needle.to_lowercase();
        let mut leading = Vec::new();
        let mut inner = Vec::new();
        for (index, candidate) in self.listing.iter().enumerate() {
            // Hidden directories stay out of the way until the segment
            // being typed asks for one by name.
            if candidate.name.starts_with('.') && !needle.starts_with('.') {
                continue;
            }
            let name = candidate.name.to_lowercase();
            if name.starts_with(&needle) {
                leading.push(index);
            } else if !needle.is_empty() && name.contains(&needle) {
                inner.push(index);
            }
        }
        leading.append(&mut inner);
        self.matched = leading;
        self.selected = 0;
    }
}

/// Splits the input into the directory to list and the segment to match
/// inside it: everything up to the last separator is the directory, so a
/// trailing separator means "everything in here, unfiltered".
fn split(input: &str) -> (PathBuf, &str) {
    let (directory, needle) = match input.rsplit_once('/') {
        Some(("", needle)) => ("/", needle),
        Some((directory, needle)) => (directory, needle),
        // A lone `~` names the home directory itself; every other bare
        // word is a name to match inside it (`expand_home`'s own rule for
        // a relative path).
        None if input == "~" => ("~", ""),
        None => ("~", input),
    };
    (expand_home(directory), needle)
}

fn read_directories(directory: &Path) -> Vec<Candidate> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut candidates: Vec<Candidate> = entries
        .flatten()
        .map(|entry| Candidate {
            name: entry.file_name().to_string_lossy().into_owned(),
            path: entry.path(),
        })
        // `is_dir` on the path rather than the entry's own file type, so a
        // symlink into a checkout is offered like the directory it is.
        .filter(|candidate| candidate.path.is_dir())
        .collect();
    candidates.sort_by_key(|candidate| candidate.name.to_lowercase());
    candidates
}

/// Resolves what the user typed against `$HOME`: `~` and a bare relative
/// path both name something inside it, since that is where checkouts live.
pub(super) fn expand_home(typed: &str) -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let path = PathBuf::from(typed);
    match (typed.strip_prefix('~'), home) {
        (Some(rest), Some(home)) => home.join(rest.trim_start_matches('/')),
        (None, Some(home)) if path.is_relative() => home.join(path),
        _ => path,
    }
}

#[cfg(test)]
mod tests;
