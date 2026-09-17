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
    let picker = RootPicker::opened_in(&root.path().display().to_string());
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

/// `Enter` on a prompt nothing has been done to takes the directory it
/// opened in, rather than whichever row happens to lead the listing.
#[test]
fn an_untouched_prompt_lands_on_the_directory_it_opened_in() {
    let (root, mut picker) = picker_over("root-picker-untouched", &["alpha", "beta"]);

    assert_eq!(
        picker.chosen().map(|(root, _)| root),
        Some(root.path().to_path_buf())
    );

    picker.move_selection(1);

    assert_eq!(
        picker.selection(),
        Some(0),
        "the first move takes the row in front"
    );
    assert_eq!(
        picker.chosen().map(|(root, _)| root),
        Some(root.join("alpha"))
    );
}

#[test]
fn a_file_is_not_a_root() {
    let root = TempDir::new("root-picker-file");
    std::fs::create_dir_all(root.join("checkout")).unwrap();
    std::fs::write(root.join("notes.md"), "").unwrap();

    let picker = RootPicker::opened_in(&root.path().display().to_string());

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

/// A separator walks into the row the prompt is on, the way `Tab` does —
/// and with no row chosen there is nowhere to walk into.
#[test]
fn a_separator_typed_with_nothing_chosen_changes_nothing() {
    let (root, mut picker) = picker_over("root-picker-separator", &["alpha"]);

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
    assert!(picker.input().is_empty(), "and nothing left typed");
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

    assert_eq!(
        picker.chosen().map(|(root, _)| root),
        Some(root.join("beta"))
    );
}

#[test]
fn the_selection_cannot_run_off_either_end_of_the_matches() {
    let (root, mut picker) = picker_over("root-picker-bounds", &["alpha", "beta"]);

    picker.move_selection(-1);
    assert_eq!(
        picker.chosen().map(|(root, _)| root),
        Some(root.join("alpha"))
    );

    picker.move_selection(9);
    assert_eq!(
        picker.chosen().map(|(root, _)| root),
        Some(root.join("beta"))
    );
}

#[test]
fn an_empty_directory_still_offers_itself_as_the_root() {
    let root = TempDir::new("root-picker-empty");
    let picker = RootPicker::opened_in(&root.path().display().to_string());

    assert_eq!(picker.match_count(), 0);
    assert_eq!(
        picker.chosen().map(|(root, _)| root),
        Some(root.path().to_path_buf())
    );
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
    let picker = RootPicker::opened_in(&root.join("checkout").display().to_string());
    assert_eq!(names(&picker), ["inner"]);

    assert_eq!(
        picker.chosen().map(|(root, _)| root),
        Some(root.join("checkout"))
    );
}

/// A worktree space can only be cut from a repository, so that is all the
/// listing offers for one — picking anything else silently created the
/// space somewhere else. The other kind stands anywhere, and offers
/// everything.
#[test]
fn the_worktree_kind_offers_repositories_and_the_other_offers_every_directory() {
    let root = TempDir::new("root-picker-filter");
    std::fs::create_dir_all(root.join("project/.git")).unwrap();
    std::fs::create_dir_all(root.join("notes")).unwrap();
    let mut picker = RootPicker::opened_in(&root.path().display().to_string());

    // A plain directory names the tenancy, which offers both rows.
    assert_eq!(names(&picker), ["notes", "project"]);

    picker.choose_kind(SpaceKind::Worktree);

    assert_eq!(
        names(&picker),
        ["project"],
        "only what a slot can be cut from"
    );

    picker.choose_kind(SpaceKind::Workspace);

    assert_eq!(names(&picker), ["notes", "project"], "and back");
}

/// A worktree space is cut from a repository, so a directory inside one is
/// that repository. A workspace space runs its agents where it stands, so
/// there the directory chosen is the root — the two questions have two
/// answers, and giving both the first put spaces where nobody picked them.
#[test]
fn a_subdirectory_is_the_repository_for_a_worktree_and_itself_for_a_workspace() {
    let root = TempDir::new("root-picker-subdirectory");
    let repository = root.join("project");
    std::fs::create_dir_all(repository.join(".git")).unwrap();
    std::fs::create_dir_all(repository.join("docs")).unwrap();
    let mut picker = RootPicker::opened_in(&repository.display().to_string());

    // Opened in a repository, the prompt is on the repository itself, and a
    // worktree space is what it would create there.
    assert_eq!(
        picker.chosen(),
        Some((repository.clone(), SpaceKind::Worktree)),
        "a slot is cut from the repository"
    );
    assert!(
        names(&picker).is_empty(),
        "and a subdirectory is no place to cut one from: {:?}",
        names(&picker)
    );

    picker.choose_kind(SpaceKind::Workspace);
    picker.move_selection(0);

    assert_eq!(
        names(&picker),
        ["docs"],
        "the other kind can stand anywhere"
    );
    assert_eq!(
        picker.chosen(),
        Some((repository.join("docs"), SpaceKind::Workspace)),
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
    let mut picker = RootPicker::opened_in(&repository.join(".worktrees").display().to_string());
    assert_eq!(names(&picker), ["4j03rn"]);

    picker.move_selection(0);
    assert_eq!(
        picker.chosen().map(|(root, _)| root),
        Some(repository.clone())
    );

    // …and the same answer for a slot typed out rather than landed on:
    // an empty listing falls back to the typed directory itself.
    let typed = RootPicker::opened_in(&repository.join(".worktrees/4j03rn").display().to_string());
    assert_eq!(typed.match_count(), 0);
    assert_eq!(typed.chosen().map(|(root, _)| root), Some(repository));
}
