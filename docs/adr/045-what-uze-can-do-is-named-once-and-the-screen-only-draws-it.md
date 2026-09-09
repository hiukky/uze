# What uze can do is named once, and the screen only draws it

Status: Accepted

## Context

Two judgements lived inside the terminal UI, each in as many copies as
there were places that drew them.

The first was **what a key means**. Keys were `match` arms over `KeyCode`
across three dispatchers — the management TUI, the workspace client, and
the Git extension's overlay — so nothing named the set of things uze can
do and nothing could check it. The consequences were not hypothetical: the
help overlay omitted nine bindings; `r` removed a plugin on one screen and
refreshed the machine on every other; three modal arms were matched *above*
an overlay's own arm and fired through it while the arms below did not; and
installing a plugin, activating a profile and adding a marketplace were
single letters nothing on screen ever mentioned.

The second was **what can be done to a thing**. Whether a plugin could be
installed, updated or removed was worked out at each site that drew it —
the screen's footer, the confirmation dialog, the letter key — three copies
of one judgement, none of which had to agree, and the only sign they had
drifted was an action that did nothing when pressed. Moving actions onto
the entities they act on, which is what makes the product usable with a
pointer alone, would have made that five copies.

Terminals are a third party to the first question. `Ctrl+I` *is* Tab on the
wire; `Ctrl+M` *is* Enter; `Ctrl` plus a digit has no legacy encoding at
all, so a keymap offering it would have shipped a row of keys that never
arrive under WSL's console host. Alt-as-Meta is off by default on macOS.
None of that is visible in a keymap file, and a binding that looks alive
and does nothing is worse than one never offered.

The product's thesis is that the keyboard is an accelerator and never the
way in. A hand-kept alphabet is the opposite of that, and it grows one
reasonable letter at a time.

## Decision

**Both questions are answered once, outside the presentation, and every
surface reads the answer.**

*What a key means* — `uze-keys`, a leaf crate in the `uze-theme` shape,
naming no rendering library, resolving no path, reading no environment:

- An `Action` (label, description, destructive or not), the `Scope` it is
  live in, a `Chord` grammar that round-trips, and a `Keymap` that resolves
  a chord against a scope stack and answers the reverse question every hint
  asks.
- `src/ui/keys.rs` is the one module allowed to name a `KeyCode`, and the
  architecture suite fails the build over a second one. Both dispatchers
  are `scopes()` → `resolve` → `act(action)`, so modal precedence is data
  rather than the order somebody wrote the arms in.
- Every chord is classified by what it takes to deliver it. Chords that
  *are* another key are refused by name before anything is written; chords
  needing the keyboard enhancement protocol are not offered on a terminal
  that lacks it; chords a host commonly claims first are annotated. An
  operator can press a key at a probe and be told what actually arrived —
  the only honest answer for the terminal, multiplexer and connection in
  front of them.
- The operator's keymap is a partial file resolved over the built-in
  default, machine-scoped beside the theme, holding only differences. Every
  key uze prints anywhere comes from it; the hand-written hint strings and
  the help overlay are gone.

*What can be done to a thing* — `uze_application::application::offers`:

- Each entity's read model gives one list of
  `ActionOffer { action, availability }`, an unavailable offer carrying its
  reason as text.
- The row menu, the drawer's action bar, the index and the keyboard's own
  dispatch all read that list. None of them decides for itself. An
  unavailable action is shown with its reason where there is room, and left
  out of the menu, which is what a menu is — but a row nothing can be done
  to still opens one, carrying the reasons, because a gesture that opened
  nothing would read exactly like the silent no-op this replaced.

## Consequences

Easier: a new action is one entry, and it cannot be added without a scope,
a label, a description and — because `tests/architecture/affordances.rs`
asks — somewhere to click it. A new offer appears on every surface at once,
and a screen cannot invent an action the domain would refuse. Rebinding
reaches everything, since nothing prints a key it did not resolve. A
conflict, a collision with another key, and a chord this terminal cannot
send are all caught before they are written rather than by pressing. "Why
is this greyed out" has an answer the application wrote, in one place.

Harder: two dispatchers that could each do as they liked now share a
resolution table, so a surface with genuinely unusual precedence has to
express it as a scope. Every future binding has to be classified by what a
terminal can deliver — it is what removed `Ctrl`+digit from the map — and
the tier table is a thing to maintain as terminals change. Presentation can
no longer answer a question about an entity from what it has in hand: a
judgement needing data the read model does not carry means extending the
read model rather than reaching past it.

Accepted trade-off: the vocabulary is a second leaf crate to keep in step
with the binary, and the enhancement protocol is enabled with
disambiguation only — uze forgoes the release and repeat events it could
otherwise ask for, rather than risk changing what a pane receives.

Source change: openspec/changes/make-shortcuts-optional/
