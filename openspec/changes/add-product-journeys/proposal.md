## Why

The suite proves the domain and stops at the frontend. `tests/` (L0-L3) runs
the real `uze` binary against fake harnesses and asserts on its own JSON
reports; `conformance/` proves what a *harness* does. Nothing drives the
28k lines under `src/ui/` and nothing reads the machine after a flow to ask
whether the world actually changed.

The bug record says exactly that: of the last 40 non-merge `fix(...)`
commits, over half are `fix(tui)`, `fix(worktree)`, `fix(workspace)` or
`fix(core)` about slots, panes, agents and sidebars — a placed agent whose
slot was already taken, a delivered agent that lost its slot while still
writing, a picked directory that opened a slot instead of its project, a
closed space that reopened across a manage round trip. Each was found by
hand, and nothing in CI would have caught any of them a second time.

The two tiers we have cannot close it. A Rust L3 test cannot click a strip
cell, and its assertions ask UZE whether UZE is happy — which is silent
precisely when the command reports success and writes nothing. The Lab
answers a different question (does the vendor binary behave), in a container,
against real harnesses.

## What Changes

- Add **product journeys**: a scripted, declarative tier that performs a
  user's flow — through the CLI *and* through the TUI — in a disposable
  sandbox HOME, then asserts against the **real machine state** the flow was
  supposed to produce: files, symlinks, config documents, receipts, Git
  worktrees and branches, terminal state, processes.
- One scenario language, two drives. A journey is one YAML file: the world it
  needs, the gestures it performs (`run:` for a command, `click`/`type`/`key`
  for the TUI), and the checks that must hold afterwards. Adding a journey
  requires **no code** — the step and check vocabulary is closed and small,
  which is the maintenance property Gherkin glue code fails to keep.
- A journey **never validates UZE with UZE**: a check reads the filesystem,
  Git, or a process. UZE's own report (`--format json`, `uze status`) is
  asserted *against* that truth, never used as the truth.
- Journeys speak product terms, not vendor paths. `delivered: hello@demo to
  claude as native` resolves through a per-harness **atlas** owned beside the
  integration; the check itself still reads the bytes on disk.
- A `no_orphans` check closes the lifecycle loop: the sandbox tree after
  `remove` equals the tree before `install`, byte for byte.
- Runs in two worlds: `synthetic` (generated fake harness binaries, offline,
  every PR) and `real` (harness binaries present, skip-if-absent, nightly and
  local).

## Non-goals

- **Not a screenshot test.** Screen text is only ever a gate that a gesture
  landed (`expect:`); no golden frames, no pixel or full-screen snapshots.
- **Not a second Lab.** A journey asserts what UZE did; whether a real
  harness then *reads* it stays a `conformance/` scenario.
- **Not a migration.** The Rust L0-L3 suites stay as they are; journeys cover
  what Rust cannot reach and are not allowed to restate A1-A12.
- **No model calls, no credentials, no network** in any journey that gates a
  PR.

## Capabilities

### New Capabilities

- `product-journeys`: the scenario language, the two drives, the check
  vocabulary that reads the machine, the world builder, the evidence and
  quarantine discipline, and the CI shape.

### Modified Capabilities

None.
