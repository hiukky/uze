## Why

The architect surface draws what somebody wrote down about a project. It
says nothing about what the project *is* — where its lines are, which
files carry most of them, which of those everybody keeps changing. That
is the first question of anyone landing in a codebase, and of anyone
reviewing what agents did to one, and today it is answered with `wc`,
`tokei` and a guess.

A treemap answers it at a glance, and it is the diagram a terminal is
best at: nothing but nested rectangles, no edge to route. It is also the
surface's first artifact nobody writes — and what it takes to list one is
what the next derived views (a crate graph, a diagram diff) will need.

## What Changes

- **A `Code` area in the architect surface, holding one artifact: the
  code map.** A squarified treemap of the active checkout — area is lines,
  directories are frames around what they hold, colour is how often a file
  changed in the last year.
- **It is derived, not declared.** It needs no file and no `agents.yaml`
  key, and it is there in a project that declares no artifacts at all —
  which stops being an empty surface.
- **It is measured by Git, off the drawing thread.** Tracked and
  untracked-but-not-ignored text files, so `target/` and `node_modules/`
  never appear; binaries are left out.
- **It fits the view instead of being moved.** A treemap is the whole at a
  glance, so it is laid out for the space it has and laid out again when
  that changes. Going closer is entering a directory, with the same trail
  and the same `level-up` the C4 levels use; the arrows, with no board to
  move, move the selection.
- **What is too small to name is folded**, per directory, into one tile
  that says how many files it stands for.
- **A file opens in the code surface**, by the same gesture a linked box
  does.
- **Its `Source` rendering is the ranking** — every file by lines, with
  its commits — since a map has no source, and the table is what the map
  is a picture of.

No breaking change. No new action, key, symbol or dependency.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `architect-surface`: adds the derived code map and its behaviour, and
  changes what a project that declares no artifacts is shown.

## Impact

- `crates/uze-extensions/src/architect/`: `treemap.rs` (the layout),
  `codemap.rs` (measure, tree, tiles, paint); `catalog.rs` gains
  `Kind::Code` and a derived artifact; `architect.rs` a second drawing
  kind and a second answer to absorb.
- `src/ui/orchestrator.rs`: `spawn_code_measure` beside
  `spawn_artifacts_read`, answering on the same channel.
- `tests/architecture/layering.rs`: nothing — shade glyphs join
  `canvas.rs`, which is already the file named for content glyphs.
- `web/content/docs/project-files.mdx`: a paragraph.
