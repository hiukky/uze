# Diagrams are drawn in cells, by an engine uze owns

Status: Accepted

## Context

uze stays a terminal product; that was decided before this and is what
forced the question. The architect surface draws a project's architecture
— Mermaid flowcharts, C4 views, sequences — and a diagram is the first
thing uze shows whose *shape* is the content.

There were two ways to put a shape in a terminal. A graphics protocol
(sixel, kitty, iTerm) draws a real image, and works in some terminals,
through no multiplexer by default, and over SSH only when every hop
agrees. Or characters in cells, which is the one unit every terminal uze
already runs in agrees on, and the one the host already knows how to
colour by role.

And two ways to get from Mermaid to a shape. The Rust crates that render
Mermaid are young, single-author, and emit SVG — which fails the
dependency bar in `AGENTS.md` and answers the wrong output anyway. Or a
parser, layout and router written in the workspace.

## Decision

A diagram is drawn with characters in cells, and only so. There is no
image path, not even as an enhancement where a terminal supports one: two
renderings of one diagram is two things to keep true, and it would make
the common case — tmux, SSH — the degraded one.

The engine is uze's own, in `crates/uze-extensions/src/architect/`: a
reader for the subset of Mermaid architecture is written in, a layered
layout applied recursively per cluster, an A* router over the cell grid,
and a painter that keeps lines as *which sides of a cell they leave by*
until the last moment, so edges meeting become junctions. Every glyph the
drawing depends on has a plain ASCII form, switched by one action.

What the engine produces is the contract's existing `ContentLine`s. The
host draws a diagram with the code that draws a diff; the extension still
never draws (ADR-041, ADR-045).

Fidelity to mermaid.js layout is not a goal. A diagram has to say the same
thing, not look the same.

## Consequences

A diagram reads the same in every terminal, over every hop, under every
theme, and costs no dependency.

uze now owns a layout engine. Its router's weights were tuned by eye
against this repository's own diagrams, which are its fixtures; a diagram
that routes badly is answered by a new fixture, never by a constant
adjusted blind. The subset will meet Mermaid outside it: styling is
ignored, an unreadable statement fails one artifact with the statement
quoted, an undrawn diagram type is listed rather than hidden, and the
source rendering is always there.

The ceiling is the cell. Curves, icons and dense graphs of hundreds of
nodes are out of reach, and that is accepted: past that size the answer is
a smaller view of the system, which is what C4 levels are for.

Every later "visual" surface inherits this: what it shows is made of
cells, coloured by role.

Source change: openspec/changes/archive/2026-09-19-add-the-architect-surface/
