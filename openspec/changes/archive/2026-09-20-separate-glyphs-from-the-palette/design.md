## Context

See proposal.md — Why.

The mechanism this needs mostly exists. `uze_theme::resolve_stack` already
takes a slice of `ThemeFile` and applies them in order, later wins; a theme
file may declare `symbols` alone, with no colours; `SymbolDef` already
carries a `width` a file can override, and every column is laid out from it.
`~/.uze/theme-overrides.json` is already a machine-scoped layer that outlasts
the active theme.

What does not exist is a second selection. `src/theme.rs::builtin_layer`
matches the literal id `"ascii"` and pushes the bundled ASCII file — so the
ASCII set reaches the stack only by being *selected as a theme*, and only
that one id has the privilege. Everything below follows from replacing that
special case with a selection.

One constraint shapes the whole design: **there is no way to ask a terminal
which font it is rendering with.** No escape sequence answers it; `$TERM` and
`$TERM_PROGRAM` name the emulator, not the font; and a cursor-position probe
measures *width*, not presence — a missing glyph renders as tofu at the same
one cell a present one does. So the set cannot be detected, and a yes/no
question at setup would ask the operator to recall a fact only the screen can
settle. This is why the change carries a screen rather than a prompt.

## Goals / Non-Goals

**Goals:**

- One selection for the palette, one for the glyphs, neither touching the
  other, both machine-scoped.
- A bundled set for a patched icon font that looks like deliberate UI chrome
  rather than a bag of clip art.
- A surface where both are chosen by looking at the result.
- A net deletion in `src/theme.rs`: the id-matching special case goes away.

**Non-Goals:**

- Detecting the operator's font, by any means, ever.
- Authoring or editing themes from inside the TUI. The Appearance screen
  selects; a theme is still a file someone writes, and
  `theme-overrides.json` is still edited by hand.
- Per-project appearance. Appearance is machine-scoped (ADR-019) and stays
  that way.
- A glyph set that carries colours. A set declares `symbols` and nothing
  else; that is what makes it composable with any palette.

## Decisions

### The set is one more layer, not a new merge rule

The resolution order becomes:

```
built-in default → glyph set → the theme's ancestry → the theme → overrides
```

`resolve_stack` is unchanged — the change is which entries `src/theme.rs`
puts in the slice. `builtin_layer` is deleted and replaced by a lookup keyed
on the selected set.

*Alternative — a `glyphs:` field in the theme file*, the way `extends` works,
so a theme could opt into a set without copying glyphs. Rejected: it inverts
ownership. A theme would be declaring a fact about the machine's installed
font, and "I installed a nice theme and half the UI turned to tofu" comes
straight back in through that door.

*Alternative — bundle `nerd` as a theme beside `ascii`.* Rejected: it is the
status quo with one more entry. N palettes × 3 sets means every theme author
authors forty-four glyphs three times, or does not, and ships a theme that is
unusable on someone else's terminal.

### The set sits below the theme, and overrides sit above both

A theme that deliberately declares `symbols` wins over the selected set. This
keeps the stack's single rule — later layer wins — with no special case, and
leaves a theme able to have a glyph identity of its own (a heavier bar, its
own prompt caret).

The cost is real and worth stating: a theme that declares a full `symbols`
block silently defeats the set the operator chose. The escape hatch is
`theme-overrides.json`, the top layer, which is precisely what it is for, and
`uze theme show` already prints the layers in the order they applied so the
operator can see which one won.

*Alternative — the set above the theme*, on the argument that an explicit
`uze theme glyphs nerd` is a more specific declaration than "I installed
dracula". Rejected: it buys one case at the price of the general rule, and it
would leave a theme no way to express glyphs at all. In practice the conflict
is rare — today exactly one bundled file declares `symbols`, and it is the
one becoming an axis value.

### The `nerd` set is Codicons, targeting the Mono variant

Nerd Fonts patches several icon sources into the private-use area. The
codepoints do not vary by base font — JetBrains Mono, Fira, Hack and Iosevka
all get the same map from the same patcher — so a bundled set does not need
to know which font is installed. Three things do vary, and each decides
something here:

**Patcher version.** Nerd Fonts v3 re-encoded Material Design Icons out of
`U+F5xx` and into `U+F0001+`. A glyph from that range means something else,
or nothing, on a v2 install. Codicons were *added* in v3 and exist nowhere in
v2, so choosing them turns a version mismatch into an honest "this set needs
Nerd Fonts v3" — visible immediately in the preview — instead of a
confidently wrong picture.

**Visual coherence.** Codicons is VS Code's own UI icon set: one weight, one
optical size, drawn for a monospace grid at terminal sizes. Mixing Devicons
with Font Awesome on the same row is visibly uneven in weight and cap height,
which is exactly the failure a "nice icons" change has to avoid.

**Cell width.** The same codepoint occupies one cell in a `Nerd Font Mono`
build and two in the plain build, and `unicode-width` reports 1 for the
private-use area in both — it cannot tell them apart. `nerd.json` therefore
declares `width` explicitly on every entry, targeting Mono, which is what a
terminal font install overwhelmingly is. The file states its assumption
rather than inheriting a measurement that is right by luck.

*Alternative — ship `nerd` and `nerd-wide`.* Rejected: two sets to keep in
step forever, for a difference an operator fixes with one `width` override —
and the Appearance screen makes the misalignment visible in the moment of
choosing, which is when it is cheapest to notice.

The exact codepoints are to be read off the official Nerd Fonts cheat sheet
during implementation and recorded in the file, never written from memory.

### A set declares a difference, not the whole vocabulary

`ascii` declares all forty-four symbols because its promise is about all
forty-four: one Unicode glyph left in an otherwise-ASCII screen breaks the
only reason to select it. `nerd` declares twenty-six — the marks, statuses,
chevrons and arrows, where an icon says the thing better than a letterform
does — and inherits the rest, because a patched font is a *superset* of an
ordinary one. Replacing `├─`, `▍` or `…` with a private-use icon would be a
downgrade dressed as a feature: box drawing is already the right picture, and
tree glyphs are load-bearing for alignment in a way an icon is not.

So completeness is a property a set may promise, not one the format demands.

*Alternative — require every set to declare every symbol.* Rejected: it would
force `nerd.json` to restate eighteen glyphs identical to the default's, and
the first time the default's tree glyphs changed, the sets would silently
stop following.

### The spinner stays braille in every set

`status.working` is the one animated symbol. Codicons has a `loading` glyph,
but it is a single mark meant to be spun by CSS — there are no frames. Taking
it would freeze the only moving thing in the UI, so `nerd` leaves
`status.working` undeclared and inherits the braille cycle, which any patched
font draws because braille is ordinary Unicode in the base font.

### The selection lives in the file that already holds a selection

`ThemeSelection` in `crates/uze-core/src/theme_state.rs` gains
`glyphs: Option<String>`, `#[serde(default)]`, in the same
`state/theme.json`. An existing file that says nothing about it reads as
`None` = the default set. There is no migration step and no compatibility
code — absence already means the right thing.

*Alternative — its own file.* Rejected: two writes and two reads for one
answer to one question ("what does this machine look like"), and `install()`
already reads that file on every command.

### `default` means no layer

The default set is not an empty file pushed onto the stack — it is the
absence of a set layer, because the built-in default's own glyphs *are* the
default set. This keeps "unset" and "explicitly default" resolving through
the same path, so there is nothing that can drift between them.

### `ascii` stops being a theme, and says so when someone asks for it

`builtin_names()` narrows to the bundled *themes*; a separate lookup answers
for glyph sets. A recorded selection that names `ascii` as a theme therefore
stops resolving — and rather than the generic "no theme `ascii`", `written()`
gains a case that names the move: `ascii` is a glyph set now, reachable with
`uze theme glyphs ascii`. That is an error message, not migration code; the
state fixes itself the moment the operator selects anything.

### `uze theme use` → `uze theme set`, with no alias

`use` was already a redundancy beside `list`/`show`, and adding a second axis
would have meant two verbs for one act. The old spelling is removed outright:
this project is pre-1.0 and does not carry compatibility shims, and clap's
own error names the subcommands that do exist.

### Appearance is a route, not a drawer

A seventh route beside Overview, Plugins, Extensions, Integrations, Profiles
and Keys. It is shaped like Keys — a list with a detail side — for the reason
Keys is shaped like Plugins: a screen someone visits rarely is better off
looking like a screen they already know.

Its one screen-specific element is the part that justifies the change: each
glyph set is rendered **in its own glyphs**, as a strip of the actual marks
against a fixed-width ruler, so a set the terminal cannot draw shows as tofu
and a set whose widths are wrong shows as misalignment — both before the
operator commits to it.

### The existing theme-picker overlay stays, and stays a palette swap

Implementation surfaced something the proposal did not account for: a
`ThemePicker` overlay already exists, reachable by a global binding, listing
themes only. Two surfaces for one choice is normally a smell, but the split
here is the same one `src/ui/view.rs` already documents between a row menu
and a detail view: a menu is what can be done *now*, a detail view is where
someone asks why.

So the overlay stays what it is — swap the palette without leaving what you
are doing, from the workspace as well as from management — and the route is
where appearance is *decided*, with both axes and the previews that make the
glyph choice possible at all. The overlay is deliberately not extended to
glyph sets: choosing a set by name, with no preview, is the guess this whole
change exists to remove.

## Candidate ADRs

- **Appearance is two independent axes, and neither is inferred** — where the
  boundary sits between what a theme owns (the palette, and any glyph it
  deliberately claims) and what the machine owns (the installed font), plus
  the standing refusal to detect a terminal's font. Both are boundaries that
  would be expensive to move once themes in the wild depend on them.

## Risks / Trade-offs

- **A third-party theme that declares a full `symbols` block silently defeats
  the chosen set.** → The layer order is documented, `uze theme show` names
  the layer that won, and `theme-overrides.json` overrides both.
- **An operator on Nerd Fonts v2 sees tofu for the whole `nerd` set.** → They
  see it in the Appearance preview, before selecting, which is the entire
  reason the preview exists. Documented as requiring v3.
- **An operator on a non-Mono Nerd Font build gets sheared columns.** →
  Visible as misalignment in the preview strip's ruler; fixed with a `width`
  override, documented beside the set.
- **Codepoints written from memory would be wrong.** → An explicit task to
  transcribe them from the official cheat sheet, and a test that every symbol
  in a bundled set resolves to a non-empty glyph with a declared width.
- **The bundled-theme no-emoji test must not reject the new set.** Codicons
  live in the private-use area, not the pictographic ranges the test scans,
  so the rule stands unchanged — but the set has to be run against it rather
  than assumed to pass.

## Migration Plan

No data migration. `state/theme.json` without `glyphs` is already valid and
means the default set.

Two operator-visible removals, both surfacing as a message that names the
replacement rather than as silence:

1. `uze theme use <id>` no longer exists → clap reports the unknown
   subcommand and lists `set`.
2. A machine with `ascii` recorded as its active theme → the resolver reports
   that `ascii` is a glyph set now and names `uze theme glyphs ascii`. UZE
   keeps drawing in the built-in default meanwhile, which is what it already
   does for any theme that will not load.

Rollback is reverting the commit. The only state written is a field older
builds ignore: `ThemeSelection` carries no `deny_unknown_fields`, so a
`state/theme.json` written by this change still loads on a build from before
it, with the palette intact and the glyph set silently dropped.
