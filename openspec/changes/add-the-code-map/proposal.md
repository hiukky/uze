## Why

The code surface answers three questions about the checkout a tab is
standing in: what changed in it, what it contains, and what happened to
it. It says nothing about what it *is* — where its lines are, which files
carry most of them, which of those everybody keeps changing. That is the
first question of anyone landing in a codebase, and of anyone reviewing
what agents did to one, and today it is answered with `wc`, `tokei` and a
guess.

A treemap answers it at a glance, and it is the diagram a terminal is
best at: rectangles, no edge to route.

It was first drawn beside the architecture diagrams, because it looks
like one. It is not: nobody writes it, it says nothing about what the
project was meant to be, and every tile on it is a *path* — the same path
the tree and the diff already answer about. Finding the file that carries
the weight and then reading it is the same seam as finding it in the diff
and then editing it, and a surface of its own puts a close and an open in
the middle of it. So it is a fourth half of the code surface, and the
architect goes back to drawing only what somebody wrote.

## What Changes

- **A map, beside the code surface's file tree.** A squarified treemap of
  the active checkout — area is lines, colour is how often a file changed
  in the last year. Reached by `m` or its chip, and toggled rather than
  opened: it is the same checkout seen another way. Offered only where
  the tree is, because a map of the whole checkout answers a question
  nobody reviewing a change is asking.
- **One level at a time.** A level draws its own children and nothing
  below them; a directory is entered rather than unfolded. How many tiles
  are on the screen is then a property of the directory being looked at,
  never of the repository's size. Every tile carries its whole name and
  what it measures, and the borders are one grid rather than a box each,
  so neighbours share a line instead of standing one beside the other.
- **It is derived, not declared.** It needs no file and no `agents.yaml`
  key, and it is there in a project that declares no artifacts at all —
  which stops being an empty surface.
- **It is measured by Git, off the drawing thread.** Tracked and
  untracked-but-not-ignored text files, so `target/` and `node_modules/`
  never appear; binaries are left out.
- **It fits the view instead of being moved.** A treemap is the whole at a
  glance, so it takes the frame, is laid out for the space it has and
  laid out again when that changes. Going closer is entering a directory,
  with a trail that begins at the checkout; the arrows walk the tiles,
  and the key that closes leaves the level before it leaves the map.
- **What cannot carry a name is folded**, per level, into one tile that
  says how many files it stands for — judged on the shape the layout
  actually gave it, not only on its share of the area.
- **A file on it is the surface's selection**, so following a tile and
  then reading or editing the file is one move, not a close and an open.
- **Three renderings on one control** — the map, the same map in ASCII,
  and the ranking: every file by lines, with its commits, which is the
  table the map is a picture of.
- **The architect's menu says what an area is.** The map's levels are a
  descent the viewer is *in*, which no surface here had a way to say: a
  trail meant "how I got here" and only ever appeared after entering. It
  now means the descent itself, with the step being stood on marked — so
  the C4 views are a ladder of three, all on show, and an area that is a
  set of peers is a list to pick from. A selector that offers only what
  is already open is not drawn as a control.
- **The key that closes goes up first**, on both surfaces. One that can
  be entered but not looked around in is one press from losing three
  levels of work.

No breaking change. No new action, key, symbol or dependency.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `code-surface`: adds the measured map as a fourth way of asking about
  the checkout, and the control that switches between them.

## Impact

- `crates/uze-extensions/src/shared/canvas.rs`: the cell canvas moves out
  of the architect, a second extension now drawing on it.
- `crates/uze-extensions/src/code/`: `treemap.rs` (the layout), `map.rs`
  (measure, tree, tiles, paint); `code.rs` a fourth content mode, the
  measurement to absorb, and one list both the chips and the click that
  picks one are read from.
- `crates/uze-extensions/src/architect/`: the map and its area leave;
  `catalog.rs` gains the level an artifact sits at; `architect.rs` gains
  which of its two offerings an area gets.
- `crates/uze-extensions/src/view.rs`: `View::trail` becomes a descent
  with a current step rather than a list of names.
- `src/ui/extension_view.rs`: the menu row draws one control or the
  other, and the trail is walked in both directions.
- `src/ui/orchestrator.rs`: `spawn_code_measure` on a channel of its own,
  asked once per checkout the code surface is opened on.
- `crates/uze-keys/`: one action, `toggle-map`, on `m` in the code scope.
- `tests/architecture/layering.rs`: nothing — the map's glyphs live in
  `canvas.rs`, which is already the file named for content glyphs.
- `web/content/docs/project-files.mdx`: a paragraph.
