# A project's artifacts are described by the files themselves

Status: Accepted

## Context

The architect surface first drew diagrams compiled into the binary. Making
them the project's own raised what a project has to write for a diagram to
appear: where the files live, which area each belongs to — C4, sequence,
flowchart — what it is called, and how C4 views relate as levels.

The proposal on the table was a directory declared in `agents.yaml` plus a
`manifest.yaml` inside it mapping each resource to its type and name. A
lighter variant put the type in each file's front matter.

Both say something the file already says. A Mermaid file cannot be Mermaid
without its first word naming the diagram type, and Mermaid already
defines `title`. A second place to state either is free to disagree with
the first, is a file every other Mermaid reader ignores, and is one more
thing to edit before a new diagram shows up.

## Decision

`agents.yaml` names one thing: the directory, as `artifacts.path`,
relative to the project and refused if it leaves it. It is declared rather
than conventional because the project already has one place where it
declares things.

Everything else is read from the files. Every `.mmd` / `.mermaid` under
the directory is an artifact. Its area is the diagram's first word. Its
name is the front-matter `title`, then the `title` statement, then the
file name. C4 artifacts are ordered by level — context, container,
component, dynamic — and joined into levels by what C4 authors already
write: a box whose alias another C4 artifact draws as a boundary leads
inside it. The one explicit join is the last, to the code, because no
alias can name a path: `$link` on a C4 element, `click … href` on a
flowchart node.

There is no manifest, and nothing uze-specific is required in a diagram.

## Consequences

A diagram appears by being saved. A project's existing Mermaid set works
unedited, and stays readable by every other tool — GitHub, an editor
preview, mermaid.js.

What files cannot say stays unsaid: an order other than level-then-name,
and a grouping other than diagram type. If a project needs either, the
path is optional front-matter keys on the file that is being ordered, and
still not a manifest — adding a manifest later is easy, and removing one
that projects have written is not.

Joining by alias is implicit, so it can be wrong silently: two boundaries
with one alias resolve to the first, and the join is restricted to C4
artifacts because an unrestricted one made a flowchart's subgraph the
inside of a container by accident of naming.

Source change: openspec/changes/archive/2026-09-19-add-the-architect-surface/
