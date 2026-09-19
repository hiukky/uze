## Context

See proposal.md — Why. This document is written after the code: it
records the decisions the proof of concept settled on, including the ones
that reversed an earlier one, because those are the ones a second version
is most likely to reopen.

The constraints the approach was shaped by:

- **An extension never draws.** It answers with a `view::View`; the host
  owns geometry, hit-testing and the palette (ADR-041, ADR-045). A diagram
  is the first content whose *shape* is the content, so the question was
  what a drawing is in a contract that only carries lines of spans.
- **Nothing the client draws waits on a repository** — or, here, on a
  directory walk.
- **The dependency bar** in `AGENTS.md`. A Mermaid renderer exists in
  Rust only as young single-author crates that emit SVG, which answers
  neither the bar nor the terminal.
- **A letter names one action, everywhere.** The keymap had six free
  letters when this started.

## Goals / Non-Goals

**Goals:**

- A diagram that reads correctly in any terminal uze already runs in.
- Artifacts a project owns as plain files, useful to every other Mermaid
  reader too.
- A view contract that grows by something a second extension could use,
  not by an escape hatch for this one.

**Non-Goals:**

- Mermaid fidelity. The layout is this surface's own; a diagram is not
  expected to look the way mermaid.js lays it out, only to say the same
  thing.
- Editing a diagram, or generating one from the code.
- Reloading while open. Artifacts are read once per opening.
- Mermaid fences inside Markdown files as artifacts.
- Any image path — sixel, kitty, iTerm — even as an enhancement. Two
  renderings of one diagram is two things to keep true.

## Decisions

### Cells, and a drawing is lines of spans

A cell is the only unit every terminal agrees on, and it is what the host
already knows how to colour. The engine paints onto its own grid of cells
— glyph, role, bold — and hands the host `ContentLine`s, the same type a
file's contents arrive as. So the contract did not need a "canvas" content
kind, and the host draws a diagram with the code that draws a diff.

Lines are kept as *which sides of the cell they leave by* until the last
moment, so two edges meeting become a junction rather than whichever was
painted last. That one representation is also what makes the ASCII set a
table rather than a second painter.

*Alternative considered:* a graphics protocol with a cell fallback. It
would look better where it works and makes the common case — tmux, SSH —
the degraded one. Rejected as the direction, not only as the first step.

### The engine is in-workspace, and split where the cost is

`model` ← `mermaid` (text to graph) → `layout` (layered, recursive per
cluster) → `route` (A* on the cell grid) → `paint`. Layout and routing run
once per artifact; paint runs on every selection and rendering change,
which is why it is its own stage — lighting a box must not re-run a search.

The router prices a turn, a crossing, a boundary crossing, walking beside a
box, and leaving a box by a side that faces away from the target; edges
that share an end may share a trunk for almost nothing. The numbers were
tuned by eye against this repository's own diagrams, which are the
fixtures — a change to them is judged by looking at those.

*Alternative considered:* a Mermaid crate. None clears the bar and none
targets cells; the parser is the small part of this anyway.

### `Layout::Board` — the host clips, the extension moves

The first cut kept the existing sidebar layout and had the extension crop
each line to a width it was told. That put geometry in the extension and
broke on the first resize. `Layout::Board` inverts it: the extension hands
over the visible window of an unbounded drawing, the host draws it
unwrapped and clipped, under a one-row menu it builds from the same
`Navigator` a sidebar is built from. The extension is told the board's
size (`board_space`) only so it can decide what window to hand over —
which is a question about its content, not about the screen.

The board's corner is `Option<(i32, i32)>`: `None` is *home* (centred, or
top-left when larger than the view), so a resize re-centres a drawing
nobody moved and leaves alone one somebody did. It is clamped so the
drawing's edge can reach the middle of the view — the first version
stopped at the drawing's edge, and the report was that it "felt locked".

### The menu: from tabs, to two rows, to two selectors

Three versions, each replaced on the operator's reading of a screenshot:
a row of every artifact (unreadable at six), areas above artifacts (two
rows of chrome over a board that wants the rows), then the area folded
into a selector with artifacts beside it, then the artifact folded into a
selector too — a horizontal list has a ceiling, and a project's diagrams
pass it. The contract carries this as `Navigator::choosing`, so the list
being open is state the extension owns and the host only draws; a list
longer than the board scrolls around its highlight and says `n/total`.

Inside a C4 level the artifact selector gives way to the trail, whose last
crumb is still the selector: where you are, and how you got there, are one
row.

### Artifacts are files; there is no manifest

The operator proposed a `manifest.yaml` mapping C4, sequence and flowchart
resources. Built without one: the area is the diagram's first word, which
the file must already carry to be Mermaid at all, and the name is the
`title` Mermaid already defines. A manifest would be a second place to say
both, free to disagree with the first, and a file every other tool ignores.
The same argument answered "should the front matter carry the type".

What the files cannot say is *order* and *grouping beyond type*. C4 order
is recovered from the level (context → container → component → dynamic).
Anything further is the case for optional `area:` / `order:` front-matter
keys — not built, because no project has asked and it is unverified that
other Mermaid readers tolerate unknown keys.

The directory is declared in `agents.yaml` (`artifacts.path`) rather than
conventional, because the project already has a place it declares things
and a convention would be a second one. `uze-application` resolves it and
refuses a path that leaves the project; the extension receives a resolved
directory and reads it through `Host`, so it never sees the manifest.

### C4 levels are joined by alias, and only C4

A box `core` leads inside when another C4 artifact draws
`*_Boundary(core, …)`. That is what C4 authors already write, so existing
diagram sets join with no annotation. It is restricted to C4 artifacts
because the first version was not, and a flowchart's `subgraph
integrations` became the inside of a container by accident of naming.

The last level is code, and it is the one join that is explicit —
`$link` on a C4 element, `click … href` on a flowchart node — because no
alias can name a path. Following it closes this surface and opens the code
surface at that path (`CodePlace::at`), which is the diff-first thesis in
reverse: from the shape to the file, in one gesture.

### Reading is a spawn/absorb pair like every other

`spawn_artifacts_read` resolves the declaration and walks the directory on
a thread; `absorb_artifacts` drops an answer whose root is not the one the
open surface asked for. The surface opens immediately and says it is
reading. No new mechanism, by design.

### Keys

`g` for the rendering and `o` for the artifact list took two of the six
free letters; pans are the arrows, selection is shift+arrows, `tab` steps
artifacts, `backspace` goes up a level. `choose-area` is a named action
with no default key: it is one `left` away from `o`, and a letter is a
scarcer thing than a keystroke.

## Candidate ADRs

- **Diagrams are drawn in cells, by an engine uze owns** — no graphics
  protocol and no Mermaid dependency. Sets what "visual" means for every
  later surface, and is expensive to walk back once projects keep
  artifacts that depend on what the subset draws.
- **A project's artifacts are described by the files themselves** — no
  manifest beside them; `agents.yaml` names only the directory. A format
  decision projects will build on, and adding a manifest later is easy
  while removing one is not.
- *Not a candidate:* `Layout::Board`. It is a contract addition with one
  user, cheap to reshape until a second one exists.

## Risks / Trade-offs

- **A subset of Mermaid will meet a diagram outside it.** → Styling is
  ignored, an unreadable statement fails one artifact with the statement
  quoted, an undrawn diagram type is listed rather than hidden. The source
  rendering is always there.
- **Router weights tuned by eye on six diagrams.** → They are the
  fixtures, and the routed/unrouted count is asserted; a dense diagram
  that routes badly is a new fixture, not a new constant.
- **A `$link` is joined to the project root and not checked to stay
  inside it**, unlike `artifacts.path`. It opens a read of a path the
  project's own file named, in the operator's own checkout — but it should
  be refused the same way. → Follow-up.
- **The space opens at the primary checkout**, so a project whose
  `artifacts:` exists only on a branch shows "declares no artifacts" until
  it merges. Correct, and surprising once.
- **Two boundaries with one alias resolve to the first, silently.**
- **The wheel over an open list moves the board, not the list.**
- **Colours and the Nerd Font glyph (`U+F0E8`) were never seen rendered
  by the author**, only asserted through the test backend.

## Architecture model

No update to `docs/architecture/likec4/` is required. The surface is an
extension inside the `Workspace Client` container, which the model does
not decompose; it adds no container, no external dependency and no
relationship — the client already reads the active checkout's files, and
`agents.yaml` is already what the application reads a project's
declarations from.
