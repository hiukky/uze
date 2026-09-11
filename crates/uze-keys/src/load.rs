//! The built-in keymap, and what an operator's file does to it.
//!
//! The split between an error and a warning is the same one the theme
//! loader documents, for the same reason. A file uze cannot make sense of —
//! a chord that is not a chord, two actions reaching for one mnemonic — is
//! an error, and the keymap already in force stays in force, because an
//! operator without a keyboard cannot fix the file that took it away. A file
//! that names something *this build* does not know is a warning: a keymap
//! written for a newer uze must still load on an older one, or every keymap
//! in the wild breaks the first time the vocabulary grows.

use std::{fs, io, path::Path, sync::OnceLock};

use crate::{
    action::Action,
    chord::{Chord, ChordProblem},
    file::KeymapFile,
    keymap::{Binding, Keymap},
    scope::Scope,
};

/// Something worth telling the operator about their keymap.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Problem {
    pub severity: Severity,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Severity {
    /// The file is unusable as written; the keymap in force is unchanged.
    Error,
    /// The file loaded; this entry did not, and the built-in default
    /// applies for it.
    Warning,
}

impl Problem {
    fn error(message: impl Into<String>) -> Problem {
        Problem {
            severity: Severity::Error,
            message: message.into(),
        }
    }

    fn warning(message: impl Into<String>) -> Problem {
        Problem {
            severity: Severity::Warning,
            message: message.into(),
        }
    }
}

/// A keymap that loaded, with whatever was worth saying about it.
#[derive(Clone, Debug)]
pub struct Loaded {
    pub keymap: Keymap,
    pub problems: Vec<Problem>,
}

/// The keymap uze ships with. Everything an operator writes is a
/// difference from this.
pub fn default_keymap() -> &'static Keymap {
    static DEFAULT: OnceLock<Keymap> = OnceLock::new();
    DEFAULT.get_or_init(|| {
        Keymap::new(default_bindings()).expect("the built-in keymap has no conflicts")
    })
}

/// Reads a keymap file, or `None` when there is none — which is not a
/// problem: the built-in default is a complete keymap on its own.
pub fn read(path: &Path) -> io::Result<Option<KeymapFile>> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    serde_json::from_str(&contents)
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))
}

/// Applies an operator's file over the built-in default.
///
/// `Err` carries the reasons the file cannot be used at all; the caller
/// keeps whatever keymap it already had. `Ok` carries the resolved keymap
/// and the entries that were skipped.
pub fn resolve(file: &KeymapFile) -> Result<Loaded, Vec<Problem>> {
    let mut problems = Vec::new();
    let mut errors = Vec::new();
    let mut bindings = default_bindings();

    if let Some(version) = file.version
        && version > crate::file::CURRENT_VERSION
    {
        problems.push(Problem::warning(format!(
            "this keymap is written for schema version {version}; this build understands {}",
            crate::file::CURRENT_VERSION
        )));
    }

    for (scope_name, actions) in &file.bindings {
        let Some(scope) = Scope::parse(scope_name) else {
            problems.push(Problem::warning(format!(
                "`{scope_name}` is not a surface this build knows; its bindings are ignored"
            )));
            continue;
        };
        for (action_name, chords) in actions {
            let Some(action) = Action::parse(action_name) else {
                problems.push(Problem::warning(format!(
                    "`{action_name}` is not an action this build knows; it is ignored"
                )));
                continue;
            };
            let mut written = Vec::new();
            let mut readable = true;
            for text in chords {
                match Chord::parse(text) {
                    Ok(chord) => written.push(chord),
                    Err(problem) => {
                        readable = false;
                        errors.push(Problem::error(format!(
                            "{scope_name}.{action_name}: {}",
                            describe(&problem)
                        )));
                    }
                }
            }
            if !readable {
                continue;
            }
            // Declaring an action replaces every default chord it had in
            // that surface — including with nothing, which is how an
            // operator says "no key, the button is enough".
            bindings.retain(|binding| !(binding.action == action && binding.scope == scope));
            bindings.extend(written.into_iter().map(|chord| Binding {
                scope,
                chord,
                action,
            }));
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }
    match Keymap::new(bindings) {
        Ok(keymap) => Ok(Loaded { keymap, problems }),
        Err(conflicts) => Err(conflicts
            .into_iter()
            .map(|conflict| Problem::error(conflict.to_string()))
            .collect()),
    }
}

/// What an operator's file would have to say to produce `keymap` —
/// only the surfaces and actions that differ from the built-in default.
///
/// This is what makes a later release able to move a chord nobody had an
/// opinion about: an entry appears here because someone changed it, not
/// because the screen that changed one had to write the whole keyboard
/// out.
pub fn difference_from_default(keymap: &Keymap) -> KeymapFile {
    let mut file = KeymapFile::default();
    let default = default_keymap();
    let mut pairs: Vec<(Scope, Action)> = default
        .bindings()
        .iter()
        .chain(keymap.bindings())
        .map(|binding| (binding.scope, binding.action))
        .collect();
    pairs.sort();
    pairs.dedup();
    for (scope, action) in pairs {
        let chords = |source: &Keymap| -> Vec<String> {
            source
                .bindings()
                .iter()
                .filter(|binding| binding.scope == scope && binding.action == action)
                .map(|binding| binding.chord.to_string())
                .collect()
        };
        let theirs = chords(keymap);
        if theirs != chords(default) {
            file.set(scope.name(), &action.name(), theirs);
        }
    }
    file
}

fn describe(problem: &ChordProblem) -> String {
    problem.to_string()
}

fn bind(scope: Scope, chord: &str, action: Action) -> Binding {
    Binding {
        scope,
        chord: Chord::parse(chord).expect("a built-in chord parses"),
        action,
    }
}

/// The keymap uze ships with.
///
/// Three rules shape it, and every entry is an instance of one of them:
///
/// 1. **In management, uze owns the keyboard**, so an action may hold a
///    bare letter — and a letter names one action, everywhere. `r` removes
///    and nothing else; a bare letter always acts on the row you are on,
///    which is why refreshing carries a modifier instead.
/// 2. **In the workspace, uze is a guest.** Every bare key belongs to the
///    program in the pane, and every chord uze takes is one an agent's
///    input loses. Function keys and modified navigation keys are the
///    register, and the five `Ctrl` letters that were here before this
///    design stay because they are already in people's fingers.
/// 3. **An action with no chord is finished, not unfinished.** Analyzing
///    context, adding a space, installing what a project declares: all are
///    offered by a button, a row's actions and the index, and none needs a
///    key invented for it.
fn default_bindings() -> Vec<Binding> {
    let mut bindings = vec![
        // --- Everywhere -------------------------------------------------
        // F1, and only F1: in the workspace `?` belongs to whatever the
        // agent is typing into, and a function key is the one register a
        // terminal program almost never claims. Management had a `?` of
        // its own for a while, which meant the way to help was a different
        // key depending on which mode you were in — and since a surface
        // prints the innermost chord it finds, the one key that works in
        // both was the one never shown.
        bind(Scope::Global, "f1", Action::OpenActionIndex),
        bind(Scope::Global, "ctrl+o", Action::SwitchMode),
        bind(Scope::Global, "ctrl+q", Action::Quit),
        // --- Management, screen-wide ------------------------------------
        bind(Scope::Management, "down", Action::SelectNext),
        bind(Scope::Management, "j", Action::SelectNext),
        bind(Scope::Management, "up", Action::SelectPrevious),
        bind(Scope::Management, "k", Action::SelectPrevious),
        bind(Scope::Management, "tab", Action::FocusNext),
        bind(Scope::Management, "shift+tab", Action::FocusPrevious),
        bind(Scope::Management, "enter", Action::Activate),
        bind(Scope::Management, "esc", Action::Dismiss),
        // The sidebar is vertical and holds the screens, exactly as the
        // workspace's sidebar is vertical and holds the spaces — so the
        // same chord walks both, from anywhere, without first having to
        // put the focus back on the list.
        bind(Scope::Management, "ctrl+down", Action::NextScreen),
        bind(Scope::Management, "ctrl+up", Action::PreviousScreen),
        bind(Scope::Management, "left", Action::FocusSidebar),
        bind(Scope::Management, "h", Action::FocusSidebar),
        bind(Scope::Management, "right", Action::FocusContent),
        bind(Scope::Management, "l", Action::FocusContent),
        // A modifier rather than a letter: refreshing is not a thing you
        // do to the row you are on, and a bare `g` beside a screen full of
        // bare letters that all act on a selection read as one of them.
        bind(Scope::Management, "ctrl+r", Action::Refresh),
        bind(Scope::Management, "f5", Action::Refresh),
        bind(Scope::Management, "/", Action::StartFilter),
        bind(Scope::Management, "t", Action::OpenThemePicker),
        bind(Scope::Management, "m", Action::AddMarketplace),
        bind(Scope::Management, "q", Action::Quit),
        bind(Scope::Management, "ctrl+c", Action::Quit),
        // --- Management, per screen -------------------------------------
        bind(Scope::Overview, "x", Action::ClearPromptHistory),
        bind(Scope::Plugins, "i", Action::InstallPlugin),
        bind(Scope::Plugins, "u", Action::UpdatePlugin),
        bind(Scope::Plugins, "r", Action::RemovePlugin),
        bind(Scope::Harnesses, "s", Action::SetupHarness),
        bind(Scope::Harnesses, "a", Action::AnalyzeContext),
        bind(Scope::Harnesses, "p", Action::ApplyContextPlan),
        bind(Scope::Profiles, "n", Action::NewProfile),
        bind(Scope::Profiles, "d", Action::DeleteProfile),
        bind(Scope::Profiles, "space", Action::ToggleProfileHarness),
        bind(Scope::Profiles, "v", Action::PreviewProfile),
        bind(Scope::ProfileEditor, "left", Action::PreviousValue),
        bind(Scope::ProfileEditor, "right", Action::NextValue),
        // --- Management, the surfaces that seal -------------------------
        bind(Scope::Filter, "enter", Action::Activate),
        bind(Scope::Filter, "esc", Action::Dismiss),
        bind(Scope::Filter, "backspace", Action::EraseBack),
        bind(Scope::TextPrompt, "enter", Action::Activate),
        bind(Scope::TextPrompt, "esc", Action::Dismiss),
        bind(Scope::TextPrompt, "backspace", Action::EraseBack),
        bind(Scope::Confirm, "enter", Action::Activate),
        bind(Scope::Confirm, "esc", Action::Dismiss),
        bind(Scope::Confirm, "tab", Action::FocusNext),
        bind(Scope::Confirm, "shift+tab", Action::FocusPrevious),
        bind(Scope::Confirm, "left", Action::FocusPrevious),
        bind(Scope::Confirm, "right", Action::FocusNext),
        bind(Scope::Confirm, "y", Action::ConfirmYes),
        bind(Scope::Confirm, "n", Action::ConfirmNo),
        bind(Scope::ThemePicker, "down", Action::SelectNext),
        bind(Scope::ThemePicker, "j", Action::SelectNext),
        bind(Scope::ThemePicker, "up", Action::SelectPrevious),
        bind(Scope::ThemePicker, "k", Action::SelectPrevious),
        bind(Scope::ThemePicker, "enter", Action::Activate),
        bind(Scope::ThemePicker, "esc", Action::Dismiss),
        bind(Scope::ThemePicker, "q", Action::Dismiss),
        bind(Scope::KeyCapture, "esc", Action::Dismiss),
        // --- The index, in either mode ----------------------------------
        bind(Scope::ActionIndex, "down", Action::SelectNext),
        bind(Scope::ActionIndex, "up", Action::SelectPrevious),
        bind(Scope::ActionIndex, "enter", Action::Activate),
        bind(Scope::ActionIndex, "esc", Action::Dismiss),
        bind(Scope::ActionIndex, "backspace", Action::EraseBack),
        // --- Workspace, the container -----------------------------------
        bind(Scope::Workspace, "ctrl+t", Action::NewShellTab),
        bind(Scope::Workspace, "ctrl+w", Action::CloseTab),
        bind(Scope::Workspace, "ctrl+g", Action::ToggleChanges),
        bind(Scope::Workspace, "ctrl+e", Action::ToggleFiles),
        bind(Scope::Workspace, "alt+n", Action::NewAgent),
        bind(Scope::Workspace, "f2", Action::RenameSelection),
        // The sidebar is vertical and holds spaces; the strip is
        // horizontal and holds tabs. Ctrl walks the container, Alt walks
        // what is inside it.
        bind(Scope::Workspace, "ctrl+up", Action::PreviousSpace),
        bind(Scope::Workspace, "ctrl+down", Action::NextSpace),
        bind(Scope::Workspace, "alt+up", Action::PreviousAgent),
        bind(Scope::Workspace, "alt+down", Action::NextAgent),
        // --- Workspace, the work ----------------------------------------
        bind(Scope::Workspace, "alt+i", Action::DeliverTask),
        bind(Scope::Workspace, "alt+shift+i", Action::DeliverAllTasks),
        bind(Scope::Workspace, "alt+p", Action::TogglePreservedWork),
        // --- Workspace, the surfaces that seal --------------------------
        bind(Scope::Code, "esc", Action::Dismiss),
        // The same chord closes it: opening and closing one thing is one
        // action to learn, not two.
        bind(Scope::Code, "ctrl+g", Action::Dismiss),
        bind(Scope::Code, "tab", Action::FocusNext),
        bind(Scope::Code, "down", Action::SelectNext),
        bind(Scope::Code, "up", Action::SelectPrevious),
        bind(Scope::Code, "left", Action::Collapse),
        bind(Scope::Code, "right", Action::Expand),
        bind(Scope::Code, "enter", Action::Activate),
        bind(Scope::Code, "pagedown", Action::ScrollPageDown),
        bind(Scope::Code, "pageup", Action::ScrollPageUp),
        bind(Scope::Code, "ctrl+e", Action::ToggleFiles),
        bind(Scope::Code, "e", Action::EditFile),
        bind(Scope::Code, "p", Action::TogglePreview),
        bind(Scope::Code, "d", Action::DeleteFile),
        bind(Scope::Code, "y", Action::ConfirmDelete),
        // Typing has a scope of its own so nothing behind it answers a
        // letter — the same reason the action index has one.
        bind(Scope::CodeEditing, "esc", Action::Dismiss),
        bind(Scope::CodeEditing, "ctrl+s", Action::SaveFile),
        bind(Scope::CodeEditing, "up", Action::SelectPrevious),
        bind(Scope::CodeEditing, "down", Action::SelectNext),
        bind(Scope::CodeEditing, "left", Action::CaretLeft),
        bind(Scope::CodeEditing, "right", Action::CaretRight),
        bind(Scope::CodeEditing, "home", Action::CaretLineStart),
        bind(Scope::CodeEditing, "end", Action::CaretLineEnd),
        bind(Scope::CodeEditing, "enter", Action::InsertNewline),
        bind(Scope::CodeEditing, "backspace", Action::EraseBack),
        bind(Scope::CodeEditing, "delete", Action::EraseForward),
        bind(Scope::PreservedWork, "esc", Action::Dismiss),
        bind(Scope::PreservedWork, "down", Action::SelectNext),
        bind(Scope::PreservedWork, "up", Action::SelectPrevious),
        bind(Scope::PreservedWork, "i", Action::DeliverTask),
        bind(Scope::PreservedWork, "f", Action::FinishTask),
        bind(Scope::PreservedWork, "r", Action::ResumeTask),
        bind(Scope::PreservedWork, "d", Action::DiscardTask),
        bind(Scope::PreservedWork, "y", Action::ConfirmDiscard),
        bind(Scope::AgentPicker, "down", Action::SelectNext),
        bind(Scope::AgentPicker, "up", Action::SelectPrevious),
        bind(Scope::AgentPicker, "enter", Action::Activate),
        bind(Scope::AgentPicker, "esc", Action::Dismiss),
        bind(Scope::ContextMenu, "down", Action::SelectNext),
        bind(Scope::ContextMenu, "up", Action::SelectPrevious),
        bind(Scope::ContextMenu, "enter", Action::Activate),
        bind(Scope::ContextMenu, "esc", Action::Dismiss),
        bind(Scope::RootPicker, "down", Action::SelectNext),
        bind(Scope::RootPicker, "up", Action::SelectPrevious),
        // Tab walks into the highlighted directory, so a root several
        // levels down is reached by narrowing rather than by typing.
        bind(Scope::RootPicker, "tab", Action::Expand),
        bind(Scope::RootPicker, "enter", Action::Activate),
        bind(Scope::RootPicker, "esc", Action::Dismiss),
        bind(Scope::RootPicker, "backspace", Action::EraseBack),
        bind(Scope::Rename, "enter", Action::Activate),
        bind(Scope::Rename, "esc", Action::Dismiss),
        bind(Scope::Rename, "backspace", Action::EraseBack),
    ];
    for index in 1..=9u8 {
        bindings.push(bind(
            Scope::Workspace,
            &format!("alt+{index}"),
            Action::SelectTab(index),
        ));
    }
    bindings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        action::ALL_ACTIONS,
        chord::Tier,
        keymap::Resolution,
        scope::{ALL_SCOPES, Scope},
    };

    #[test]
    fn the_built_in_keymap_has_no_conflicts() {
        // `Keymap::new` refuses one, so this passing is the whole claim.
        default_keymap();
    }

    #[test]
    fn every_action_is_either_bound_or_deliberately_unbound() {
        // The list is the design: an action here is one the product offers
        // by pointer and does not spend a chord on. Adding an action
        // without a chord means adding it here, which is where someone
        // asks whether that was intended.
        let unbound: Vec<String> = ALL_ACTIONS
            .iter()
            .filter(|action| default_keymap().any_chord_for(**action).is_none())
            .map(|action| action.name())
            .collect();
        assert_eq!(
            unbound,
            vec![
                // Enter on the Keys screen already asks for a key, and a
                // reset is rare enough to live on its button and menu.
                "change-key",
                "reset-key",
                "install-project-environment",
                "open-glossary",
                "apply-profile",
                "new-space"
            ],
            "an action gained or lost a chord; say so here on purpose"
        );
    }

    #[test]
    fn a_destructive_action_on_a_bare_key_is_a_decision_someone_made() {
        // A bare key is one keystroke away at all times, so putting a
        // destructive action on one is a choice that has to be defended —
        // each of these does ask before it acts. The list is here rather
        // than in a comment so that adding another fails the build and
        // makes someone say why.
        //
        // The code surface's `d`/`y` are the newest pair, and they are the
        // same shape as preserved work's: `d` only raises the question,
        // and `y` is the answer to a question the footer is asking at that
        // moment. Neither is live outside that surface — the scope is
        // sealed while it is open — and neither can reach a directory.
        let bare: Vec<String> = default_keymap()
            .bindings()
            .iter()
            .filter(|binding| {
                binding.action.destructive() && !binding.chord.mods.ctrl && !binding.chord.mods.alt
            })
            .map(|binding| {
                format!(
                    "{}.{}={}",
                    binding.scope.name(),
                    binding.action,
                    binding.chord
                )
            })
            .collect();
        assert_eq!(
            bare,
            vec![
                "overview.clear-prompt-history=x",
                "plugins.remove-plugin=r",
                "profiles.delete-profile=d",
                "code.delete-file=d",
                "code.confirm-delete=y",
                "preserved-work.discard-task=d",
                "preserved-work.confirm-discard=y",
            ]
        );
    }

    #[test]
    fn the_workspace_spends_only_what_it_means_to() {
        // Every workspace chord is one an agent's input loses, so the set
        // is small on purpose and every member carries a modifier or is a
        // function key — a bare letter there would be stolen typing.
        for binding in default_keymap().bindings() {
            if binding.scope != Scope::Workspace {
                continue;
            }
            let carries_a_modifier = binding.chord.mods.ctrl || binding.chord.mods.alt;
            let is_function_key = matches!(binding.chord.key, crate::chord::Key::F(_));
            assert!(
                carries_a_modifier || is_function_key,
                "`{}` is a bare key the pane would never receive",
                binding.chord
            );
        }
    }

    #[test]
    fn no_default_chord_needs_a_protocol_the_operator_may_not_have() {
        for binding in default_keymap().bindings() {
            assert_ne!(
                binding.chord.tier(),
                Tier::EnhancementOnly,
                "`{}` is unbindable on a plain terminal, so it cannot be a default",
                binding.chord
            );
        }
    }

    #[test]
    fn every_scope_that_seals_can_be_left() {
        for scope in ALL_SCOPES.iter().filter(|scope| scope.seals()) {
            let leaves = default_keymap()
                .bindings()
                .iter()
                .any(|binding| binding.scope == *scope && binding.action == Action::Dismiss);
            assert!(leaves, "{} seals with no way out", scope.name());
        }
    }

    #[test]
    fn an_operators_file_replaces_only_what_it_names() {
        let mut file = KeymapFile::default();
        file.set("workspace", "new-shell-tab", vec!["f4".to_owned()]);
        let loaded = resolve(&file).expect("resolves");
        assert!(loaded.problems.is_empty());
        assert_eq!(
            loaded.keymap.resolve(
                Chord::parse("f4").unwrap(),
                &[Scope::Workspace, Scope::Pane]
            ),
            Resolution::Act(Action::NewShellTab)
        );
        assert_eq!(
            loaded.keymap.resolve(
                Chord::parse("ctrl+t").unwrap(),
                &[Scope::Workspace, Scope::Pane]
            ),
            Resolution::Fallthrough,
            "declaring an action replaces its chords rather than adding to them"
        );
        assert_eq!(
            loaded.keymap.resolve(
                Chord::parse("ctrl+w").unwrap(),
                &[Scope::Workspace, Scope::Pane]
            ),
            Resolution::Act(Action::CloseTab),
            "everything unnamed keeps whatever the default says"
        );
    }

    #[test]
    fn an_empty_list_unbinds_and_the_action_survives_it() {
        let mut file = KeymapFile::default();
        file.set("workspace", "close-tab", Vec::new());
        let loaded = resolve(&file).expect("resolves");
        assert_eq!(
            loaded
                .keymap
                .chord_for(Action::CloseTab, &[Scope::Workspace]),
            None
        );
        assert!(
            ALL_ACTIONS.contains(&Action::CloseTab),
            "unbinding takes the key, never the action"
        );
    }

    #[test]
    fn a_keymap_written_for_a_newer_uze_still_loads() {
        let mut file = KeymapFile::default();
        file.set("workspace", "teleport", vec!["f7".to_owned()]);
        file.set("holodeck", "close-tab", vec!["f8".to_owned()]);
        file.set("workspace", "new-shell-tab", vec!["f4".to_owned()]);
        file.version = Some(crate::file::CURRENT_VERSION + 1);

        let loaded = resolve(&file).expect("the rest of the file still applies");
        assert_eq!(loaded.problems.len(), 3, "{:?}", loaded.problems);
        assert!(
            loaded
                .problems
                .iter()
                .all(|problem| problem.severity == Severity::Warning)
        );
        assert_eq!(
            loaded
                .keymap
                .chord_for(Action::NewShellTab, &[Scope::Workspace]),
            Chord::parse("f4").ok(),
            "the entries this build understands are still applied"
        );
    }

    #[test]
    fn a_file_that_cannot_be_used_leaves_the_keyboard_alone() {
        let mut refused = KeymapFile::default();
        refused.set("workspace", "close-tab", vec!["ctrl+m".to_owned()]);
        let problems = resolve(&refused).expect_err("ctrl+m is Enter");
        assert!(problems[0].message.contains("Enter"), "{problems:?}");
        assert_eq!(problems[0].severity, Severity::Error);

        let mut conflicting = KeymapFile::default();
        conflicting.set("plugins", "install-plugin", vec!["r".to_owned()]);
        let problems = resolve(&conflicting).expect_err("`r` already removes");
        assert!(problems[0].message.contains("`r`"), "{problems:?}");
    }

    /// The screen that rebinds writes only what someone changed, so a
    /// later release can still move a chord nobody had an opinion about.
    #[test]
    fn only_what_differs_is_written_down() {
        assert!(
            difference_from_default(default_keymap()).is_empty(),
            "an untouched keyboard writes nothing at all"
        );

        let moved = default_keymap()
            .rebind(
                Action::NewShellTab,
                Scope::Workspace,
                Chord::parse("f4").ok(),
            )
            .expect("no conflict");
        let file = difference_from_default(&moved);
        assert_eq!(
            file.bindings["workspace"]["new-shell-tab"],
            vec!["f4".to_owned()]
        );
        assert_eq!(file.bindings["workspace"].len(), 1, "and nothing else");

        // And it round-trips: what the screen writes is what the loader
        // reads back.
        assert_eq!(resolve(&file).expect("resolves").keymap, moved);
    }

    #[test]
    fn an_unbinding_survives_the_round_trip() {
        let unbound = default_keymap()
            .rebind(Action::CloseTab, Scope::Workspace, None)
            .expect("no conflict");
        let file = difference_from_default(&unbound);
        assert_eq!(
            file.bindings["workspace"]["close-tab"],
            Vec::<String>::new(),
            "an empty list is how `no key` is written"
        );
        assert_eq!(resolve(&file).expect("resolves").keymap, unbound);
    }

    #[test]
    fn a_missing_file_is_not_a_problem() {
        let missing = std::env::temp_dir().join("uze-keys-there-is-no-such-file.json");
        assert_eq!(read(&missing).expect("no error"), None);
    }
}
