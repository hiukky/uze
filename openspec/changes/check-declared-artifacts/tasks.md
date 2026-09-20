## 1. Asking the parser

- [x] 1.1 `architect::check` — the surface's own catalog, parser and
  layout, reporting per artifact instead of a canvas.
- [x] 1.2 `Verdict` — `Drawn`, `Unrouted { edges }`, `Undrawable(reason)`,
  with the reason quoted from the parser and never composed here.
- [x] 1.3 `Checkup` — `Checked`, `Unusable`, `Nothing`: the declaration
  judged apart from what the directory holds.
- [x] 1.4 The two messages the surface and the check both give (`no
  artifacts declared`, `the directory could not be read`) said once.
- [x] 1.5 Tests: verdicts per artifact, a dense graph that really does
  leave edges unrouted, and the three states of a declaration.

## 2. The command

- [x] 2.1 `uze agent artifacts check [path]`, hidden with the rest of the
  `agent` namespace, `--format text|json`.
- [x] 2.2 The verdict is the exit code; the failing sentence is said once,
  as the error, rather than twice on one screen.
- [x] 2.3 `command_performance.rs`: `JustifiedSlow`, with the reason.
- [x] 2.4 `artifacts_declared_in` beside the grant, and the orchestrator
  asks it instead of mapping the manifest itself.
- [x] 2.5 `UzeError::ArtifactsNotDrawable`.

## 3. The judgment a check cannot make

- [x] 3.1 `plugins/uze/skills/architect/SKILL.md`: altitude before syntax,
  naming for the reader, `click … href`, the check as the loop, and the
  refusal to recall a grammar.
- [x] 3.2 `plugins/uze` metadata, `marketplace.json`,
  `docs/capabilities/uze-skill.md`, and the embedded-snapshot test.

## 4. The gate

- [x] 4.1 `make artifacts`, in `make check`.
- [x] 4.2 `AGENTS.md`: the check, and why the tests cannot replace it.
- [x] 4.3 `docs/architecture/crate-layering.mmd`: `cli --> ext`.
