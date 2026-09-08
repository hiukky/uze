## Context

uze draws two TUIs and hosts a third surface inside them. The management
client owns the whole keyboard, because nothing else is listening. The
workspace client owns almost none of it, because every key it does not take
belongs to the agent running in the pane. The Git extension's overlay sits
inside the second and answers for itself. Those are three genuinely
different keyboard situations, and today they are three genuinely different
pieces of code with nothing shared but the crossterm types.

The product's own thesis is that none of this should have to be memorized:
everything clickable, nothing behind a keystroke you had to read about
first. The keyboard is not in tension with that — it is what the same
product owes someone on their fortieth session. What breaks the thesis is a
keyboard that is *only* discoverable by reading source, and an alphabet
where the same letter destroys a plugin on one screen and refreshes a list
on the next.

The prior art is in the repository already. `uze-theme` took a vocabulary
that used to be literal values scattered across the render path, gave it
names, put it in a file, resolved a partial file over a built-in default,
told the author what it could not understand, and left one adapter as the
only place allowed to speak the rendering library's dialect. Two
architecture tests hold it. This change is that, for input.

## Goals / Non-Goals

**Goals**

- One resolved keymap, in one place, that every dispatcher asks and every
  hint is generated from
- An action reachable by pointer for every action reachable by key
- One surface that lists everything, reachable by key and by button, in
  both modes
- Chords the operator can change, and honest reporting of the ones their
  terminal cannot deliver
- Management actions offered where the thing they act on is, rather than
  behind a letter

**Non-Goals**

- A modal editor, a prefix key, or chord sequences. uze is not tmux and not
  vim; one chord, one action
- Per-project keymaps. Appearance is machine-scoped and so is this: a
  project does not get to decide what the operator's fingers do
- Vim keys as a preset. `j`/`k` stay as aliases where they already are;
  building a preset system before anyone has asked for one is speculation
- Rebinding what happens *inside* a pane. Keys uze does not claim are the
  agent's, unchanged

## Decisions

### 1. Input is a vocabulary, not a match arm

`Action` is to the keyboard what `Token` is to colour: a name for a
meaning, decided by the product, with the physical key as a separate
question. `Scope` says where an action is live — `Global`, `Management`,
`Route(_)`, `Overlay(_)`, `Workspace`, `Modal(_)`, `Pane` — and resolution
walks the stack innermost-first, which is what turns modal precedence from
an ordering of `match` arms into a value a test can construct.

`Pane` is the last scope and it is total: anything unbound above it is the
agent's. That single fact is what the workspace client's dispatcher becomes.

### 2. The crate names no terminal library

`uze-keys` defines its own `Key`/`Mods`/`Chord`, and `src/ui/keys.rs`
adapts crossterm to it — the same shape as `uze_theme::Rgb` being adapted
to ratatui in `src/ui/theme.rs`. The gain is not purity: it is that chord
parsing, conflict detection, scope resolution and the reverse lookup are
testable without a terminal, and that the reserved-chord rules below live
with the grammar rather than with the reader.

The architecture suite gets the mirror of the rule it already holds for
colour: **nothing outside `src/ui/keys.rs` names `KeyCode`**, with
`orchestrator::input::encode_key` sanctioned by name — it translates a key
into the bytes a PTY expects and binds nothing, which is a different job.

### 3. Every advertised key is asked for, never written

`keymap.chord_for(action, scope)` is the whole fix for the drifting help.
`render_help`'s fifteen hand-typed lines and `route_hint`'s five hand-typed
strings are deleted, not corrected: a help text that is generated cannot
omit `t`, and a footer that is generated cannot advertise a key that was
rebound. The test is structural — no render module may contain a chord
literal — and it is cheap because there is nothing left to write.

### 4. Help and the command palette are one surface

They answer the same question, so building two would guarantee they
disagree. `F1` opens the list of every action live in the current scope,
filterable, each row showing its own chord, each row clickable and
performing the action. Someone who does not know the keyboard uses it as a
menu; someone who does uses it as documentation; someone learning reads the
chord off the row they just clicked. It is also the answer to "a button for
help that is always there": a footer button in both modes opens the same
thing.

`F1` and not `?`, because in the workspace `?` belongs to whatever the
agent is typing into. Function keys are the one register a terminal
program almost never claims — which is the same reason they carry the rest
of the workspace's new bindings below.

### 5. The workspace's chord budget is spent out of the agent's pocket

uze is a multiplexer: every chord it binds in the workspace is a chord the
agent inside the pane never receives. That makes the budget small and the
choice of register the important decision, not the choice of letter.
readline and every agent input box use `Ctrl`+letter constantly — `Ctrl+A`,
`Ctrl+E`, `Ctrl+K`, `Ctrl+U`, `Ctrl+W`, `Ctrl+R`, `Ctrl+N`, `Ctrl+P` — and
essentially never use function keys.

So: **function keys and modified navigation keys are uze's register in the
workspace.** The five existing `Ctrl`+letter bindings stay, because they are
already in people's fingers and moving them costs more than it buys, and no
sixth is added by default. `Ctrl+W` in particular stays and is *annotated*:
it takes delete-word away from every agent input, which is a real cost the
Keys screen now states out loud and the operator can now undo in one
screen.

The converse asymmetry is deliberate and stated: every chord needs a
pointer affordance; **not** every affordance needs a chord. "New space" is a
button and a palette entry with no chord at all, and that is a finished
design, not a gap.

### 6. What a terminal can actually deliver

This is the question that decides the map, so it is settled before the map.
uze reads legacy VT input today: `enable_raw_mode` plus mouse and bracketed
paste, no keyboard enhancement pushed (`src/ui.rs:195`). Under that
encoding `Ctrl`+letter arrives as a single control byte, which makes five
chords not chords at all, and leaves `Ctrl`+digit with no encoding
whatsoever.

**Tier A — universal.** `Ctrl`+letter (excluding the five below), `F1`–`F12`,
`Shift+F*`, and unmodified `Enter`/`Esc`/`Tab`/`Shift+Tab`/arrows/`Page*`/
`Home`/`End`. These reach any terminal, over ssh, inside tmux, on WSL.

**Tier B — portable, conditional on the host.** `Alt`+letter and
`Alt`+digit, which are ESC-prefixed and require the host to send Meta
rather than compose a character: default on Linux, WSL and Windows
Terminal; **off by default on macOS Terminal.app and iTerm2**, where Option
composes. Today's `Alt+i`, `Alt+I`, `Alt+p` and `Alt+1..9` are therefore
dead on a stock Mac, and `support-macos` is still open. Also Tier B:
modified navigation keys (`Ctrl+↑/↓`, `Ctrl+PageUp/PageDown`), which the
emulator itself often claims first — gnome-terminal, Konsole and Windows
Terminal all use `Ctrl+PageUp/PageDown` or `Ctrl+Tab` for their own tabs.

**Tier C — needs the keyboard enhancement protocol.** `Ctrl`+digit,
`Ctrl+Shift`+letter, `Shift+Enter`, `Ctrl+Enter`. Available in kitty, foot,
WezTerm, ghostty, Alacritty, recent iTerm2 and recent Windows Terminal, and
detectable at runtime with `crossterm::terminal::supports_keyboard_enhancement`.
Offered by the Keys screen only when the running terminal answers yes, and
shown with the reason when it does not. This retires the `Ctrl+1..9`
idea for switching spaces from the first pass of this design: it is
unbindable on a plain terminal.

**Tier D — refused by the parser, with the reason.** `Ctrl+I` (Tab),
`Ctrl+M` (Enter), `Ctrl+J` (LF), `Ctrl+H` (Backspace), `Ctrl+[` (Esc),
`Ctrl+Space`/`Ctrl+@` (NUL). Binding one of these is how an operator locks
themselves out of their own UI, so the grammar refuses rather than the
resolver warning.

**The WSL question, answered directly.** A Linux uze under Windows Terminal
reads VT bytes from a pty — the Win32 console key API is not in the path,
so it sees exactly what any Linux terminal sees. Every Tier A chord works
there, `Alt` works (Windows Terminal sends ESC-prefixed by default), and
`Ctrl`+digit does not unless Windows Terminal's keyboard-protocol support
is on. Separately, Windows Terminal keeps `Ctrl+Shift+*`, `Alt+Enter`,
`Ctrl+Tab` and `Ctrl+V` for itself, so those never reach uze at all — which
is why `Ctrl+Shift` is not offered as Tier A even where the protocol
exists. Under tmux or screen the multiplexer's own prefix and bindings win
first, on the same footing as an emulator's.

**And the honest answer on top of the matrix.** No table enumerates every
emulator × multiplexer × ssh × keyboard-layout combination. The Keys screen
carries a probe: press a chord, and uze shows whether it arrived and what
it decoded to. That is a few lines of code once input is data, and it is
the only answer that is true for the machine in front of you.

When enhancement is available uze pushes **`DISAMBIGUATE_ESCAPE_CODES`
only** — never `REPORT_EVENT_TYPES`, whose release events would double
every keystroke forwarded into a pane — and filters to
`KeyEventKind::Press`. The flags are popped with the rest of the terminal
cleanup.

### 7. One chord names one action within a mode

Scopes decide where a chord is live; they never decide what it means. This
is the rule that retires `r` = remove-or-refresh, `a` = add-or-analyze and
`s` = setup-or-activate, and it is a test, not a convention: no two actions
in one mode may hold the same chord, whatever their scopes.

The alternative considered and rejected was unbinding the letters
altogether and leaving only navigation plus the palette. It is defensible —
the menu makes them unnecessary — but it throws away real muscle memory
(`i`, `u`, `r` in a package list) to solve a problem that only the
*collisions* caused. Making the alphabet consistent is the smaller change
with the same benefit.

### 8. An entity carries its offers; two surfaces render them

What can be done to a plugin is decided once, in the read model that
`uze-application` already hands the TUI, as a list of offers — an action, a
label, whether it is available now and why not, and whether it is
destructive. The row's `⋯` menu renders the available ones; the detail
drawer renders all of them, disabled ones with their reason; the palette
renders the same list again for the selected row. They cannot disagree,
because there is one list.

This is also what fixes the silent no-op: `u` on a plugin with no update
does nothing today and looks broken. An offer that is not available is
either absent from the menu (a menu is what you can do now) or present and
explained in the drawer (a drawer is where you learn why you cannot).

The mechanics are not new. The workspace client already raises an action
menu on right-click for tabs and spaces, with a documented rule that
closing is never one click — open, navigate, confirm. Management gains the
same widget rather than a second one, and destructive offers keep their
confirmation overlay and never sit first in the list.

### 9. The Keys screen is a route, not a preferences pane

`Route::Keys` in the management sidebar, shaped like `Plugins` because that
shape is already understood: a clickable filter box, a list grouped by
scope, a detail drawer. Each row is an action: its description, its chord,
whether the chord is the default or the operator's, what tier it is in, and
what its host is likely to take first. Clicking the chord captures the next
keystroke; a conflict or a refused chord is shown before anything is
written. `~/.uze/keys.json` holds only what differs from the default, the
way a theme file does, so a default that changes in a later release is
adopted rather than frozen.

## Model update

This change adds a component: `uze-keys`, a leaf crate beside the design
system. `docs/architecture/likec4/model.c4` gains it with the same shape
its neighbour has, plus the relationship from the terminal UI — `resolves
input against the active keymap`, the mirror of the existing `resolves
colour tokens and symbols against the active theme`. No container and no
external dependency changes.

## Candidate ADRs

- **Input as a resolved vocabulary in a leaf crate** — a second crate
  following the `uze-theme` shape, with the matching architecture rule that
  one adapter owns the terminal dialect. Structural and expensive to undo.
- **Chord portability tiers and runtime enhancement detection** — the
  product commits to classifying what it binds by what a terminal can
  deliver, and to reporting rather than silently failing.
- **Offers as a read-model field** — what can be done to an entity is
  answered by the application layer, not by the presentation that draws it.

## Risks / Trade-offs

- **Generated help reads flatter than hand-written help.** The current text
  groups and qualifies ("Remove plugin (Plugins) / Refresh elsewhere") in a
  way a table cannot. Mitigation: the qualifier disappears with the
  collision that produced it, and each action carries a written description
  in the vocabulary — the prose moves, it is not lost.
- **`Ctrl+W` stays contested.** Keeping it is a deliberate cost paid to
  muscle memory; the mitigation is that it is now visible and rebindable
  rather than discovered by losing a word mid-prompt.
- **A rebindable keyboard is a support surface.** An operator can bind
  something unusable. Mitigation is the refusal list, conflict detection
  before writing, the tier annotation, and the probe.
- **Two dispatchers change shape at once.** The workspace client's
  precedence is subtle and the change fixes a real ordering bug inside it.
  Mitigation: the resolution table is unit-tested before either dispatcher
  is touched, and the existing `TestBackend` suites and journeys cover the
  gestures.
