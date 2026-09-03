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
    let (_root, picker) = picker_over("root-picker-open", &["alpha", "beta"]);

    assert_eq!(names(&picker), ["alpha", "beta"]);
    assert!(picker.input().ends_with('/'), "{}", picker.input());
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
    let (_root, mut picker) = picker_over("root-picker-descend", &["repo/crates", "repo/docs"]);

    picker.descend();

    assert_eq!(names(&picker), ["crates", "docs"]);
    assert!(picker.input().ends_with("repo/"), "{}", picker.input());
}

#[test]
fn descending_with_nothing_matching_leaves_the_prompt_where_it_is() {
    let (_root, mut picker) = picker_over("root-picker-descend-empty", &["repo"]);

    picker.typed('z');
    let before = picker.input().to_owned();
    picker.descend();

    assert_eq!(picker.input(), before);
}

#[test]
fn the_chosen_root_is_the_selected_directory() {
    let (root, mut picker) = picker_over("root-picker-chosen", &["alpha", "beta"]);

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
    let picker = RootPicker::opened_in(&root.path().display().to_string());

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
    picker.move_selection(7);
    assert_eq!(picker.window_start(8), 0);
    picker.move_selection(1);
    assert_eq!(picker.window_start(8), 1);
}

/// Deleting the trailing separator is how the directory being listed is
/// picked: it becomes the segment matched inside its own parent.
#[test]
fn backspacing_the_separator_selects_the_listed_directory_itself() {
    let root = TempDir::new("root-picker-self");
    std::fs::create_dir_all(root.join("checkout/inner")).unwrap();
    let mut picker = RootPicker::opened_in(&root.join("checkout").display().to_string());
    assert_eq!(names(&picker), ["inner"]);

    picker.backspace();

    assert_eq!(picker.chosen(), Some(root.join("checkout")));
}
