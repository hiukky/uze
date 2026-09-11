//! The action vocabulary: every meaning uze's keyboard can reach, named
//! once and independently of any key.
//!
//! This is to input what [`crate::Scope`] is to modality and what a colour
//! token is to appearance: the product decides what an action *means*, and
//! which chord reaches it is a separate, changeable question. Nothing here
//! knows a key.
//!
//! Two properties travel with the name because every surface needs them and
//! none should re-derive them: what the action does in words ([`Action::description`]),
//! which is what the index and the Keys screen read out, and whether
//! performing it destroys something ([`Action::destructive`]), which decides
//! whether it may hold an unmodified letter and whether it may sit first in
//! a menu.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Which of uze's two keyboards an action belongs to.
///
/// The distinction is not cosmetic. In management uze owns the whole
/// keyboard, so an action may hold a bare letter. In the workspace every
/// bare key belongs to the program running in the pane, so an action there
/// must carry a modifier or a function key. `Both` is for the handful of
/// actions that mean the same thing in either place.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Mode {
    Management,
    Workspace,
    Both,
}

impl Mode {
    /// Whether an action of this mode is live in `other`'s keyboard.
    pub fn covers(self, other: Mode) -> bool {
        self == Mode::Both || other == Mode::Both || self == other
    }
}

macro_rules! actions {
    ($($variant:ident => $name:literal, $mode:expr, $destructive:expr, $label:literal, $description:literal;)*) => {
        /// Everything uze's keyboard can reach.
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
        pub enum Action {
            $($variant,)*
            /// Select the nth tab on the strip — the agent in front of
            /// the person and the shells opened alongside it, which is
            /// what the numbers on screen are counted along. One action
            /// per position rather than one action carrying a number,
            /// because the keymap binds positions, not a counter.
            SelectTab(u8),
        }

        /// Every action this build knows, in vocabulary order — the order
        /// the index and the Keys screen list them in.
        pub const ALL_ACTIONS: &[Action] = &[
            $(Action::$variant,)*
            Action::SelectTab(1), Action::SelectTab(2), Action::SelectTab(3),
            Action::SelectTab(4), Action::SelectTab(5), Action::SelectTab(6),
            Action::SelectTab(7), Action::SelectTab(8), Action::SelectTab(9),
        ];

        impl Action {
            /// The name this action carries in a keymap file.
            pub fn name(self) -> String {
                match self {
                    $(Action::$variant => $name.to_owned(),)*
                    Action::SelectTab(index) => format!("select-tab-{index}"),
                }
            }

            /// Reads a name back. Unknown names are the caller's to report
            /// as a warning: a keymap written for a newer uze must still
            /// load on this one.
            pub fn parse(name: &str) -> Option<Action> {
                match name {
                    $($name => Some(Action::$variant),)*
                    other => other
                        .strip_prefix("select-tab-")
                        .and_then(|index| index.parse::<u8>().ok())
                        .filter(|index| (1..=9).contains(index))
                        .map(Action::SelectTab),
                }
            }

            /// Which keyboard this action belongs to.
            pub fn mode(self) -> Mode {
                match self {
                    $(Action::$variant => $mode,)*
                    Action::SelectTab(_) => Mode::Workspace,
                }
            }

            /// Whether performing this destroys something. A destructive
            /// action never holds an unmodified letter and is never the
            /// entry a menu opens highlighted.
            pub fn destructive(self) -> bool {
                match self {
                    $(Action::$variant => $destructive,)*
                    Action::SelectTab(_) => false,
                }
            }

            /// The imperative a menu entry or button carries.
            pub fn label(self) -> String {
                match self {
                    $(Action::$variant => $label.to_owned(),)*
                    Action::SelectTab(index) => format!("Tab {index}"),
                }
            }

            /// What it does, in words — the prose that used to live in a
            /// hand-written help overlay, now attached to the action so no
            /// surface has to restate it.
            pub fn description(self) -> String {
                match self {
                    $(Action::$variant => $description.to_owned(),)*
                    Action::SelectTab(index) => {
                        format!(
                            "Select tab {index} on the strip: the agent in front of \
                             you and the shells opened alongside it"
                        )
                    }
                }
            }
        }
    };
}

actions! {
    // --- Global ---------------------------------------------------------
    OpenActionIndex => "open-action-index", Mode::Both, false,
        "Everything you can do", "List every action available here, with the key that reaches it";
    SwitchMode => "switch-mode", Mode::Both, false,
        "Switch mode", "Move between the workspace and management";
    Quit => "quit", Mode::Both, false,
        "Quit", "Leave uze, detaching from the session rather than ending it";

    // --- Navigation, in either keyboard ---------------------------------
    SelectNext => "select-next", Mode::Both, false,
        "Next", "Move the selection down one";
    SelectPrevious => "select-previous", Mode::Both, false,
        "Previous", "Move the selection up one";
    FocusNext => "focus-next", Mode::Both, false,
        "Next pane", "Move focus to the next part of the screen";
    FocusPrevious => "focus-previous", Mode::Both, false,
        "Previous pane", "Move focus to the previous part of the screen";
    Activate => "activate", Mode::Both, false,
        "Open", "Open, inspect, or confirm whatever is selected";
    Dismiss => "dismiss", Mode::Both, false,
        "Close", "Close what is open, without acting on it";
    Expand => "expand", Mode::Both, false,
        "Expand", "Unfold the selected row";
    Collapse => "collapse", Mode::Both, false,
        "Collapse", "Fold the selected row";
    ScrollPageDown => "scroll-page-down", Mode::Both, false,
        "Page down", "Scroll the focused view down a page";
    ScrollPageUp => "scroll-page-up", Mode::Both, false,
        "Page up", "Scroll the focused view up a page";
    EraseBack => "erase-back", Mode::Both, false,
        "Erase", "Delete the character before the cursor";

    // --- Management, screen-wide ----------------------------------------
    NextScreen => "next-screen", Mode::Management, false,
        "Next screen", "Move to the next screen in the sidebar, from wherever you are";
    PreviousScreen => "previous-screen", Mode::Management, false,
        "Previous screen", "Move to the previous screen in the sidebar, from wherever you are";
    FocusSidebar => "focus-sidebar", Mode::Management, false,
        "Back to the sidebar", "Move focus from the content back to the route list";
    FocusContent => "focus-content", Mode::Management, false,
        "Into the content", "Move focus from the route list into the screen";
    Refresh => "refresh", Mode::Management, false,
        "Refresh", "Re-read the machine: harnesses, plugins, marketplaces";
    StartFilter => "start-filter", Mode::Management, false,
        "Search", "Narrow the list by typing";
    OpenThemePicker => "open-theme-picker", Mode::Management, false,
        "Appearance", "Choose the theme every uze surface draws in";
    ConfirmYes => "confirm-yes", Mode::Management, false,
        "Yes", "Answer the open question with yes";
    ConfirmNo => "confirm-no", Mode::Management, false,
        "No", "Answer the open question with no";

    // --- Management, things done to a key ------------------------------
    ChangeKey => "change-key", Mode::Management, false,
        "Change key", "Bind the next key pressed to the selected action";
    ResetKey => "reset-key", Mode::Management, false,
        "Reset key", "Put back the key uze ships with for the selected action";

    // --- Management, things done to a package ---------------------------
    InstallPlugin => "install-plugin", Mode::Management, false,
        "Install", "Install the selected plugin onto this machine";
    UpdatePlugin => "update-plugin", Mode::Management, false,
        "Update", "Update the selected plugin to what its marketplace offers";
    RemovePlugin => "remove-plugin", Mode::Management, true,
        "Remove", "Remove the selected plugin from this machine";
    AddMarketplace => "add-marketplace", Mode::Management, false,
        "Add marketplace", "Register a marketplace by path or URL";

    // --- Management, things done to a project ---------------------------
    InstallProjectEnvironment => "install-project-environment", Mode::Management, false,
        "Install the project's environment", "Install what this project declares but the machine lacks";
    ClearPromptHistory => "clear-prompt-history", Mode::Management, true,
        "Clear history", "Forget the prompts this machine has recorded";

    // --- Management, things done to a harness ---------------------------
    SetupHarness => "setup-harness", Mode::Management, false,
        "Set up", "Prepare the selected harness to receive what uze delivers";
    AnalyzeContext => "analyze-context", Mode::Management, false,
        "Analyze context", "Read what this project's context would become";
    ApplyContextPlan => "apply-context-plan", Mode::Management, false,
        "Apply the plan", "Write the analyzed context into the project";
    OpenGlossary => "open-glossary", Mode::Management, false,
        "What these words mean", "Explain the labels this screen uses";

    // --- Management, things done to a profile ---------------------------
    NewProfile => "new-profile", Mode::Management, false,
        "New profile", "Create a profile of preferences";
    DeleteProfile => "delete-profile", Mode::Management, true,
        "Delete", "Delete the selected profile";
    ApplyProfile => "apply-profile", Mode::Management, false,
        "Apply", "Make the selected profile active and write it into the checked harnesses";
    PreviewProfile => "preview-profile", Mode::Management, false,
        "Preview", "Show what the selected profile writes into each harness";
    ToggleProfileHarness => "toggle-profile-harness", Mode::Management, false,
        "Toggle harness", "Include or exclude the highlighted harness";
    NextValue => "next-value", Mode::Management, false,
        "Next value", "Change the highlighted preference to the next value";
    PreviousValue => "previous-value", Mode::Management, false,
        "Previous value", "Change the highlighted preference to the previous value";

    // --- Workspace, the container ---------------------------------------
    NewShellTab => "new-shell-tab", Mode::Workspace, false,
        "New shell", "Open a shell beside what is running";
    CloseTab => "close-tab", Mode::Workspace, true,
        "Close tab", "Close the selected tab";
    NewAgent => "new-agent", Mode::Workspace, false,
        "New agent", "Start an agent in a checkout of its own";
    NewSpace => "new-space", Mode::Workspace, false,
        "New space", "Open a space at a directory";
    RenameSelection => "rename-selection", Mode::Workspace, false,
        "Rename", "Rename the selected tab or space";
    NextSpace => "next-space", Mode::Workspace, false,
        "Next space", "Move to the next space in the sidebar";
    PreviousSpace => "previous-space", Mode::Workspace, false,
        "Previous space", "Move to the previous space in the sidebar";
    NextAgent => "next-agent", Mode::Workspace, false,
        "Next agent", "Move to the next agent in this space";
    PreviousAgent => "previous-agent", Mode::Workspace, false,
        "Previous agent", "Move to the previous agent in this space";
    ToggleChanges => "toggle-changes", Mode::Workspace, false,
        "Changes", "Open or close the changes in the selected tab's checkout";
    ToggleFiles => "toggle-files", Mode::Workspace, false,
        "Files", "Open or close the files of the selected tab's checkout";

    // --- The code surface, and typing into a file ------------------------
    EditFile => "edit-file", Mode::Workspace, false,
        "Edit", "Open the selected file's contents and start typing";
    TogglePreview => "toggle-preview", Mode::Workspace, false,
        "Preview", "Show a markdown file as the document it describes, and back";
    SaveFile => "save-file", Mode::Workspace, false,
        "Save", "Write what was typed back to the file";
    DeleteFile => "delete-file", Mode::Workspace, true,
        "Delete", "Delete the selected file, having been asked once";
    ConfirmDelete => "confirm-delete", Mode::Workspace, true,
        "Confirm delete", "Confirm deleting the file, having been asked once";
    CaretLeft => "caret-left", Mode::Workspace, false,
        "Left", "Move the caret one character left";
    CaretRight => "caret-right", Mode::Workspace, false,
        "Right", "Move the caret one character right";
    CaretLineStart => "caret-line-start", Mode::Workspace, false,
        "Line start", "Move the caret to the start of its line";
    CaretLineEnd => "caret-line-end", Mode::Workspace, false,
        "Line end", "Move the caret to the end of its line";
    InsertNewline => "insert-newline", Mode::Workspace, false,
        "New line", "Split the line at the caret";
    EraseForward => "erase-forward", Mode::Workspace, false,
        "Delete", "Delete the character under the caret";

    // --- Workspace, the work --------------------------------------------
    DeliverTask => "deliver-task", Mode::Workspace, false,
        "Deliver", "Deliver the selected task the way the project says";
    DeliverAllTasks => "deliver-all-tasks", Mode::Workspace, false,
        "Deliver all", "Deliver every deliverable task in this space";
    TogglePreservedWork => "toggle-preserved-work", Mode::Workspace, false,
        "Preserved work", "Show the work no live tab is in front of";
    ResumeTask => "resume-task", Mode::Workspace, false,
        "Resume", "Put the selected preserved task back into a slot";
    FinishTask => "finish-task", Mode::Workspace, false,
        "Mark done", "Record the selected task as finished";
    DiscardTask => "discard-task", Mode::Workspace, true,
        "Discard", "Destroy the selected task's uncommitted work";
    ConfirmDiscard => "confirm-discard", Mode::Workspace, true,
        "Confirm discard", "Confirm destroying the work, having been asked once";
}

impl fmt::Display for Action {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.name())
    }
}

impl Serialize for Action {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.name())
    }
}

impl<'de> Deserialize<'de> for Action {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Action, D::Error> {
        let name = String::deserialize(deserializer)?;
        Action::parse(&name)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown action `{name}`")))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn every_action_names_itself_uniquely_and_reads_back() {
        let mut seen = BTreeSet::new();
        for action in ALL_ACTIONS {
            let name = action.name();
            assert!(seen.insert(name.clone()), "two actions named `{name}`");
            assert_eq!(Action::parse(&name), Some(*action), "`{name}`");
        }
    }

    #[test]
    fn an_action_this_build_does_not_know_is_not_an_error_here() {
        // The caller reports it as a warning; the vocabulary just says no.
        assert_eq!(Action::parse("teleport"), None);
        assert_eq!(Action::parse("select-tab-0"), None);
        assert_eq!(Action::parse("select-tab-10"), None);
    }

    #[test]
    fn every_action_carries_prose_a_surface_can_print() {
        for action in ALL_ACTIONS {
            assert!(!action.label().is_empty(), "{action} has no label");
            assert!(
                action.description().len() > action.label().len(),
                "{action}'s description says no more than its label"
            );
        }
    }

    #[test]
    fn the_destructive_set_is_stated_not_guessed() {
        let destructive: BTreeSet<String> = ALL_ACTIONS
            .iter()
            .filter(|action| action.destructive())
            .map(|action| action.name())
            .collect();
        assert_eq!(
            destructive,
            BTreeSet::from_iter(
                [
                    "clear-prompt-history",
                    "close-tab",
                    "confirm-delete",
                    "confirm-discard",
                    "delete-file",
                    "delete-profile",
                    "discard-task",
                    "remove-plugin",
                ]
                .map(str::to_owned)
            ),
            "changing what is destructive changes what may hold a bare letter"
        );
    }
}
