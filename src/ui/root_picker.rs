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

use uze_application::{PlacementKind, RootProfile};

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
    /// The kind the space is created as: what the operator chose, or the
    /// default the root's profile lands on when they chose nothing.
    kind: PlacementKind,
    chosen_kind: bool,
    /// The profile of the root that would be chosen right now, once a
    /// worker answered it — asked off the frame, because it asks Git, and
    /// the picker never waits on a repository.
    profile: Option<(PathBuf, RootProfile)>,
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
            kind: PlacementKind::Slot,
            chosen_kind: false,
            profile: None,
        };
        picker.refresh();
        picker
    }

    /// The kind the space would be created as right now.
    pub(super) fn kind(&self) -> PlacementKind {
        self.kind
    }

    /// Whether the slot kind is available for the root that would be
    /// chosen: unknown until the profile answers, which the chips show as
    /// both available.
    pub(super) fn slots_available(&self) -> bool {
        match &self.profile {
            Some((root, profile)) if Some(root) == self.landed().as_ref() => profile.allows_slots(),
            _ => true,
        }
    }

    /// Chooses a kind. A choice the root cannot honour — slots where there
    /// is no repository — is refused, and the chips say so by drawing it
    /// unavailable.
    pub(super) fn choose_kind(&mut self, kind: PlacementKind) {
        if kind == PlacementKind::Slot && !self.slots_available() {
            return;
        }
        self.kind = kind;
        self.chosen_kind = true;
    }

    pub(super) fn toggle_kind(&mut self) {
        let other = match self.kind {
            PlacementKind::Slot => PlacementKind::Tenant,
            PlacementKind::Tenant => PlacementKind::Slot,
        };
        self.choose_kind(other);
    }

    /// The root a worker should profile: the one `chosen` would answer
    /// with, when there is one.
    pub(super) fn landed(&self) -> Option<PathBuf> {
        self.landed_root()
    }

    /// Takes a worker's answer about `root`. Nothing the operator chose is
    /// overridden — except a choice the root turns out not to allow, which
    /// lands on the only kind it does.
    pub(super) fn absorb_profile(&mut self, root: PathBuf, profile: RootProfile) {
        if Some(&root) != self.landed().as_ref() {
            return;
        }
        if !self.chosen_kind {
            self.kind = profile.default_placement();
        } else if self.kind == PlacementKind::Slot && !profile.allows_slots() {
            self.kind = PlacementKind::Tenant;
        }
        self.profile = Some((root, profile));
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
        // A separator on top of a separator names nothing more: the
        // directory is already the one being listed.
        if character == '/' && self.input.ends_with('/') {
            return;
        }
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
    ///
    /// The answer is the *space's* root, not the directory literally
    /// landed on: `uze_application::space_root` maps a subdirectory to the
    /// project it belongs to and an agent's slot to the repository the
    /// slot was cut from. The attach path already resolves its launch
    /// directory that way; a directory picked here is the same question
    /// asked with the mouse, and answering it differently is what put a
    /// `.worktrees/<id>` space in the sidebar beside the space whose agent
    /// was working in it.
    pub(super) fn chosen(&self) -> Option<(PathBuf, PlacementKind)> {
        let root = self.landed_root()?;
        // A kind the root cannot honour never leaves the picker: an answer
        // still in flight lands on the only kind every directory allows.
        let kind = if self.kind == PlacementKind::Slot && !self.slots_available() {
            PlacementKind::Tenant
        } else {
            self.kind
        };
        Some((root, kind))
    }

    fn landed_root(&self) -> Option<PathBuf> {
        let landed = match self.candidate(self.selected) {
            Some(candidate) => candidate.path.clone(),
            None => {
                let typed = expand_home(&self.input);
                if !typed.is_dir() {
                    return None;
                }
                typed
            }
        };
        Some(uze_application::space_root(&landed))
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
fn split(input: &str) -> (PathBuf, String) {
    match input.rsplit_once('/') {
        Some(("", needle)) => (PathBuf::from("/"), needle.to_owned()),
        Some((directory, needle)) => (expand_home(directory), needle.to_owned()),
        // A lone `~` is the home directory with its separator deleted, and
        // that is how a directory is picked: as its own name matched inside
        // its parent, the rule every other segment follows.
        None if input == "~" => {
            let home = expand_home("~");
            match (home.parent(), home.file_name()) {
                (Some(parent), Some(name)) => {
                    (parent.to_path_buf(), name.to_string_lossy().into_owned())
                }
                _ => (home, String::new()),
            }
        }
        // Every other bare word is a name to match inside the home
        // directory (`expand_home`'s own rule for a relative path).
        None => (expand_home("~"), input.to_owned()),
    }
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
