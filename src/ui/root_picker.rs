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

use uze_application::RootProfile;
use uze_terminal::SpaceKind;

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
    /// The directory the listing is read from. Not part of what is typed:
    /// a prompt that opens with a path already in it asks to be deleted
    /// before it can be used, and the only thing worth typing here is the
    /// name being looked for.
    base: PathBuf,
    input: String,
    /// Whether anything the operator did has chosen a row yet. Until then
    /// the listing shows what is there without claiming one of them, and
    /// `Enter` takes the directory being listed.
    touched: bool,
    /// The directory `listing` was read from, so a keystroke that only
    /// narrows the filter does not touch the filesystem again.
    listed: PathBuf,
    listing: Vec<Candidate>,
    /// Indices into `listing` matching the input's trailing segment, best
    /// match first.
    matched: Vec<usize>,
    selected: usize,
    /// The directory the prompt is on right now — the row chosen in the
    /// listing, or the directory being listed — resolved when the input or
    /// the selection changes rather than on every frame: it probes the
    /// filesystem.
    picked: Option<PathBuf>,
    /// The repository or workspace `picked` belongs to, which is what a
    /// worktree space is cut from and what the profile is asked about.
    landed: Option<PathBuf>,
    /// The kind the operator chose, if they chose one.
    chosen: Option<SpaceKind>,
    /// The profile of `landed`, once a worker answered it — asked off the
    /// frame, because it asks Git, and the picker never waits on a
    /// repository.
    profile: Option<RootProfile>,
}

impl RootPicker {
    /// Opens the prompt inside `prefill` — the selected space's root, the
    /// directory a sibling space is most likely to be found next to.
    pub(super) fn opened_in(prefill: &str) -> Self {
        let mut picker = Self {
            base: expand_home(prefill.trim_end_matches('/')),
            input: String::new(),
            touched: false,
            listed: PathBuf::new(),
            listing: Vec::new(),
            matched: Vec::new(),
            selected: 0,
            picked: None,
            landed: None,
            chosen: None,
            profile: None,
        };
        picker.refresh();
        picker
    }

    /// The kind the space would be created as right now: what the operator
    /// chose while the root allows it, and otherwise what the root's
    /// profile lands on — a worktree until the profile answers.
    pub(super) fn kind(&self) -> SpaceKind {
        match (self.chosen, self.slots_available()) {
            (Some(kind), _) => kind,
            // Until the profile answers, the directory being listed decides:
            // a repository is somewhere agents get checkouts of their own,
            // and anywhere else a worktree space would list nothing at all
            // (see the filter in `refresh`).
            (None, _) => match self.profile {
                Some(profile) => crate::ui::default_space_kind(profile),
                None if holds_a_repository(&self.base) => SpaceKind::Worktree,
                None => SpaceKind::Workspace,
            },
        }
    }

    /// Whether the worktree kind is available for the landed root: unknown
    /// until the profile answers, which the chips show as available.
    pub(super) fn slots_available(&self) -> bool {
        self.profile.is_none_or(|profile| profile.slots_possible)
    }

    /// Chooses a kind. Always answered: the kind says what is being looked
    /// for, and the listing narrows to the directories that can be it (see
    /// `refresh`) — where it was refused, the control was dead in exactly
    /// the place a person would use it, standing in a directory that is no
    /// repository and looking for one under it.
    pub(super) fn choose_kind(&mut self, kind: SpaceKind) {
        self.chosen = Some(kind);
        // What the listing offers depends on the kind (see `refresh`), so
        // the rows are read again against the one just chosen.
        self.refresh();
    }

    pub(super) fn toggle_kind(&mut self) {
        self.choose_kind(match self.kind() {
            SpaceKind::Worktree => SpaceKind::Workspace,
            SpaceKind::Workspace => SpaceKind::Worktree,
        });
    }

    /// The root a worker should profile: the one `chosen` would answer
    /// with, when there is one.
    pub(super) fn landed(&self) -> Option<PathBuf> {
        self.landed.clone()
    }

    /// Takes a worker's answer about `root`, when it is still the root
    /// landed on.
    pub(super) fn absorb_profile(&mut self, root: PathBuf, profile: RootProfile) {
        if self.landed.as_ref() != Some(&root) {
            return;
        }
        self.profile = Some(profile);
        // The first answer decides the kind, and from then on only the
        // operator does: a kind that keeps re-deciding itself as the
        // selection moves changes what `Enter` would create while nobody
        // asked it to.
        if self.chosen.is_none() {
            self.chosen = Some(crate::ui::default_space_kind(profile));
            self.refresh();
        }
    }

    pub(super) fn input(&self) -> &str {
        &self.input
    }

    /// The directory being listed, which is also what `Enter` takes while
    /// nothing has been chosen in it.
    pub(super) fn base(&self) -> &Path {
        &self.base
    }

    pub(super) fn selected(&self) -> usize {
        self.selected
    }

    /// The row the listing marks, if the operator has chosen one yet.
    pub(super) fn selection(&self) -> Option<usize> {
        self.touched.then_some(self.selected)
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
            self.touched = true;
            self.reland();
        }
    }

    pub(super) fn move_selection(&mut self, delta: isize) {
        let last = self.matched.len().saturating_sub(1);
        // The first move chooses the row already in front, rather than
        // stepping past it.
        if std::mem::replace(&mut self.touched, true) {
            self.selected = self.selected.saturating_add_signed(delta).min(last);
        }
        self.reland();
    }

    pub(super) fn typed(&mut self, character: char) {
        // A separator is how a name being typed becomes the directory to
        // look in — the same move `Tab` makes on the row it lands on.
        if character == '/' {
            self.descend();
            return;
        }
        self.input.push(character);
        self.refresh();
    }

    pub(super) fn pasted(&mut self, text: &str) {
        self.input.push_str(text);
        self.refresh();
    }

    /// Deletes the last character typed — or, with nothing typed, leaves
    /// the directory being listed for the one above it: the way back out of
    /// a directory `Tab` walked into.
    pub(super) fn backspace(&mut self) {
        if self.input.pop().is_none()
            && let Some(parent) = self.base.parent()
        {
            self.base = parent.to_path_buf();
        }
        self.refresh();
    }

    /// Descends into the selected directory: the input becomes that
    /// directory, and the list becomes its children.
    pub(super) fn descend(&mut self) {
        let Some(candidate) = self.selection().and_then(|index| self.candidate(index)) else {
            return;
        };
        self.base = candidate.path.clone();
        self.input.clear();
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
    pub(super) fn chosen(&self) -> Option<(PathBuf, SpaceKind)> {
        let kind = self.kind();
        // A worktree space cuts its agents' checkouts from a repository, so
        // a directory inside one is that repository. A workspace space runs
        // its agents where it stands, and there the directory chosen is the
        // answer — mapping it up put the space somewhere nobody picked.
        let root = match kind {
            // Only a repository can have slots cut from it, so a worktree
            // space over anything else is nothing to create yet: the prompt
            // stays open on a listing of the repositories under it.
            SpaceKind::Worktree => {
                let landed = self.landed.clone()?;
                if !self.slots_available() || !holds_a_repository(&landed) {
                    return None;
                }
                landed
            }
            // An agent's own checkout is still the repository it was cut
            // from — a space there is a space in the project, not in one
            // agent's working directory (see `uze_application::slot_key`).
            SpaceKind::Workspace => uze_application::slot_key(self.picked.as_deref()?),
        };
        Some((root, kind))
    }

    fn resolve_picked(&self) -> Option<PathBuf> {
        let landed = match self.selection().and_then(|index| self.candidate(index)) {
            Some(candidate) => candidate.path.clone(),
            // Nothing chosen in it: the directory being listed is the
            // answer, which is what `Enter` on an untouched prompt takes.
            // A name typed that matches nothing is not that — it is a
            // request for a directory there is no row for, and the prompt
            // stays open rather than quietly creating the space one level
            // up.
            None => {
                if !self.input.is_empty() || !self.base.is_dir() {
                    return None;
                }
                self.base.clone()
            }
        };
        Some(landed)
    }

    /// Re-resolves the landed root after the selection moved; a different
    /// root forgets the previous one's profile.
    fn reland(&mut self) {
        let picked = self.resolve_picked();
        let landed = picked.as_deref().map(uze_application::space_root);
        self.picked = picked;
        if landed != self.landed {
            self.profile = None;
            self.landed = landed;
        }
    }

    fn refresh(&mut self) {
        if self.base != self.listed {
            self.listing = read_directories(&self.base);
            self.listed = self.base.clone();
        }
        let needle = self.input.to_lowercase();
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
        // A worktree space cuts its agents' checkouts from a repository, so
        // only a directory that is one can be picked for it. The test is a
        // `.git` beside the name — one look at the filesystem per row,
        // where asking Git would be a process per row on every keystroke.
        if self.kind() == SpaceKind::Worktree {
            let listing = &self.listing;
            leading.retain(|index| {
                listing
                    .get(*index)
                    .is_some_and(|candidate| holds_a_repository(&candidate.path))
            });
        }
        self.matched = leading;
        self.selected = 0;
        // Typing is choosing: the best match leads the list, and it is the
        // one `Enter` takes.
        self.touched = !self.input.is_empty();
        self.reland();
    }
}

/// Whether `directory` is the root of a Git repository: a checkout carries
/// `.git` as a directory, a worktree of one as a file pointing at it.
fn holds_a_repository(directory: &Path) -> bool {
    directory.join(".git").exists()
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
