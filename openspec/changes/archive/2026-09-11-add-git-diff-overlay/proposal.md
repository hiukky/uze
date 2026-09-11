## Why

Anyone using the workspace TUI's terminal tabs today has to leave it (usually
to VS Code) just to see `git status`/`git diff` — the one thing an external
editor still gets reached for. Adding a read-only git changes view directly
in the workspace TUI removes that context switch for the one task that
otherwise forces it.

## What Changes

- Add a global button, right-aligned in the workspace TUI's horizontal tab
  strip, plus a `Ctrl+G` shortcut, that opens a full-frame overlay showing
  the changed files (from `git status`) and the diff of the selected one
  (from `git diff`), with syntax-highlighted diff content.
- The overlay is scoped to the currently active tab's live working
  directory (not the workspace root) — matching the existing hierarchy
  `Workspace > Space > Agent/Shell`, this view is one level further down,
  contextual to whichever Agent/Shell tab is active when it opens.
- The overlay is a dismissible popup (`Esc` closes it back to exactly the
  prior view), the same interaction shape the existing agent picker and
  tab/space close-confirmation menus already use in this TUI, just sized to
  the full frame instead of a small anchored box — not a new persistent
  navigation mode alongside the existing Work/Manage switch.
- Read-only for this change: viewing changed files and their diffs only.
  No stage/unstage/commit/discard action is added.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `terminal-runtime`: adds a read-only git changes/diff view to the
  workspace TUI client, scoped to the active tab's working directory.

## Impact

- `src/` (workspace TUI client): a new `src/ui/git_diff.rs` module (git
  status/diff subprocess invocation, unified-diff parsing, syntax
  highlighting, and its own rendering/input handling), plus a new
  `Ctrl+G`/tab-strip-button entry point and one new `WorkspaceModel` field
  wired into `src/ui/orchestrator.rs`.
- New dependency: `syntect` (diff-content syntax highlighting), used only
  by this client-side module.
- No server-side (`crates/uze-terminal`) or protocol changes — this reads
  the filesystem/git directly from the client process, the same way the
  client already reads a pane's live `cwd` for display.
