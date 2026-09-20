---
name: architect
description: Writing and changing the Mermaid diagrams a project keeps under `artifacts:` — choosing which diagram and which level a change belongs in, naming it so the architect surface lists it where a reader expects, and proving it still draws. Use when adding or editing a `.mmd`/`.mermaid` file, when asked to diagram or document an architecture, when a diagram does not appear or does not draw in the workspace's architect surface, or when setting up where a project keeps its architecture.
slash: true
metadata:
  opencode/autoinvoke: "true"
---

# UZE — a project's architecture, as files somebody wrote

A project tells UZE where its architecture lives:

```yaml
# agents.yaml
artifacts:
  path: docs/architecture
```

Every `.mmd` or `.mermaid` file under that directory is one **artifact**,
and the workspace's architect surface draws it in terminal cells. Nothing
lists the files and nothing registers them: adding a file is the whole act
of adding a diagram, and an index beside them would be the one document
that is wrong the moment somebody adds the next one.

That is also the boundary of what belongs here. These diagrams are what
somebody **wrote down**. What a project *is* — where its lines are, which
files changed, how large each one is — is measured rather than written and
is already answered elsewhere; a diagram that restates it is a second
answer to go stale.

## Before you add a file

Ask whether the change belongs in a diagram that already exists. Most do.
A project's architecture is a handful of views that stay accurate, not a
directory that grows one file per conversation — and a view nobody updates
is worse than a view nobody drew, because it still looks current.

Add one only when the project has no view at the altitude your change is
about, and say in the diagram's title what it is a view *of*.

## Choose the altitude before the syntax

The single most common mistake is drawing at the wrong level, and nothing
catches it: a diagram full of components under `C4Context` draws perfectly
and tells the reader something false about the system's boundary.

- **`C4Context`** — the system as one box, and who and what is outside it.
  Nothing internal appears here. If you are naming a crate, you are in the
  wrong view.
- **`C4Container`** — the deployable or runnable pieces *inside* the
  system: a CLI, a server, a database, a shared library.
- **`C4Component`** — what one container is made of. One view per
  container, never all of them at once.
- **`sequenceDiagram`** — an interaction over time. Use it when the claim
  is about **order**; a structure drawn as a sequence hides that it has no
  order.
- **`flowchart` / `graph`** — everything whose claim is about
  relationships that are neither containment nor time: a dependency
  direction, a pipeline, a lifecycle.

The first word of the file decides which area the surface lists it under,
so the choice above is also the choice of that first word. The C4 views
are listed from the outside in — context, then container, then component —
which is the order a reader meets them.

## Name it for the reader, not for the filesystem

The surface lists an artifact by its Mermaid front-matter `title`, else a
`title` statement in the diagram, else the file name made readable. Give
it a title:

```
---
title: Containers
---
C4Container
  ...
```

A name is what somebody scanning a list of a dozen diagrams reads. Name
the view (`Containers`, `Install pipeline`, `Agent lifecycle`), not the
occasion for drawing it.

## Join a box to the code it stands for

In a flowchart, a node can point at a real path in the checkout:

```
flowchart LR
  core[uze-core] --> integrations[uze-integrations]
  click core href "crates/uze-core/src/lib.rs"
```

The surface opens it. This is the one thing that turns a diagram from a
picture into a way into the code, and nothing else in the file offers it —
use it wherever a box stands for something a reader would then want to go
and read. Paths are relative to the project.

## Prove it draws — this is not optional

```bash
uze agent artifacts check          # the whole project
uze agent artifacts check --format json
```

It reads every declared file and **draws** it the way the surface would,
then reports each one. It exits non-zero when any of them fails, so it is
a gate and not a report to skim.

Run it after every change to a diagram. Three things it catches that
reading the file does not:

1. **A diagram type that is listed but not drawn.** The surface draws the
   C4 family, sequences and flowcharts. Valid Mermaid outside those
   appears in the list and opens onto a sentence saying it cannot be
   drawn — which is not what you want a reader to find.
2. **A statement that cannot be read.** The check quotes it back.
3. **Relations with nowhere to go.** A diagram can draw with edges
   *missing* — every box present, the board looking finished, a
   relationship simply absent. This is the only failure here that nobody
   catches by looking, and it usually means the view is carrying more
   relationships than it can show. Split it, or raise its altitude.

**Never describe the accepted syntax from memory, including from this
page.** The parser is the only authority on what draws, it moves, and a
remembered grammar diverges from it silently. Write the diagram, run the
check, read what it says. If a statement is refused, the answer is the
refusal — not a guess at what the parser meant.

## When a diagram does not appear at all

- The file is not under the declared `artifacts.path`, or does not end in
  `.mmd` / `.mermaid`.
- `agents.yaml` declares no `artifacts:` at all, or declares a path that
  leaves the project — both of which the check names.
- The directory is read only a few levels deep, so a diagram buried far
  below the declared root is not found.
