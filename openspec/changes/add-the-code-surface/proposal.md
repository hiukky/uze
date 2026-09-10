## Why

`add-git-diff-overlay` removed the one reason a person still left the
workspace TUI for an external editor: seeing what changed. It did not
remove the next one — reading a file that is *not* in the diff, and
changing it.

The first attempt at that shipped a file explorer as a **sibling** of the
changes overlay: its own button, its own tree, its own selection. Using it
showed why that is wrong, and the reason is about the workflow rather than
about the code. The flow this product is for is **agent first → diff →
and only last, touch the real file**. A person's first contact with the
work is the diff; editing is the exception at the end. So "I see it in the
diff, let me fix that line" is the most-crossed seam there is — and two
overlays put a close, an open and a re-navigation to *the same file* in
the middle of it.

Merging them is not a de-duplication argument. What was genuinely shared
is already shared: the host draws both, the geometry and hit-testing are
one module, and syntax highlighting was extracted when the second reader
appeared. The two navigators solve genuinely different problems — a flat,
complete list from `git status`, compacted; and a partial, unbounded tree
listed on demand. What only one extension can have is **a selection that
survives the switch**, and that is the whole of the value.

## What Changes

- **One extension, `code`**, replacing `git` and `explorer`. Three
  surfaces of one checkout: what changed in it, what it contains, and
  what happened to it (the sidebar's commit timeline comes along).
- **Two doors, not one.** A changes chip and a code chip in the action
  bar, and the `toggle-changes`/`toggle-files` actions behind them, each
  opening the same surface in its own mode. One button would only defer a
  choice the person already made before pressing it. Once the surface is
  open those same two actions switch modes rather than opening anything.
- **The surface is asked in meanings, never in keys.** It takes a
  `view::Command`, and typing reaches it as `Command::Type(char)` from
  the host's own text resolution — so every one of its keys is named
  once, rebindable, and listed in the index, like every other.
- **The selection is a path, and it survives.** Switching from the diff
  to the file's contents keeps the file *and* the line: a diff row knows
  its new-side line number, so the caret lands where the reader was
  looking. Switching back does the same.
- **The changes chip stays a badge.** It reads `+N -M` and is absent when
  nothing has changed — with nothing to say it says nothing rather than
  saying zero. That costs no reachability: the code chip is always there,
  and the diff is one mode switch away inside the surface it opens.
- **A background refresh never touches the buffer.** The changes half
  re-reads on a timer, as it always did; it now re-reads *only* the
  changes half, because the same view holds a file someone may be typing
  into.
- **`Symbol::Files` becomes `Symbol::Code`**, with a glyph that is not the
  placeholder the first attempt shipped.
- **The key vocabulary grows by what an editor needs** — `edit-file`,
  `save-file`, `delete-file`, `confirm-delete`, the four caret motions,
  `insert-newline` and `erase-forward` — and gains a `code-editing`
  scope that seals the keyboard while a file is being typed into, the way
  the action index already does.
- **BREAKING** for anything naming `uze_extensions::git` or
  `uze_extensions::explorer` — both paths are gone. Every caller is in
  this repository.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `terminal-runtime`: replaces the separate file explorer requirements
  with one code surface — its two entry points, the selection that
  survives a mode switch, and the editing and deleting it allows.

## Impact

- `crates/uze-keys`: `Scope::GitChanges` becomes `Scope::Code` plus
  `Scope::CodeEditing`; `Action::ToggleGitChanges` becomes
  `ToggleChanges`, with `ToggleFiles` and the editing actions beside it.
- `crates/uze-extensions`: `git/` and `explorer/` become `code/`
  (`changes`, `diff`, `history`, `files`, `editor`, `request`, `render`);
  `ExtensionHit::Git`/`Explorer` become `Code`, and `GitTimeline` becomes
  `CodeTimeline`.
- `src/ui/orchestrator*`: two overlay states collapse into one, with one
  navigator width, one scroll and one drag; two entry points where there
  were two buttons for two overlays.
- `crates/uze-theme`: `Symbol::Files` → `Symbol::Code`, both shipped
  themes, regenerated schema.
- No new external dependency, no server-side or protocol change.
