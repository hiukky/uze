## 1. The vocabulary and the grammar

- [x] 1.1 `crates/uze-keys`, a leaf crate depending only on `serde`. Its own
  `Key`/`Mods`/`Chord`; it names no terminal and no rendering library, the
  way `uze-theme` names none.
- [x] 1.2 `Chord` parses and prints the same text (`ctrl+o`, `alt+shift+i`,
  `f1`, `ctrl+pageup`). Round-trip is a property test: anything that prints
  parses back to itself.
- [x] 1.3 The refusal list is part of the grammar, not of the resolver:
  `ctrl+i`, `ctrl+m`, `ctrl+j`, `ctrl+h`, `ctrl+[`, `ctrl+space` are refused
  naming the key they actually are on a terminal. Test one per chord.
- [x] 1.4 `Tier` — universal, host-conditional, enhancement-only — carried
  by the chord itself, plus the annotation list of chords a host commonly
  claims first (emulator tab switching, multiplexer prefix).
- [x] 1.5 `Action`: the whole vocabulary, each with a written description
  and a `destructive` flag. Derived from the inventory of what the three
  dispatchers do today — nothing is dropped in this step, only named.
- [x] 1.6 `Scope` and the stack; `Scope::Pane` is total and last.

## 2. Resolution and the file

- [x] 2.1 `Keymap::resolve(event, &[Scope])` — innermost first, `None` for
  "belongs to whatever is below". Table-driven tests, including the two
  cases that are bugs today: with the Git overlay open nothing outside it
  resolves, and the deliver/preserved chords stop being an exception.
- [x] 2.2 `Keymap::chord_for(action, scope)` — the reverse lookup, and the
  only way any surface may learn a key.
- [x] 2.3 The built-in default map, as data. Two tests hold it: one chord
  names one action per mode, and every action is either bound or
  deliberately unbound.
- [x] 2.4 `KeymapFile` + `resolve`: partial over the default, unknown
  action or scope is a warning, unparseable chord or a conflict is an error
  and the keymap in force stays. Same error/warning split the theme loader
  documents, for the same reason.
- [x] 2.5 `active()`/`set_active()` behind `RwLock<Arc<Keymap>>`, answering
  the built-in default before anything is loaded — so a CLI path or a unit
  test needs no I/O to ask a question about keys.
- [x] 2.6 `UzeHome::keymap_path()` → `~/.uze/keys.json`, beside
  `theme-overrides.json`: authored intent, never reconstructable, not state.

## 3. The adapter and the three dispatchers

- [x] 3.1 `src/ui/keys.rs`: crossterm `KeyEvent` ↔ `uze_keys::Chord`, and
  the enhancement handshake — `supports_keyboard_enhancement`, push
  `DISAMBIGUATE_ESCAPE_CODES` only, filter to `KeyEventKind::Press`, pop
  with the existing terminal cleanup in `src/ui.rs`.
- [x] 3.2 A test that a keystroke forwarded to a pane is byte-identical
  with the enhancement on and off. Release events doubling pane input is
  the failure this guards.
- [x] 3.3 `TuiModel::scopes()` and `apply_key` reduced to resolve + act.
  Behaviour unchanged except the collisions retired in 3.6.
- [x] 3.4 `Attach::scopes()` and `Attach::key` reduced the same way. The
  hand-ordered precedence list goes away; the ordering bug goes with it.
- [x] 3.5 `git::handle_key` takes a resolved action rather than a
  `KeyEvent`, so the extension keeps answering for itself without owning a
  second keyboard vocabulary.
- [x] 3.6 One chord, one action per mode: `r` is remove only, `m` adds a
  marketplace, `a` analyzes, `s` sets up, activating a profile is `Enter`
  in its list. Update the affected `TestBackend` tests.
- [x] 3.7 The architecture test: nothing outside `src/ui/keys.rs` names
  `KeyCode`, with `orchestrator::input::encode_key` sanctioned by name and
  the reason written where the other sanctions are.

## 4. Offers

- [x] 4.1 `ActionOffer { action, label, availability, destructive }` in
  `uze-application`, and an `offers` list on the plugin, harness and profile
  read models. The TUI stops filtering on `installed`/`update_available` to
  decide what is possible.
- [x] 4.2 Both TUIs' action menus speak one vocabulary: the workspace's
  private `MenuAction` is gone and its context menu carries
  `uze_keys::Action`, so a menu entry's words and its weight come from the
  same place management's does. Open, navigate, confirm; a destructive
  entry is never the one a menu opens highlighted, and when every offer is
  destructive nothing is highlighted at all. The two renders stay
  separate — they are anchored differently and already look different, and
  forcing them together is a design change this one does not carry.
- [x] 4.3 Row affordance (`⋯`) plus right-click on every management list
  row, and an action bar in every detail drawer — available offers in the
  menu, all offers in the drawer with the unavailable ones explained.
- [x] 4.4 The filter box gains a hit in Plugins, Extensions and Harnesses.
  It is drawn today and clicking it does nothing.
- [x] 4.5 A test that the menu, the drawer and the index show the same
  offers for the same row.

## 5. The index, the help, the hints

- [x] 5.1 The index surface: every action live in the current scope, its
  chord, filterable, clickable, performing from the row. One widget, opened
  by `F1` in both modes, by `?` in management, and by a footer button in
  both.
- [x] 5.2 Delete `render_help` and `route_hint`; generate help and every
  footer hint from the keymap. Same for the Git overlay's footer and the
  preserved overlay's `[r] resume …` line.
- [x] 5.3 The architecture test: no rendering module contains a chord
  literal.
- [x] 5.4 The affordance test: every action bound outside `Scope::Pane`
  names its pointer affordance or is declared keyboard-only with a reason.

## 6. The Keys screen

- [x] 6.1 `Route::Keys`, shaped like `Plugins`: clickable filter, list
  grouped by scope, detail drawer. It reaches no CLI command, so there is
  nothing to classify in `command_performance.rs`: it reads the keymap
  already in force and writes one small file.
- [x] 6.2 Capture-to-rebind by clicking a chord cell, with conflict, tier
  and refusal shown before anything is written; reset per row and per
  scope; only the differences are persisted.
- [x] 6.3 The probe: press a chord, see whether it arrived and what it
  resolved to — the answer for the terminal, multiplexer and connection
  actually in use.
- [x] 6.4 Tier and host-collision annotations rendered per row, including
  `ctrl+w` taking delete-word from an agent's input.

## 7. Proof

- [x] 7.1 `TestBackend` tests: the row menu lists the right offers for each
  row state; the drawer explains the unavailable ones; the index lists the
  open scope and nothing else.
- [x] 7.2 Journey, chapter `02-packages`: install a package with the
  pointer alone — filter box, row menu, confirm — checked against the Store
  and the harness on disk, never against uze's own report.
- [x] 7.3 Journey, chapter `01-first-run`: a `keys.json` rebinding an action
  is in force at launch, and the rebound chord performs it; the default
  chord no longer does.
- [x] 7.4 A refused chord and a conflicting chord each produce a stated
  problem and leave the keymap in force — covered in the crate, not through
  the UI.
- [x] 7.5 `docs/keymap.md`: the authoring guide, the way `docs/theming.md`
  is for themes — where the file lives, the chord grammar, the tiers, what
  a keymap deliberately cannot change. `docs/architecture/invariants.md`
  gains the affordance rule and the test that proves it.

## 8. Model

- [x] 8.1 `docs/architecture/likec4/model.c4`: `uze-keys` as a component
  beside the design system, with the relationship from the terminal UI
  (`resolves input against the active keymap`). This repository has no
  `arch:validate` script — the model is the artifact, so the check is that
  the new element parses in the same shape as its neighbours.
