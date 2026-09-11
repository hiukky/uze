## Context

See proposal.md — Why.

The constraints are the ones `add-git-diff-overlay` and the
`uze-extensions` split left behind, unchanged by this: an extension
describes and the host draws; an extension reaches nothing it was not
handed; nothing the workspace client draws waits on a repository. Three
architecture tests hold them.

One of them is worth restating because it decides the shape below: an
extension already has *several* surfaces. The `git` extension draws the
changes overlay and the sidebar's commit timeline, with one
`ExtensionHit` variant each, and the crate's own doc names that as the
pattern. So a merged extension with three surfaces is the documented
shape, not an exception — the architecture does not decide this question,
which means it can be decided on merit.

## Goals / Non-Goals

**Goals:**

- One selection, shared by every mode, so the switch a person makes most
  often costs nothing.
- Two entry points, each landing where the person meant to land.
- Keep the two internal models apart. They are not the same problem.
- A timer refresh that cannot reach a buffer being typed into.

**Non-Goals:**

- De-duplicating the two navigators. They look alike and are not (see
  Decisions).
- Making the diff editable in place. Editing happens in the file's
  contents; the diff is a reading surface.
- A third mode. Changes, contents and history are what exist.

## Decisions

### One extension, two models, one selection

The merged view holds both models side by side and one path:

```
CodeView {
    selected: Option<PathBuf>,   // the file every mode is about
    navigator: NavigatorMode,    // Changes | Files
    content:   ContentMode,      // Diff | Contents
    changes:   Changes,          // git status + the compacted tree
    files:     Files,            // listings + what is expanded
    open:      Option<OpenFile>, // the buffer, when there is one
}
```

The rule that keeps this from rotting is that **no handler branches on
the mode to decide what the state means**. The mode decides *who is
asked*; each side answers for the same path and knows nothing about the
other. `Changes` answers "the diff of this path"; `Files` answers "the
tree containing this path"; `OpenFile` answers "the text of this path".

This is why the selection had to stop being an index. `GitView` selected
by position in `Vec<ChangedFile>`, which cannot mean anything to a tree
that lists directories on demand. A path means the same thing to both,
and it is what a person means by "this file".

*Alternative considered:* keep two extensions and put the shared
selection on the host. Rejected — the host would have to learn both
vocabularies (an index into changed files, and a path), which is worse
than any duplication this avoids. That the selection cannot live above
them is the argument that they are one extension.

### Two doors into one surface

A single button defers a decision the person already made: they know
whether they are reviewing or navigating before they press anything.
So there are two chips and two shortcuts, each opening the same view with
its own `navigator`/`content` pair — and once inside, the same two keys
switch modes rather than opening anything.

This also dissolves the question of which mode is "primary". It is
whichever door was used. A clean checkout is no longer an awkward case:
the changes door still opens and still says there is nothing, which is a
legitimate answer rather than an empty screen.

Only one of the two chips is unconditional. A checkout always has files,
so the code chip is always drawn; the changes chip is a *badge* that is
also a door, and a badge with nothing to report says nothing rather than
saying zero — `+0 -0` on every clean checkout is noise the eye has to
learn to skip.

Making it conditional would be wrong if it were the only way in. It is
not: the surface opens from the chip that never leaves, and the diff is
one mode switch away inside it — which is the same move the two doors
already are once the surface is open. The shortcut that lands directly on
the changes is unconditional too. So what disappears is a report, not a
route.

### The switch carries the line, not just the file

A diff row carries the line number it has on the new side. Switching from
the diff to the contents puts the caret on that line; switching back
scrolls the diff to the row that mentions it. Carrying only the file
would leave the reader at the top of a file they were looking at the
middle of — which is most of the trip they were trying to avoid.

Switching to the contents of a file the tree has not listed yet expands
its ancestors, queueing the listings that requires. The selection is what
is authoritative; the tree catches up to it.

### A refresh re-reads only what it owns

`GitView::reload` rebuilt the whole view off-thread and installed it over
the old one. That was safe when nothing in the view was authored by the
viewer. It is not safe now: the same struct holds a buffer.

So the timer path returns a `Changes` and nothing else, and the file path
stays the request queue the explorer introduced. Two cadences, two
pending flags, one view — and no code path where a refresh can reach the
text someone is typing.

### The internals stay apart

The two navigators are the thing that *looks* duplicated and is not.
`changes` compacts a flat, complete, small list from `git status`, fusing
single-child directories, with ids that index the changed files.
`files` flattens a partial map that grows as directories are opened, with
ids that are paths and an expansion that costs a round trip to the host.
Same silhouette, different algorithms, different failure modes.

What was genuinely common was extracted before this change (the host
draws both; `shared/highlight.rs` colours both) or belongs to the host
already (geometry, hit-testing, the palette). Merging the models would
join about forty lines of `focus`/`scroll` and produce a struct carrying
both a `Vec<ChangedFile>` and a `BTreeMap<PathBuf, Vec<DirEntry>>` behind
a discriminant — the DRY-over-silhouette trade that costs more than the
repetition it removes.

### Typing is a scope, not an exception

An extension names no key, and `uze-keys` is what made that true rather
than aspirational — so the editor could not keep the `KeyEvent` the first
attempt handed it. It takes a `view::Command`, and the vocabulary grows by
what an editor actually needs: a caret moves by character, and text
arrives as `Command::Type(char)`.

The one thing a keymap cannot enumerate is "every printable character",
and the host already had the answer: `Resolution::Text`, which the action
index and the rename buffer use. So a file open for typing is its own
scope, `CodeEditing`, sealed the way the action index is — nothing behind
it may answer a letter — and `type_character` gains one more sink.

*Alternative considered:* let the extension keep taking keys, sanctioned
as an exception. Rejected: the exception would be the one surface where
a rebound key does not work, which is precisely the claim `uze-keys`
exists to make true everywhere.

### The name

`code`. The constraint that decided it: the name cannot presuppose Git,
because the file side works in a directory that is no repository at all
and the diff side draws a "not a git repository" state. That rules out
`git`, `repo`, `checkout` and `source` (which drags "Source Control", a
Git-only association). The thesis rules out the other side — if the diff
is the entry point, the name cannot be file-centric, so not `files` and
not `explorer`.

`workspace`, `project` and `work` are all taken elsewhere in this
product. `review` names the activity well but is too narrow: browsing a
file that did not change is not review, and neither is editing.

## Candidate ADRs

- **An extension is a surface, not a data source** — the merge is decided
  by what a person does in one place rather than by what the code reads,
  and it sets how the next extension is scoped. Cheap to reverse today;
  worth recording only if it holds.

## Risks / Trade-offs

- **One struct holding two models invites a mode flag.** → The rule above
  is the mitigation, and it is testable: the switch must be expressible
  as "ask the other side about the same path".
- **A path selection is weaker than an index for the changes list.** A
  file that stops being changed leaves the selection pointing at
  something the list no longer holds. → That is already the truth the
  tree side lives with, and it is what the contents mode can still show.
- **Two chips take more of the header than one.** → The header lays
  actions out from the right before it lays out any message, so what a
  chip costs comes out of the message zone, not out of another control.
- **A large rename with no behaviour change in most of it.** → Held by
  the tests that already covered both extensions; every one of them moves
  with the code it exercises.

## Architecture model

No update to `docs/architecture/likec4/` is required — the `Workspace
Client` container's description already says it browses and edits the
project's files, which stays true.
