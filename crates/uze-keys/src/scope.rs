//! Where an action is live.
//!
//! A scope is one surface of the product — a screen, an overlay, a picker,
//! the pane itself. Callers describe what is currently open as a stack,
//! outermost first, and [`crate::Keymap::resolve`] walks it inward-out.
//!
//! This is the part that used to be an ordering of `match` arms in two
//! dispatchers, with a comment admitting it was easy to get wrong by
//! inserting one in the wrong place — and it *was* wrong: three chords sat
//! above the Git overlay's arm and fired while it was open, while five sat
//! below it and did not. As a stack of values that is a table test, not a
//! reading of the source.
//!
//! Two properties do the work:
//!
//! - A scope that [`Scope::seals`] answers for everything. Nothing outside
//!   it fires, so "what does this key do" has one answer per open surface
//!   rather than one per position in a list. [`Scope::Global`] is the
//!   exception and the only one: leaving and asking for help stay reachable
//!   from anywhere, which is what makes sealing safe.
//! - A scope that [`Scope::consumes_text`] takes ordinary typing as text.
//!   Typing `q` into a filter must never quit, and that is a property of
//!   the filter, not a guard every binding has to remember.

use serde::{Deserialize, Serialize};

use crate::action::Mode;

/// One surface of the product, as far as the keyboard is concerned.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scope {
    /// Live everywhere, including inside a sealed surface.
    Global,

    /// Management, whatever route is open.
    Management,
    /// The route list, when it has focus.
    ManagementSidebar,
    Overview,
    Plugins,
    Extensions,
    Harnesses,
    Profiles,
    /// The preference editor inside Profiles. Its own surface because
    /// left/right change a value there and move focus everywhere else —
    /// which is a different surface, not a guard on a binding.
    ProfileEditor,
    /// The Keys screen itself.
    Keys,

    /// A list's live filter.
    Filter,
    /// A modal asking for a line of text.
    TextPrompt,
    /// A modal asking a yes/no question.
    Confirm,
    /// The theme picker.
    ThemePicker,
    /// The action menu a row raises.
    RowMenu,

    /// The workspace client, with nothing of uze's own open.
    Workspace,
    /// The Git extension's changes overlay.
    GitChanges,
    /// The directory picker a new space is born from.
    RootPicker,
    /// The inline rename buffer over a tab or space label.
    Rename,
    /// The harness picker a new agent is born from.
    AgentPicker,
    /// The list of work no live tab is in front of.
    PreservedWork,
    /// The tab/space context menu.
    ContextMenu,

    /// The index of everything, in either mode.
    ActionIndex,
    /// The Keys screen waiting for a chord to bind. Takes every keystroke,
    /// including ones that are bound elsewhere — that is the point.
    KeyCapture,

    /// The program running in a pane. Last, and total: anything unbound
    /// above it belongs to whatever is in there.
    Pane,
}

impl Scope {
    /// Which keyboard this surface belongs to.
    pub fn mode(self) -> Mode {
        match self {
            Scope::Global | Scope::ActionIndex => Mode::Both,
            Scope::Management
            | Scope::ManagementSidebar
            | Scope::Overview
            | Scope::Plugins
            | Scope::Extensions
            | Scope::Harnesses
            | Scope::Profiles
            | Scope::ProfileEditor
            | Scope::Keys
            | Scope::Filter
            | Scope::TextPrompt
            | Scope::Confirm
            | Scope::ThemePicker
            | Scope::RowMenu
            | Scope::KeyCapture => Mode::Management,
            Scope::Workspace
            | Scope::GitChanges
            | Scope::RootPicker
            | Scope::Rename
            | Scope::AgentPicker
            | Scope::PreservedWork
            | Scope::ContextMenu
            | Scope::Pane => Mode::Workspace,
        }
    }

    /// Whether this surface answers for everything below it, so nothing
    /// outside it — [`Scope::Global`] excepted — can fire while it is open.
    pub fn seals(self) -> bool {
        self.consumes_text()
            || matches!(
                self,
                Scope::Confirm
                    | Scope::ThemePicker
                    | Scope::RowMenu
                    | Scope::GitChanges
                    | Scope::AgentPicker
                    | Scope::PreservedWork
                    | Scope::ContextMenu
                    | Scope::KeyCapture
            )
    }

    /// Whether ordinary typing reaches this surface as text rather than as
    /// a chord to resolve.
    pub fn consumes_text(self) -> bool {
        matches!(
            self,
            Scope::Filter
                | Scope::TextPrompt
                | Scope::RootPicker
                | Scope::Rename
                | Scope::ActionIndex
        )
    }

    /// The name a keymap file writes.
    pub fn name(self) -> &'static str {
        match self {
            Scope::Global => "global",
            Scope::Management => "management",
            Scope::ManagementSidebar => "management-sidebar",
            Scope::Overview => "overview",
            Scope::Plugins => "plugins",
            Scope::Extensions => "extensions",
            Scope::Harnesses => "harnesses",
            Scope::Profiles => "profiles",
            Scope::ProfileEditor => "profile-editor",
            Scope::Keys => "keys",
            Scope::Filter => "filter",
            Scope::TextPrompt => "text-prompt",
            Scope::Confirm => "confirm",
            Scope::ThemePicker => "theme-picker",
            Scope::RowMenu => "row-menu",
            Scope::Workspace => "workspace",
            Scope::GitChanges => "git-changes",
            Scope::RootPicker => "root-picker",
            Scope::Rename => "rename",
            Scope::AgentPicker => "agent-picker",
            Scope::PreservedWork => "preserved-work",
            Scope::ContextMenu => "context-menu",
            Scope::ActionIndex => "action-index",
            Scope::KeyCapture => "key-capture",
            Scope::Pane => "pane",
        }
    }

    /// How the Keys screen groups this surface.
    pub fn heading(self) -> &'static str {
        match self {
            Scope::Global => "Everywhere",
            Scope::Management => "Management",
            Scope::ManagementSidebar => "The route list",
            Scope::Overview => "Overview",
            Scope::Plugins => "Plugins",
            Scope::Extensions => "Extensions",
            Scope::Harnesses => "Integrations",
            Scope::Profiles => "Profiles",
            Scope::ProfileEditor => "Editing a preference",
            Scope::Keys => "Keys",
            Scope::Filter => "While searching",
            Scope::TextPrompt => "While typing an answer",
            Scope::Confirm => "While being asked",
            Scope::ThemePicker => "Appearance",
            Scope::RowMenu => "A row's actions",
            Scope::Workspace => "Workspace",
            Scope::GitChanges => "Changes",
            Scope::RootPicker => "Choosing a directory",
            Scope::Rename => "While renaming",
            Scope::AgentPicker => "Choosing an agent",
            Scope::PreservedWork => "Preserved work",
            Scope::ContextMenu => "A tab's actions",
            Scope::ActionIndex => "The index",
            Scope::KeyCapture => "While binding a key",
            Scope::Pane => "The pane",
        }
    }

    pub fn parse(name: &str) -> Option<Scope> {
        ALL_SCOPES
            .iter()
            .copied()
            .find(|scope| scope.name() == name)
    }
}

/// Every scope this build knows, outermost kinds first.
pub const ALL_SCOPES: &[Scope] = &[
    Scope::Global,
    Scope::Management,
    Scope::ManagementSidebar,
    Scope::Overview,
    Scope::Plugins,
    Scope::Extensions,
    Scope::Harnesses,
    Scope::Profiles,
    Scope::ProfileEditor,
    Scope::Keys,
    Scope::Filter,
    Scope::TextPrompt,
    Scope::Confirm,
    Scope::ThemePicker,
    Scope::RowMenu,
    Scope::Workspace,
    Scope::GitChanges,
    Scope::RootPicker,
    Scope::Rename,
    Scope::AgentPicker,
    Scope::PreservedWork,
    Scope::ContextMenu,
    Scope::ActionIndex,
    Scope::KeyCapture,
    Scope::Pane,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_scope_names_itself_uniquely_and_reads_back() {
        for scope in ALL_SCOPES {
            assert_eq!(Scope::parse(scope.name()), Some(*scope));
            assert!(!scope.heading().is_empty());
        }
        let names: std::collections::BTreeSet<&str> =
            ALL_SCOPES.iter().map(|scope| scope.name()).collect();
        assert_eq!(names.len(), ALL_SCOPES.len(), "two scopes share a name");
    }

    #[test]
    fn a_surface_that_takes_typing_always_seals() {
        // Otherwise typing `q` into a filter quits, which is the bug this
        // property exists to make impossible rather than remembered.
        for scope in ALL_SCOPES.iter().filter(|scope| scope.consumes_text()) {
            assert!(
                scope.seals(),
                "{} takes text but does not seal",
                scope.name()
            );
        }
    }

    #[test]
    fn the_global_scope_never_seals() {
        assert!(!Scope::Global.seals());
    }
}
