//! The workspace TUI's code surface — one place for the three things a
//! person asks of the checkout a tab is standing in: what changed in it,
//! what it contains, and what happened to it.
//!
//! # Why one extension and not two
//!
//! The diff and the file tree shipped separately first, and using them
//! showed why that is wrong. The flow this product is for is **agent
//! first → diff → and only last, touch the real file**: the diff is where
//! a person's contact with the work begins, and editing is the exception
//! at the end. So "I see it in the diff, let me fix that line" is the
//! most-crossed seam there is, and two surfaces put a close, an open and
//! a re-navigation to *the same file* in the middle of it.
//!
//! The merge is not a de-duplication. What was genuinely shared already
//! is: the host draws both (`src/ui/extension_view.rs`), and highlighting
//! is [`highlight`]. The two navigators only look alike —
//! [`changes`] compacts a flat, complete list from `git status`, and
//! [`files`] flattens a partial tree that grows as directories are
//! opened. What one extension can have and two cannot is a **selection
//! that survives the switch**, and that is the whole of the value.
//!
//! # The rule that keeps it from rotting
//!
//! No handler branches on the mode to decide what the state *means*. The
//! mode decides who is *asked*; each half answers about the same path and
//! knows nothing about the other. [`changes`] answers "the diff of this
//! path", [`files`] answers "the tree containing it", [`editor`] answers
//! "the text of it".
//!
//! This is why the selection is a [`PathBuf`] rather than an index into
//! the changed files: an index cannot mean anything to a tree that lists
//! directories on demand, and a path means the same thing to both — which
//! is also what a person means by "this file".
//!
//! # Scope
//!
//! The checkout the active tab is *in*, and nothing else. An agent UZE
//! isolated works in `.worktrees/<name>`, and `git worktree list` answers
//! repository-wide from anywhere inside it — so listing linked worktrees
//! here would put every other agent's diff, plus every checkout nobody
//! ever cleaned up, inside a tab that owns exactly one of them.
//!
//! # Nothing here touches a disk
//!
//! Two cadences, both off the drawing thread. The changes half re-reads
//! on a timer ([`Changes::read`]); the files half asks for what it needs
//! one [`FileRequest`] at a time and the host answers. Neither can reach
//! the other: a refresh installs a new [`Changes`] and nothing else,
//! which is what keeps it from eating a buffer someone is typing into.

use std::{
    collections::{BTreeSet, VecDeque},
    path::{Path, PathBuf},
    time::Duration,
};

use crate::{
    Host,
    view::{Caret, Command, ScrollDirection, Size, ViewHit},
};

mod changes;
mod changes_tree;
mod diff;
mod editor;
mod files;
mod highlight;
mod history;
mod markdown;
mod render;
mod request;

pub use changes::{ChangeSummary, change_summary};
pub use history::{Commit, CommitDetail, Timeline, commit_detail, timeline, timeline_section};
pub use render::view;
pub use request::{FileAnswer, FileRequest, LoadedFile, fulfill, unanswered};

use changes::Changes;
use changes_tree::{FileTreeItem, file_tree_items};
use diff::DiffLineKind;
use editor::OpenFile;
use files::Files;

/// This extension's registry entry — registered once in
/// `ExtensionRegistry::builtin`; the management TUI's Extensions screen
/// renders it.
pub const CATALOG: crate::registry::BuiltinExtension = crate::registry::BuiltinExtension {
    id: "code",
    name: "Code",
    description: "Changes, contents and history of the active checkout.",
    surface: "Workspace TUI",
    usage: "The timeline sits in the sidebar; open the changes with Ctrl+G or the changes chip, and the files with Ctrl+E or the code chip.",
};

const REFRESH_INTERVAL: Duration = Duration::from_millis(750);

/// What to do after an event reaches an open [`CodeView`] — the host only
/// needs to know whether to keep the surface open.
pub enum CodeOutcome {
    Stay,
    Close,
}

/// Which list the navigator is showing — always the one the content mode
/// reads from, which is why it is derived rather than kept.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NavigatorMode {
    /// The files `git status` reports, compacted into a tree.
    Changes,
    /// The checkout's own tree, listed as it is opened.
    Files,
}

/// Which half answers for the selection. Also which door was used: the
/// mode a person arrives in is the one they chose before pressing
/// anything, which is why there are two entry points rather than one that
/// asks again.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ContentMode {
    #[default]
    Diff,
    Contents,
    /// A markdown file as the document it describes, rather than as the
    /// markup that describes it. Offered only for a file that is one.
    Preview,
}

/// Where the keyboard is. Rendered more strongly than mere selection.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Focus {
    #[default]
    Navigator,
    Content,
}

/// Open state of the code surface (`WorkspaceModel::code`).
pub struct CodeView {
    /// The checkout this is scoped to, resolved once at open time from
    /// the active tab's live `cwd`. Inside a linked worktree this is that
    /// worktree, not the primary it hangs off.
    root: PathBuf,
    /// `root` as a person would recognise it, resolved by whoever opened
    /// the surface. Drawing then needs no host at all, which is what lets
    /// the renderer hold none.
    display_root: String,
    /// The branch `root` is on, for the title — the one place a scoped
    /// surface still has to say *which* checkout you are looking at.
    branch: String,
    /// The file every mode is about. The one piece of state both halves
    /// share, and the reason this is one extension.
    selected: Option<PathBuf>,
    content: ContentMode,
    focus: Focus,
    scroll: u16,
    changes: Changes,
    files: Files,
    open: Option<OpenFile>,
    /// What the host still has to do for the files half, oldest first.
    queue: VecDeque<FileRequest>,
    /// A failure about the surface rather than about one file.
    error: Option<String>,
    /// The last thing that happened, shown in the footer until the next
    /// thing does.
    notice: Option<String>,
    /// A delete waiting for its second keystroke. Deleting is the one
    /// gesture here that cannot be undone, so it is the one that asks.
    confirming_delete: Option<PathBuf>,
    /// Closing with unsaved changes, waiting for its second Esc.
    confirming_discard: bool,
}

/// Where a viewer was on a checkout's code surface, so that opening it
/// again is returning to it rather than starting over.
///
/// Opaque to the host, which keeps one per checkout: it takes one from
/// [`CodeView::place`] as the surface closes and hands it back to
/// [`CodeView::resuming`] the next time that checkout is opened.
///
/// What it holds is navigation and nothing else — the file being read,
/// the directories opened to reach it, the ones folded away, and how far
/// down it. Deliberately not the content mode: the mode is the door that
/// was used (`Ctrl+G` reviews, `Ctrl+E` navigates), and a door that
/// remembered where it last led would stop being one. Deliberately not a
/// buffer either: an unsaved edit belongs to the surface that has it
/// open, and reopening a closed one must not resurrect typing.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CodePlace {
    selected: Option<PathBuf>,
    expanded: BTreeSet<PathBuf>,
    folded: BTreeSet<String>,
    scroll: u16,
}

/// What a viewer did inside an open [`CodeView`] that a re-read of the
/// changes must not undo.
///
/// Opaque to the host — it takes one from [`CodeView::placement`] and
/// hands it back with the answer without looking inside.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ViewPlacement {
    path: Option<PathBuf>,
}

impl CodeView {
    /// A surface that has been asked for but not read yet.
    ///
    /// Opening reads a repository, which is exactly the cost a refresh
    /// pays and belongs on exactly the same thread. So it appears the
    /// instant it is asked for, saying it is reading, and fills in when
    /// the host's answers land.
    ///
    /// `mode` is the door: the changes chip and `Ctrl+G` open on the
    /// diff, the code chip and `Ctrl+E` on the tree.
    pub fn opening(cwd: PathBuf, display_root: String, mode: ContentMode) -> Self {
        let mut view = Self {
            root: cwd,
            display_root,
            branch: String::new(),
            selected: None,
            content: mode,
            focus: Focus::Navigator,
            scroll: 0,
            changes: Changes {
                diff_pending: true,
                ..Changes::default()
            },
            files: Files::default(),
            open: None,
            queue: VecDeque::new(),
            error: None,
            notice: None,
            confirming_delete: None,
            confirming_discard: false,
        };
        if view.navigator() == NavigatorMode::Files {
            view.expand(view.root.clone());
        }
        view
    }

    /// The same surface, put back where [`CodePlace`] says the viewer
    /// left this checkout.
    ///
    /// Every restored directory is asked for again rather than assumed:
    /// what a listing said last time is a claim about a tree that has
    /// been under an agent's hands since, and this surface never draws a
    /// read it did not just make. The requests go through the same queue
    /// a click on a disclosure uses, so nothing here touches a disk.
    pub fn resuming(mut self, place: CodePlace) -> Self {
        let CodePlace {
            selected,
            expanded,
            folded,
            scroll,
        } = place;
        self.changes.folded = folded;
        // Sorted, so a directory is asked for after the one containing it.
        for directory in expanded {
            self.expand(directory);
        }
        let Some(path) = selected else {
            return self;
        };
        self.selected = Some(path);
        self.read_selection_as_what_it_is();
        match self.navigator() {
            NavigatorMode::Files => self.load_selection(None),
            // The list and the diff both arrive from the refresh the host
            // schedules the moment the surface opens.
            NavigatorMode::Changes => self.changes.diff_pending = true,
        }
        // After the read is asked for, not before: opening a file puts the
        // content back at the top, and this is the one case where the top
        // is not where the viewer was.
        self.scroll = scroll;
        self
    }

    /// Where the viewer is, for the host to hand back to [`Self::resuming`].
    pub fn place(&self) -> CodePlace {
        CodePlace {
            selected: self.selected.clone(),
            expanded: self.files.expanded.clone(),
            folded: self.changes.folded.clone(),
            scroll: self.scroll,
        }
    }

    /// The list that goes with what the content is showing.
    fn navigator(&self) -> NavigatorMode {
        match self.content {
            ContentMode::Diff => NavigatorMode::Changes,
            ContentMode::Contents | ContentMode::Preview => NavigatorMode::Files,
        }
    }

    /// Which mode the surface is on, so a door pressed twice can tell that
    /// it is the one already open and close instead of doing nothing.
    pub fn showing(&self) -> ContentMode {
        self.content
    }

    /// The checkout this is scoped to, so the host can say which
    /// repository an answer it is holding was read for.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Whether the selection is a document the preview can render — what
    /// decides whether the mode is offered at all.
    pub(super) fn selected_is_markdown(&self) -> bool {
        self.selected.as_deref().is_some_and(markdown::is_markdown)
    }

    /// Whether the selection is a file rather than a directory — what
    /// decides whether editing and deleting are on offer.
    pub(super) fn selected_is_a_file(&self) -> bool {
        self.selected.as_ref().is_some_and(|path| {
            !self
                .files
                .row_at(&self.root, path)
                .is_some_and(|row| row.directory)
        })
    }

    /// Whether a file is open for typing — which scope the host puts the
    /// keyboard in, and the one state where a letter is text rather than
    /// a shortcut.
    pub fn editing(&self) -> bool {
        self.content == ContentMode::Contents
            && self
                .open
                .as_ref()
                .is_some_and(|open| open.editing && open.error.is_none())
    }

    /// Whether the diff on screen is the selected file's yet.
    pub fn diff_pending(&self) -> bool {
        self.changes.diff_pending
    }

    pub fn refresh_due(&self) -> bool {
        self.changes
            .refreshed_at
            .is_none_or(|at| at.elapsed() >= REFRESH_INTERVAL)
    }

    /// Where the viewer is, for the re-read to answer about.
    pub fn placement(&self) -> ViewPlacement {
        ViewPlacement {
            path: self.selected.clone(),
        }
    }

    /// Re-reads the changes half, and nothing else.
    ///
    /// Takes no `&self`: the read is the slow part and runs wherever the
    /// host puts it, so nothing here may borrow the view being refreshed.
    /// It answers with a [`Changes`] rather than a whole view, because
    /// the view now holds a buffer — see the module doc.
    pub fn refresh(host: &dyn Host, root: PathBuf, placement: ViewPlacement) -> RefreshedChanges {
        let branch = current_branch(host, &root);
        let changes = match host.repository_root(&root) {
            Ok(resolved) => Changes::read(host, &resolved, placement.path.as_deref()),
            Err(message) => Changes::failed(message),
        };
        RefreshedChanges {
            placement,
            branch,
            changes,
        }
    }

    /// Installs a refresh, if it still describes where the viewer is.
    pub fn absorb_changes(&mut self, refreshed: RefreshedChanges) {
        let RefreshedChanges {
            placement,
            branch,
            changes,
        } = refreshed;
        self.branch = branch;
        // The folds are the viewer's, not the read's: a refresh answers
        // what changed, and tidying the tree around it is not something
        // it gets to undo.
        let folded = std::mem::take(&mut self.changes.folded);
        self.changes = changes;
        self.changes.folded = folded;
        // The selection moved while the read was out, so what came back
        // is a diff of a file nobody is looking at. Keep the list, ask
        // again for the diff.
        self.changes.diff_pending = placement.path != self.selected;
        if self.selected.is_none() {
            self.selected = self.changes.files.first().map(|file| file.path.clone());
            self.changes.diff_pending = self.selected.is_some();
            self.read_selection_as_what_it_is();
        }
    }

    /// The next thing the host should do for the files half, or `None`.
    pub fn take_request(&mut self) -> Option<FileRequest> {
        self.queue.pop_front()
    }

    /// Whether anything is still out.
    pub fn waiting(&self) -> bool {
        !self.queue.is_empty()
    }

    /// Installs what the host did about a [`FileRequest`].
    pub fn absorb(&mut self, answer: FileAnswer) {
        match answer {
            FileAnswer::Listed { path, entries } => match entries {
                Ok(entries) => {
                    self.files.listings.insert(path.clone(), entries);
                    if self.selected.is_none() {
                        self.selected = self
                            .files
                            .rows(&self.root)
                            .first()
                            .map(|row| row.path.clone());
                        // Landing on the first row is a selection like any
                        // other, and a tree whose first row is a README is
                        // the commonest way into this surface.
                        self.read_selection_as_what_it_is();
                    }
                }
                Err(message) if path == self.root => self.error = Some(message),
                Err(message) => {
                    // One unreadable directory is not a broken tree: it
                    // folds shut again and says why, and everything else
                    // stays navigable.
                    self.files.expanded.remove(&path);
                    self.notice = Some(message);
                }
            },
            FileAnswer::Read { path, file } => {
                let Some(open) = self.open.as_mut().filter(|open| open.path == path) else {
                    return;
                };
                open.loading = false;
                // A read landing on a buffer someone has typed into since
                // is the save's own re-read arriving a keystroke late.
                // Installing it would silently undo those keystrokes.
                if open.modified {
                    return;
                }
                match file {
                    Ok(loaded) => {
                        let wanted = open.caret.line;
                        open.install(loaded);
                        open.place_caret(wanted);
                    }
                    Err(message) => open.error = Some(message),
                }
            }
            FileAnswer::Saved { path, outcome } => match outcome {
                Ok(()) => {
                    if let Some(open) = self.open.as_mut().filter(|open| open.path == path) {
                        open.modified = false;
                    }
                    self.notice = Some(format!("saved {}", file_name(&path)));
                    // Re-read what was written: every line typed since the
                    // last read was highlighted approximately, and this is
                    // where that debt is paid off.
                    self.queue.push_back(FileRequest::Read(path));
                }
                Err(message) => self.notice = Some(message),
            },
            FileAnswer::Deleted { path, outcome } => match outcome {
                Ok(()) => {
                    self.notice = Some(format!("deleted {}", file_name(&path)));
                    if self.open.as_ref().is_some_and(|open| open.path == path) {
                        self.open = None;
                    }
                    if self.selected.as_ref() == Some(&path) {
                        self.selected = None;
                    }
                    if let Some(parent) = path.parent() {
                        self.queue
                            .push_back(FileRequest::List(parent.to_path_buf()));
                    }
                }
                Err(message) => self.notice = Some(message),
            },
        }
    }

    /// Where the selection sits in the changed-file list, if it changed.
    fn selected_change(&self) -> Option<usize> {
        self.changes.position_of(self.selected.as_deref())
    }

    /// Points the surface at `path`: every mode follows.
    fn select(&mut self, path: PathBuf) {
        if self.selected.as_deref() == Some(path.as_path()) {
            return;
        }
        self.selected = Some(path);
        self.scroll = 0;
        self.changes.diff = Vec::new();
        self.changes.diff_pending = true;
        self.open = None;
        self.read_selection_as_what_it_is();
    }

    /// Puts the content mode where the new selection wants it: a markdown
    /// file is a document before it is a file, so it is read as one.
    ///
    /// Arriving at a README and being shown its markup is the wrong
    /// default — the markup is what `p` is for, and that choice lasts as
    /// long as the file it was made about. Diff is never touched: a
    /// document's changes are still changes, and the navigator would have
    /// to change lists to show anything else.
    fn read_selection_as_what_it_is(&mut self) {
        if self.content == ContentMode::Diff {
            return;
        }
        self.content = if self.selected_is_markdown() {
            ContentMode::Preview
        } else {
            ContentMode::Contents
        };
    }

    /// Shows `mode` for whatever is selected, bringing the selection with
    /// it — the switch this whole surface exists for, and what the two
    /// doors mean once the surface is already open.
    ///
    /// The line comes too where it is known: a diff row carries the line
    /// number it has on the new side, so a reader who was looking at line
    /// 42 of a diff lands on line 42 of the file. Carrying only the file
    /// would leave them at the top of something they were reading the
    /// middle of, which is most of the trip they were trying to avoid.
    pub fn show(&mut self, mode: ContentMode) {
        if self.content == mode {
            return;
        }
        let line = self.line_in_view();
        self.content = mode;
        match mode {
            ContentMode::Contents | ContentMode::Preview => {
                self.reveal_selection();
                self.load_selection(line);
            }
            ContentMode::Diff => {
                self.scroll = line
                    .and_then(|line| self.diff_row_of(line))
                    .unwrap_or(self.scroll);
            }
        }
    }

    /// The line of the file the viewer is currently looking at, in the
    /// mode they are in — one-based, as a file's lines are numbered.
    fn line_in_view(&self) -> Option<usize> {
        match self.content {
            ContentMode::Contents | ContentMode::Preview => {
                self.open.as_ref().map(|open| open.caret.line + 1)
            }
            ContentMode::Diff => self
                .changes
                .diff
                .get(self.scroll as usize)
                .map(|cell| cell.line_no as usize),
        }
    }

    /// The diff row that mentions `line` on the new side. A removal's
    /// number is the old file's, so it never answers for the new one.
    fn diff_row_of(&self, line: usize) -> Option<u16> {
        self.changes
            .diff
            .iter()
            .position(|cell| cell.kind != DiffLineKind::Removed && cell.line_no as usize == line)
            .map(|row| row as u16)
    }

    /// Opens the directories containing the selection, so the tree can
    /// show a file it has never listed. The selection is authoritative;
    /// the tree catches up to it.
    fn reveal_selection(&mut self) {
        let Some(selected) = self.selected.clone() else {
            return;
        };
        let mut directories = Vec::new();
        let mut walk = selected.parent();
        while let Some(directory) = walk {
            directories.push(directory.to_path_buf());
            if directory == self.root {
                break;
            }
            walk = directory.parent();
        }
        for directory in directories.into_iter().rev() {
            self.expand(directory);
        }
    }

    /// Reads the selected file, putting the caret on `line` when it
    /// lands.
    fn load_selection(&mut self, line: Option<usize>) {
        let Some(path) = self.selected.clone() else {
            return;
        };
        if self.open.as_ref().is_some_and(|open| open.path == path) {
            if let Some(line) = line
                && let Some(open) = self.open.as_mut()
            {
                open.place_caret(line.saturating_sub(1));
            }
            return;
        }
        let mut open = OpenFile::opening(path.clone());
        open.caret = Caret {
            line: line.unwrap_or(1).saturating_sub(1),
            column: 0,
        };
        self.open = Some(open);
        self.scroll = 0;
        self.queue.push_back(FileRequest::Read(path));
    }

    /// Opens a directory, reading it the first time it is opened.
    fn expand(&mut self, path: PathBuf) {
        // Neither read nor already asked for: a directory opened twice
        // before the first answer lands — which is what resuming a place
        // does to the root — is one read, not two.
        let asked = self
            .queue
            .iter()
            .any(|request| matches!(request, FileRequest::List(queued) if *queued == path));
        if !asked && !self.files.listings.contains_key(&path) {
            self.queue.push_back(FileRequest::List(path.clone()));
        }
        self.files.expanded.insert(path);
    }

    /// Moves the selection one row in whichever list is showing.
    fn step(&mut self, direction: ScrollDirection) {
        match self.navigator() {
            NavigatorMode::Changes => {
                let Some(from) = self.selected_change() else {
                    if let Some(first) = self.changes.files.first().map(|file| file.path.clone()) {
                        self.select(first);
                    }
                    return;
                };
                if let Some(index) = self.changes.neighbour(&self.root, from, direction)
                    && let Some(file) = self.changes.files.get(index)
                {
                    let path = file.path.clone();
                    self.select(path);
                }
            }
            NavigatorMode::Files => {
                let rows = self.files.rows(&self.root);
                if rows.is_empty() {
                    return;
                }
                let current = self
                    .selected
                    .as_ref()
                    .and_then(|path| rows.iter().position(|row| &row.path == path));
                let next = match (current, direction) {
                    (Some(index), ScrollDirection::Up) => index.saturating_sub(1),
                    (Some(index), ScrollDirection::Down) => (index + 1).min(rows.len() - 1),
                    (None, _) => 0,
                };
                // A tree row may be a directory, which nothing else in
                // this surface can be selected on — moving to it is still
                // right, it just has no diff and no contents.
                let row = &rows[next];
                if row.directory {
                    self.selected = Some(row.path.clone());
                    self.read_selection_as_what_it_is();
                } else {
                    self.select(row.path.clone());
                }
            }
        }
    }

    fn save(&mut self) {
        let Some(open) = self.open.as_ref().filter(|open| open.error.is_none()) else {
            return;
        };
        if !open.modified {
            self.notice = Some("no changes to save".to_owned());
            return;
        }
        self.queue.push_back(FileRequest::Save {
            path: open.path.clone(),
            contents: open.contents(),
        });
    }

    /// Keeps the caret on screen after it moved, given how many lines the
    /// host says fit.
    fn follow_caret(&mut self, visible: u16) {
        let Some(open) = self.open.as_ref().filter(|open| open.editing) else {
            return;
        };
        let line = open.caret.line as u16;
        let visible = visible.max(1);
        if line < self.scroll {
            self.scroll = line;
        } else if line >= self.scroll + visible {
            self.scroll = line.saturating_sub(visible - 1);
        }
    }
}

/// One answered refresh of the changes half, with the placement it was
/// read for — so an answer describing where the viewer *was* can be told
/// from one describing where they *are*.
pub struct RefreshedChanges {
    placement: ViewPlacement,
    branch: String,
    changes: Changes,
}

impl RefreshedChanges {
    /// What a refresh that never ran answers.
    ///
    /// The host reserves the surface against a second refresh while one
    /// is out and releases it when the answer lands, so a read that ends
    /// without answering — a thread that unwound over whatever the
    /// repository happened to contain — would leave the changes half
    /// frozen for the rest of the session. This is the same shape a
    /// checkout that is no repository already produces, which the view
    /// draws as the reason where the diff would be.
    pub fn failed(placement: ViewPlacement, reason: String) -> Self {
        Self {
            placement,
            branch: String::new(),
            changes: Changes::failed(reason),
        }
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// The branch the checkout is on, for the title. Answers `detached HEAD`
/// for a checkout with no branch, and nothing at all outside a
/// repository.
fn current_branch(host: &dyn Host, root: &Path) -> String {
    match host.git(root, &["rev-parse", "--abbrev-ref", "HEAD"], &[]) {
        Ok(name) if !name.trim().is_empty() && name.trim() != "HEAD" => name.trim().to_owned(),
        Ok(_) => "detached HEAD".to_owned(),
        Err(_) => String::new(),
    }
}

/// One command reaching an open [`CodeView`].
///
/// A command, never a key: which chord reaches this is the host's, the
/// same way which colour a role becomes is. `space` is how much room the
/// content column has, which the host knows and this does not — used for
/// nothing but keeping the caret and the page keys inside the file.
pub fn handle_command(view: &mut CodeView, command: Command, space: Size) -> CodeOutcome {
    // Typing is modal, and the host says so by asking in the editing
    // scope. What is left here is the same command set the reading modes
    // answer, plus the two that leave typing.
    if view.editing() {
        let outcome = edit_command(view, command);
        view.follow_caret(space.height);
        return outcome;
    }

    if let Some(path) = view.confirming_delete.clone() {
        view.confirming_delete = None;
        if command == Command::ConfirmDelete {
            view.queue.push_back(FileRequest::Delete(path));
        } else {
            view.notice = Some("delete cancelled".to_owned());
        }
        return CodeOutcome::Stay;
    }

    match command {
        Command::Close => {
            // Unsaved work is never closed away on one keystroke. The
            // second press is the answer to a question the footer is now
            // asking — so the flag is read here, before anything clears
            // it, or the question is asked forever and never answered.
            if view.open.as_ref().is_some_and(|open| open.modified) && !view.confirming_discard {
                view.confirming_discard = true;
                return CodeOutcome::Stay;
            }
            return CodeOutcome::Close;
        }
        // Anything that is not an answer to the question withdraws it.
        _ => view.confirming_discard = false,
    }

    match command {
        Command::FocusNext => {
            view.focus = match view.focus {
                Focus::Navigator => Focus::Content,
                Focus::Content => Focus::Navigator,
            };
        }
        Command::SelectPrevious => match view.focus {
            Focus::Navigator => view.step(ScrollDirection::Up),
            Focus::Content => view.scroll = view.scroll.saturating_sub(1),
        },
        Command::SelectNext => match view.focus {
            Focus::Navigator => view.step(ScrollDirection::Down),
            Focus::Content => view.scroll = view.scroll.saturating_add(1),
        },
        Command::Collapse if view.focus == Focus::Navigator => match view.navigator() {
            NavigatorMode::Changes => {
                if let Some(selected) = view.selected_change() {
                    view.changes.fold(&view.root, selected);
                }
            }
            NavigatorMode::Files => fold_tree_row(view),
        },
        Command::Expand if view.focus == Focus::Navigator => match view.navigator() {
            NavigatorMode::Changes => {
                if let Some(selected) = view.selected_change() {
                    view.changes.unfold(&view.root, selected);
                }
            }
            NavigatorMode::Files => {
                if let Some(row) = view
                    .selected
                    .clone()
                    .and_then(|path| view.files.row_at(&view.root, &path))
                    .filter(|row| row.directory)
                {
                    view.expand(row.path);
                }
            }
        },
        Command::Activate if view.focus == Focus::Navigator => activate_selection(view),
        Command::ScrollPageUp => view.scroll = view.scroll.saturating_sub(space.height.max(1)),
        Command::ScrollPageDown => view.scroll = view.scroll.saturating_add(space.height.max(1)),
        // The move the whole surface is for: from a line of the diff into
        // that line of the file, ready to change it.
        Command::Edit => {
            view.show(ContentMode::Contents);
            if let Some(open) = view
                .open
                .as_mut()
                .filter(|open| open.error.is_none() && !open.loading)
            {
                open.editing = true;
                view.focus = Focus::Content;
            }
        }
        // Reading a document and editing it are two modes of the same
        // file, so this toggles rather than opening anything.
        Command::TogglePreview if view.selected_is_markdown() => {
            view.show(match view.content {
                ContentMode::Preview => ContentMode::Contents,
                _ => ContentMode::Preview,
            });
        }
        Command::Delete => {
            match view.selected.clone().filter(|path| {
                !view
                    .files
                    .row_at(&view.root, path)
                    .is_some_and(|row| row.directory)
            }) {
                Some(path) => view.confirming_delete = Some(path),
                None => view.notice = Some("only files are deletable".to_owned()),
            }
        }
        _ => {}
    }
    CodeOutcome::Stay
}

/// A command while the buffer is being typed into.
fn edit_command(view: &mut CodeView, command: Command) -> CodeOutcome {
    if command == Command::Save {
        view.save();
        return CodeOutcome::Stay;
    }
    let Some(open) = view.open.as_mut() else {
        return CodeOutcome::Stay;
    };
    match command {
        // Leaves typing, not the file: the buffer and everything unsaved
        // in it stay exactly as they are, and the next Close is the one
        // that asks about leaving.
        Command::Close => open.editing = false,
        Command::Newline => open.split_line(),
        Command::EraseBack => open.backspace(),
        Command::EraseForward => open.delete_forward(),
        Command::Type(character) => open.insert(character),
        Command::CaretLeft
        | Command::CaretRight
        | Command::SelectPrevious
        | Command::SelectNext
        | Command::CaretLineStart
        | Command::CaretLineEnd => open.move_caret(command),
        _ => {}
    }
    CodeOutcome::Stay
}

/// Left in the tree: fold the selected directory, or step out to the one
/// holding the selected file — what Left means in every tree a person has
/// used before this one.
fn fold_tree_row(view: &mut CodeView) {
    let Some(row) = view
        .selected
        .clone()
        .and_then(|path| view.files.row_at(&view.root, &path))
    else {
        return;
    };
    if row.directory && row.expanded {
        view.files.expanded.remove(&row.path);
    } else if let Some(parent) = row.path.parent().filter(|parent| *parent != view.root) {
        view.selected = Some(parent.to_path_buf());
    }
}

/// Activate on a navigator row: a directory folds or unfolds, a file
/// becomes the selection and the content follows.
fn activate_selection(view: &mut CodeView) {
    match view.navigator() {
        NavigatorMode::Changes => {
            if view.selected.is_some() {
                view.focus = Focus::Content;
            }
        }
        NavigatorMode::Files => {
            let Some(row) = view
                .selected
                .clone()
                .and_then(|path| view.files.row_at(&view.root, &path))
            else {
                return;
            };
            if row.directory {
                if row.expanded {
                    view.files.expanded.remove(&row.path);
                } else {
                    view.expand(row.path);
                }
                return;
            }
            view.select(row.path);
            view.load_selection(None);
            view.focus = Focus::Content;
        }
    }
}

pub fn handle_mouse(view: &mut CodeView, hit: Option<ViewHit>) -> CodeOutcome {
    match hit {
        Some(ViewHit::SelectItem(index)) => match view.navigator() {
            NavigatorMode::Changes => {
                if let Some(file) = view.changes.files.get(index) {
                    let path = file.path.clone();
                    view.select(path);
                }
            }
            NavigatorMode::Files => {
                if let Some(row) = view.files.rows(&view.root).into_iter().nth(index) {
                    view.select(row.path);
                    view.load_selection(None);
                    view.focus = Focus::Content;
                }
            }
        },
        Some(ViewHit::ToggleGroup(row)) => match view.navigator() {
            NavigatorMode::Changes => {
                // The id handed back is the row's place in the tree this
                // view last described — rebuilt here from the same state,
                // so it names the same directory.
                if let Some(FileTreeItem::Directory { path, .. }) =
                    file_tree_items(&view.changes, &view.root)
                        .into_iter()
                        .nth(row)
                {
                    view.changes.toggle_directory(path);
                }
            }
            NavigatorMode::Files => {
                if let Some(row) = view.files.rows(&view.root).into_iter().nth(row) {
                    view.selected = Some(row.path.clone());
                    if row.expanded {
                        view.files.expanded.remove(&row.path);
                    } else {
                        view.expand(row.path);
                    }
                }
            }
        },
        Some(ViewHit::PlaceCaret { line, cell }) => {
            view.focus = Focus::Content;
            if view.content == ContentMode::Contents
                && let Some(open) = view.open.as_mut()
            {
                let line = line.min(open.lines.len().saturating_sub(1));
                open.caret = Caret {
                    line,
                    column: open.column_at_cell(line, cell),
                };
            }
        }
        // The order is `render::modes`' own, and it is the only side
        // that knows it — which is why the hit carries an index rather
        // than a mode the host would have to name.
        Some(ViewHit::SelectMode(index)) => {
            if let Some(mode) = [ContentMode::Preview, ContentMode::Contents]
                .get(index)
                .copied()
            {
                view.show(mode);
            }
        }
        Some(ViewHit::Close) => return CodeOutcome::Close,
        _ => {}
    }
    CodeOutcome::Stay
}

/// Puts the content at `first`, which is what a scrollbar dragged to a
/// position means. Absolute, unlike the wheel: a handle taken hold of
/// says *where*, not *how far*.
pub fn scroll_to(view: &mut CodeView, first: usize) {
    view.scroll = first.min(u16::MAX as usize) as u16;
}

/// Mouse-wheel over the content of an open [`CodeView`].
///
/// Only the content: the wheel over a list scrolls that list, and how far
/// a list of rows can scroll is a question about how many fit, which the
/// host answers because the host laid them out.
pub fn handle_scroll(view: &mut CodeView, direction: ScrollDirection) {
    view.scroll = match direction {
        ScrollDirection::Up => view.scroll.saturating_sub(3),
        ScrollDirection::Down => view.scroll.saturating_add(3),
    };
}

#[cfg(test)]
mod tests;
