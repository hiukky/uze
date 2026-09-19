## 1. Layout

- [x] 1.1 `architect/treemap.rs`: squarified layout in seen space, edges
      rounded cumulatively; tests for coverage without gap or overlap, for
      proportion, and for tiles staying close to square as seen.

## 2. The code map

- [x] 2.1 `architect/codemap.rs`: `measure` through `Host::git` (lines,
      churn, changed), parsed from NUL-separated output, renames included.
- [x] 2.2 The tree: directories summed, single chains merged, children by
      weight; heat ranked into four steps, a directory taking its hottest.
- [x] 2.3 Tiles for a space: fold what is too small, recurse while a
      directory can show its contents, otherwise one tile.
- [x] 2.4 Paint: frames, names, line counts, shade for the hotter steps in
      both glyph sets (shade glyphs in `canvas.rs`), the changed mark, the
      selection.
- [x] 2.5 Selection by path, toward a direction, by click; enter a
      directory or a folded tile; back with the entered one selected; a
      file answers the path to open.
- [x] 2.6 The ranking table as the artifact's source.

## 3. The surface

- [x] 3.1 `Kind::Code`, sorted last; the derived artifact.
- [x] 3.2 `ArchitectView`: a code map drawing that fits the view — no
      corner, no minimap, no grid; pans select; trail from the zoom.
- [x] 3.3 `absorb_code`: appended without moving what is on show; shown
      when there is nothing else, with the declaration's trouble in the
      caption.
- [x] 3.4 Tests through the surface: late measurement, undeclared project,
      enter/back/open, resize re-lays, ASCII is ASCII.

## 4. Host side

- [x] 4.1 `spawn_code_measure` on the artifacts channel, tagged with its
      root; dropped when the surface moved on.

## 5. Docs and gates

- [x] 5.1 `web/content/docs/project-files.mdx`: the code map paragraph.
- [x] 5.2 `cargo fmt --check`, `cargo clippy --all-targets -- -D
      warnings`, `cargo test --workspace --no-fail-fast --locked`,
      `openspec validate --all --strict`.
- [x] 5.3 Validate by hand in an isolated TUI.
