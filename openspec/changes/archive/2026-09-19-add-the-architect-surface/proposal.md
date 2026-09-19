## Why

The workspace shows a checkout as text: what changed in it, what it
contains, what happened to it. It shows nothing of the *shape* of the
project — and the people this product is increasingly for, the ones who
review what several agents did to a system rather than to a file, read
shape first. Today that means leaving the terminal for a browser tab that
renders Mermaid, which is the same exit the code surface was built to
close.

The direction was decided before the feature: uze stays a terminal
product, and what it adds is a better terminal rather than a window. A
diagram therefore has to survive what a terminal survives — SSH, a
multiplexer, a terminal with no graphics protocol — and that rules out
every image path and leaves the cell.

This change is written after the fact. The surface was built and steered
by hand through a proof of concept, and what is recorded here is what
shipped, so that the first version has a contract before it has a second
one.

## What Changes

- **A new built-in extension, `architect`**, and a surface of its own: a
  project's architecture drawn as diagrams in cells. It opens from the
  workspace and from the code surface, by a key and by a chip in the tab
  strip, the way the code surface does.
- **Mermaid is read in-workspace**, in the subset architecture is written
  in: flowcharts with subgraphs, the C4 family, and sequence diagrams. A
  statement outside that subset is skipped rather than failing the
  drawing; a file that yields nothing to draw says so.
- **Diagrams are project artifacts, not code.** `agents.yaml` gains
  `artifacts: { path }`, a directory inside the project. Every `.mmd` /
  `.mermaid` file under it is an artifact. There is no manifest beside
  them: the **area** an artifact belongs to is the diagram's own first
  word, and its **name** is its Mermaid `title`, falling back to the file
  name. A project that declares nothing is told how to, not shown an
  error.
- **A new layout in the view contract, `Layout::Board`**: a full-frame
  surface whose content is clipped rather than wrapped, under a one-row
  menu. The menu is two selectors — the area, and the artifact within it
  — because a row of names stops being readable at about the number of
  diagrams a real project has.
- **The board moves like a whiteboard.** It is dragged with the pointer,
  moved with the arrows and the wheel, and is free to leave the drawing
  behind: a view that stops at the drawing's edge cannot bring a corner
  box to the middle of the screen. A minimap of a fixed size says where
  the view is and takes a click as "go here".
- **C4 views are levels, and the levels are joined.** A box whose alias
  another C4 artifact draws as a boundary leads inside it; the way back is
  a trail in the menu. The last level is the code: a box carrying a
  Mermaid `$link` (or a flowchart `click … href`) opens that path in the
  code surface. C4 artifacts are listed from the outside in — context,
  container, component, dynamic — rather than alphabetically.
- **A selected box lights what it connects to**, and the footer says how
  many edges come in and go out. Selection moves by key toward a
  direction, not only by pointer.
- **The key vocabulary grows by what a board needs** — `toggle-architect`,
  the four pans, next/previous artifact, `choose-artifact`,
  `next-rendering` (box-drawing or plain ASCII), the four directional
  selections and `level-up` — under a new `architect` scope that seals
  the keyboard the way the code surface's does.
- **`Symbol::Architect`** joins the theme vocabulary, in all three shipped
  glyph sets.
- This repository declares its own artifacts
  (`docs/architecture/diagrams/`), which are also what the engine's tests
  draw.

No breaking change: every addition to `view::View`, `view::Command` and
`view::ViewHit` has a default the code surface already satisfies.

## Capabilities

### New Capabilities

- `architect-surface`: the surface that draws a project's declared
  architecture artifacts in terminal cells — where artifacts come from and
  how they are named and ordered, what of Mermaid is drawn, how the board
  is moved, how C4 levels are walked down to the code, and the rule that
  none of it waits on the filesystem.

### Modified Capabilities

- `ui-theme`: an extension may now put glyphs on screen that belong to
  its content rather than to the chrome — a diagram's lines and arrows —
  and the requirement that forbids glyphs outside the vocabulary has to
  say where that line is, the way it already does for colour.

## Impact

- `crates/uze-extensions`: `architect.rs` and `architect/` (`model`,
  `mermaid`, `layout`, `route`, `paint`, `canvas`, `sequence`, `minimap`,
  `catalog`); `view.rs` gains `Layout`, `View::trail`,
  `Navigator::choosing`, the new `Command`s and `ViewHit`s;
  `ExtensionHit::Architect`; one `ExtensionRegistry::builtin` entry.
  `CodePlace::at` is how another surface opens the code surface at a path.
- `crates/uze-core`: `ProjectManifest::artifacts` and its scaffold entry.
- `crates/uze-application`: `services::artifacts::project_artifacts`, the
  read model the client asks — including the refusal of a path that leaves
  the project.
- `crates/uze-keys`: `Scope::Architect`, fifteen actions, their default
  bindings.
- `crates/uze-theme`: `Symbol::Architect`, three themes, regenerated
  schema.
- `src/ui`: `extension_view.rs` draws the board layout, its selectors and
  trail; the orchestrator gains a `spawn_artifacts_read` /
  `absorb_artifacts` pair, the drag, and the tab-strip chip.
- `tests/architecture`: affordances for every new action; `canvas.rs`
  sanctioned in the glyph rule as content, with the reason written down.
- `web/content/docs/project-files.mdx`: the `artifacts` section.
- No new external dependency — no Mermaid crate clears the dependency
  bar, and none draws in cells. No server-side or protocol change.
