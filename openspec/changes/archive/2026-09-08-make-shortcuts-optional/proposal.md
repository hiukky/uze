## Why

uze is meant to be interactive-first — everything clickable, nothing to
memorize — and its management TUI is the opposite: seventeen single-letter
commands, three of them meaning two different things depending on which
route you are standing on (`r` removes a plugin and refreshes everywhere
else, `a` adds a marketplace and analyzes context on Harnesses, `s` sets up
a harness and activates a profile). Nothing on screen offers them. The one
place they are written down is a hand-typed list in `render_help`
(`src/ui/overlay.rs:237`) that already omits nine of them — `t`, `?`, `F5`,
`Ctrl+O`, `Ctrl+C`, `n`, `d`, `x`, `Space` — because nothing makes it
agree with the dispatcher it describes. The workspace client has no help
surface at all: `Alt+I` delivers every task in a space and there is no way
to find that out.

Underneath that, input is not modelled. Three independent dispatchers
(`TuiModel::apply_key`, `Attach::key`, `git::handle_key`) each match on
`KeyCode` with route and modal guards written inline, in an order whose own
comment admits is "easy to get wrong by inserting an arm in the wrong
place" — and it already is wrong: `Alt+i`/`Alt+I`/`Alt+p` are matched
*before* the Git overlay's arm, so they fire while it is open, while
`Ctrl+T`/`Ctrl+W`/`Ctrl+G`/`Ctrl+O`/`Ctrl+Q` and `Alt+1..9`, matched after
it, do not. Colour and glyph are a resolved vocabulary in a file anyone can
override (`uze-theme`); the keyboard is a hundred and forty-one literal
`KeyCode::` mentions across three files, and not one of them is
configurable.

Two consequences fall out of the same gap. An action nobody can discover is
an action nobody uses — pressing `u` on a plugin with no update available
does nothing, silently, and looks broken. And an action bound to a key
nobody's terminal can deliver is worse than unbound: today's `Alt+`
bindings are dead on a stock macOS terminal, where Option composes
characters instead of sending Meta, and the product has no way to say so.

## What Changes

- **`crates/uze-keys`** — a leaf crate that is to the keyboard what
  `uze-theme` is to the palette: a named `Action` vocabulary, a `Scope`
  stack, a `Chord` grammar that parses and prints, a built-in default
  keymap, a resolver that completes a partial `keys.json` over it and
  reports conflicts, and — the piece that makes every advertised key true —
  a reverse lookup from action to chord. It names no rendering or terminal
  library, exactly as the theme crate names none
- **Input becomes two steps everywhere** — derive the scope stack from what
  is open, resolve the event to an `Action`, then act on the action. The
  modal precedence that is a hand-ordered `match` today becomes data, and
  "with the Git overlay open, `Ctrl+T` resolves to nothing" becomes a unit
  test rather than a reading of the source
- **Help, footers and hints are generated** from the keymap. `render_help`
  and `route_hint` stop being prose. A rebound key updates every surface
  that mentions it, and no surface can mention a key that is not bound
- **`F1` opens one surface that is both** — every action live in the
  current scope, filterable, clickable, each row printing its own chord.
  Help and command palette are the same widget because they answer the same
  question; `?` keeps opening it in management, and a persistent footer
  button opens it with the mouse in both modes
- **Management stops being letter-driven** — every row gains a `⋯`
  affordance and a right-click that raise the action menu the workspace
  client already has for tabs and spaces, and every detail drawer gains the
  same actions as a bar. The offers come from one list per entity in the
  read model, so the menu, the drawer and the palette cannot disagree about
  what can be done to a plugin
- **One chord names one action within a mode** — scopes decide where a
  chord is live, never what it means. `r` stops meaning two things, `a`
  stops meaning two things, `s` stops meaning two things
- **Chords are classified by what a terminal can actually deliver** — three
  portability tiers, a refusal list for the five chords that are not chords
  at all on a terminal (`Ctrl+I` is Tab, `Ctrl+M` is Enter, `Ctrl+H` is
  Backspace, `Ctrl+J` is LF, `Ctrl+[` is Esc), detection of the keyboard
  enhancement protocol, and a probe in the Keys screen that answers "does
  this chord reach uze on this machine" by having you press it
- **A dedicated Keys screen** (`Route::Keys`) — every action grouped by
  scope, its chord, whether it is the default or yours, what its host may
  take first, and capture-to-rebind by clicking the chord
- **The filter box becomes clickable** in all three routes that draw one —
  it is drawn today with no hit registered at all, so the search field
  visibly exists and cannot be used with the mouse

## Capabilities

### New Capabilities
- `input-keymap`: how a keystroke becomes an action — the vocabulary,
  scope resolution, the keymap file, what a terminal can deliver, and the
  reverse lookup that makes every advertised key true
- `action-discovery`: how someone performs an action without knowing a key
  — the offers an entity carries, the two surfaces that render them, the
  always-reachable index of everything, and the rule that binds them
  together

## Impact

- **New crate** — `crates/uze-keys`, a leaf like `uze-theme`. No new
  external dependency: `serde` only, already in the workspace
- **Application** — an offers list per entity on the plugin, harness and
  profile read models, so the TUI never decides what is possible
- **TUI** — one adapter (`src/ui/keys.rs`) between crossterm and the chord
  vocabulary; the three dispatchers reduced to scope + resolve + act; the
  action menu generalized out of the workspace client; the Keys screen;
  generated help and footers
- **Machine** — `UzeHome::keymap_path()` (`~/.uze/keys.json`), authored
  intent beside `theme-overrides.json` rather than under `state/`
- **Terminal** — keyboard enhancement pushed when the host supports it, and
  popped with the rest of the terminal cleanup
- **Docs** — `docs/keymap.md` as the authoring guide `docs/theming.md` is
  for themes; `docs/architecture/invariants.md` gains the affordance rule
- **Journeys** — chapter `02-packages` gains a mouse-only install, and a
  keymap journey proves a rebind reaches a real gesture
