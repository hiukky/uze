## 1. Layout

- [x] 1.1 `code/treemap.rs`: squarified layout in seen space, edges
      rounded cumulatively; tests for coverage without gap or overlap, for
      proportion, and for tiles staying close to square as seen.

## 2. The map

- [x] 2.1 `code/map.rs`: `measure` through `Host::git` (lines,
      churn, changed), parsed from NUL-separated output, renames included.
- [x] 2.2 The tree: directories summed, single chains merged, children by
      weight; heat ranked into four steps, a directory taking its hottest.
- [x] 2.3 Tiles for a space, one level deep: fold what is too small, and
      fold again until every tile kept can carry its name. At most a
      screenful of them.
- [x] 2.4 Paint: one border grid rather than a box per tile, names, the
      measures under them, both glyph sets, the changed mark that
      outlives a long name, the selection.
- [x] 2.5 Selection by path, toward a direction, by click; enter a
      directory or a folded tile; back with the entered one selected; a
      file answers the path to open.
- [x] 2.6 The ranking table as the artifact's source.

## 3. The surface

- [x] 3.1 `shared/canvas.rs`: the cell canvas leaves the architect, a
      second extension drawing on it.
- [x] 3.2 `ContentMode::Map`: the frame, the trail from the checkout down,
      the arrows walking tiles, close leaving the level before the map.
- [x] 3.3 `absorb_measure`: never changes what is on show; `showings()`,
      the one list the chips and the click that picks one are read from,
      offering the map only where the tree is.
- [x] 3.4 Tests through the surface: a tile followed is the selection the
      other halves answer about; the map takes the frame and says which
      level it is on; the changes half neither shows it nor answers its
      key.

## 4. Host side

- [x] 4.1 `spawn_code_measure` on a channel of its own, asked once per
      checkout; dropped when the surface moved on.
- [x] 4.2 `toggle-map` on `m` in the code scope, with the chip that is its
      pointer affordance.

## 5. The architect, after

- [x] 5.1 The map, its area and its kind leave; the board is a board again.
- [x] 5.2 An area is a descent or a set, read off the area: `View::trail`
      becomes the descent with a current step, and the menu draws one
      control or the other.
- [x] 5.3 The key that closes peels one level at a time; a selector that
      offers only what is on show is not drawn as one.

## 6. Docs and gates

- [x] 6.1 `web/content/docs/project-files.mdx` and `AGENTS.md`: where the
      map lives, and why it is not an architecture diagram.
- [x] 6.2 `cargo fmt --check`, `cargo clippy --all-targets -- -D
      warnings`, `cargo test --workspace --no-fail-fast --locked`,
      `openspec validate --all --strict`.
- [ ] 6.3 Validate by hand in an isolated TUI.
