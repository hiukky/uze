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
    diff::{highlight_diff_rows, pair_side_by_side, parse_unified_diff},
    render::changes_navigator as navigator,
    *,
};
use crate::{
    DirEntry,
    shared::highlight::FALLBACK_SYNTAX_THEME,
    view::{Command, Content, LineTone, NavigatorRow, Role, Size},
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
            NavigatorMode::Changes,
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
    view.changes.diff = highlight_diff_rows(
        pair_side_by_side(parse_unified_diff(
            "@@ -1,3 +1,4 @@\n context\n-removed line\n+added line\n",
        )),
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
        NavigatorMode::Changes,
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

    handle_mouse(&mut view, Some(ViewHit::ToggleGroup(ui_group)));

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

    handle_mouse(&mut view, Some(ViewHit::ToggleGroup(ui_group)));
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
    assert_eq!(section.rows[0].marker.text, "\u{25c9}");
    assert_eq!(section.rows[0].marker.role, Role::Info);
    assert_eq!(section.rows[1].marker.text, "\u{25cf}");
    assert_eq!(section.rows[1].marker.role, Role::Warning);
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
    fn git(&self, _root: &Path, _args: &[&str]) -> Result<String, String> {
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

    fn display_path(&self, path: &Path) -> String {
        path.display().to_string()
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
    CodeView::opening(PathBuf::from(root), root.to_owned(), NavigatorMode::Files)
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
        handle_mouse(&mut view, Some(ViewHit::PlaceCaret { line: 0, cell }));
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
    let mut view = CodeView::opening(PathBuf::from("/w"), "/w".to_owned(), NavigatorMode::Changes);
    view.selected = Some(PathBuf::from("/w/a.rs"));
    view.changes.files = vec![ChangedFile {
        status: FileStatus::Modified,
        path: PathBuf::from("/w/a.rs"),
    }];
    view.changes.diff = highlight_diff_rows(
        pair_side_by_side(parse_unified_diff(
            "@@ -1,4 +1,4 @@\n one\n two\n-old\n+three\n four\n",
        )),
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
        view.navigator,
        NavigatorMode::Files,
        "the navigator follows the content"
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
    let mut view = CodeView::opening(PathBuf::from("/w"), "/w".to_owned(), NavigatorMode::Changes);
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

    show(&mut view, ContentMode::Contents);
    assert_eq!(view.content, ContentMode::Contents);
    assert_eq!(
        view.navigator,
        NavigatorMode::Files,
        "the navigator follows the content"
    );

    show(&mut view, ContentMode::Diff);
    assert_eq!(view.content, ContentMode::Diff);
    assert_eq!(view.navigator, NavigatorMode::Changes);
}

/// A title is three things at once, and one run of text gives them all
/// the same weight — which is how a title stops being read.
#[test]
fn the_title_tells_its_three_parts_apart() {
    let mut view = fixture();
    view.branch = "feat/thing".to_owned();
    view.display_root = "~/uze/.worktrees/joipv0".to_owned();

    let title = super::view(&view, space()).title;
    let said: String = title.iter().map(|span| span.text.as_str()).collect();
    assert!(said.contains("~/uze/.worktrees/"));
    assert!(said.contains("joipv0"));
    assert!(said.contains("feat/thing"));

    let weight = |text: &str| {
        title
            .iter()
            .find(|span| span.text == text)
            .map(|span| (span.role, span.bold))
    };
    assert_eq!(
        weight("code"),
        Some((Role::Muted, false)),
        "the surface's name is a label, said once and quietly"
    );
    assert_eq!(
        weight("~/uze/.worktrees/"),
        Some((Role::Dim, false)),
        "the directories leading to it are context"
    );
    assert_eq!(
        weight("joipv0"),
        Some((Role::Bright, true)),
        "the checkout's own name identifies it"
    );
    assert_eq!(
        weight("feat/thing"),
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
        offered[1].active,
        "and opens on its source, because opening a file is for changing it"
    );

    handle_mouse(&mut view, Some(ViewHit::SelectMode(0)));
    assert_eq!(view.content, ContentMode::Preview);
    assert!(super::view(&view, space()).modes[0].active);

    // A file that is not a document has one way of being read. Picked by
    // pointing at it: the focus is on the content now, so an arrow would
    // scroll rather than move the selection.
    handle_mouse(&mut view, Some(ViewHit::SelectItem(1)));
    settle(&mut view, &machine);
    assert!(
        super::view(&view, space()).modes.is_empty(),
        "nothing to choose between for a file that is not a document"
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
