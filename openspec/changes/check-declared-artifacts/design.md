## Context

The architect surface reads a directory, parses each file and lays it out.
Everything needed to answer "does this draw" already runs; nothing asks it
except a human opening the TUI.

## Decisions

### The check runs the surface's own path, not a validator

`check` calls `catalog::read`, `mermaid::parse` and `Scene::of` — the three
the surface calls. A separate validator would be a second opinion about a
grammar that moves, and the first time the two disagreed, the one being
read would be the wrong one. The consequence to accept: the check costs a
layout pass per diagram, which is why the command is `JustifiedSlow`.

For the same reason nothing anywhere restates what the parser accepts. The
Skill says which view to draw and refuses to say what a statement may look
like; the failure text an author reads is quoted from the parser.

### Three verdicts, because a fourth state is invisible

`Drawn` and `Undrawable` are the obvious two. `Unrouted` is the one that
earns its place: a diagram whose edges found no path still draws every box,
so the board looks finished and a relationship is simply not there. It is a
failure, and it is the only one an author cannot find by looking — so it is
counted, named, and fails the check rather than being a footnote the way
the surface's footer has it.

### The declaration is a separate question from what it holds

An absent `artifacts:` is an answer, not a fault — most projects have none.
An empty declared directory is an answer too: a project may say where its
diagrams will go before it has drawn one. A declared path the host will not
follow, or a directory that cannot be read, is somebody's mistake. So
`Checkup` has three arms and only one of them fails, rather than the
surface's two, which exist to decide what to print on an empty board.

### The host is the client's, and the CLI borrows it rather than growing one

`uze_extensions::Host` is granted in `src/ui/extension_host.rs`. The check
runs an extension outside the client, and a second grant would mean two
answers to "what can this code reach" for one piece of code. Resolving
where the artifacts are declared moves there too, because
`ArtifactSource`'s own doc already says whose answer that is: the host's,
since only the host may read the project's manifest. The client and the
check now ask one function.

### The gate is the command, not the test

`uze-extensions` may not name a filesystem API, so its test names eight
files in a `vec!` of `include_str!`. That cannot notice a ninth. `make
artifacts` runs the command over the declared directory and joins `make
check`, which is what makes "a diagram that stops routing is a red build"
true of the directory rather than of a list.

### Diagram updates

`docs/architecture/crate-layering.mmd` gains `cli --> ext`: the CLI now
reaches `uze-extensions`, which only the TUI did before. Verified with
`cargo test -p uze-extensions` and with the command itself.

## Candidate ADRs

None. The CLI reaching `uze-extensions` is an edge the layering already
permits and the architecture suite already governs; the rest is a command,
a Skill and a report shape, all cheap to change.
