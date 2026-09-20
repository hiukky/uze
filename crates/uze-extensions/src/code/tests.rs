//! What the code surface must keep doing.
//!
//! Two kinds here, deliberately mixed: fixtures that build a surface in
//! memory and prove what it *describes*, and a handful that drive real
//! `git` in a scratch repository, because the porcelain this parses is a
//! contract with a program nobody here controls.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use super::{
    changes::{ChangedFile, Changes, FileStatus},
    diff,
    render::changes_navigator as navigator,
    *,
};
use crate::{
    DirEntry,
    code::highlight::FALLBACK_SYNTAX_THEME,
    view::{Command, Content, LineTone, NavigatorRow, Role, RowIcon, RowMark, Size, Span},
};

fn space() -> Size {
    Size {
        width: 80,
        height: 20,
    }
}

/// One command, the way the host hands one down.
fn press(view: &mut CodeView, command: Command) -> CodeOutcome {
    handle_command(view, command, space())
}

/// Types a string, one character at a time, the way `Resolution::Text`
/// reaches the surface.
fn type_text(view: &mut CodeView, text: &str) {
    for character in text.chars() {
        press(view, Command::Type(character));
    }
}

/// The two halves in memory, with no host and no disk — what most of
/// these are about.
fn surface(root: &Path, files: Vec<ChangedFile>, selected: usize) -> CodeView {
    let selected_path = files.get(selected).map(|file| file.path.clone());
    CodeView {
        branch: "main".to_owned(),
        selected: selected_path,
        changes: Changes {
            files,
            ..Changes::default()
        },
        ..CodeView::opening(
            root.to_path_buf(),
            root.display().to_string(),
            ContentMode::Diff,
        )
    }
}

/// Two changed files, opened on the second, with a diff already read.
fn fixture() -> CodeView {
    let root = PathBuf::from("/repo");
    let mut view = surface(
        &root,
        vec![
            ChangedFile {
                status: FileStatus::Modified,
                path: root.join("src/ui/git_diff.rs"),
            },
            ChangedFile {
                status: FileStatus::Added,
                path: root.join("src/ui.rs"),
            },
        ],
        1,
    );
    view.changes.diff = diff::read(
        "@@ -1,3 +1,4 @@\n context\n-removed line\n+added line\n",
        Path::new("/repo/src/ui.rs"),
        FALLBACK_SYNTAX_THEME,
    );
    view.changes.diff_pending = false;
    view
}

/// Three files under two directories, opened on the deepest one.
fn tree_fixture() -> CodeView {
    let root = PathBuf::from("/repo");
    let mut view = surface(
        &root,
        vec![
            ChangedFile {
                status: FileStatus::Modified,
                path: root.join("src/ui/git_diff.rs"),
            },
            ChangedFile {
                status: FileStatus::Added,
                path: root.join("src/ui.rs"),
            },
            ChangedFile {
                status: FileStatus::Untracked,
                path: root.join("README.md"),
            },
        ],
        0,
    );
    view.changes.diff_pending = false;
    view
}

fn group_names(view: &CodeView) -> Vec<(String, bool)> {
    navigator(view)
        .rows
        .into_iter()
        .filter_map(|row| match row {
            NavigatorRow::Group {
                name, collapsed, ..
            } => Some((name, collapsed)),
            NavigatorRow::Item { .. } => None,
        })
        .collect()
}

fn item_names(view: &CodeView) -> Vec<String> {
    navigator(view)
        .rows
        .into_iter()
        .filter_map(|row| match row {
            NavigatorRow::Item { name, .. } => Some(name),
            NavigatorRow::Group { .. } => None,
        })
        .collect()
}

/// The view carries meaning, never appearance: a status mark is a
/// [`Role`], not a colour, so the host's palette stays the only place
/// chrome colour is decided.
#[test]
fn the_view_names_meaning_rather_than_colour() {
    let view = view(&fixture(), space());
    let navigator = view.navigator.expect("files to navigate");
    assert_eq!(navigator.badge, "2");
    assert!(navigator.focused);

    let marker = navigator
        .rows
        .iter()
        .find_map(|row| match row {
            NavigatorRow::Item { name, marker, .. } if name == "ui.rs" => Some(marker),
            _ => None,
        })
        .expect("the added file is listed");
    assert_eq!(marker.role, Role::Success, "added, not a colour");
    assert!(
        marker.color.is_none(),
        "chrome never carries its own colour"
    );
}

/// Syntax colour is the one thing that does travel as data: it comes
/// from a theme the extension ships, and a role would throw it away.
#[test]
fn syntax_colour_survives_as_the_extensions_own_data() {
    let view = view(&fixture(), space());
    let Content::Lines { lines, .. } = view.content else {
        panic!("a selected file has a diff");
    };
    assert!(lines.iter().any(|line| line.tone == LineTone::Added));
    assert!(lines.iter().any(|line| line.tone == LineTone::Removed));
    assert!(
        lines
            .iter()
            .flat_map(|line| &line.spans)
            .any(|span| span.color.is_some()),
        "highlighting reaches the host"
    );
}

/// Outside a git repository the *changes* have nothing to say. The
/// surface does not: the tree still works, which is why that failure is
/// content rather than the whole view.
#[test]
fn a_checkout_that_is_no_repository_still_has_a_tree() {
    let mut view = CodeView::opening(
        PathBuf::from("/nope"),
        "/nope".to_owned(),
        ContentMode::Diff,
    );
    view.changes.error = Some("not a git repository".to_owned());
    view.changes.diff_pending = false;

    let rendered = super::view(&view, space());
    assert!(
        rendered.navigator.is_some(),
        "the file tree is unaffected by git being absent"
    );
    assert!(matches!(
        rendered.content,
        Content::Message {
            role: Role::Danger,
            ..
        }
    ));
}

/// Moving the selection reads nothing — the property that keeps an arrow
/// key off the thread that draws.
#[test]
fn selecting_a_file_asks_for_its_diff_rather_than_reading_it() {
    let mut open = fixture();
    assert!(!open.diff_pending(), "a fresh surface shows what it read");

    let outcome = press(&mut open, Command::SelectPrevious);

    assert!(matches!(outcome, CodeOutcome::Stay));
    assert_eq!(open.selected_change(), Some(0), "the selection moved");
    assert!(open.diff_pending(), "and a read is owed for it");
    assert!(
        open.changes.diff.is_empty(),
        "the previous file's diff is not left under the new name"
    );

    assert!(
        matches!(&view(&open, space()).content, Content::Message { text, .. } if text == "reading…"),
    );
}

/// A folded directory keeps its files off the list and the selection
/// where it was: the diff being read is not changed by tidying the tree
/// around it.
#[test]
fn folding_a_directory_hides_its_files_and_moves_nothing_else() {
    let mut view = tree_fixture();
    let ui_group = navigator(&view)
        .rows
        .iter()
        .position(|row| matches!(row, NavigatorRow::Group { name, .. } if name == "ui/"))
        .expect("the ui directory is a group");

    handle_mouse(&mut view, Some(ViewHit::ToggleGroup(ui_group)), space());

    assert_eq!(item_names(&view), vec!["ui.rs", "README.md"]);
    assert_eq!(
        group_names(&view),
        vec![("src/".to_owned(), false), ("ui/".to_owned(), true)]
    );
    assert_eq!(
        view.selected_change(),
        Some(0),
        "the selection is hidden, not moved"
    );
    assert!(!view.diff_pending(), "and its diff was not re-read");
    assert_eq!(
        navigator(&view).anchor,
        None,
        "nothing to keep on screen while the selection is folded away"
    );

    handle_mouse(&mut view, Some(ViewHit::ToggleGroup(ui_group)), space());
    assert_eq!(item_names(&view), vec!["git_diff.rs", "ui.rs", "README.md"]);
    assert_eq!(navigator(&view).anchor, Some(2));
}

/// The arrows walk the tree as drawn — directories first, folded ones
/// skipped — rather than the flat order `git status` answered in.
#[test]
fn the_arrows_walk_the_tree_as_drawn_and_step_over_a_fold() {
    let mut view = tree_fixture();

    press(&mut view, Command::SelectNext);
    assert_eq!(
        view.selected_change(),
        Some(1),
        "src/ui.rs follows src/ui/git_diff.rs"
    );
    press(&mut view, Command::SelectNext);
    assert_eq!(
        view.selected_change(),
        Some(2),
        "README.md is last, under every directory"
    );
    press(&mut view, Command::SelectNext);
    assert_eq!(view.selected_change(), Some(2), "the last row holds");

    view.changes.folded.insert("src".to_owned());
    press(&mut view, Command::SelectPrevious);
    assert_eq!(
        view.selected_change(),
        Some(2),
        "nothing above README.md is showing"
    );

    view.changes.folded.clear();
    view.changes.folded.insert("src/ui".to_owned());
    press(&mut view, Command::SelectPrevious);
    assert_eq!(
        view.selected_change(),
        Some(1),
        "the folded file is stepped over"
    );
    press(&mut view, Command::SelectPrevious);
    assert_eq!(
        view.selected_change(),
        Some(1),
        "and the fold is the top of what shows"
    );
}

/// Left closes the tree outward from the selection, Right opens it back
/// inward, one directory at a time.
#[test]
fn left_folds_outward_and_right_unfolds_inward() {
    let mut view = tree_fixture();

    press(&mut view, Command::Collapse);
    assert_eq!(view.changes.folded, BTreeSet::from(["src/ui".to_owned()]));
    press(&mut view, Command::Collapse);
    assert_eq!(
        view.changes.folded,
        BTreeSet::from(["src".to_owned(), "src/ui".to_owned()])
    );
    assert_eq!(item_names(&view), vec!["README.md"]);
    press(&mut view, Command::Collapse);
    assert_eq!(
        view.changes.folded.len(),
        2,
        "nothing above the root to fold"
    );

    press(&mut view, Command::Expand);
    assert_eq!(view.changes.folded, BTreeSet::from(["src/ui".to_owned()]));
    press(&mut view, Command::Expand);
    assert!(view.changes.folded.is_empty());
    assert_eq!(
        view.selected_change(),
        Some(0),
        "folding never moved the selection"
    );
}

/// The wheel over the content scrolls the content and nothing else: the
/// selection is not a scroll position.
#[test]
fn the_wheel_scrolls_the_content_and_leaves_the_selection_alone() {
    let mut view = fixture();

    handle_scroll(&mut view, ScrollDirection::Down);

    assert_eq!(view.scroll, 3);
    assert_eq!(view.selected_change(), Some(1));
    assert!(!view.diff_pending());
}

/// A refresh answers what changed. Tidying the tree around it is the
/// viewer's, and not something the answer gets to undo.
#[test]
fn a_refresh_keeps_the_folds_the_viewer_made() {
    let mut view = tree_fixture();
    view.changes.folded.insert("src/ui".to_owned());

    view.absorb_changes(RefreshedChanges {
        placement: view.placement(),
        branch: "main".to_owned(),
        changes: Changes {
            files: view.changes.files.clone(),
            ..Changes::default()
        },
    });

    assert_eq!(view.changes.folded, BTreeSet::from(["src/ui".to_owned()]));
}

/// The section names meaning, never colour — the same contract the
/// full-frame view is held to.
#[test]
fn the_timeline_section_names_meaning_rather_than_colour() {
    let timeline = Timeline {
        branch: "agent/x".to_owned(),
        commits: vec![
            Commit {
                hash: "a".to_owned(),
                subject: "feat: ahead".to_owned(),
                age: "3h".to_owned(),
                ahead: true,
            },
            Commit {
                hash: "b".to_owned(),
                subject: "chore: landed".to_owned(),
                age: "2d".to_owned(),
                ahead: false,
            },
        ],
    };

    let section = timeline_section(&timeline, false, 0);

    assert_eq!(section.title, "timeline");
    assert_eq!(section.caption.text, "agent/x");
    assert!(section.resizable);
    // HEAD is ringed; standing is the hue, and the hue is a role.
    assert_eq!(section.rows[0].mark, RowMark::Head);
    assert_eq!(section.rows[0].mark_role, Role::Info);
    assert_eq!(section.rows[1].mark, RowMark::Commit);
    assert_eq!(section.rows[1].mark_role, Role::Warning);
    assert_eq!(section.rows[0].trailing.text, "3h");

    let folded = timeline_section(&timeline, true, 0);
    assert!(folded.collapsed);
    assert_eq!(
        folded.rows.len(),
        section.rows.len(),
        "folding is the host's to draw; the section still holds what it holds"
    );
}

// --- the files half, and the switch between them -------------------------

/// The same grant the workspace client makes, for the tests that drive real
/// `git` in a scratch repository. A fake is the right tool for testing *the
/// view*; these test what the view reads.
pub(super) struct RepositoryHost;

impl Host for RepositoryHost {
    fn git(&self, root: &Path, args: &[&str], answers: &[i32]) -> Result<String, String> {
        uze_git::read(root, args)
            .map_err(|error| error.to_string())?
            .or_exit(answers)
    }

    fn repository_root(&self, path: &Path) -> Result<PathBuf, String> {
        uze_git::repository::root(path)
    }

    fn read_file(&self, path: &Path) -> Result<String, String> {
        std::fs::read_to_string(path).map_err(|error| error.to_string())
    }

    fn syntax_theme(&self) -> String {
        FALLBACK_SYNTAX_THEME.to_owned()
    }

    fn list_dir(&self, _path: &Path) -> Result<Vec<DirEntry>, String> {
        unreachable!("what the repository tests read, they read through `git`")
    }

    fn write_file(&self, _path: &Path, _contents: &str) -> Result<(), String> {
        unreachable!("reading a repository writes nothing")
    }

    fn delete_file(&self, _path: &Path) -> Result<(), String> {
        unreachable!("reading a repository deletes nothing")
    }
}

/// A repository that answers the same thing every time, and counts how
/// often it was asked to diff.
struct StillRepository {
    diffs: std::cell::Cell<usize>,
}

impl Host for StillRepository {
    fn git(&self, _root: &Path, args: &[&str], _answers: &[i32]) -> Result<String, String> {
        match args.first() {
            Some(&"status") => Ok(" M src/ui.rs\n".to_owned()),
            Some(&"diff") => {
                self.diffs.set(self.diffs.get() + 1);
                Ok("@@ -1,2 +1,2 @@\n context\n-old line\n+new line\n".to_owned())
            }
            _ => Ok("main\n".to_owned()),
        }
    }

    fn repository_root(&self, path: &Path) -> Result<PathBuf, String> {
        Ok(path.to_path_buf())
    }

    fn read_file(&self, _path: &Path) -> Result<String, String> {
        Err("nothing is read here".to_owned())
    }

    fn list_dir(&self, _path: &Path) -> Result<Vec<DirEntry>, String> {
        Ok(Vec::new())
    }

    fn write_file(&self, _path: &Path, _contents: &str) -> Result<(), String> {
        unreachable!("reading a repository writes nothing")
    }

    fn delete_file(&self, _path: &Path) -> Result<(), String> {
        unreachable!("reading a repository deletes nothing")
    }

    fn syntax_theme(&self) -> String {
        FALLBACK_SYNTAX_THEME.to_owned()
    }
}

/// The changes are re-read on a timer, because the checkout is under an
/// agent's hands. Colouring a diff that did not move is the same picture
/// drawn twice — the one unbounded cost this surface pays over and over
/// if nothing notices.
#[test]
fn a_diff_that_has_not_moved_is_not_coloured_again() {
    let host = StillRepository {
        diffs: std::cell::Cell::new(0),
    };
    let root = PathBuf::from("/repo");
    let mut view = CodeView::opening(root.clone(), "/repo".to_owned(), ContentMode::Diff);

    // The first read has no selection yet, so there is no diff to ask
    // about; it is what lands the selection.
    let first = CodeView::refresh(&host, root.clone(), view.placement());
    assert!(!first.changes.diff_unchanged);
    view.absorb_changes(first);
    let second = CodeView::refresh(&host, root.clone(), view.placement());
    assert!(
        !second.changes.diff_unchanged,
        "the selected file's own diff"
    );
    view.absorb_changes(second);
    let coloured = view.changes.diff.len();
    assert!(coloured > 0, "there is a diff on screen");

    let third = CodeView::refresh(&host, root.clone(), view.placement());
    assert!(
        third.changes.diff_unchanged,
        "the same bytes in the same palette are the same picture"
    );
    assert!(third.changes.diff.is_empty(), "so none of it travels back");
    view.absorb_changes(third);
    assert_eq!(
        view.changes.diff.len(),
        coloured,
        "and what is on screen stays on screen"
    );
    assert_eq!(
        host.diffs.get(),
        2,
        "git is still asked every time — only the colouring is skipped"
    );
}

/// A filesystem that only ever existed in memory, so these prove the
/// surface's own behaviour rather than a temp directory's.
#[derive(Default)]
struct FakeMachine {
    directories: BTreeMap<PathBuf, Vec<DirEntry>>,
    files: RefCell<BTreeMap<PathBuf, String>>,
}

impl FakeMachine {
    fn with_file(mut self, path: &str, contents: &str) -> Self {
        let path = PathBuf::from(path);
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let parent = path.parent().unwrap().to_path_buf();
        self.directories.entry(parent).or_default().push(DirEntry {
            directory: false,
            name,
        });
        self.files.borrow_mut().insert(path, contents.to_owned());
        self
    }

    fn with_directory(mut self, path: &str) -> Self {
        let path = PathBuf::from(path);
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let parent = path.parent().unwrap().to_path_buf();
        self.directories.entry(parent).or_default().push(DirEntry {
            directory: true,
            name,
        });
        self.directories.entry(path).or_default();
        self
    }
}

impl Host for FakeMachine {
    fn git(&self, _root: &Path, _args: &[&str], _answers: &[i32]) -> Result<String, String> {
        Err("no git here".to_owned())
    }

    fn repository_root(&self, _path: &Path) -> Result<PathBuf, String> {
        Err("no git here".to_owned())
    }

    fn read_file(&self, path: &Path) -> Result<String, String> {
        self.files
            .borrow()
            .get(path)
            .cloned()
            .ok_or_else(|| "not readable as text".to_owned())
    }

    fn list_dir(&self, path: &Path) -> Result<Vec<DirEntry>, String> {
        self.directories
            .get(path)
            .cloned()
            .ok_or_else(|| format!("no such directory: {}", path.display()))
    }

    fn write_file(&self, path: &Path, contents: &str) -> Result<(), String> {
        self.files
            .borrow_mut()
            .insert(path.to_path_buf(), contents.to_owned());
        Ok(())
    }

    fn delete_file(&self, path: &Path) -> Result<(), String> {
        self.files.borrow_mut().remove(path);
        Ok(())
    }

    fn syntax_theme(&self) -> String {
        FALLBACK_SYNTAX_THEME.to_owned()
    }
}

/// Drains the surface's file requests against `machine` until it stops
/// asking — what the host's background thread does, minus the thread.
fn settle(view: &mut CodeView, machine: &FakeMachine) {
    let mut guard = 0;
    while let Some(request) = view.take_request() {
        view.absorb(fulfill(machine, request));
        guard += 1;
        assert!(guard < 64, "the surface never stopped asking for things");
    }
}

fn files_at(root: &str) -> CodeView {
    CodeView::opening(PathBuf::from(root), root.to_owned(), ContentMode::Contents)
}

#[test]
fn a_directory_is_read_only_when_it_is_opened() {
    let machine = FakeMachine::default()
        .with_directory("/w/src")
        .with_file("/w/src/main.rs", "fn main() {}\n")
        .with_file("/w/README.md", "# hi\n");
    let mut view = files_at("/w");
    settle(&mut view, &machine);

    assert_eq!(item_names_of_tree(&view), ["src", "README.md"]);

    press(&mut view, Command::Expand);
    settle(&mut view, &machine);
    assert_eq!(item_names_of_tree(&view), ["src", "main.rs", "README.md"]);
}

fn item_names_of_tree(view: &CodeView) -> Vec<String> {
    view.files
        .rows(&view.root)
        .into_iter()
        .map(|row| row.name)
        .collect()
}

/// A file is on screen before all of it is coloured, and colouring the
/// rest never changes a character of it.
///
/// The whole point of the two passes: colouring costs per line, a screen
/// does not, and a file that has to be coloured to the end before it can
/// be read is a file that opens in half a second.
#[test]
fn a_long_file_is_readable_before_all_of_it_is_coloured() {
    let long: String = (1..=900).map(|n| format!("fn line_{n}() {{}}\n")).collect();
    let machine = FakeMachine::default().with_file("/w/long.rs", &long);
    let mut view = files_at("/w");
    settle(&mut view, &machine);
    press(&mut view, Command::Activate);

    // Everything up to, but not including, the pass that colours the
    // rest: this is the state the first frame draws.
    let mut asked = Vec::new();
    while let Some(request) = view.take_request() {
        asked.push(request.clone());
        if matches!(request, FileRequest::Colour(_)) {
            break;
        }
        view.absorb(fulfill(&machine, request));
    }
    assert_eq!(
        asked.last(),
        Some(&FileRequest::Colour(PathBuf::from("/w/long.rs"))),
        "the rest of the colour is asked for once the file is shown"
    );

    let open = view.open.as_ref().expect("the first row is read");
    assert_eq!(open.lines.len(), 900, "every line is there to read");
    assert!(
        !open.highlighted[0].is_empty(),
        "what the viewer is looking at is coloured"
    );
    assert!(
        open.highlighted[899].is_empty(),
        "the far end of the file is not, yet"
    );
    let Content::Lines { lines, .. } = super::view(&view, space()).content else {
        panic!("an open file is lines");
    };
    assert!(
        lines.iter().all(|line| !line.spans.is_empty()),
        "an uncoloured line is still drawn as the text it is"
    );

    view.absorb(fulfill(
        &machine,
        FileRequest::Colour(PathBuf::from("/w/long.rs")),
    ));
    let open = view.open.as_ref().expect("still open");
    assert!(
        open.highlighted.iter().all(|spans| !spans.is_empty()),
        "the second pass colours the whole file"
    );
    assert_eq!(open.lines.len(), 900, "and changes none of its text");
    assert_eq!(open.lines[899], "fn line_900() {}");
}

/// A document opens as the document it is, and its markup is never
/// coloured for a screen nobody asked for — until somebody asks for the
/// source, which is when that work becomes worth doing.
#[test]
fn a_documents_markup_is_coloured_only_once_the_source_is_asked_for() {
    let machine = FakeMachine::default().with_file("/w/README.md", "# hi\n\n`code`\n");
    let mut view = files_at("/w");
    settle(&mut view, &machine);
    press(&mut view, Command::Activate);
    settle(&mut view, &machine);

    assert_eq!(
        view.content,
        ContentMode::Preview,
        "a document reads as one"
    );
    let open = view.open.as_ref().expect("the document is read");
    assert!(
        open.highlighted.iter().all(|spans| spans.is_empty()),
        "nothing coloured the markup"
    );

    view.show(ContentMode::Contents);
    settle(&mut view, &machine);
    let open = view.open.as_ref().expect("still open");
    assert!(
        open.highlighted.iter().any(|spans| !spans.is_empty()),
        "asking for the source is what pays for colouring it"
    );
}

/// The content the host is handed is a window that starts where the
/// viewer is, and says where that is — so drawing the end of a long file
/// costs the end of it rather than all of it.
#[test]
fn the_content_window_starts_where_the_viewer_is() {
    let long: String = (1..=900).map(|n| format!("line {n}\n")).collect();
    let machine = FakeMachine::default().with_file("/w/long.rs", &long);
    let mut view = files_at("/w");
    settle(&mut view, &machine);
    press(&mut view, Command::Activate);
    settle(&mut view, &machine);
    scroll_to(&mut view, 500);

    let Content::Lines {
        first,
        lines,
        total,
        scroll,
        ..
    } = super::view(&view, space()).content
    else {
        panic!("an open file is lines");
    };
    assert_eq!(total, 900, "the whole file is still what it is");
    assert_eq!(first, 500);
    assert_eq!(scroll, 500, "and the host still applies it");
    assert_eq!(lines.first().map(|line| line.number.as_str()), Some("501"));
    assert!(
        lines.len() <= usize::from(space().height) * 2,
        "a window, not a file: {} lines",
        lines.len()
    );
}

#[test]
fn editing_a_file_and_saving_it_writes_what_was_typed() {
    let machine = FakeMachine::default().with_file("/w/notes.txt", "one\n");
    let mut view = files_at("/w");
    settle(&mut view, &machine);

    press(&mut view, Command::Activate);
    settle(&mut view, &machine);
    press(&mut view, Command::Edit);
    press(&mut view, Command::CaretLineEnd);
    type_text(&mut view, "!");
    assert!(
        view.open.as_ref().is_some_and(|open| open.modified),
        "typing marks the buffer unsaved"
    );

    press(&mut view, Command::Save);
    settle(&mut view, &machine);

    assert_eq!(machine.files.borrow()[Path::new("/w/notes.txt")], "one!\n");
    assert!(
        view.open.as_ref().is_some_and(|open| !open.modified),
        "a saved buffer is no longer unsaved"
    );
}

/// The one irreversible gesture, so it is the one that must never happen
/// on a single keystroke.
#[test]
fn deleting_asks_before_it_deletes() {
    let machine = FakeMachine::default().with_file("/w/scratch.txt", "x\n");
    let mut view = files_at("/w");
    settle(&mut view, &machine);

    press(&mut view, Command::Delete);
    assert!(view.queue.is_empty(), "asking is not deleting");
    press(&mut view, Command::Close);
    settle(&mut view, &machine);
    assert!(
        machine
            .files
            .borrow()
            .contains_key(Path::new("/w/scratch.txt"))
    );

    press(&mut view, Command::Delete);
    press(&mut view, Command::ConfirmDelete);
    settle(&mut view, &machine);
    assert!(
        !machine
            .files
            .borrow()
            .contains_key(Path::new("/w/scratch.txt"))
    );
}

#[test]
fn a_directory_is_never_deletable() {
    let machine = FakeMachine::default().with_directory("/w/src");
    let mut view = files_at("/w");
    settle(&mut view, &machine);

    press(&mut view, Command::Delete);
    assert!(view.confirming_delete.is_none());
    assert_eq!(view.notice.as_deref(), Some("only files are deletable"));
}

#[test]
fn closing_with_unsaved_changes_asks_once() {
    let machine = FakeMachine::default().with_file("/w/notes.txt", "one\n");
    let mut view = files_at("/w");
    settle(&mut view, &machine);
    press(&mut view, Command::Activate);
    settle(&mut view, &machine);
    press(&mut view, Command::Edit);
    type_text(&mut view, "x");
    press(&mut view, Command::Close);

    assert!(
        matches!(press(&mut view, Command::Close), CodeOutcome::Stay),
        "the first esc asks rather than closing"
    );
    assert!(
        matches!(press(&mut view, Command::Close), CodeOutcome::Close),
        "the second is the answer"
    );
}

/// A file with no trailing newline gains one on the way through
/// `str::lines`; one that had it must not gain a second.
#[test]
fn saving_a_file_nobody_edited_a_newline_into_keeps_its_ending() {
    let machine = FakeMachine::default().with_file("/w/a.txt", "one\ntwo\n");
    let mut view = files_at("/w");
    settle(&mut view, &machine);
    press(&mut view, Command::Activate);
    settle(&mut view, &machine);

    assert_eq!(
        view.open.as_ref().expect("a file is open").contents(),
        "one\ntwo\n"
    );
}

/// Opening a file and saving it unedited writes back the bytes it was
/// given — whatever those bytes were. The buffer is not the place a
/// project's line endings get an opinion held about them.
#[test]
fn a_file_opened_and_saved_unedited_is_byte_for_byte_what_it_was() {
    for original in [
        "one\r\ntwo\r\n",
        "one\r\ntwo",
        "one\ntwo",
        "one\ntwo\n",
        "",
        "\n",
        "one",
    ] {
        let machine = FakeMachine::default().with_file("/w/a.txt", original);
        let mut view = files_at("/w");
        settle(&mut view, &machine);
        press(&mut view, Command::Activate);
        settle(&mut view, &machine);

        assert_eq!(
            view.open.as_ref().expect("a file is open").contents(),
            original,
            "opening and saving rewrote {original:?}"
        );
    }
}

/// Editing a CRLF file leaves the lines nobody touched as they were: a
/// one-character change must not arrive as a whole-file diff.
#[test]
fn editing_a_crlf_file_keeps_every_other_line_crlf() {
    let machine = FakeMachine::default().with_file("/w/a.txt", "one\r\ntwo\r\n");
    let mut view = files_at("/w");
    settle(&mut view, &machine);
    press(&mut view, Command::Activate);
    settle(&mut view, &machine);
    press(&mut view, Command::Edit);
    type_text(&mut view, "x");

    assert_eq!(
        view.open.as_ref().expect("a file is open").contents(),
        "xone\r\ntwo\r\n"
    );
}

#[test]
fn an_unreadable_file_says_so_where_its_contents_would_be() {
    let mut machine = FakeMachine::default();
    machine.directories.insert(
        PathBuf::from("/w"),
        vec![DirEntry {
            directory: false,
            name: "binary.bin".to_owned(),
        }],
    );
    let mut view = files_at("/w");
    settle(&mut view, &machine);
    press(&mut view, Command::Activate);
    settle(&mut view, &machine);

    assert!(matches!(
        super::view(&view, space()).content,
        Content::Message {
            role: Role::Danger,
            ..
        }
    ));
}

/// A click says "this many cells into that line"; the caret has to end
/// up on the character those cells actually reach. A double-width glyph
/// is where the two counts come apart.
#[test]
fn a_click_lands_on_the_character_its_cells_reach() {
    let machine = FakeMachine::default().with_file("/w/wide.txt", "a漢b\n");
    let mut view = files_at("/w");
    settle(&mut view, &machine);
    press(&mut view, Command::Activate);
    settle(&mut view, &machine);

    for (cell, expected, why) in [
        (0, 0, "the first cell is the first character"),
        (1, 1, "the wide glyph's first cell is the wide glyph"),
        (2, 1, "and so is its second"),
        (
            3,
            2,
            "the character after it starts a cell later than its index",
        ),
        (99, 3, "past the end is the end"),
    ] {
        handle_mouse(
            &mut view,
            Some(ViewHit::PlaceCaret { line: 0, cell }),
            space(),
        );
        assert_eq!(
            view.open.as_ref().expect("a file is open").caret.column,
            expected,
            "{why}"
        );
    }
}

/// The save's own re-read arrives a beat later. Anything typed in that
/// beat must survive it.
#[test]
fn a_reread_never_overwrites_keystrokes_typed_while_it_was_out() {
    let machine = FakeMachine::default().with_file("/w/notes.txt", "one\n");
    let mut view = files_at("/w");
    settle(&mut view, &machine);
    press(&mut view, Command::Activate);
    settle(&mut view, &machine);
    press(&mut view, Command::Edit);
    press(&mut view, Command::CaretLineEnd);
    type_text(&mut view, "!");

    view.absorb(FileAnswer::Read {
        path: PathBuf::from("/w/notes.txt"),
        file: Ok(LoadedFile::of("one\n")),
    });
    assert_eq!(
        view.open.as_ref().map(|open| open.lines.clone()),
        Some(vec!["one!".to_owned()]),
        "what was typed is still what is on screen"
    );
}

/// Moving to another file in the tree points every mode at it: the diff
/// read for the file left behind is not left standing under the new one,
/// by the arrows or by a click.
#[test]
fn moving_to_another_file_in_the_tree_asks_for_its_diff() {
    let machine = FakeMachine::default()
        .with_file("/w/a.txt", "aaa\n")
        .with_file("/w/b.txt", "bbb\n");
    let mut view = files_at("/w");
    settle(&mut view, &machine);
    view.changes.diff_pending = false;

    press(&mut view, Command::SelectNext);
    assert_eq!(view.selected.as_deref(), Some(Path::new("/w/b.txt")));
    assert!(view.diff_pending(), "the arrows moved to another file");

    view.changes.diff_pending = false;
    handle_mouse(&mut view, Some(ViewHit::SelectItem(0)), space());
    assert_eq!(view.selected.as_deref(), Some(Path::new("/w/a.txt")));
    assert!(view.diff_pending(), "and so did the click");
}

/// An answer names the file it is about, so one that lands after the
/// viewer opened something else is dropped.
#[test]
fn a_read_that_arrives_late_does_not_replace_a_different_file() {
    let machine = FakeMachine::default()
        .with_file("/w/a.txt", "aaa\n")
        .with_file("/w/b.txt", "bbb\n");
    let mut view = files_at("/w");
    settle(&mut view, &machine);
    press(&mut view, Command::SelectNext);
    press(&mut view, Command::Activate);
    settle(&mut view, &machine);

    view.absorb(FileAnswer::Read {
        path: PathBuf::from("/w/a.txt"),
        file: Ok(LoadedFile::of("aaa\n")),
    });
    assert_eq!(
        view.open.as_ref().map(|open| open.lines.clone()),
        Some(vec!["bbb".to_owned()]),
        "the file on screen is still the one that was asked for"
    );
}

// --- the switch this whole surface exists for ---------------------------

/// The move the merge was for: from a line of the diff into that line of
/// the file. Carrying only the file would leave the reader at the top of
/// something they were reading the middle of.
#[test]
fn switching_from_a_diff_to_the_contents_keeps_the_file_and_the_line() {
    let machine = FakeMachine::default().with_file("/w/a.rs", "one\ntwo\nthree\nfour\n");
    let mut view = CodeView::opening(PathBuf::from("/w"), "/w".to_owned(), ContentMode::Diff);
    view.selected = Some(PathBuf::from("/w/a.rs"));
    view.changes.files = vec![ChangedFile {
        status: FileStatus::Modified,
        path: PathBuf::from("/w/a.rs"),
    }];
    view.changes.diff = diff::read(
        "@@ -1,4 +1,4 @@\n one\n two\n-old\n+three\n four\n",
        Path::new("/w/a.rs"),
        FALLBACK_SYNTAX_THEME,
    );
    view.changes.diff_pending = false;
    // Scrolled to the replacement, which is line three of the new file.
    view.scroll = 3;

    press(&mut view, Command::Edit);
    settle(&mut view, &machine);

    let open = view.open.as_ref().expect("the file opened");
    assert_eq!(open.path, PathBuf::from("/w/a.rs"), "the same file");
    assert_eq!(
        open.lines[open.caret.line], "three",
        "and the line that was being read"
    );
    assert_eq!(view.content, ContentMode::Contents);
    assert_eq!(
        view.navigator(),
        NavigatorMode::Files,
        "the navigator follows the content"
    );
}

/// A replacement of several lines is drawn in Git's order — every removal,
/// then every addition — and the line carried across the switch is still
/// the one on screen, both ways: an addition's number is the new file's,
/// and a removal that shares it is not where the reader comes back to.
#[test]
fn a_multi_line_replacement_carries_the_line_both_ways() {
    let machine = FakeMachine::default().with_file("/w/a.rs", "one\nc\nd\nfour\n");
    let mut view = CodeView::opening(PathBuf::from("/w"), "/w".to_owned(), ContentMode::Diff);
    view.selected = Some(PathBuf::from("/w/a.rs"));
    view.changes.files = vec![ChangedFile {
        status: FileStatus::Modified,
        path: PathBuf::from("/w/a.rs"),
    }];
    view.changes.diff = diff::read(
        "@@ -1,4 +1,4 @@\n one\n-a\n-b\n+c\n+d\n four\n",
        Path::new("/w/a.rs"),
        FALLBACK_SYNTAX_THEME,
    );
    view.changes.diff_pending = false;
    let Content::Lines { lines, .. } = super::view(&view, space()).content else {
        panic!("a selected file has a diff");
    };
    assert_eq!(
        lines
            .iter()
            .map(|line| (line.gutter.as_str(), line.number.as_str()))
            .collect::<Vec<_>>(),
        [
            (" ", "1"),
            ("-", "2"),
            ("-", "3"),
            ("+", "2"),
            ("+", "3"),
            (" ", "4")
        ]
    );
    // On `+d`, line three of the new file.
    view.scroll = 4;

    press(&mut view, Command::Edit);
    settle(&mut view, &machine);
    let open = view.open.as_ref().expect("the file opened");
    assert_eq!(open.lines[open.caret.line], "d");

    press(&mut view, Command::Close);
    view.show(ContentMode::Diff);
    assert_eq!(
        view.scroll, 4,
        "back on `+d`, not on `-b` which shares its number"
    );
}

/// The selection is authoritative and the tree catches up to it: a file
/// that changed may sit under directories nobody has opened.
#[test]
fn switching_to_a_file_the_tree_has_not_listed_opens_its_ancestors() {
    let machine = FakeMachine::default()
        .with_directory("/w/src")
        .with_directory("/w/src/ui")
        .with_file("/w/src/ui/deep.rs", "fn deep() {}\n");
    let mut view = CodeView::opening(PathBuf::from("/w"), "/w".to_owned(), ContentMode::Diff);
    view.selected = Some(PathBuf::from("/w/src/ui/deep.rs"));

    press(&mut view, Command::Edit);
    settle(&mut view, &machine);

    assert!(
        item_names_of_tree(&view).contains(&"deep.rs".to_owned()),
        "the tree opened down to the file: {:?}",
        item_names_of_tree(&view)
    );
}

/// The doors are shortcuts once the surface is open: the same two the
/// host uses to choose where to land also choose what to show.
#[test]
fn the_doors_switch_modes_once_the_surface_is_open() {
    let mut view = fixture();
    assert_eq!(view.content, ContentMode::Diff);

    view.show(ContentMode::Contents);
    assert_eq!(view.content, ContentMode::Contents);
    assert_eq!(
        view.navigator(),
        NavigatorMode::Files,
        "the navigator follows the content"
    );

    view.show(ContentMode::Diff);
    assert_eq!(view.content, ContentMode::Diff);
    assert_eq!(view.navigator(), NavigatorMode::Changes);
}

/// The frame's two edges: what the surface is on top, where it is open
/// at the foot — and each part of the second told apart by weight, since
/// one run of text gives them all the same and that is how a line stops
/// being read.
#[test]
fn the_frame_says_what_this_is_on_top_and_where_it_is_at_the_foot() {
    let mut view = fixture();
    view.branch = "feat/thing".to_owned();
    view.display_root = "~/uze/.worktrees/joipv0".to_owned();

    let drawn = super::view(&view, space());
    let said = |spans: &[Span]| {
        spans
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>()
    };
    assert_eq!(
        said(&drawn.title),
        CATALOG.name,
        "the name it is registered under"
    );
    assert_eq!(said(&drawn.caption), "~/uze/.worktrees/joipv0 · feat/thing");

    let weight = |spans: &[Span], text: &str| {
        spans
            .iter()
            .find(|span| span.text == text)
            .map(|span| (span.role, span.bold))
    };
    assert_eq!(
        weight(&drawn.title, CATALOG.name),
        Some((Role::Muted, false)),
        "the surface's name is a label, said once and quietly"
    );
    assert_eq!(
        weight(&drawn.caption, "~/uze/.worktrees/"),
        Some((Role::Dim, false)),
        "the directories leading to it are context"
    );
    assert_eq!(
        weight(&drawn.caption, "joipv0"),
        Some((Role::Bright, true)),
        "the checkout's own name identifies it"
    );
    assert_eq!(
        weight(&drawn.caption, "feat/thing"),
        Some((Role::Accent, true)),
        "and so does the branch, which is what changes"
    );
}

/// The control is offered only where it means something, and clicking a
/// segment shows that mode — the pointer's way to the same place the key
/// reaches.
#[test]
fn a_document_offers_its_two_modes_and_a_click_picks_one() {
    let machine = FakeMachine::default()
        .with_file("/w/README.md", "# Title\n")
        .with_file("/w/main.rs", "fn main() {}\n");
    let mut view = files_at("/w");
    settle(&mut view, &machine);

    // The tree lists directories first, then files by name: README.md
    // before main.rs.
    press(&mut view, Command::Activate);
    settle(&mut view, &machine);
    let offered = super::view(&view, space()).modes;
    assert_eq!(
        offered
            .iter()
            .map(|mode| mode.label.as_str())
            .collect::<Vec<_>>(),
        ["Preview", "Source"],
        "a document offers both ways of reading it"
    );
    assert!(
        offered[0].active,
        "and opens as the document it is, not as the markup describing it"
    );

    handle_mouse(&mut view, Some(ViewHit::SelectMode(1)), space());
    assert_eq!(view.content, ContentMode::Contents);
    assert!(super::view(&view, space()).modes[1].active);

    // A file that is not a document has one way of being read. Picked by
    // pointing at it: the focus is on the content now, so an arrow would
    // scroll rather than move the selection.
    handle_mouse(&mut view, Some(ViewHit::SelectItem(1)), space());
    settle(&mut view, &machine);
    assert!(
        super::view(&view, space()).modes.is_empty(),
        "nothing to choose between for a file that is not a document"
    );
}

/// Every way into a document lands on the document. The rule is the
/// selection's, not one entry point's, so it holds for the row the tree
/// opens on, for the next row, and after the reader has asked to see the
/// markup of the file before.
#[test]
fn a_document_is_read_as_one_however_it_was_reached() {
    let machine = FakeMachine::default()
        .with_file("/w/README.md", "# Title\n")
        .with_file("/w/main.rs", "fn main() {}\n")
        .with_file("/w/NOTES.markdown", "# Notes\n");
    let mut view = files_at("/w");
    settle(&mut view, &machine);

    assert_eq!(
        view.selected,
        Some(PathBuf::from("/w/README.md")),
        "the tree opens on its first row"
    );
    assert_eq!(
        view.content,
        ContentMode::Preview,
        "and a README is a document before it is a file"
    );

    // Asking for the markup lasts as long as the file it was asked about.
    press(&mut view, Command::TogglePreview);
    assert_eq!(view.content, ContentMode::Contents);

    press(&mut view, Command::SelectNext);
    settle(&mut view, &machine);
    assert_eq!(view.selected, Some(PathBuf::from("/w/main.rs")));
    assert_eq!(
        view.content,
        ContentMode::Contents,
        "a file that is not a document is read as a file"
    );

    press(&mut view, Command::SelectNext);
    settle(&mut view, &machine);
    assert_eq!(view.selected, Some(PathBuf::from("/w/NOTES.markdown")));
    assert_eq!(
        view.content,
        ContentMode::Preview,
        "`.markdown` is the same document `.md` is, and the source asked \
         for on another file was about that file"
    );
}

/// Closing the surface and opening it again is returning to it: the file
/// being read, the directories opened to reach it, and how far down it.
/// Without this the commonest gesture in the product — glance at the diff,
/// close, come back — costs the whole walk down the tree again.
#[test]
fn reopening_a_checkout_returns_to_where_the_viewer_was() {
    let machine = FakeMachine::default()
        .with_directory("/w/src")
        .with_file("/w/src/deep.rs", "one\ntwo\nthree\n")
        .with_file("/w/README.md", "# hi\n");
    let mut view = files_at("/w");
    settle(&mut view, &machine);

    press(&mut view, Command::Expand);
    settle(&mut view, &machine);
    press(&mut view, Command::SelectNext);
    settle(&mut view, &machine);
    assert_eq!(view.selected, Some(PathBuf::from("/w/src/deep.rs")));
    view.scroll = 2;

    // What the host keeps when the surface closes.
    let place = view.place();
    drop(view);

    let mut view = files_at("/w").resuming(place);
    settle(&mut view, &machine);

    assert_eq!(
        view.selected,
        Some(PathBuf::from("/w/src/deep.rs")),
        "the file being read is the file being read"
    );
    assert_eq!(
        item_names_of_tree(&view),
        ["src", "deep.rs", "README.md"],
        "and the directory opened to reach it is still open"
    );
    assert_eq!(view.scroll, 2, "at the line it was left at");
    assert_eq!(
        view.open.as_ref().map(|open| open.lines.len()),
        Some(3),
        "the contents were read again rather than assumed"
    );
}

/// The door still decides what the surface is showing. A place that
/// carried the mode would make `Ctrl+G` mean "wherever I was last time",
/// which is the one thing a door must not mean.
#[test]
fn a_restored_place_does_not_outrank_the_door_it_came_through() {
    let machine = FakeMachine::default().with_file("/w/a.rs", "one\n");
    let mut view = files_at("/w");
    settle(&mut view, &machine);
    assert_eq!(view.content, ContentMode::Contents);

    let place = view.place();
    let view =
        CodeView::opening(PathBuf::from("/w"), "/w".to_owned(), ContentMode::Diff).resuming(place);

    assert_eq!(view.content, ContentMode::Diff);
    assert_eq!(
        view.selected,
        Some(PathBuf::from("/w/a.rs")),
        "the file came with it, which is what makes the switch worth having"
    );
    assert!(
        view.changes.diff_pending,
        "and its diff is what the surface is now waiting for"
    );
}

/// Reviewing a document is still reviewing. The preview reads one file as
/// it is on disk and a diff is about two of them, so the half that answers
/// for changes keeps its mode whatever the selection is.
#[test]
fn a_documents_diff_is_still_a_diff() {
    let machine = FakeMachine::default().with_file("/w/README.md", "# Title\n");
    let mut view = CodeView::opening(PathBuf::from("/w"), "/w".to_owned(), ContentMode::Diff);
    view.changes.files = vec![
        ChangedFile {
            status: FileStatus::Modified,
            path: PathBuf::from("/w/a.rs"),
        },
        ChangedFile {
            status: FileStatus::Modified,
            path: PathBuf::from("/w/README.md"),
        },
    ];
    view.selected = Some(PathBuf::from("/w/a.rs"));

    view.select(PathBuf::from("/w/README.md"));
    settle(&mut view, &machine);

    assert_eq!(view.selected, Some(PathBuf::from("/w/README.md")));
    assert_eq!(
        view.content,
        ContentMode::Diff,
        "a document selected in the changes is still being reviewed"
    );
}

/// A refresh answers about the changes and nothing else. The same struct
/// holds a buffer, and a refresh that could reach it would be one that
/// eats what was typed.
#[test]
fn a_changes_refresh_leaves_an_unsaved_buffer_alone() {
    let machine = FakeMachine::default().with_file("/w/notes.txt", "one\n");
    let mut view = files_at("/w");
    settle(&mut view, &machine);
    press(&mut view, Command::Activate);
    settle(&mut view, &machine);
    press(&mut view, Command::Edit);
    press(&mut view, Command::CaretLineEnd);
    type_text(&mut view, "!");

    view.absorb_changes(RefreshedChanges {
        placement: view.placement(),
        branch: "main".to_owned(),
        changes: Changes {
            files: vec![ChangedFile {
                status: FileStatus::Modified,
                path: PathBuf::from("/w/notes.txt"),
            }],
            ..Changes::default()
        },
    });

    assert_eq!(
        view.open.as_ref().map(|open| open.lines.clone()),
        Some(vec!["one!".to_owned()]),
        "the buffer is untouched by a read of what changed"
    );
    assert!(view.open.as_ref().is_some_and(|open| open.modified));
}

/// A file tree's rows say what they *are*, so the host can mark them.
/// Classified by whole name as well as by extension, because the files at
/// the root of a repository carry their meaning without one.
#[test]
fn a_row_is_classified_by_what_it_is_rather_than_by_its_language() {
    use crate::code::files::icon_for;
    use crate::view::RowIcon;

    assert_eq!(icon_for("src", true, false), RowIcon::Directory);
    assert_eq!(icon_for("src", true, true), RowIcon::DirectoryOpen);

    // A whole name, where an extension would say nothing.
    assert_eq!(icon_for("Makefile", false, false), RowIcon::Code);
    assert_eq!(icon_for("LICENSE", false, false), RowIcon::Legal);
    assert_eq!(icon_for(".gitignore", false, false), RowIcon::Git);

    // And by extension, across the kinds this repository's own tree has.
    assert_eq!(icon_for("main.rs", false, false), RowIcon::Code);
    assert_eq!(icon_for("AGENTS.md", false, false), RowIcon::Markup);
    assert_eq!(icon_for("Cargo.toml", false, false), RowIcon::Config);
    assert_eq!(icon_for("Cargo.lock", false, false), RowIcon::Lock);
    assert_eq!(icon_for("logo.svg", false, false), RowIcon::Image);
    assert_eq!(icon_for("bundle.tar", false, false), RowIcon::Archive);

    // Case is not a classification, and an unknown extension is a file.
    assert_eq!(icon_for("README.MD", false, false), RowIcon::Markup);
    assert_eq!(icon_for("notes.qqq", false, false), RowIcon::File);
    assert_eq!(icon_for("noextension", false, false), RowIcon::File);
}

/// The changes list marks status, so it takes no file icon: two marks per
/// row is one too many, and the status is the reason that list exists.
#[test]
fn the_changes_list_marks_status_rather_than_kind() {
    let view = view(&fixture(), space());
    let rows = &view.navigator.as_ref().expect("a navigator").rows;
    assert!(
        rows.iter().all(|row| match row {
            crate::view::NavigatorRow::Group { icon, .. }
            | crate::view::NavigatorRow::Item { icon, .. } => *icon == crate::view::RowIcon::None,
        }),
        "the changes list asked for a file icon: {rows:?}"
    );
}

#[test]
fn a_place_somebody_was_sent_to_opens_every_directory_on_the_way() {
    let root = Path::new("/project");
    let file = CodePlace::at(root, Path::new("/project/crates/core/src/store.rs"), false);
    assert_eq!(
        file.selected.as_deref(),
        Some(Path::new("/project/crates/core/src/store.rs"))
    );
    let opened: Vec<&Path> = file.expanded.iter().map(PathBuf::as_path).collect();
    assert_eq!(
        opened,
        [
            Path::new("/project"),
            Path::new("/project/crates"),
            Path::new("/project/crates/core"),
            Path::new("/project/crates/core/src"),
        ],
        "and nothing above the project"
    );

    let directory = CodePlace::at(root, Path::new("/project/crates"), true);
    assert_eq!(directory.selected, None, "a directory is opened, not read");
    assert!(directory.expanded.contains(Path::new("/project/crates")));
}

/// A checkout measured into two directories and a file at the root.
fn measured() -> map::Measure {
    let file = |path: &str, lines: u32, commits: u32| map::FileMeasure {
        path: path.to_owned(),
        lines,
        commits,
        changed: false,
    };
    map::Measure {
        root: PathBuf::from("/repo"),
        files: vec![
            file("src/ui/render.rs", 900, 12),
            file("src/ui/input.rs", 700, 3),
            file("src/main.rs", 600, 4),
            file("docs/guide.md", 800, 1),
        ],
    }
}

/// A directory is a place the cursor can be, and no file to read.
///
/// Coming back to one asked the host to read it: the answer is "not
/// readable as text", drawn in the colour of a failure, where what is
/// true is that nothing is open. The same surface says exactly that
/// when it opens on an empty selection, and it is what a reader who has
/// selected nothing needs to be told.
#[test]
fn coming_back_to_a_directory_row_reads_nothing_and_says_nothing_is_open() {
    let machine = FakeMachine::default()
        .with_directory("/w/src")
        .with_file("/w/src/main.rs", "fn main() {}\n")
        .with_file("/w/notes.txt", "one\n");
    let mut view = files_at("/w");
    settle(&mut view, &machine);
    assert_eq!(
        view.selected.as_deref(),
        Some(Path::new("/w/src")),
        "the tree opens on its first row, which is a directory"
    );

    let mut again = files_at("/w").resuming(view.place());
    settle(&mut again, &machine);

    assert!(again.open.is_none(), "a directory was never a file to open");
    let Content::Message { role, hint, .. } = super::view(&again, space()).content else {
        panic!("nothing is open, so the content is what to do about it");
    };
    assert_eq!(role, Role::Muted, "nothing failed");
    assert!(hint.is_some(), "and there is something to do about it");

    // The other way to the same read: leave the files half with the
    // cursor on a directory, and come back to it.
    again.show(ContentMode::Diff);
    again.show(ContentMode::Contents);
    settle(&mut again, &machine);
    assert!(
        again.open.is_none(),
        "the half is the same one, and so is the row it was left on"
    );
}

/// The map is a fourth way of looking at the same checkout, and the
/// selection is what survives the switch: a tile followed is the file
/// selected, read by whichever half was on show before the map.
#[test]
fn a_tile_followed_on_the_map_is_the_file_the_surface_goes_back_to() {
    let mut view = surface(Path::new("/repo"), Vec::new(), 0);
    view.show(ContentMode::Contents);
    assert!(!view.has_map(), "nothing measured yet");
    press(&mut view, Command::ToggleMap);
    assert_ne!(view.showing(), ContentMode::Map, "and nothing to show");

    view.absorb_measure(measured());
    press(&mut view, Command::ToggleMap);
    assert_eq!(view.showing(), ContentMode::Map);

    let tile = |view: &CodeView, path: &str| {
        view.map_view()
            .expect("measured")
            .tiles((80, 20))
            .into_iter()
            .find(|tile| tile.path == path)
            .unwrap_or_else(|| panic!("no tile for {path}"))
            .frame
    };
    // Into `src`, then onto the file: a click selects, a second follows.
    for path in ["src", "src", "src/main.rs", "src/main.rs"] {
        let (x, y) = tile(&view, path).center();
        handle_mouse(
            &mut view,
            Some(ViewHit::PlaceCaret {
                line: y as usize,
                cell: x as usize,
            }),
            space(),
        );
    }
    assert_eq!(
        view.selected.as_deref(),
        Some(Path::new("/repo/src/main.rs")),
        "the tile is the selection every other half answers about"
    );
    assert_eq!(
        view.showing(),
        ContentMode::Contents,
        "and the map hands back to whatever it was opened over"
    );
}

fn chips(modes: &[crate::view::Mode]) -> Vec<(&str, bool)> {
    modes
        .iter()
        .map(|mode| (mode.label.as_str(), mode.active))
        .collect()
}

/// The two ends of the header row answer two questions: what the surface
/// is about, over the half that does the finding, and how what was found
/// is shown, over the half that shows it.
#[test]
fn the_header_offers_the_halves_on_one_side_and_the_renderings_on_the_other() {
    let machine = FakeMachine::default()
        .with_file("/w/README.md", "# hi\n")
        .with_file("/w/main.rs", "fn main() {}\n");
    let mut view = files_at("/w");
    settle(&mut view, &machine);
    view.absorb_measure(crate::code::Measure {
        root: PathBuf::from("/w"),
        files: vec![crate::code::FileMeasure {
            path: "main.rs".to_owned(),
            lines: 1,
            commits: 1,
            changed: false,
        }],
    });

    // On a document: the two ways of reading it, and the two halves.
    press(&mut view, Command::Activate);
    settle(&mut view, &machine);
    let drawn = render::view(&view, space());
    assert_eq!(
        chips(&drawn.subjects),
        [("Files", true), ("Map", false), ("Changes", false)]
    );
    assert_eq!(chips(&drawn.modes), [("Preview", true), ("Source", false)]);
    // Each half is a thing, and says which. A way of *drawing* one is
    // not a thing, and carries no mark at all.
    assert_eq!(
        drawn
            .subjects
            .iter()
            .map(|mode| mode.icon)
            .collect::<Vec<_>>(),
        [RowIcon::Directory, RowIcon::Map, RowIcon::Changes]
    );
    assert!(
        drawn.modes.iter().all(|mode| mode.icon == RowIcon::None),
        "a rendering has nothing to be a picture of"
    );

    // On anything else there is one way to read it, and a control with
    // one option is a label that can be clicked. Back to the tree first:
    // activating a file puts the keyboard in what it opened.
    press(&mut view, Command::FocusNext);
    press(&mut view, Command::SelectNext);
    press(&mut view, Command::Activate);
    settle(&mut view, &machine);
    let drawn = render::view(&view, space());
    assert_eq!(
        chips(&drawn.subjects),
        [("Files", true), ("Map", false), ("Changes", false)],
        "the halves never come and go — the row that says where you are \
         is the one thing that may not move when it is used"
    );
    assert!(drawn.modes.is_empty(), "nothing to choose between");
}

/// The map takes the frame, and its levels are the breadcrumb: an
/// either/or with the tree, not a column beside it.
#[test]
fn the_map_takes_the_frame_and_says_which_level_it_is_on() {
    let mut view = surface(Path::new("/repo"), Vec::new(), 0);
    view.absorb_measure(measured());

    // One of the three halves, so it is reachable from each of the
    // others — including the changes, where going to the map is also
    // leaving them, which is what the press said.
    assert!(
        render::view(&view, space())
            .footer
            .contains(&Command::ToggleMap),
        "a measured checkout offers it wherever you are in it"
    );
    press(&mut view, Command::ToggleMap);
    assert_eq!(view.showing(), ContentMode::Map, "from the changes too");
    press(&mut view, Command::ToggleMap);
    assert_eq!(
        view.showing(),
        ContentMode::Diff,
        "and back to the half it was opened over"
    );

    view.show(ContentMode::Contents);
    press(&mut view, Command::ToggleMap);

    let drawn = render::view(&view, space());
    assert!(drawn.navigator.is_none(), "the map is the navigator");
    assert_eq!(drawn.layout, crate::view::Layout::Board);
    let steps: Vec<&str> = drawn.trail.iter().map(|step| step.name.as_str()).collect();
    assert_eq!(steps, ["repo"], "the checkout, and nothing entered yet");
    assert_eq!(
        chips(&drawn.subjects),
        [("Files", false), ("Map", true), ("Changes", false)],
        "which half you are in, and the way to the others as chips \
         somebody can point at rather than keys they have to know"
    );
    assert_eq!(
        chips(&drawn.modes),
        [("Unicode", true), ("ASCII", false), ("Ranking", false)],
        "and beside the content, only the ways of drawing it — named as \
         what they are, since the nav above already says Map"
    );

    // Down a level, and the key that closes comes back up before it
    // leaves the map. Entered the way a viewer does: a click selects the
    // tile, a second one goes in.
    let frame = view
        .map_view()
        .expect("measured")
        .tiles((80, 20))
        .into_iter()
        .find(|tile| tile.path == "src")
        .expect("src is a tile")
        .frame;
    let (x, y) = frame.center();
    for _ in 0..2 {
        handle_mouse(
            &mut view,
            Some(ViewHit::PlaceCaret {
                line: y as usize,
                cell: x as usize,
            }),
            space(),
        );
    }
    let steps: Vec<String> = render::view(&view, space())
        .trail
        .into_iter()
        .map(|step| step.name)
        .collect();
    assert_eq!(steps, ["repo", "src"]);

    press(&mut view, Command::Close);
    let steps: Vec<String> = render::view(&view, space())
        .trail
        .into_iter()
        .map(|step| step.name)
        .collect();
    assert_eq!(steps, ["repo"], "back up a level, not out of the map");
    // Coming back selects the directory that was entered, which is a
    // level of its own: the next press lets go of it.
    press(&mut view, Command::Close);
    assert_eq!(view.showing(), ContentMode::Map, "still on the map");
    press(&mut view, Command::Close);
    assert_eq!(
        view.showing(),
        ContentMode::Contents,
        "and only then out of it"
    );
}
