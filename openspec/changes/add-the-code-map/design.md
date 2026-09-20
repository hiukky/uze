## Context

See proposal.md — Why. The architect surface (ADR-049, ADR-050) has an
engine that paints cells and a catalog of artifacts that are all files. A
treemap needs the first and breaks the second: nobody writes it.

Constraints: nothing the client draws waits on a repository; an extension
reaches Git only through `Host::git`; the glyphs a drawing is made of live
in `canvas.rs`, which is the file the architecture suite names for them.

## Goals / Non-Goals

**Goals:**

- "Where are this project's lines, and which of them are hot" at a glance,
  in any terminal.
- A catalog that can list something derived, shaped so the next derived
  view costs an entry and not a redesign.

**Non-Goals:**

- A metric picker. Area is lines and colour is churn; a second colour
  metric (age, authors, coverage) waits for someone to miss it.
- Language statistics, complexity, or anything that needs a parser.
- History. The map is of the checkout as it stands.

## Decisions

### Squarified, in seen space

The layout is the squarified treemap (Bruls, Huizing, van Wijk): children
by weight, largest first, laid in strips along the shorter side, a strip
closed when the next child would worsen its worst aspect ratio.
Slice-and-dice is a third of the code and produces slivers, which in cells
means tiles no name fits in.

"Shorter side" and "aspect ratio" are judged in *seen* space, a row
counting as two columns, or every square comes out a tower. Strip and tile
edges are rounded cumulatively — the edge between two tiles is one number,
rounded once — so snapping to cells can neither open a gap nor overlap.

*Alternative considered:* a crate. It is sixty lines with no dependency
worth the supply chain.

### Too small is folded before layout, not dropped after

A tile under about 6×3 cells carries no name. Children whose share of the
directory's cells falls under that are summed into one `+N files` tile
*before* squarifying, so the layout never spends strips on them; the
folded tile is entered like a directory and shows only what it stood for.
What still comes out unnameable after snapping is drawn as a bare shaded
tile — present, because its area is the truth, and unlabelled.

### It fits the view; it is not a board to move

Every other drawing here has a size of its own and the view moves over it.
A treemap has none: it is whatever rectangle it is given, and a map larger
than the screen loses the one thing it is for. So it is laid out *for* the
space — in `view`, and again in each handler, which are all already told
the space — and nothing is cached: layout and paint are linear in tiles
and run only when something changed. With no board to move, the pan
actions move the selection, and the minimap and dot grid are off.

Selection is therefore a **path**, not an index: a resize re-lays the map
and an index would point at another tile.

*Alternative considered:* a fixed large board with pan and minimap, for
more named tiles. Entering a directory gives the same detail and keeps the
whole in view.

### Measured by two Git reads and a status

`git grep -I -c -z --untracked ''` counts lines of every tracked and
untracked-unignored text file in one multithreaded process — 40 ms on this
repository — and `-I` is what leaves binaries out. `git log --no-renames
--format= --name-only -z --since=1.year` is the churn, `git status
--porcelain -z` the changed mark. Reading each file through
`Host::count_lines` would be a process-free equivalent of the first at a
hundred times the cost, and would need an ignore parser.

Churn is ranked, not scaled: a lockfile touched by every commit would
flatten a linear scale to one hot tile and a grey map. The top tenth of
touched files is the hottest step, the next quarter the one below.

### Colour by role, shade as the second channel

Four steps on `Faint` → `Muted` → `Warning` → `Danger`. Roles are
semantic, so a theme may render two of them alike; the hotter two steps
also fill their tile with a shade glyph (`░`, `▒`; `:` and `#` in ASCII),
which carries the ranking where colour does not — including in ASCII.
A content gradient through `theme::content` was the alternative; it would
be the first place an extension chose a hue for meaning rather than for
syntax, which is a door worth not opening for four steps.

### A derived artifact is an `Artifact` whose source is what it shows

`Kind::Code` sorts after every declared kind, so a derived artifact
arriving late appends and no index — which is an artifact's identity —
moves. Its `source` is the ranking table, which is exactly what `Source`
should show; its `origin` says what it was measured from. The catalog
learns nothing else. The measurement itself lives beside the catalog in
the view's state, because it is a tree and not text.

### A second read on the same channel

`spawn_code_measure` beside `spawn_artifacts_read`, both asked once per
opening, both answering `ArtifactsResolution` with the root they were
asked for; the answer is an enum of the two. Independent threads, so the
diagrams never wait for a large repository's `git grep`.

### The trail is shared, the levels are not

Inside the code map the trail's crumbs are directories, not artifacts.
`View::trail` is already just names, so the host is untouched; the
extension keeps the map's own zoom (a list of paths) apart from the C4
trail, and whichever is non-empty is what the menu shows.

## Risks / Trade-offs

- **`git grep` on a very large repository** takes seconds. → Off-thread,
  and the rest of the surface does not wait for it.
- **Lines is a crude weight** — generated files and lockfiles loom large.
  → They are large; that is information. Tracked-but-generated is what
  `.gitattributes linguist-generated` is for, a follow-up if asked.
- **Nested frames spend two columns and two rows a level.** → Single
  chains are merged, and a directory too small to open is one tile.
- **Four steps is coarse.** → Accepted; see above.

## Architecture model

No update to `docs/architecture/*.mmd` is required: no container,
dependency or relationship is added.
