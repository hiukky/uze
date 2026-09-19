//! Tests for the "+ new" space root picker.
//!
//! Every case works on real directories under a disposable temp root and
//! addresses them absolutely, so nothing here depends on the developer's
//! `$HOME` — the one thing `expand_home` reads.

use super::*;
use uze_testkit::temp::TempDir;

/// A temp root holding `directories`, plus a picker opened inside it.
fn picker_over(label: &str, directories: &[&str]) -> (TempDir, RootPicker) {
    let root = TempDir::new(label);
    for directory in directories {
        std::fs::create_dir_all(root.join(directory)).unwrap();
    }
    let picker = RootPicker::opened_in(&root.path().display().to_string(), None);
    (root, picker)
}

fn names(picker: &RootPicker) -> Vec<String> {
    picker
        .matches()
        .map(|candidate| candidate.name.clone())
        .collect()
}

#[test]
fn opening_lists_the_directories_inside_the_prefilled_root() {
    let (root, picker) = picker_over("root-picker-open", &["alpha", "beta"]);

    assert_eq!(names(&picker), ["alpha", "beta"]);
    assert!(picker.input().is_empty(), "nothing to delete before typing");
    assert_eq!(picker.base(), root.path());
    assert_eq!(picker.selection(), None, "and no row claimed yet");
}

/// The listing and the row the prompt is on answer two questions. The
/// listing is where projects are; the row is where the operator already
/// is. Marking it is what keeps a second space over the project already
/// open one `Enter` away, while the listing still shows everywhere else
/// they could go — before this, the prompt listed the project's own
/// subdirectories, and reaching another project meant walking back out.
#[test]
fn the_project_the_operator_is_standing_in_is_the_row_the_prompt_opens_on() {
    let root = TempDir::new("root-picker-standing-in");
    for directory in ["alpha", "here", "zeta"] {
        std::fs::create_dir_all(root.join(directory)).unwrap();
    }
    let here = root.join("here");
    let picker = RootPicker::opened_in(&root.path().display().to_string(), Some(&here));

    assert_eq!(
        names(&picker),
        ["alpha", "here", "zeta"],
        "everywhere they could go is listed"
    );
    assert_eq!(
        picker.chosen(),
        Some(here.clone()),
        "and the prompt is on where they are"
    );
    assert!(picker.input().is_empty(), "with nothing to delete first");

    // A listing read again — which is what choosing a kind does — must
    // not move them off it.
    let mut picker = picker;
    assert_eq!(picker.chosen(), Some(here));

    // Typing is choosing, and it chooses among the rows rather than
    // returning to the mark.
    picker.typed('z');
    assert_eq!(
        picker.chosen(),
        Some(root.join("zeta")),
        "what was typed leads, not what was marked"
    );
}

/// A project that is not one of the rows leaves the prompt as it would
/// have been: the mark is an offer, never a claim about the listing.
#[test]
fn a_project_outside_the_listing_marks_nothing() {
    let root = TempDir::new("root-picker-elsewhere");
    std::fs::create_dir_all(root.join("alpha")).unwrap();
    let picker = RootPicker::opened_in(
        &root.path().display().to_string(),
        Some(std::path::Path::new("/somewhere/else")),
    );

    assert_eq!(picker.selection(), None, "no row claimed");
    assert_eq!(
        picker.chosen(),
        Some(root.path().to_path_buf()),
        "so `Enter` still takes the directory being listed"
    );
}

/// `Enter` on a prompt nothing has been done to takes the directory it
/// opened in, rather than whichever row happens to lead the listing.
#[test]
fn an_untouched_prompt_lands_on_the_directory_it_opened_in() {
    let (root, mut picker) = picker_over("root-picker-untouched", &["alpha", "beta"]);

    assert_eq!(picker.chosen(), Some(root.path().to_path_buf()));

    picker.move_selection(1);

    assert_eq!(
        picker.selection(),
        Some(0),
        "the first move takes the row in front"
    );
    assert_eq!(picker.chosen(), Some(root.join("alpha")));
}

#[test]
fn a_file_is_not_a_root() {
    let root = TempDir::new("root-picker-file");
    std::fs::create_dir_all(root.join("checkout")).unwrap();
    std::fs::write(root.join("notes.md"), "").unwrap();

    let picker = RootPicker::opened_in(&root.path().display().to_string(), None);

    assert_eq!(names(&picker), ["checkout"]);
}

#[test]
fn typing_narrows_the_listing_to_what_matches() {
    let (_root, mut picker) = picker_over("root-picker-narrow", &["uze", "uze-docs", "other"]);

    for character in "uze".chars() {
        picker.typed(character);
    }

    assert_eq!(names(&picker), ["uze", "uze-docs"]);
}

#[test]
fn a_name_matched_from_its_start_outranks_one_matched_inside() {
    let (_root, mut picker) = picker_over("root-picker-rank", &["my-api", "api"]);

    for character in "api".chars() {
        picker.typed(character);
    }

    assert_eq!(names(&picker), ["api", "my-api"]);
}

#[test]
fn matching_ignores_case() {
    let (_root, mut picker) = picker_over("root-picker-case", &["Projects"]);

    picker.typed('p');

    assert_eq!(names(&picker), ["Projects"]);
}

#[test]
fn a_hidden_directory_appears_only_once_it_is_asked_for_by_name() {
    let (_root, mut picker) = picker_over("root-picker-hidden", &[".worktrees", "src"]);

    assert_eq!(names(&picker), ["src"]);
    picker.typed('.');
    assert_eq!(names(&picker), [".worktrees"]);
}

/// A separator is a character on the line, not a gesture. It used to
/// walk into whichever row happened to be selected and clear what had
/// been typed — an implicit `Tab` that answered with a directory nobody
/// had asked for, and lost the name being typed on the way.
#[test]
fn a_separator_is_typed_into_the_line_rather_than_acted_on() {
    let (root, mut picker) = picker_over("root-picker-separator", &["alpha/inner"]);

    for character in "alpha".chars() {
        picker.typed(character);
    }
    picker.typed('/');

    assert_eq!(picker.input(), "alpha/", "the line keeps what was typed");
    assert_eq!(
        picker.base(),
        root.join("alpha"),
        "and names the directory it ends in"
    );
    assert_eq!(names(&picker), ["inner"]);
}

/// The two spellings that leave the directory the prompt opened on: the
/// same ones a shell answers to.
#[test]
fn a_line_that_starts_at_the_root_or_at_home_is_read_from_there() {
    let (root, mut picker) = picker_over("root-picker-absolute", &["alpha"]);

    for character in root.path().display().to_string().chars() {
        picker.typed(character);
    }
    picker.typed('/');

    assert_eq!(picker.base(), root.path());
    assert_eq!(names(&picker), ["alpha"]);
}

/// Backspace with nothing typed is the way back out of a directory `Tab`
/// walked into: the prompt lists the one above it.
#[test]
fn backspacing_with_nothing_typed_leaves_the_directory_for_the_one_above() {
    let (root, mut picker) = picker_over("root-picker-up", &["repo/crates"]);
    picker.move_selection(0);
    picker.descend();
    assert_eq!(names(&picker), ["crates"]);

    picker.backspace();

    assert_eq!(picker.base(), root.path());
    assert_eq!(names(&picker), ["repo"]);
}

#[test]
fn backspacing_past_the_filter_brings_the_whole_listing_back() {
    let (_root, mut picker) = picker_over("root-picker-backspace", &["alpha", "second"]);

    picker.typed('a');
    assert_eq!(names(&picker), ["alpha"]);

    picker.backspace();
    assert_eq!(names(&picker), ["alpha", "second"]);
}

#[test]
fn descending_lists_the_selected_directorys_own_children() {
    let (root, mut picker) = picker_over("root-picker-descend", &["repo/crates", "repo/docs"]);

    picker.move_selection(0);
    picker.descend();

    assert_eq!(names(&picker), ["crates", "docs"]);
    assert_eq!(picker.base(), root.join("repo"));
    assert_eq!(
        picker.input(),
        "repo/",
        "completion is written into the line, not into state beside it"
    );
}

/// Completing twice walks two directories down, and the line reads the
/// path it walked — the segment being matched is what each completion
/// replaces, never the whole line.
#[test]
fn completing_again_replaces_the_segment_and_keeps_the_path() {
    let (_root, mut picker) = picker_over("root-picker-descend-twice", &["repo/crates/core"]);

    picker.move_selection(0);
    picker.descend();
    picker.move_selection(0);
    picker.descend();

    assert_eq!(picker.input(), "repo/crates/");
    assert_eq!(names(&picker), ["core"]);
}

#[test]
fn descending_with_nothing_matching_leaves_the_prompt_where_it_is() {
    let (root, mut picker) = picker_over("root-picker-descend-empty", &["repo"]);

    picker.typed('z');
    picker.descend();

    assert_eq!(picker.base(), root.path());
    assert_eq!(picker.input(), "z");
}

#[test]
fn the_chosen_root_is_the_selected_directory() {
    let (root, mut picker) = picker_over("root-picker-chosen", &["alpha", "beta"]);

    picker.move_selection(1);
    picker.move_selection(1);

    assert_eq!(picker.chosen(), Some(root.join("beta")));
}

#[test]
fn the_selection_cannot_run_off_either_end_of_the_matches() {
    let (root, mut picker) = picker_over("root-picker-bounds", &["alpha", "beta"]);

    picker.move_selection(-1);
    assert_eq!(picker.chosen(), Some(root.join("alpha")));

    picker.move_selection(9);
    assert_eq!(picker.chosen(), Some(root.join("beta")));
}

#[test]
fn an_empty_directory_still_offers_itself_as_the_root() {
    let root = TempDir::new("root-picker-empty");
    let picker = RootPicker::opened_in(&root.path().display().to_string(), None);

    assert_eq!(picker.match_count(), 0);
    assert_eq!(picker.chosen(), Some(root.path().to_path_buf()));
}

#[test]
fn a_typed_name_that_matches_nothing_is_not_a_root() {
    let (_root, mut picker) = picker_over("root-picker-nothing", &["alpha"]);

    picker.typed('z');

    assert_eq!(picker.chosen(), None);
}

#[test]
fn the_window_scrolls_only_far_enough_to_keep_the_selection_visible() {
    let directories: Vec<String> = (0..12).map(|index| format!("d{index:02}")).collect();
    let borrowed: Vec<&str> = directories.iter().map(String::as_str).collect();
    let (_root, mut picker) = picker_over("root-picker-window", &borrowed);

    assert_eq!(picker.window_start(8), 0);
    picker.move_selection(1);
    picker.move_selection(7);
    assert_eq!(picker.window_start(8), 0);
    picker.move_selection(1);
    assert_eq!(picker.window_start(8), 1);
}

/// The directory being listed is what the prompt lands on until something
/// in it is chosen — picking it needs no gesture at all.
#[test]
fn the_listed_directory_is_what_the_prompt_lands_on() {
    let root = TempDir::new("root-picker-self");
    std::fs::create_dir_all(root.join("checkout/inner")).unwrap();
    let picker = RootPicker::opened_in(&root.join("checkout").display().to_string(), None);
    assert_eq!(names(&picker), ["inner"]);

    assert_eq!(picker.chosen(), Some(root.join("checkout")));
}

/// A space is a directory a person works in, and nothing about it says
/// whether a repository is underneath: isolation is asked of one agent,
/// later, in the space it is already running in. So the listing offers
/// every directory, and the filter that once kept only repositories and
/// the folders leading to one is gone with the kind that needed it —
/// that filter is what drew a `projects` folder holding nothing but
/// checkouts as empty.
#[test]
fn every_directory_is_offered_because_the_space_asks_nothing_of_the_tree() {
    let root = TempDir::new("root-picker-filter");
    std::fs::create_dir_all(root.join("projects/engine/.git")).unwrap();
    std::fs::create_dir_all(root.join("notes/drafts")).unwrap();
    let mut picker = RootPicker::opened_in(&root.path().display().to_string(), None);

    assert_eq!(
        names(&picker),
        ["notes", "projects"],
        "a folder with no repository under it is a place to work too"
    );
    assert_eq!(
        picker.chosen(),
        Some(root.path().to_path_buf()),
        "the directory being listed is what the prompt lands on"
    );

    picker.select(0);
    picker.descend();
    assert_eq!(names(&picker), ["drafts"]);

    let mut picker = RootPicker::opened_in(&root.path().display().to_string(), None);
    picker.select(1);
    picker.descend();
    assert_eq!(names(&picker), ["engine"]);
    picker.select(0);
    assert_eq!(picker.chosen(), Some(root.join("projects/engine")));
}

/// A directory inside a repository is a space of its own, not the
/// repository it sits in. The two kinds answered this differently — a
/// worktree resolved up to the repository, a workspace stood where it was
/// picked — and resolving up is what put spaces where nobody picked them.
#[test]
fn a_subdirectory_of_a_repository_is_a_space_where_it_stands() {
    let root = TempDir::new("root-picker-subdirectory");
    let repository = root.join("project");
    std::fs::create_dir_all(repository.join(".git")).unwrap();
    std::fs::create_dir_all(repository.join("docs")).unwrap();
    let mut picker = RootPicker::opened_in(&repository.display().to_string(), None);

    assert_eq!(
        picker.chosen(),
        Some(repository.clone()),
        "the repository itself, until something in it is picked"
    );
    assert_eq!(names(&picker), ["docs"]);

    picker.select(0);
    assert_eq!(
        picker.chosen(),
        Some(repository.join("docs")),
        "and it stands where it was picked"
    );
}

/// A slot is a checkout of the project, not a project of its own — so
/// landing on one asks for the repository it was cut from. Picking it
/// literally is what put a `.worktrees/<id>` row in the sidebar next to
/// the space whose own agent was working inside it.
#[test]
fn choosing_an_agents_slot_opens_the_repository_it_was_cut_from() {
    let root = TempDir::new("root-picker-slot");
    let repository = root.join("project");
    std::fs::create_dir_all(repository.join(".worktrees/4j03rn")).unwrap();
    let mut picker =
        RootPicker::opened_in(&repository.join(".worktrees").display().to_string(), None);
    assert_eq!(names(&picker), ["4j03rn"]);

    picker.move_selection(0);
    assert_eq!(picker.chosen(), Some(repository.clone()));

    // …and the same answer for a slot typed out rather than landed on:
    // an empty listing falls back to the typed directory itself.
    let typed = RootPicker::opened_in(
        &repository.join(".worktrees/4j03rn").display().to_string(),
        None,
    );
    assert_eq!(typed.match_count(), 0);
    assert_eq!(typed.chosen(), Some(repository));
}
