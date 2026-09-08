//! Every chord has something to click.
//!
//! The product's thesis is that the keyboard is an accelerator, never the
//! way in: everything clickable, nothing behind a keystroke you had to
//! read about first. That is a claim about the whole action vocabulary,
//! and a claim nothing checks is a claim that decays — the alphabet this
//! change replaced grew one letter at a time, each of them reasonable, and
//! none of them ever offered on screen.
//!
//! So each action says where its pointer lands. The table is the point: a
//! new binding does not compile past it without someone answering "and
//! where do you click that?", which is the question that keeps this a
//! product you can use without a manual.
//!
//! The converse is deliberately *not* required. An affordance needs no
//! chord: "new space" is a button and an index entry with no key at all,
//! and that is a finished design rather than a gap.

use std::collections::BTreeMap;

use uze_keys::{ALL_ACTIONS, Action, Scope, default_keymap};

/// Where an action is reachable with the pointer alone.
enum Affordance {
    /// A control drawn for it: a button, a row, a tab, a menu entry.
    Control(&'static str),
    /// Reached from the index of everything, which is itself opened by a
    /// persistent on-screen control. Every action is reachable this way;
    /// this is for the ones that have no *other* control.
    Index,
    /// Genuinely keyboard-only, with the reason. Nothing may sit here
    /// because writing the affordance was inconvenient.
    KeyboardOnly(&'static str),
}

fn affordances() -> BTreeMap<Action, Affordance> {
    use Affordance::{Control, Index, KeyboardOnly};

    let mut map = BTreeMap::new();
    let mut put = |action: Action, affordance: Affordance| {
        map.insert(action, affordance);
    };

    // --- Everywhere -----------------------------------------------------
    put(
        Action::OpenActionIndex,
        Control("the first-steps section, at the foot of both sidebars"),
    );
    put(
        Action::SwitchMode,
        Control("the sidebar's work/manage control, in both modes"),
    );
    put(
        Action::Quit,
        Control("the sidebar's quick strip, in both modes"),
    );

    // --- Navigation -----------------------------------------------------
    // Every list row, tab and menu entry is clickable, and the wheel moves
    // a selection; these are what a pointer does natively.
    put(Action::SelectNext, Control("clicking a row, or the wheel"));
    put(
        Action::SelectPrevious,
        Control("clicking a row, or the wheel"),
    );
    put(Action::FocusNext, Control("clicking the part you want"));
    put(Action::FocusPrevious, Control("clicking the part you want"));
    put(Action::Activate, Control("clicking the row itself"));
    put(
        Action::Dismiss,
        Control("clicking away from what is open, or its close control"),
    );
    put(Action::Expand, Control("clicking a row's disclosure"));
    put(Action::Collapse, Control("clicking a row's disclosure"));
    put(Action::ScrollPageDown, Control("the wheel"));
    put(Action::ScrollPageUp, Control("the wheel"));
    put(
        Action::EraseBack,
        KeyboardOnly("erasing a character is what a keyboard is for"),
    );

    // --- Management -----------------------------------------------------
    put(Action::NextScreen, Control("clicking a sidebar row"));
    put(Action::PreviousScreen, Control("clicking a sidebar row"));
    put(Action::FocusSidebar, Control("clicking the sidebar"));
    put(Action::FocusContent, Control("clicking the screen"));
    put(Action::Refresh, Index);
    put(Action::StartFilter, Control("clicking the search field"));
    put(
        Action::OpenThemePicker,
        Control("the sidebar's quick strip"),
    );
    put(
        Action::OpenRowActions,
        Control("a row's `⋯`, or right-clicking the row"),
    );
    put(Action::ConfirmYes, Control("the dialog's own button"));
    put(Action::ConfirmNo, Control("the dialog's own button"));
    put(Action::OpenGlossary, Index);

    put(Action::InstallPlugin, Control("the row's actions"));
    put(Action::UpdatePlugin, Control("the row's actions"));
    put(Action::RemovePlugin, Control("the row's actions"));
    put(Action::AddMarketplace, Index);
    put(Action::InstallProjectEnvironment, Index);
    put(Action::ClearPromptHistory, Index);
    put(Action::SetupHarness, Control("the row's actions"));
    put(Action::AnalyzeContext, Index);
    put(Action::ApplyContextPlan, Index);
    put(
        Action::NewProfile,
        Control("the Profiles screen's new button"),
    );
    put(Action::DeleteProfile, Control("the row's actions"));
    put(Action::ActivateProfile, Control("the row's actions"));
    put(
        Action::ToggleProfileHarness,
        Control("clicking the harness checkbox"),
    );
    put(Action::NextValue, Control("clicking a preference row"));
    put(Action::PreviousValue, Control("clicking a preference row"));

    // --- Workspace ------------------------------------------------------
    put(Action::NewShellTab, Control("the tab strip's `+`"));
    put(Action::CloseTab, Control("a tab's close control"));
    put(Action::NewAgent, Control("the tab strip's `✦`"));
    put(Action::NewSpace, Control("the sidebar's `+ new` row"));
    put(
        Action::RenameSelection,
        Control("double-clicking the label, or the row's actions"),
    );
    put(Action::NextSpace, Control("clicking a space header"));
    put(Action::PreviousSpace, Control("clicking a space header"));
    put(Action::NextAgent, Control("clicking an agent row"));
    put(Action::PreviousAgent, Control("clicking an agent row"));
    put(
        Action::ToggleGitChanges,
        Control("the tab strip's git button"),
    );
    put(
        Action::DeliverTask,
        Control("the tab strip's deliver button"),
    );
    put(
        Action::DeliverAllTasks,
        KeyboardOnly(
            "delivering every task in a space at once is a deliberate bulk \
             gesture; a button for it would be one misclick from doing it",
        ),
    );
    put(
        Action::TogglePreservedWork,
        Control("the first-steps section"),
    );
    put(Action::ResumeTask, Control("its row in the preserved list"));
    put(Action::FinishTask, Control("its row in the preserved list"));
    put(
        Action::DiscardTask,
        Control("its row in the preserved list"),
    );
    put(
        Action::ConfirmDiscard,
        Control("the confirmation the row raises"),
    );
    for position in 1..=9u8 {
        put(Action::SelectTab(position), Control("clicking the tab"));
    }
    map
}

#[test]
fn every_action_says_where_its_pointer_lands() {
    let affordances = affordances();
    let missing: Vec<String> = ALL_ACTIONS
        .iter()
        .filter(|action| !affordances.contains_key(action))
        .map(|action| action.name())
        .collect();
    assert!(
        missing.is_empty(),
        "\n\nthese actions do not say how they are reached with a pointer:\n\n  {}\n\n\
         Add each to `affordances()` naming the control that performs it. If it \
         genuinely has none, say so with `KeyboardOnly` and the reason — the \
         keyboard is an accelerator here, never the way in.\n",
        missing.join("\n  ")
    );
    // A blank answer is no answer: whoever adds a row has to say what the
    // control is, or why there is none.
    for (action, affordance) in &affordances {
        let words = match affordance {
            Affordance::Control(control) => *control,
            Affordance::KeyboardOnly(reason) => *reason,
            Affordance::Index => continue,
        };
        assert!(
            words.len() > 4,
            "{action} names its affordance as {words:?}, which says nothing"
        );
    }
}

#[test]
fn a_bound_action_is_never_reachable_by_keyboard_alone_without_a_reason() {
    let affordances = affordances();
    let unreachable: std::collections::BTreeSet<String> = default_keymap()
        .bindings()
        .iter()
        .filter(|binding| binding.scope != Scope::Pane)
        .filter_map(|binding| match affordances.get(&binding.action) {
            Some(Affordance::KeyboardOnly(_)) => Some(binding.action.name()),
            _ => None,
        })
        .collect();
    // Each of these is a stated decision, not an oversight. The list is
    // short on purpose: it is the exact set of things the product asks
    // someone to know a key for, and it should stay embarrassing to add
    // to. Every entry's reason is in `affordances()` beside it.
    assert_eq!(
        unreachable,
        std::collections::BTreeSet::from_iter(
            ["deliver-all-tasks", "erase-back"].map(str::to_owned)
        ),
        "\n\nAdding one means the product now has a thing you can only do if you \
         knew the key. Say why in `affordances()`, or give it a control.\n"
    );
}
