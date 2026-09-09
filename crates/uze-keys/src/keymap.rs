//! The resolved keymap: which chord reaches which action, where.
//!
//! Resolution and its reverse are the two things every surface needs and
//! neither may re-derive. Dispatch asks [`Keymap::resolve`] what a
//! keystroke means; anything that prints a key asks [`Keymap::chord_for`]
//! what key reaches an action. There is no third way to learn either, which
//! is what makes a rebound key correct everywhere at once and a printed key
//! impossible to invent.

use std::collections::BTreeMap;

use crate::{
    action::Action,
    chord::{Chord, Key},
    scope::Scope,
};

/// One entry of a keymap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Binding {
    pub scope: Scope,
    pub chord: Chord,
    pub action: Action,
}

/// Two actions reachable by one chord in one keyboard — refused, because
/// which one fires would otherwise depend on where the surfaces happen to
/// sit in a stack.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Conflict {
    pub chord: Chord,
    pub first: Binding,
    pub second: Binding,
}

impl std::fmt::Display for Conflict {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "`{}` reaches both `{}` ({}) and `{}` ({})",
            self.chord,
            self.first.action,
            self.first.scope.name(),
            self.second.action,
            self.second.scope.name()
        )
    }
}

/// What a keystroke turned out to mean.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Resolution {
    /// It is this action.
    Act(Action),
    /// The innermost open surface takes typing, and this is typing.
    Text,
    /// Nothing is bound: it belongs to whatever is below — the program in
    /// a pane, or nothing at all.
    Fallthrough,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Keymap {
    bindings: Vec<Binding>,
    by_chord: BTreeMap<(Scope, Chord), Action>,
}

impl Keymap {
    /// Builds a keymap, refusing one in which a chord reaches two actions
    /// in the same keyboard.
    pub fn new(bindings: Vec<Binding>) -> Result<Keymap, Vec<Conflict>> {
        let conflicts = conflicts_in(&bindings);
        if !conflicts.is_empty() {
            return Err(conflicts);
        }
        let by_chord = bindings
            .iter()
            .map(|binding| ((binding.scope, binding.chord), binding.action))
            .collect();
        Ok(Keymap { bindings, by_chord })
    }

    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }

    /// What `chord` means with `scopes` open, given outermost first.
    ///
    /// Walks inward-out: the innermost surface answers first, a surface
    /// that seals answers for everything below it, and [`Scope::Global`] is
    /// consulted whatever else is open — which is what keeps leaving and
    /// asking for help reachable from inside a modal.
    pub fn resolve(&self, chord: Chord, scopes: &[Scope]) -> Resolution {
        for scope in scopes.iter().rev() {
            if let Some(action) = self.by_chord.get(&(*scope, chord)) {
                return Resolution::Act(*action);
            }
            if scope.consumes_text() && is_text(chord) {
                return Resolution::Text;
            }
            if scope.seals() {
                break;
            }
        }
        match self.by_chord.get(&(Scope::Global, chord)) {
            Some(action) => Resolution::Act(*action),
            None => Resolution::Fallthrough,
        }
    }

    /// The scopes a keystroke can actually reach with `scopes` open,
    /// innermost first: everything up to and including the first surface
    /// that seals, plus [`Scope::Global`], which nothing seals off.
    ///
    /// Resolution and the reverse lookup both walk this, so a surface can
    /// never print a key that a seal would have swallowed.
    fn reachable(scopes: &[Scope]) -> Vec<Scope> {
        let mut reachable = Vec::new();
        for scope in scopes.iter().rev() {
            reachable.push(*scope);
            if scope.seals() {
                break;
            }
        }
        if !reachable.contains(&Scope::Global) {
            reachable.push(Scope::Global);
        }
        reachable
    }

    /// Every chord that reaches `action` with `scopes` open, innermost
    /// first. Empty for an action that is deliberately unbound — which is a
    /// finished design, not a gap, and every surface prints it as one.
    pub fn chords_for(&self, action: Action, scopes: &[Scope]) -> Vec<Chord> {
        let mut chords: Vec<Chord> = Vec::new();
        for scope in Keymap::reachable(scopes) {
            for binding in &self.bindings {
                if binding.scope == scope
                    && binding.action == action
                    && !chords.contains(&binding.chord)
                {
                    chords.push(binding.chord);
                }
            }
        }
        chords
    }

    /// The chord a surface prints for `action` — the first one that reaches
    /// it. The only way any surface may learn a key.
    pub fn chord_for(&self, action: Action, scopes: &[Scope]) -> Option<Chord> {
        self.chords_for(action, scopes).into_iter().next()
    }

    /// The same question asked without a context, for surfaces that list
    /// the whole vocabulary rather than what is open right now.
    pub fn any_chord_for(&self, action: Action) -> Option<Chord> {
        self.bindings
            .iter()
            .find(|binding| binding.action == action)
            .map(|binding| binding.chord)
    }

    /// Every action reachable with `scopes` open, in vocabulary order, each
    /// with the chord that reaches it. This is the index, and it is also
    /// the help: one list, so there is no second one to disagree with it.
    pub fn available(&self, scopes: &[Scope]) -> Vec<(Action, Option<Chord>)> {
        let reachable = Keymap::reachable(scopes);
        crate::action::ALL_ACTIONS
            .iter()
            .filter(|action| {
                self.bindings
                    .iter()
                    .any(|binding| binding.action == **action && reachable.contains(&binding.scope))
            })
            // The same lookup every other surface uses, so the key the
            // index prints is the key that would fire.
            .map(|action| (*action, self.chord_for(*action, scopes)))
            .collect()
    }

    /// Replaces every binding of `action` in `scope` with `chord`, or
    /// removes them all when `chord` is `None`. Refuses a chord that would
    /// reach a second action in the same keyboard.
    pub fn rebind(
        &self,
        action: Action,
        scope: Scope,
        chord: Option<Chord>,
    ) -> Result<Keymap, Vec<Conflict>> {
        let mut bindings: Vec<Binding> = self
            .bindings
            .iter()
            .copied()
            .filter(|binding| !(binding.action == action && binding.scope == scope))
            .collect();
        if let Some(chord) = chord {
            bindings.push(Binding {
                scope,
                chord,
                action,
            });
        }
        Keymap::new(bindings)
    }
}

/// Whether a keystroke is ordinary typing: no `Ctrl`, no `Alt`, and a key
/// that produces a character. `Shift` is typing — it is how a capital is
/// written.
fn is_text(chord: Chord) -> bool {
    !chord.mods.ctrl && !chord.mods.alt && matches!(chord.key, Key::Char(_) | Key::Space)
}

/// Every pair of bindings that would make one mnemonic mean two things in
/// one keyboard. Scopes decide where a chord is live; they never decide
/// what a *mnemonic* means, so two scopes of the same mode may not
/// disagree about one. A structural key — Enter, Esc, an arrow — is
/// contextual by nature and is exempt; see [`Chord::is_mnemonic`].
fn conflicts_in(bindings: &[Binding]) -> Vec<Conflict> {
    let mut conflicts = Vec::new();
    for (index, first) in bindings.iter().enumerate() {
        for second in &bindings[index + 1..] {
            if first.chord == second.chord
                && first.chord.is_mnemonic()
                && first.action != second.action
                && shares_a_keyboard(first.scope, second.scope)
            {
                conflicts.push(Conflict {
                    chord: first.chord,
                    first: *first,
                    second: *second,
                });
            }
        }
    }
    conflicts
}

fn shares_a_keyboard(first: Scope, second: Scope) -> bool {
    // A sealed surface is the exception the rule needs: its keyboard is
    // its own for as long as it is open, so a chord it claims cannot be
    // confused with one claimed by a surface that is not on screen.
    if (first.seals() || second.seals()) && first != second {
        return false;
    }
    first.mode().covers(second.mode()) || second.mode().covers(first.mode())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chord::Mods;

    fn bind(scope: Scope, chord: &str, action: Action) -> Binding {
        Binding {
            scope,
            chord: Chord::parse(chord).expect(chord),
            action,
        }
    }

    fn chord(text: &str) -> Chord {
        Chord::parse(text).expect(text)
    }

    fn sample() -> Keymap {
        Keymap::new(vec![
            bind(Scope::Global, "f1", Action::OpenActionIndex),
            bind(Scope::Global, "ctrl+o", Action::SwitchMode),
            bind(Scope::Workspace, "ctrl+t", Action::NewShellTab),
            bind(Scope::Workspace, "ctrl+g", Action::ToggleChanges),
            bind(Scope::Code, "esc", Action::Dismiss),
            bind(Scope::Code, "down", Action::SelectNext),
            bind(Scope::Management, "down", Action::SelectNext),
            bind(Scope::Management, "j", Action::SelectNext),
            bind(Scope::Plugins, "r", Action::RemovePlugin),
            bind(Scope::Filter, "esc", Action::Dismiss),
        ])
        .expect("no conflicts")
    }

    #[test]
    fn the_innermost_surface_answers_first() {
        let keymap = sample();
        assert_eq!(
            keymap.resolve(chord("esc"), &[Scope::Workspace, Scope::Code]),
            Resolution::Act(Action::Dismiss)
        );
    }

    #[test]
    fn a_sealed_surface_answers_for_everything_except_global() {
        let keymap = sample();
        let open = [Scope::Workspace, Scope::Code];
        // The bug this replaces: three chords were tested above the Git
        // overlay's arm and fired while it was open; five were tested
        // below it and did not.
        assert_eq!(
            keymap.resolve(chord("ctrl+t"), &open),
            Resolution::Fallthrough,
            "a workspace binding must not fire through an open overlay"
        );
        assert_eq!(
            keymap.resolve(chord("ctrl+g"), &open),
            Resolution::Fallthrough
        );
        // And uniformly so: leaving and asking for help are the exception,
        // by being global rather than by being tested earlier.
        assert_eq!(
            keymap.resolve(chord("ctrl+o"), &open),
            Resolution::Act(Action::SwitchMode)
        );
        assert_eq!(
            keymap.resolve(chord("f1"), &open),
            Resolution::Act(Action::OpenActionIndex)
        );
    }

    #[test]
    fn the_pane_receives_anything_nothing_claims() {
        let keymap = sample();
        let attached = [Scope::Workspace, Scope::Pane];
        assert_eq!(
            keymap.resolve(chord("a"), &attached),
            Resolution::Fallthrough
        );
        assert_eq!(
            keymap.resolve(chord("ctrl+a"), &attached),
            Resolution::Fallthrough
        );
        assert_eq!(
            keymap.resolve(chord("down"), &attached),
            Resolution::Fallthrough
        );
        // What uze does claim still reaches uze.
        assert_eq!(
            keymap.resolve(chord("ctrl+t"), &attached),
            Resolution::Act(Action::NewShellTab)
        );
    }

    #[test]
    fn typing_into_a_filter_is_typing() {
        let keymap = sample();
        let filtering = [Scope::Management, Scope::Plugins, Scope::Filter];
        // `r` removes a plugin one scope out, and `j` selects the next row.
        // Neither may happen while someone is typing a search.
        assert_eq!(keymap.resolve(chord("r"), &filtering), Resolution::Text);
        assert_eq!(keymap.resolve(chord("j"), &filtering), Resolution::Text);
        assert_eq!(keymap.resolve(chord("R"), &filtering), Resolution::Text);
        assert_eq!(keymap.resolve(chord("space"), &filtering), Resolution::Text);
        // Its own bindings still answer.
        assert_eq!(
            keymap.resolve(chord("esc"), &filtering),
            Resolution::Act(Action::Dismiss)
        );
        // And a chord that is not typing still reaches the global scope.
        assert_eq!(
            keymap.resolve(chord("ctrl+o"), &filtering),
            Resolution::Act(Action::SwitchMode)
        );
    }

    #[test]
    fn an_action_bound_in_two_places_reports_the_nearest_chord() {
        let keymap = sample();
        assert_eq!(
            keymap.chord_for(Action::SelectNext, &[Scope::Management, Scope::Plugins]),
            Some(chord("down"))
        );
        assert_eq!(
            keymap.chords_for(Action::SelectNext, &[Scope::Management]),
            vec![chord("down"), chord("j")]
        );
    }

    #[test]
    fn an_unbound_action_has_no_chord_and_that_is_an_answer() {
        let keymap = sample();
        assert_eq!(
            keymap.chord_for(Action::NewSpace, &[Scope::Workspace]),
            None
        );
    }

    #[test]
    fn one_chord_names_one_action_within_a_keyboard() {
        let conflicting = Keymap::new(vec![
            bind(Scope::Plugins, "r", Action::RemovePlugin),
            bind(Scope::Overview, "r", Action::Refresh),
        ]);
        let conflicts = conflicting.expect_err("two meanings for `r`");
        assert_eq!(conflicts.len(), 1);
        assert!(conflicts[0].to_string().contains("`r`"));
    }

    #[test]
    fn two_keyboards_do_not_conflict_with_each_other() {
        // `r` removes a plugin in management and resumes a task in the
        // preserved-work overlay. Different keyboards, no collision.
        Keymap::new(vec![
            bind(Scope::Plugins, "r", Action::RemovePlugin),
            bind(Scope::PreservedWork, "r", Action::ResumeTask),
        ])
        .expect("different keyboards");
    }

    #[test]
    fn the_index_lists_what_is_reachable_and_nothing_else() {
        let keymap = sample();
        let inside_the_overlay = keymap.available(&[Scope::Workspace, Scope::Code]);
        let names: Vec<Action> = inside_the_overlay
            .iter()
            .map(|(action, _)| *action)
            .collect();
        assert!(names.contains(&Action::Dismiss));
        assert!(
            names.contains(&Action::SwitchMode),
            "global stays reachable"
        );
        assert!(
            !names.contains(&Action::NewShellTab),
            "a sealed surface does not offer what it seals off"
        );
        assert!(
            !names.contains(&Action::RemovePlugin),
            "the other keyboard's actions are not on offer"
        );
    }

    #[test]
    fn the_index_carries_the_chord_that_reaches_each_action() {
        let keymap = sample();
        let available = keymap.available(&[Scope::Management, Scope::Plugins]);
        let remove = available
            .iter()
            .find(|(action, _)| *action == Action::RemovePlugin)
            .expect("offered");
        assert_eq!(remove.1, Some(chord("r")));
    }

    #[test]
    fn rebinding_replaces_and_refuses() {
        let keymap = sample();
        let moved = keymap
            .rebind(Action::NewShellTab, Scope::Workspace, Some(chord("f4")))
            .expect("no conflict");
        assert_eq!(
            moved.resolve(chord("f4"), &[Scope::Workspace, Scope::Pane]),
            Resolution::Act(Action::NewShellTab)
        );
        assert_eq!(
            moved.resolve(chord("ctrl+t"), &[Scope::Workspace, Scope::Pane]),
            Resolution::Fallthrough,
            "the old chord is released, not kept as an alias"
        );
        keymap
            .rebind(Action::NewShellTab, Scope::Workspace, Some(chord("ctrl+g")))
            .expect_err("ctrl+g already means something in this keyboard");
    }

    #[test]
    fn unbinding_leaves_the_action_reachable_by_pointer_alone() {
        let keymap = sample();
        let unbound = keymap
            .rebind(Action::NewShellTab, Scope::Workspace, None)
            .expect("no conflict");
        assert_eq!(
            unbound.chord_for(Action::NewShellTab, &[Scope::Workspace]),
            None
        );
    }

    #[test]
    fn shift_never_makes_a_second_chord_out_of_one_key() {
        assert_eq!(
            Chord::new(Mods::ALT_SHIFT, Key::Char('I')),
            chord("alt+shift+i")
        );
    }
}
