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

/// Which of uze's two keyboards a surface belongs to.
///
/// The distinction is not cosmetic. In management uze owns the whole
/// keyboard, so an action may hold a bare letter. In the workspace every
/// bare key belongs to the program running in the pane, so an action there
/// must carry a modifier or a function key. `Both` is for the handful of
/// surfaces that mean the same thing in either place.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Mode {
    Management,
    Workspace,
    Both,
}

impl Mode {
    /// Whether a surface of this mode shares `other`'s keyboard.
    pub fn covers(self, other: Mode) -> bool {
        self == Mode::Both || other == Mode::Both || self == other
    }
}

/// One row per scope: its variant, the name a keymap file writes, how the
/// Keys screen groups it, its keyboard, whether it seals, and whether it
/// takes typing as text.
macro_rules! scopes {
    ($(
        $(#[$doc:meta])*
        $variant:ident => $name:literal, $heading:literal, $mode:ident,
            seals: $seals:literal, text: $consumes_text:literal;
    )*) => {
        /// One surface of the product, as far as the keyboard is concerned.
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
        pub enum Scope {
            $($(#[$doc])* $variant,)*
        }

        /// Every scope this build knows, outermost kinds first.
        pub const ALL_SCOPES: &[Scope] = &[$(Scope::$variant,)*];

        impl Scope {
            /// The name a keymap file writes.
            pub fn name(self) -> &'static str {
                match self {
                    $(Scope::$variant => $name,)*
                }
            }

            /// How the Keys screen groups this surface.
            pub fn heading(self) -> &'static str {
                match self {
                    $(Scope::$variant => $heading,)*
                }
            }

            /// Which keyboard this surface belongs to.
            pub fn mode(self) -> Mode {
                match self {
                    $(Scope::$variant => Mode::$mode,)*
                }
            }

            /// Whether this surface answers for everything below it, so
            /// nothing outside it — [`Scope::Global`] excepted — can fire
            /// while it is open.
            pub fn seals(self) -> bool {
                match self {
                    $(Scope::$variant => $seals,)*
                }
            }

            /// Whether ordinary typing reaches this surface as text rather
            /// than as a chord to resolve.
            pub fn consumes_text(self) -> bool {
                match self {
                    $(Scope::$variant => $consumes_text,)*
                }
            }

            pub fn parse(name: &str) -> Option<Scope> {
                match name {
                    $($name => Some(Scope::$variant),)*
                    _ => None,
                }
            }
        }
    };
}

scopes! {
    /// Live everywhere, including inside a sealed surface.
    Global => "global", "Everywhere", Both, seals: false, text: false;

    /// Management, whatever route is open.
    Management => "management", "Management", Management, seals: false, text: false;
    /// The route list, when it has focus.
    ManagementSidebar => "management-sidebar", "The route list", Management,
        seals: false, text: false;
    Overview => "overview", "Overview", Management, seals: false, text: false;
    Plugins => "plugins", "Plugins", Management, seals: false, text: false;
    Extensions => "extensions", "Extensions", Management, seals: false, text: false;
    Harnesses => "harnesses", "Integrations", Management, seals: false, text: false;
    Profiles => "profiles", "Profiles", Management, seals: false, text: false;
    /// The preference editor inside Profiles. Its own surface because
    /// left/right change a value there and move focus everywhere else —
    /// which is a different surface, not a guard on a binding.
    ProfileEditor => "profile-editor", "Editing a preference", Management,
        seals: false, text: false;
    /// The Keys screen itself.
    Keys => "keys", "Shortcuts", Management, seals: false, text: false;
    /// The Appearance screen: the palette and the glyph set, each chosen
    /// on its own.
    Appearance => "appearance", "Appearance", Management, seals: false, text: false;

    /// A list's live filter.
    Filter => "filter", "While searching", Management, seals: true, text: true;
    /// A modal asking for a line of text.
    TextPrompt => "text-prompt", "While typing an answer", Management,
        seals: true, text: true;
    /// A modal asking a yes/no question.
    Confirm => "confirm", "While being asked", Management, seals: true, text: false;
    /// The theme picker.
    ThemePicker => "theme-picker", "The theme picker", Management,
        seals: true, text: false;

    /// The workspace client, with nothing of uze's own open.
    Workspace => "workspace", "Workspace", Workspace, seals: false, text: false;
    /// The code surface: a checkout's changes, its files and a file's
    /// contents, read.
    Code => "code", "Code", Workspace, seals: true, text: false;
    /// The same surface with a file open for typing. Its own scope
    /// because it takes text — nothing behind it may answer a letter.
    CodeEditing => "code-editing", "Editing a file", Workspace, seals: true, text: false;
    /// The directory picker a new space is born from.
    RootPicker => "root-picker", "Choosing a directory", Workspace, seals: true, text: true;
    /// The inline rename buffer over a tab or space label.
    Rename => "rename", "While renaming", Workspace, seals: true, text: true;
    /// The harness picker a new agent is born from.
    AgentPicker => "agent-picker", "Choosing an agent", Workspace, seals: true, text: false;
    /// The list of work no live tab is in front of.
    PreservedWork => "preserved-work", "Preserved work", Workspace,
        seals: true, text: false;
    /// The tab/space context menu.
    ContextMenu => "context-menu", "A tab's actions", Workspace, seals: true, text: false;

    /// The index of everything, in either mode.
    ActionIndex => "action-index", "The index", Both, seals: true, text: true;
    /// The Keys screen waiting for a chord to bind. Takes every keystroke,
    /// including ones that are bound elsewhere — that is the point.
    KeyCapture => "key-capture", "While binding a key", Management, seals: true, text: false;

    /// The program running in a pane. Last, and total: anything unbound
    /// above it belongs to whatever is in there.
    Pane => "pane", "The pane", Workspace, seals: false, text: false;
}

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
