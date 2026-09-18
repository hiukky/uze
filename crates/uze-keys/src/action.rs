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

macro_rules! actions {
    ($($variant:ident => $name:literal, $destructive:expr, $label:literal, $description:literal;)*) => {
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
    OpenActionIndex => "open-action-index", false,
        "Everything you can do", "List every action available here, with the key that reaches it";
    SwitchMode => "switch-mode", false,
        "Manage", "Open or close the management modal over the workspace";
    Quit => "quit", false,
        "Quit", "Leave uze, detaching from the session rather than ending it";

    // --- Navigation, in either keyboard ---------------------------------
    SelectNext => "select-next", false,
        "Next", "Move the selection down one";
    SelectPrevious => "select-previous", false,
        "Previous", "Move the selection up one";
    FocusNext => "focus-next", false,
        "Next pane", "Move focus to the next part of the screen";
    FocusPrevious => "focus-previous", false,
        "Previous pane", "Move focus to the previous part of the screen";
    Activate => "activate", false,
        "Open", "Open, inspect, or confirm whatever is selected";
    Dismiss => "dismiss", false,
        "Close", "Close what is open, without acting on it";
    Expand => "expand", false,
        "Expand", "Unfold the selected row";
    Collapse => "collapse", false,
        "Collapse", "Fold the selected row";
    ScrollPageDown => "scroll-page-down", false,
        "Page down", "Scroll the focused view down a page";
    ScrollPageUp => "scroll-page-up", false,
        "Page up", "Scroll the focused view up a page";
    EraseBack => "erase-back", false,
        "Erase", "Delete the character before the cursor";

    // --- Management, screen-wide ----------------------------------------
    NextScreen => "next-screen", false,
        "Next screen", "Move to the next screen in the sidebar, from wherever you are";
    PreviousScreen => "previous-screen", false,
        "Previous screen", "Move to the previous screen in the sidebar, from wherever you are";
    FocusSidebar => "focus-sidebar", false,
        "Back to the sidebar", "Move focus from the content back to the route list";
    FocusContent => "focus-content", false,
        "Into the content", "Move focus from the route list into the screen";
    Refresh => "refresh", false,
        "Refresh", "Re-read the machine: harnesses, plugins, marketplaces";
    StartFilter => "start-filter", false,
        "Search", "Narrow the list by typing";
    OpenThemePicker => "open-theme-picker", false,
        "Appearance", "Choose the theme every uze surface draws in";
    ConfirmYes => "confirm-yes", false,
        "Yes", "Answer the open question with yes";
    ConfirmNo => "confirm-no", false,
        "No", "Answer the open question with no";

    // --- Management, things done to a key ------------------------------
    ChangeKey => "change-key", false,
        "Change key", "Bind the next key pressed to the selected action";
    ResetKey => "reset-key", false,
        "Reset key", "Put back the key uze ships with for the selected action";

    // --- Management, things done to a package ---------------------------
    InstallPlugin => "install-plugin", false,
        "Install", "Install the selected plugin onto this machine";
    UpdatePlugin => "update-plugin", false,
        "Update", "Update the selected plugin to what its marketplace offers";
    RemovePlugin => "remove-plugin", true,
        "Remove", "Remove the selected plugin from this machine";
    AddMarketplace => "add-marketplace", false,
        "Add marketplace", "Register a marketplace by path or URL";

    // --- Management, things done to a project ---------------------------
    InstallProjectEnvironment => "install-project-environment", false,
        "Install the project's environment", "Install what this project declares but the machine lacks";
    ClearPromptHistory => "clear-prompt-history", true,
        "Clear history", "Forget the prompts this machine has recorded";

    // --- Management, things done to a harness ---------------------------
    SetupHarness => "setup-harness", false,
        "Set up", "Prepare the selected harness to receive what uze delivers";
    AnalyzeContext => "analyze-context", false,
        "Analyze context", "Read what this project's context would become";
    ApplyContextPlan => "apply-context-plan", false,
        "Apply the plan", "Write the analyzed context into the project";
    OpenGlossary => "open-glossary", false,
        "What these words mean", "Explain the labels this screen uses";

    // --- Management, things done to a profile ---------------------------
    NewProfile => "new-profile", false,
        "New profile", "Create a profile of preferences";
    DeleteProfile => "delete-profile", true,
        "Delete", "Delete the selected profile";
    ApplyProfile => "apply-profile", false,
        "Apply", "Make the selected profile active and write it into the checked harnesses";
    PreviewProfile => "preview-profile", false,
        "Preview", "Show what the selected profile writes into each harness";
    ToggleProfileHarness => "toggle-profile-harness", false,
        "Toggle harness", "Include or exclude the highlighted harness";
    NextValue => "next-value", false,
        "Next value", "Change the highlighted preference to the next value";
    PreviousValue => "previous-value", false,
        "Previous value", "Change the highlighted preference to the previous value";

    // --- Workspace, the container ---------------------------------------
    NewShellTab => "new-shell-tab", false,
        "New shell", "Open a shell beside what is running";
    CloseTab => "close-tab", true,
        "Close tab", "Close the selected tab";
    NewAgent => "new-agent", false,
        "New agent", "Start an agent in a checkout of its own";
    NewSpace => "new-space", false,
        "New space", "Open a space at a directory";
    RenameSelection => "rename-selection", false,
        "Rename", "Rename the selected tab or space";
    NextSpace => "next-space", false,
        "Next space", "Move to the next space in the sidebar";
    PreviousSpace => "previous-space", false,
        "Previous space", "Move to the previous space in the sidebar";
    NextAgent => "next-agent", false,
        "Next agent", "Move to the next agent in this space";
    PreviousAgent => "previous-agent", false,
        "Previous agent", "Move to the previous agent in this space";
    ToggleChanges => "toggle-changes", false,
        "Changes", "Open or close the changes in the selected tab's checkout";
    ToggleFiles => "toggle-files", false,
        "Files", "Open or close the files of the selected tab's checkout";
    ToggleArchitect => "toggle-architect", false,
        "Architect", "Open or close the project's architecture diagrams";

    // --- The code surface, and typing into a file ------------------------
    EditFile => "edit-file", false,
        "Edit", "Open the selected file's contents and start typing";
    TogglePreview => "toggle-preview", false,
        "Preview", "Show a markdown file as the document it describes, and back";
    SaveFile => "save-file", false,
        "Save", "Write what was typed back to the file";
    DeleteFile => "delete-file", true,
        "Delete", "Delete the selected file, having been asked once";
    ConfirmDelete => "confirm-delete", true,
        "Confirm delete", "Confirm deleting the file, having been asked once";
    CaretLeft => "caret-left", false,
        "Left", "Move the caret one character left";
    CaretRight => "caret-right", false,
        "Right", "Move the caret one character right";
    CaretLineStart => "caret-line-start", false,
        "Line start", "Move the caret to the start of its line";
    CaretLineEnd => "caret-line-end", false,
        "Line end", "Move the caret to the end of its line";
    InsertNewline => "insert-newline", false,
        "New line", "Split the line at the caret";
    EraseForward => "erase-forward", false,
        "Delete", "Delete the character under the caret";

    // --- Workspace, the work --------------------------------------------
    DeliverTask => "deliver-task", false,
        "Deliver", "Deliver the selected task the way the project says";
    DeliverAllTasks => "deliver-all-tasks", false,
        "Deliver all", "Deliver every deliverable task in this space";
    TogglePreservedWork => "toggle-preserved-work", false,
        "Preserved work", "Show the work no live tab is in front of";
    ResumeTask => "resume-task", false,
        "Resume", "Put the selected preserved task back into a slot";
    FinishTask => "finish-task", false,
        "Mark done", "Record the selected task as finished";
    DiscardTask => "discard-task", true,
        "Discard", "Destroy the selected task's uncommitted work";
    ConfirmDiscard => "confirm-discard", true,
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
