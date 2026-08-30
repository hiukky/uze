## 1. Setup

- [x] 1.1 Add `syntect = "5"` (default features) to the root `Cargo.toml`
- [x] 1.2 Confirm `docs/adr/039-adopt-syntect-for-diff-syntax-highlighting.md` exists (per the `adr` artifact)

## 2. Git subprocess + parsing (pure, testable)

- [x] 2.1 Run `git -C <root> status --porcelain=v1` and parse it into `Vec<ChangedFile>` (modified/added/deleted/untracked/renamed)
- [x] 2.2 Run `git -C <root> diff HEAD -- <path>` for tracked files and `git -C <root> diff --no-index -- /dev/null <path>` for untracked ones, treating exit code `1` (a diff exists) as success, not an error
- [x] 2.3 Parse unified diff output into `Vec<DiffLine>` (hunk-header line-number tracking, added/removed/context classification, multiple hunks)
- [x] 2.4 Represent "not a git repository" and "`git` not found" as a displayable `GitView` state, never a panic or a refused open

## 3. Syntax highlighting

- [x] 3.1 Load the bundled `SyntaxSet`/`ThemeSet` (`load_defaults_newlines`/`load_defaults`) once per `GitView::open`
- [x] 3.2 Resolve the syntax for the selected file by name/extension
- [x] 3.3 Convert `syntect`'s highlighted spans to ratatui `Style`/`Color::Rgb`, applied per `DiffLine`

## 4. `GitView` module and rendering

- [x] 4.1 Add `src/ui/git_diff.rs` with `GitView`, `ChangedFile`, `DiffLine`, `GitViewFocus`. Implementation refinement: no separate `GitViewHit` type — its two hit variants (`OpenGitView`, `GitSelectFile`) were added directly to `orchestrator::WorkspaceHit` instead, reusing the one `hits` vec every other overlay (`AgentPicker`, `ContextMenu`) already shares, rather than threading a second parallel hit-testing vec through the render loop for this one overlay.
- [x] 4.2 `GitView::open(root: PathBuf) -> Self` wiring status + first file's diff into initial state
- [x] 4.3 `render(frame, view, hits)`: `Clear` + bordered block over the full frame, two-column layout (changed files | diff), `+`/`-`/` ` gutter with dim line numbers ahead of the highlighted content, footer hint line
- [x] 4.4 `handle_key`/`handle_mouse`: file selection, `Tab` toggles focus between the files list and the diff, diff scrolling, `Esc` (or the same open shortcut) returns `GitViewOutcome::Close`

## 5. Workspace TUI integration

- [x] 5.1 Declare `mod git_diff;` in `src/ui.rs`; add `git_view: Option<git_diff::GitView>` to `WorkspaceModel`
- [x] 5.2 Tab strip: a button pinned to the strip's far-right edge (reserved-width + `Alignment::Right`, same technique the sidebar's "+ new" row already uses) pushing `WorkspaceHit::OpenGitView`
- [x] 5.3 `Ctrl+G` and `WorkspaceHit::OpenGitView` both resolve `root` from `session.selected_tab()`'s pane `cwd` (via the existing `pane_in_layout` helper) and set `model.git_view`
- [x] 5.4 A guarded `Event::Key`/`Event::Mouse` arm (same shape as the existing `renaming`/`agent_picker` guards) forwards to `git_diff::handle_key`/`handle_mouse` while `model.git_view.is_some()`, clearing it on `Close`
- [x] 5.5 `render()`: draw the git view last (after the agent picker/context menu), covering the full frame, when `model.git_view` is `Some`

## 6. Tests

- [x] 6.1 Unit tests for the status parser (modified, added, deleted, untracked, renamed lines)
- [x] 6.2 Unit tests for the unified-diff parser (hunk headers, line-number tracking, multiple hunks)
- [x] 6.3 An integration test that drives real `git` in a scratch repo (a staged change, an unstaged change, an untracked file) and asserts the parsed `ChangedFile`/`DiffLine` shapes match

## 7. Verification

- [ ] 7.1 `cargo build --locked --bin uze`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, and the full test suite all pass
- [ ] 7.2 Live verification (isolated tmux session, this project's established method): open the overlay via the tab-strip button and via `Ctrl+G`; confirm the changed-files list, the first file's diff, and visible syntax-highlight color; confirm selecting a different file updates the diff; confirm `Esc` returns to the exact prior sidebar/tab-strip/pane state; confirm opening it from a tab whose `cwd` differs from the workspace root scopes to that directory
