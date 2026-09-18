//! Choosing the directory a new space is born from.
//!
//! A space is born from a root, and that root is a directory that already
//! exists — so the prompt is a filter over real directories rather than a
//! free-text path. What the user types names a directory to list and a
//! segment to match inside it: the list narrows on every keystroke, `Tab`
//! completes the row it lands on, and `Enter` creates the space there.
//!
//! **The input is the path.** A separator is a character like any other:
//! the text before the last one names the directory to list, the text
//! after it is matched inside it, and nothing the prompt is doing lives
//! anywhere the typed line does not show. A `/` used to descend into
//! whichever row happened to be selected and clear the line, which is an
//! implicit `Tab` nobody asked for — it took what was being typed and
//! answered with something else.
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
    /// Where a typed path starts from. Not part of what is typed: a
    /// prompt that opens with a path already in it asks to be deleted
    /// before it can be used, and the only thing worth typing here is the
    /// name being looked for. `Backspace` on an empty line moves it up.
    origin: PathBuf,
    /// Everything the operator typed, separators included — a path from
    /// `origin`, or from `$HOME` once it starts with `~` or `/`.
    input: String,
    /// The directory `input` names, derived on every [`Self::refresh`]:
    /// what the listing is read from, and what `Enter` takes while
    /// nothing inside it is chosen.
    base: PathBuf,
    /// Whether anything the operator did has chosen a row yet. Until then
    /// the listing shows what is there without claiming one of them, and
    /// `Enter` takes the directory being listed.
    touched: bool,
    /// The directory `listing` was read from, so a keystroke that only
    /// narrows the filter does not touch the filesystem again.
    listed: PathBuf,
    listing: Vec<Candidate>,
    /// Which rows of `listing` a worktree space could come from, answered
    /// once per row per listing: the walk below costs a few `stat`s, and
    /// a keystroke only narrows what was already read.
    reaches: std::collections::HashMap<PathBuf, bool>,
    /// Indices into `listing` matching the input's trailing segment, best
    /// match first.
    matched: Vec<usize>,
    selected: usize,
    /// The project the operator is standing in, when it is one of the
    /// rows. The prompt opens on it and returns to it whenever the
    /// listing is read again with nothing typed — a kind chosen, a
    /// profile landing — so "another space over this project" stays one
    /// `Enter` away while the listing shows everywhere else they could
    /// go.
    marked: Option<PathBuf>,
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
    /// Opens the prompt listing `prefill`, with the project the operator
    /// is standing in marked when it is one of the rows.
    ///
    /// The two are a different question. What is *listed* is where
    /// projects are — the same premise the prompt resolves typing
    /// against — and what the prompt is *on* is where the operator
    /// already is, so neither the walk to another project nor a second
    /// space over this one costs a gesture the other one saves.
    pub(super) fn opened_in(prefill: &str, standing_in: Option<&Path>) -> Self {
        // Trailing separators go, but never all of them: `/` trimmed to
        // nothing is a relative path, and a relative path is resolved
        // against home — which would answer "home" for a project sitting
        // directly under the root.
        let trimmed = prefill.trim_end_matches('/');
        let origin = expand_home(if trimmed.is_empty() { prefill } else { trimmed });
        let mut picker = Self {
            base: origin.clone(),
            origin,
            input: String::new(),
            touched: false,
            listed: PathBuf::new(),
            listing: Vec::new(),
            reaches: std::collections::HashMap::new(),
            matched: Vec::new(),
            selected: 0,
            marked: standing_in.map(Path::to_path_buf),
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
                // The directory the prompt *opened* on: the project it
                // is standing in when there is one, and otherwise the
                // one being listed. Deliberately not the row it has
                // moved to since — that is what the profile answers,
                // and reading `picked` here would read it mid-refresh,
                // before this pass has resolved it.
                None if holds_a_repository(self.marked.as_deref().unwrap_or(&self.origin)) => {
                    SpaceKind::Worktree
                }
                None => SpaceKind::Workspace,
            },
        }
    }

    /// Whether the worktree kind is available for the landed root: unknown
    /// until the profile answers, which the chips show as available.
    pub(super) fn slots_available(&self) -> bool {
        self.profile.is_none_or(|profile| profile.slots_possible)
    }

    /// Chooses a kind. Always answered: where it was refused, the control
    /// was dead in exactly the place a person would use it, standing in a
    /// directory that is no repository and looking for one under it.
    pub(super) fn choose_kind(&mut self, kind: SpaceKind) {
        self.chosen = Some(kind);
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

    /// The directory the listing comes from and the segment matched
    /// inside it: everything before the last separator, and everything
    /// after it.
    fn split_input(&self) -> (PathBuf, &str) {
        match self.input.rfind('/') {
            Some(cut) => {
                let (directory, segment) = self.input.split_at(cut + 1);
                (self.resolve(directory), segment)
            }
            None => (self.origin.clone(), self.input.as_str()),
        }
    }

    /// Where a typed directory lands: `~` and a leading separator leave
    /// `origin` behind the way they do in a shell, and everything else is
    /// read from where the prompt opened.
    fn resolve(&self, typed: &str) -> PathBuf {
        if typed.starts_with('~') || typed.starts_with('/') {
            expand_home(typed)
        } else {
            self.origin.join(typed)
        }
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
        self.input.push(character);
        self.refresh();
    }

    pub(super) fn pasted(&mut self, text: &str) {
        self.input.push_str(text);
        self.refresh();
    }

    /// Deletes the last character typed — or, with nothing typed, leaves
    /// the directory the prompt starts from for the one above it: the way
    /// out of the directory it opened on. Everywhere else the way out is
    /// the separator, which is on the line and deletes like any other
    /// character.
    pub(super) fn backspace(&mut self) {
        if self.input.pop().is_none()
            && let Some(parent) = self.origin.parent()
        {
            self.origin = parent.to_path_buf();
        }
        self.refresh();
    }

    /// Completes the selected row into the line: its name replaces the
    /// segment being matched, and the separator after it makes the line
    /// name that directory, so the list becomes its children.
    ///
    /// Completion writes into the input rather than into state beside it
    /// — what the prompt is listing is always what the line says.
    pub(super) fn descend(&mut self) {
        let Some(candidate) = self.selection().and_then(|index| self.candidate(index)) else {
            return;
        };
        let name = candidate.name.clone();
        let segment = self.input.rfind('/').map_or(0, |cut| cut + 1);
        self.input.truncate(segment);
        self.input.push_str(&name);
        self.input.push('/');
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
                // The *segment*, not the whole line: a path typed out
                // with its separator at the end names the directory it
                // ends in, and `Enter` there takes it.
                if !self.split_input().1.is_empty() || !self.base.is_dir() {
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

    /// Reads the directory the line names and matches the segment inside
    /// it. Every directory is offered, whichever kind is being created:
    /// the worktree kind used to keep only the rows that were
    /// repositories, which made every repository that is not a direct
    /// child of the directory being listed unreachable — `~/projects`
    /// holds nothing but repositories and showed as empty, because
    /// `projects` is not one itself. A directory is the way to what is
    /// under it whether or not it is the thing being looked for, and
    /// what a worktree space may be created on is decided by
    /// [`Self::chosen`], which is where it was always decided.
    fn refresh(&mut self) {
        let (base, segment) = self.split_input();
        let needle = segment.to_lowercase();
        self.base = base;
        if self.base != self.listed {
            self.listing = read_directories(&self.base);
            self.listed = self.base.clone();
            self.reaches.clear();
        }
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
        // A worktree space is cut from a repository, so a row that is
        // neither one nor the way to one is not an answer to what is
        // being looked for. Applied to the row *itself* this hid the way:
        // a `projects` folder holding nothing but repositories drew as
        // empty, because `projects` is not one. So the question is asked
        // of the rows below it too, as far as `REPOSITORY_REACH`.
        if self.kind() == SpaceKind::Worktree {
            let mut reaches = std::mem::take(&mut self.reaches);
            leading.retain(|index| match self.listing.get(*index) {
                Some(candidate) => *reaches
                    .entry(candidate.path.clone())
                    .or_insert_with(|| reaches_a_repository(&candidate.path, REPOSITORY_REACH)),
                None => false,
            });
            self.reaches = reaches;
        }
        self.matched = leading;
        self.selected = 0;
        // Typing is choosing: the best match leads the list, and it is the
        // one `Enter` takes. A line ending in a separator has typed no
        // segment, so it has chosen nothing inside what it names.
        self.touched = !needle.is_empty();
        // Nothing typed, so the prompt is back on the project the
        // operator is standing in. The listing is re-read whenever the
        // kind changes — including when the profile lands and decides it
        // — and a re-read that moved them off it would change what
        // `Enter` creates while nobody asked.
        if self.input.is_empty()
            && let Some(marked) = self.marked.clone()
            && let Some(index) = self
                .matched
                .iter()
                .position(|candidate| self.listing[*candidate].path == marked)
        {
            self.selected = index;
            self.touched = true;
        }
        self.reland();
    }
}

/// Whether `directory` is the root of a Git repository: a checkout carries
/// `.git` as a directory, a worktree of one as a file pointing at it.
fn holds_a_repository(directory: &Path) -> bool {
    directory.join(".git").exists()
}

/// How far below a row a repository still counts as reachable from it.
/// Two, because that is where they are: a folder of projects, and a
/// folder of folders of projects for anyone who groups them by client or
/// by org. Deeper is a search, and this is a listing.
const REPOSITORY_REACH: usize = 2;

/// How many entries of one directory are looked at while answering. The
/// walk is bounded on every axis on purpose — a `node_modules` on the way
/// must cost a handful of `stat`s, not a traversal.
const ENTRIES_SCANNED: usize = 64;

/// Whether a worktree space could be created at `directory` or anywhere
/// `depth` levels below it.
fn reaches_a_repository(directory: &Path, depth: usize) -> bool {
    if holds_a_repository(directory) {
        return true;
    }
    if depth == 0 {
        return false;
    }
    let Ok(entries) = std::fs::read_dir(directory) else {
        return false;
    };
    entries
        .flatten()
        .take(ENTRIES_SCANNED)
        // A hidden directory is not offered as a row, so it is not a way
        // to one either — and skipping it keeps the walk out of the
        // caches and histories every home is full of.
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .any(|path| reaches_a_repository(&path, depth - 1))
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
