Written after the work: every task below is done, and each group names
the commit that carried it, so the list reads as a map of the branch
rather than as a plan.

## 1. The engine: Mermaid into cells (`38573f27`)

- [x] 1.1 `architect/model.rs`: the graph a diagram becomes — nodes with
      a shape and an `external` flag, nested clusters, edges with a stroke
      and a label, and a sequence as its own model.
- [x] 1.2 `architect/mermaid.rs`: read flowcharts (subgraphs, shapes,
      chained and grouped edges, the three strokes), the C4 family
      (elements, boundaries, deployment nodes, `Rel*` and `RelIndex`) and
      sequence diagrams; ignore styling statements; fail one diagram with
      the statement quoted when it cannot be read.
- [x] 1.3 `architect/layout.rs`: layered placement, applied recursively
      per cluster so a subgraph is laid out before the level that holds
      it.
- [x] 1.4 `architect/route.rs`: A* over the cell grid, pricing turns,
      crossings, boundaries, box halos and a wrong-facing exit, and
      letting edges that share an end share a trunk; count what could not
      be routed instead of failing.
- [x] 1.5 `architect/canvas.rs` and `paint.rs`: cells that remember which
      sides a line leaves by, junction glyphs from that mask, the ASCII
      set beside the box-drawing one, and labels placed on the line,
      beside it, or broken over two rows — or left off.
- [x] 1.6 `architect/sequence.rs`: lifelines and messages in order.
- [x] 1.7 Sanction `canvas.rs` in the chrome-glyph rule as content, with
      the reason written beside it.

## 2. The surface and its doors (`38573f27`, `d68618cc`)

- [x] 2.1 `architect.rs`: the view state, `CATALOG`, the registry entry
      and `ExtensionHit::Architect`.
- [x] 2.2 `Scope::Architect`, sealing; `toggle-architect` bound in the
      workspace and in the code surface; the pans, next/previous artifact
      and `next-rendering`; an affordance entry for every action.
- [x] 2.3 `Symbol::Architect` in the default, ASCII and Nerd Font sets;
      regenerate the theme schema.
- [x] 2.4 The architect chip in the tab strip, emitting
      `WorkspaceHit::OpenArchitect`.
- [x] 2.5 Drag: press, move and release tracked by the orchestrator, a
      release after movement never read as a click, and the surface
      counted in `no_modal_open` so no part of the gesture reaches the
      pane under it.
- [x] 2.6 Selection: a click lights a box and the edges that touch it;
      the footer reads its in and out counts.

## 3. A board instead of a document (`52dac57c`, `4bb6d000`)

- [x] 3.1 `view::Layout::Board` and its drawing in `extension_view.rs`:
      content clipped instead of wrapped, `board_rows` / `board_space`,
      the caption right-aligned in the footer row.
- [x] 3.2 The dot grid, and `Cell::solid` so it never shows through a box
      or between two words.
- [x] 3.3 The minimap: braille, a fixed 28×7 cells for every artifact,
      the view marked on it, and a click as "go here".
- [x] 3.4 Free the board: a corner that is `None` at home and clamped so
      the drawing's edge reaches the middle of the view.
- [x] 3.5 Wheel in both axes, page up and down.

## 4. Artifacts a project owns (`6f7dc487`)

- [x] 4.1 `ProjectManifest::artifacts` (`path`, unknown keys denied), the
      commented scaffold entry, and the scaffold test.
- [x] 4.2 `uze-application` `services::artifacts::project_artifacts`:
      undeclared, declared, or refused — a path that is absolute or leaves
      the project is never followed.
- [x] 4.3 `architect/catalog.rs`: every `.mmd` / `.mermaid` under the
      directory, bounded depth, hidden entries skipped; area from the
      first word, name from front-matter `title`, then the `title`
      statement, then the file name.
- [x] 4.4 `spawn_artifacts_read` / `absorb_artifacts`: the read off the
      drawing thread, the answer tagged with the root it was asked for.
- [x] 4.5 The three empty states: nothing declared (with how to), a
      refused path, a directory with no Mermaid in it.
- [x] 4.6 Delete the static samples; declare this repository's own
      artifacts under `docs/architecture/diagrams/` and draw those in the
      tests.
- [x] 4.7 `web/content/docs/project-files.mdx`: the `artifacts` section.

## 5. The menu (`40fccb87`, `7aa88795`)

- [x] 5.1 `Navigator::choosing` and the `ChooseGroup` / `ChooseItem`
      commands and hits: an open list is the extension's state, drawn by
      the host.
- [x] 5.2 One row: the area selector with a count per area, then the
      artifact selector; menu hits placed ahead of the board's so a click
      on a list never reaches what is under it.
- [x] 5.3 An open list takes the keyboard — up, down, left and right
      between the two, activate, dismiss.
- [x] 5.4 A list longer than the board scrolls around its highlight and
      says `n/total`.
- [x] 5.5 `choose-artifact` on `o`; `choose-area` named and deliberately
      unbound, with the reason in the keymap test.

## 6. C4 levels, down to the code (`260641c4`, `b409e833`)

- [x] 6.1 Join a box to the C4 artifact that draws a boundary with its
      alias; C4 artifacts only.
- [x] 6.2 `$link` on C4 elements and `click … href` on flowchart nodes.
- [x] 6.3 The mark in a box's border — inside, or out to the code — in
      both glyph sets, and the footer saying what activate does.
- [x] 6.4 Follow by activate, by a click on the mark, or by a second
      click on the box.
- [x] 6.5 The trail in the menu (`View::trail`, `ViewHit::SelectTrail`),
      `level-up` on backspace, and coming back with the entered box
      selected and in view.
- [x] 6.6 `ArchitectOutcome::OpenPath` → `CodePlace::at`: the code
      surface opened on the linked path.
- [x] 6.7 Directional selection (`select-box-*` on shift+arrows), bringing
      the box into view.
- [x] 6.8 Order the C4 area by level — context, container, component,
      dynamic — before name.

## 7. Gates

- [x] 7.1 `cargo fmt --check`, `cargo clippy --all-targets -- -D
      warnings`, `cargo test --workspace --no-fail-fast --locked`.
- [x] 7.2 Validated by hand in an isolated TUI after each step, steered
      by the operator's screenshots.
- [x] 7.3 No LikeC4 update required (design.md — Architecture model), so
      no arch-validate run is owed.
- [x] 7.4 `openspec validate --all --strict`.
