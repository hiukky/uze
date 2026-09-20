## Why

A project declares `artifacts:` and UZE draws what is there. Nothing tells
whoever wrote a diagram whether it draws.

The contract an author is judged against exists in three places, and an
author reads none of them: `mermaid.rs`, which is the truth; the
`architect-surface` spec, which describes it; and one line of `AGENTS.md`.
Valid Mermaid is not the same as drawable Mermaid — the parser is this
workspace's own, written because Mermaid's is JavaScript bound to a DOM —
so a `gantt` or an `erDiagram` is listed and opens onto a sentence saying
it cannot be drawn. An agent writing a `.mmd` today writes it from memory
and finds out when somebody opens the TUI, or never.

The two feedback loops that exist arrive too late to be one. The surface
shows the reason in place of the board, which only a person opening it
sees. `uze-extensions`'s tests draw the eight diagrams this repository
names in a `vec!` of `include_str!` — a ninth file is not covered, and no
other project is covered at all, because a crate forbidden from naming a
filesystem API cannot walk a directory.

The answer is not to write the grammar down a fourth time. A page
describing a parser diverges from it silently, which is the same failure
the catalog refuses when it declines to keep an index beside the files.
The answer is to make the parser answerable — and then to teach the
judgment a parser cannot check.

## What Changes

- **`uze agent artifacts check [path]`** — reads every file the project
  declares and *draws* it through the surface's own catalog, parser and
  layout, reporting each one and exiting non-zero when any fails. Text for
  reading, `--format json` for a tool. In the `agent` namespace because its
  reader is the agent that just wrote the file
- **Three verdicts, not two** — drawn; undrawable, with the parser's own
  sentence quoted; and **drawn with relations missing**, held apart because
  a board with an edge silently absent still looks finished, which is the
  one failure nobody catches by looking
- **A declaration is judged apart from what it holds** — a project that
  declares nothing passes, an empty declared directory passes (a project
  may say where its diagrams will go before drawing one), and a declaration
  the host will not follow fails
- **`uze:architect`** — a third Skill in the official plugin, carrying what
  a check cannot: which view a change belongs in, why altitude is the
  mistake nothing catches, how the surface names and lists a file, and
  `click … href` as the thing that turns a diagram into a way into the
  code. It states, in as many words, that the accepted syntax is never to
  be recalled from memory — including from the Skill — because the parser
  moves
- **`make artifacts`, in `make check`** — the whole declared directory
  becomes the gate, where the tests could only cover the files they name

## Capabilities

### Modified Capabilities
- `architect-surface`: gains the requirement that a project can be *told*
  whether its artifacts draw, without opening the surface. Nothing about
  what is drawn or how changes — the check runs the same catalog, the same
  parser and the same layout, which is the point: a second implementation
  would be a second answer to drift from

## Impact

- **Extensions** — `architect::check`, and two messages the surface and the
  check both give lifted to one place
- **CLI** — the `agent artifacts` noun and its one verb; a leaf command to
  classify in `command_performance.rs` (`JustifiedSlow`: the cost is the
  project's own and there is nothing to cache)
- **TUI** — none, beyond where the artifact source is resolved: the client
  and the check now ask one function, because `ArtifactSource` says whose
  answer it is
- **Core** — one `UzeError` variant, so the verdict is an exit code
- **Plugin** — a third Skill; `plugins/uze` metadata and
  `docs/capabilities/uze-skill.md`
- **Docs** — `AGENTS.md`'s architecture bullet names the check and says why
  the tests cannot replace it; `docs/architecture/crate-layering.mmd` gains
  the `cli → uze-extensions` edge this change creates
