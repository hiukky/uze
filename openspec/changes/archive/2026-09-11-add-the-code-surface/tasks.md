## 1. Widen the two contracts

- [x] 1.1 Add `DirEntry` and the three grants (`list_dir`, `write_file`,
      `delete_file`) to `uze_extensions::Host`, with the trait doc stating
      plainly that the grant is no longer read-only and why.
- [x] 1.2 Give `DirEntry` a hand-written `Ord` that puts directories
      before files — derived, it would order them the other way round,
      which is the opposite of every file tree a person has used.
- [x] 1.3 Add `view::Caret` and `Content::Lines::caret`, and
      `ViewHit::PlaceCaret { line, cell }`, with the doc stating why the
      caret is a text position and why the hit answers in display cells
      that the extension turns into a character.

## 2. Reorganize the extensions crate

- [x] 2.1 Move syntax highlighting out of `git` into
      `shared/highlight.rs` and have `git` use it; the fallback theme
      name lives there now.
- [x] 2.2 Split `git.rs` into `git.rs` + `git/{status,diff,history,tree,render}.rs`
      with no behaviour change, moving each unit test to the module it
      exercises and leaving the repository-backed ones with `GitView`.
- [x] 2.3 State the crate's layout rule in `lib.rs` and in `AGENTS.md`:
      one directory per extension, `shared/` only for what a second
      extension actually reached for, the two contracts at the root, and
      module files beside their directory rather than `mod.rs` inside it.

## 3. The explorer extension

- [x] 3.1 `explorer/request.rs`: `FileRequest`, `FileAnswer`,
      `LoadedFile`, and `fulfill` — the one function in the extension
      that takes a `Host`.
- [x] 3.2 `explorer/tree.rs`: flatten the listings and the open set into
      the rows a viewer can see, derived on every call rather than cached.
- [x] 3.3 `explorer/editor.rs`: the buffer, the caret, and the edits that
      move both, including re-colouring the caret's line after a change.
- [x] 3.4 `explorer.rs`: view state, the request queue, `absorb`, and the
      key/mouse/wheel handling — browsing, opening, editing, saving, and
      deleting with confirmation.
- [x] 3.5 `explorer/render.rs`: the navigator, the content, and the
      footer, producing lines from the first one so the host's `scroll` is
      applied exactly once.
- [x] 3.6 Register `explorer::CATALOG` in `ExtensionRegistry::builtin`
      and add the `ExtensionHit::Explorer` variant.
- [x] 3.7 Tests: a folded directory contributes one row; editing and
      saving writes what was typed; deleting asks first; a directory is
      never deletable; closing with unsaved changes asks once; a file that
      ended in a newline still does; an unreadable file says so; a late
      answer never replaces a different file; a re-read never overwrites
      keystrokes typed while it was out.

## 4. Host and renderer

- [x] 4.1 Implement the three grants in `src/ui/extension_host.rs`,
      including the two refusals (a save writes only to an existing file;
      a delete removes only a file) and the listing order.
- [x] 4.2 Test the grants: both refusals, that a refused delete leaves the
      directory intact, and that a listing is ordered the same way twice.
- [x] 4.3 Place the caret in `src/ui/extension_view.rs`, converting the
      text position to a screen cell through the same wrap the line was
      drawn with, and emit a `PlaceCaret` hit per drawn *visual* row —
      a wrapped line covers several, and which one the pointer is on is
      half of where in the text it landed.
- [x] 4.4 Draw the caret by inverting the cell it sits on rather than by
      drawing a mark into it — a mark replaces the character underneath,
      making the letter being edited the one letter the person cannot see.
- [x] 4.5 Resolve a click in the contents to a text position in two
      halves: the host answers in display cells from the row it drew
      (`caret_cell_at`), the extension turns cells into a character
      (`column_at_cell`), because a double-width glyph is two cells and
      one character and only the extension holds the text.
- [x] 4.6 Test both: that the character under the caret is still on
      screen and its cell inverted, that a click resolves to the cell it
      landed on, that cells become the right character across a wide
      glyph, and that a click inside the overlay resolves to the row the
      frame drew.

## 5. Wire it into the workspace client

- [x] 5.1 Add `ExplorerResolution` and `spawn_explorer_request` — the
      reach for the host, inside a `thread::spawn` as the architecture
      suite requires.
- [x] 5.2 Add the model's fields (view, tree width, tree scroll, drag
      flag, pending flag), `schedule_explorer_request`,
      `absorb_explorer_answer`, and `open_explorer`; include the explorer
      in `no_modal_open`.
- [x] 5.3 Thread the channel through `WorkspaceMemory`, `AttachAnswers`,
      `AttachInbox` and the pump.
- [x] 5.4 Route keys (`Ctrl+E` to open, the overlay's own handler while
      open), the mouse-down branch, the wheel branch, and the tree
      divider's drag.
- [x] 5.5 Render the overlay and draw the tab-strip button
      unconditionally, laid out before the change badge.
- [x] 5.6 Test that the button is present with and without changes, and
      that the badge appears to the right of it.

## 6. Theme and documentation

- [x] 6.1 Add `Symbol::Files` with a glyph in both shipped themes and
      regenerate `themes/theme.schema.json`.
- [x] 6.2 Record the narrowed write grant in
      `docs/architecture/invariants.md`, under the invariant that says an
      extension reaches nothing it was not handed, naming the test.
- [x] 6.3 Widen the `Workspace Client` container's description in
      `docs/architecture/likec4/model.c4` to say it now edits the
      project's files. No structural model change is needed (see
      design.md — Architecture model), and the repository has no
      arch-validate script to run.

## 7. Merge the two extensions into the code surface

- [x] 7.1 Rename `Symbol::Files` to `Symbol::Code` and give it a glyph
      that is not a placeholder, in both shipped themes; regenerate
      `themes/theme.schema.json`.
- [x] 7.2 Move `git/` and `explorer/` into one `code/` — `changes` (the
      status model and its compacted navigator), `diff`, `history`,
      `files` (the filesystem tree), `editor`, `request`, `render` — with
      one `CATALOG`, and replace `ExtensionHit::Git`/`Explorer`/
      `GitTimeline` with `Code`/`CodeTimeline`.
- [x] 7.3 Make the selection a path rather than an index into the changed
      files, so both halves address the same thing, and derive the
      changes list's position from it.
- [x] 7.4 Add `NavigatorMode` and `ContentMode`, and the switch that
      carries the selection — including the line, read from the diff row
      the viewer is on, and the ancestors the tree has to open to show a
      file it has not listed.
- [x] 7.5 Split the background paths: the timer re-reads only the changes
      half (never the whole view, which now holds a buffer), and the file
      requests stay the queue they were.
- [x] 7.6 Collapse the client's two overlay states into one — one view,
      one navigator width, one scroll, one drag — and give it two entry
      points (`Ctrl+G`/the changes chip, `Ctrl+E`/the code chip) that open
      it in their own mode and switch modes once it is open.
- [x] 7.7 Keep the changes chip a badge — absent when nothing changed,
      because `+0 -0` on every clean checkout is noise — and prove the
      route survives it: the code chip is unconditional, and the diff is
      a mode switch away inside the surface it opens.
- [x] 7.8 Tests: each entry point opens the surface in its own mode;
      switching from a diff to the contents keeps the file and the line;
      switching to a file the tree has not listed opens its ancestors; a
      changes refresh leaves an unsaved buffer alone; a clean checkout
      draws the code chip and no badge, and the badge arriving never
      moves it.

## 8. Speak the product's own input vocabulary

- [x] 8.1 Replace `Scope::GitChanges` with `Scope::Code` and add
      `Scope::CodeEditing`, sealed so nothing behind a file being typed
      into answers a letter.
- [x] 8.2 Rename `Action::ToggleGitChanges` to `ToggleChanges`, add
      `ToggleFiles`, and add what an editor needs: `edit-file`,
      `save-file`, `delete-file`, `confirm-delete`, the four caret
      motions, `insert-newline`, `erase-forward` — each with its default
      binding.
- [x] 8.3 Grow `view::Command` by the same meanings, including
      `Command::Type(char)`, and have the surface take commands rather
      than keys; `View::footer` lists commands so the host prints
      whatever chord currently reaches each.
- [x] 8.4 Route typing through the host's existing `Resolution::Text`
      sink, and answer the three frozen ledgers the new actions touch:
      the destructive set, the bare-key decisions, and the keyboard-only
      affordances.

## 9. Scrollbars in both halves

- [x] 9.1 Add `Content::Lines::total` — the extension sends a *window* of
      lines, and a scrollbar measured against the window would say the
      content is exactly as long as the screen.
- [x] 9.2 Extract the one scrollbar into `src/ui/scrollbar.rs`. The Keys
      screen already had one, so this is the second — which is when a
      shared control is worth making, and what the duplication had already
      produced was two grooves with different glyphs and different hues
      doing the same job. It reuses `Symbol::BarThick`/`BarThin` rather
      than growing the vocabulary, and takes its width from the theme so a
      wider glyph cannot shear the column beside it.
- [x] 9.3 Draw a scrollbar beside the navigator and beside the content,
      only when what they hold outgrows them, with the column taken out
      of their own width rather than laid over it.
- [x] 9.4 Add `ViewHit::DragNavigatorScrollbar`/`DragContentScrollbar`
      and report each frame's `Rendered` geometry back, so a drag can
      answer *where in the content* the pointer went without deriving the
      layout twice.
- [x] 9.5 Move the right scroll on drag: the navigator's is the host's,
      the content's is the extension's (`code::scroll_to`). Measure from
      the middle of the handle, so grabbing it scrolls nothing.
- [x] 9.6 Keep both mappings, named: a list whose window follows its
      selection asks `item_at` (the track pictures the whole list), one
      with a scroll offset of its own asks `first_at` (it pictures where
      the window can go). Iron either into the other and one of them
      scrolls a line short.
- [x] 9.7 Tests: a list that fits has no scrollbar, the whole groove is
      the drag target, a window drag stops at the last screenful, a
      selection drag reaches the last item, and the handle never vanishes
      on the list that most needs it.

## 10. Markdown, previewed

- [x] 10.1 Add `pulldown-cmark` (default features off) — what rustdoc
      itself parses with, MIT, pulldown-cmark org, no C anywhere in the
      tree, and one crate the workspace does not already have
      (`unicase`). The terminal renderers, `termimad` and `tui-markdown`,
      were the obvious reach and the wrong one: both *draw*.
- [x] 10.2 Give `view::Span` an `italic`, because emphasis has two
      weights and a role cannot carry the difference, and honour it where
      spans are styled.
- [x] 10.3 `code/markdown.rs`: headings, emphasis, inline and fenced code
      (highlighted as the language the fence names, through a new
      `highlight::highlighter_for_language` — a fence says `rust` where a
      file says `.rs`), lists with nesting and numbering, block quotes,
      rules, task lists and aligned GFM tables. Raw HTML is shown rather
      than swallowed.
- [x] 10.4 Add `ContentMode::Preview` and `Command::TogglePreview`,
      offered only for a file that is a document, and reading the
      *buffer* rather than the disk so previewing while writing shows
      what was typed.
- [x] 10.5 Bind `toggle-preview` in `Scope::Code`, and give it a real
      control: `View::modes` plus `ViewHit::SelectMode`, drawn as a
      segmented Preview/Source pair on the content's own heading row. A
      mode the keyboard can reach has to be one a pointer can reach, or
      it belongs to whoever read the keymap — which is what its
      affordance entry said before, and it said `Index`.
- [x] 10.6 Tests: the markup becomes the document, emphasis survives as
      weight, a fence is highlighted as what it says it is, lists nest
      and number, a table lines its columns up, HTML survives, and an
      empty document still has a line.

## 11. Seams

- [x] 11.1 Put each groove in the padding column it was leaving blank, so
      it sits flush against the divider beside it.
- [x] 11.2 Draw the handle and not the groove. Adjacent was not enough —
      two full-height lines beside each other are still two lines,
      whatever the gap between them. With the groove undrawn the divider
      is the line and the handle is a mark on it saying where you are,
      which is what the control is for; the whole track stays the drag
      target, because a groove you cannot see is still one you can grab.

- [x] 11.3 Take the box off the overlay. It covers the whole frame, so a
      border draws a line around something with no outside — two rows,
      two columns and four lines of decoration competing with the ones
      inside that mean something. A title row and the divider between the
      columns say all of it.
- [x] 11.4 Put the tree's handle *on* the divider rather than beside it.
      Beside it was two lines and adjacent was still two; on it, the
      divider is the line and the handle is the stretch that says where
      you are.
- [x] 11.6 Give that one line both of its gestures, decided by direction
      rather than by target. A press carries no direction, so nothing is
      decided when it lands: the first movement says which — sideways
      moves the split, along it scrolls — and the decision holds until
      release so a wandering hand cannot switch mid-drag. A press that
      never moves is a click, which brings back jumping the scroll.
      `DraggingTab` already waits like this, which is why waiting is a
      pattern here rather than an invention.
- [x] 11.7 Give the handle a `Symbol::ScrollThumb` of its own — a heavy
      vertical *rule*, not one of the bars. The difference is where the
      glyph sits in its cell rather than what it means: a block element
      is flush left and a box-drawing rule is centred, so reusing the bar
      put a sideways jog in the divider for exactly the rows the handle
      covered.
- [x] 11.5 Reserve the gutter only for lines that are numbered. A
      rendered document has none, and the column was indenting every
      paragraph by seven cells of blank.

## 12. The frame, and what it frames

- [x] 12.1 Make `View::title` spans rather than a string. A title is
      three things at once — what the surface is, which checkout, which
      branch — and one run of text gives them all the same weight, which
      is how a title stops being read. The extension says which part is
      which; the host decides what each looks like, as with every role.
- [x] 12.2 Weight them: the surface's name is a label, said quietly; the
      directories leading to the checkout are context; the checkout's own
      name and its branch identify it, and are what the eye lands on.
- [x] 12.3 Bring the frame back, faint. What made it wrong was never that
      it existed — it was that every line weighed the same, so a box, a
      divider and two grooves argued in one hue. With the weights a
      hierarchy (frame faintest, divider ordinary, handle the only bright
      rule) the box reads as the edge of a surface rather than as another
      control.

## 13. Gate

- [x] 13.1 `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
      `cargo test --workspace --no-fail-fast` — including the architecture
      suite, which must still hold that the extension names no filesystem
      API and that the host is reached only from a thread.
