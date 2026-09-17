# PR #67 — report

`feat/space-kinds` vs `main`: 130 commits, 251 files changed, +18 563 / −18 368.

| Phase | Commits | Diff |
|---|---|---|
| Feature (space kinds, launch-carried identity) | 3 | +6 113 / −1 374 |
| Refactor and hardening | 127 | +13 443 / −18 276 (net −4 833, excluding .md) |

## Feature

- **Launch-carried identity.** An agent is the `UZE_AGENT` stamp on its launch, echoed back by the server. It is no longer inferred from the directory it runs in.
- **Space kinds.**
  - A *worktree* space gives each agent a slot of its own.
  - A *workspace* space runs agents directly in its root, on the operator's branch; these agents are *tenants*.
  - The sidebar draws a tree for worktree spaces and a flat list for workspace spaces.
- **Last space.** It can be closed; a home space replaces it.
- **Stale servers.** A server of another build, or of a deleted `UZE_HOME`, is replaced on attach.

## Refactor

The code was reviewed against the settled design, one review per area, and changed without altering behaviour except where noted in `BREAKING_CHANGES.md`.

- **Removed (dead in production):**
  - project resource composition;
  - the importer/bundle pipeline;
  - verification and representation types;
  - per-session exposure;
  - `plugin_marketplaces.json`;
  - `Layout::Split`/`Focus`;
  - the tenant read model;
  - the protocol version in the pid file;
  - a bootstrap loop that never ran.
- **Unified:**
  - the Claude/Codex generated marketplace, as one dialect;
  - hook, MCP and skill plans across four harnesses;
  - the slot-owner rule (4 copies) and the slot path (5);
  - management drawers (5), confirmations (9) and list screens (4).
- **Types that carry invariants:** `Launch`, `SpaceSeat`, `HookTarget`, `ExposureMechanism::Managed`.

## Bugs fixed

| Area | Bug |
|---|---|
| Workspace | Resuming a preserved task opened an unstamped tab |
| Workspace | Notices were typed into a shell matched by directory |
| Workspace | A tenant stayed live after its tab closed |
| Terminal | A pane that printed before it was registered was never drawn (intermittent "starting shell…") |
| Terminal | Closed panes left zombies; SIGHUP-deaf workers outlived their pane |
| Terminal | A client that stopped reading made the server buffer without limit |
| Terminal | A persisted empty space misaligned restored tabs |
| UI | Moving up from the first Appearance choice froze the client |
| Integrations | Codex was provisioned through UZE's own shim (recursion) |
| Integrations | Codex coverage ignored invocation policy |
| Integrations | Claude's shim skipped a skill's sibling files |
| Code surface | The diff interleaved replacement lines |
| Journeys | 4 gate journeys were already failing on this PR before the refactor (stale selectors, `/proc` on macOS) |

## Verification

| Check | Result |
|---|---|
| `cargo fmt`, `clippy --workspace --all-targets --all-features -D warnings` | clean |
| `cargo test --workspace --all-features` | 1 909 passed, 0 failed |
| Gate journeys (Docker, pinned image) | 23/23 |
| Conformance Lab (rebuilt image) | Claude 38/38 · Codex 50/50 · OpenCode 44/44 (6 ADAPTED) · Antigravity 48/48 |
| Coverage (`llvm-cov`) | 85.0 % lines, 86.1 % regions (CI floor 68/69) |
| `cargo deny` | advisories, bans, licences, sources ok |
| Frame cost, 80 agents on 260×70 | 0.73 ms per frame (release) |
| CI on `649e34e` | 21/21 jobs green: fmt, clippy, test and journeys on linux and macos, conformance for all four harnesses, coverage, MSRV, audit, licences. First green run on this PR. |

## Test quality

- **Strong:**
  - Every bug above has a test that fails without its fix; each was checked by reverting the fix.
  - Process, race and resource tests synchronize on events (the damage channel, a FIFO's end, a reaper's join) rather than sleeping.
  - Journeys assert on the machine (Git, filesystem, process table), never on UZE's own output.
- **Weak:**
  - `orchestrator/session.rs` is still at 44 % coverage. The worker's result handling has since gained characterization tests, and the last-space gesture has its own.
  - There are no property tests for the task, slot or space state machines.
  - 28 test sleeps remain. Each was reviewed: they model time (a mid-save kill, a gate window, clock resolution, a dribbling peer) or wait on another process's `exec`, exit or listen, and all are bounded by a failing deadline.
- **Caught along the way:**
  - A first zombie test passed without the fix, because SIGHUP beat the trap. It was rewritten to wait for the program.
  - A `poll` on a FIFO passed on Linux and failed on macOS. It was rewritten as a thread read.

## Open

- **Backlog** (`BACKLOG.md`):
  - `orchestrator/session.rs` coverage;
  - host-only gestures inside the extension contract;
  - CLI startup measurement.
- **Decisions** (`QUESTIONS.md`):
  - whether `uze-core` must be free of I/O (recommendation: no);
  - removing the seven `.worktrees/simplify*` checkouts;
  - installing `hyperfine`/`perf`.
- **Breaking changes** (`BREAKING_CHANGES.md`, pre-1.0):
  - an old `workspace.json` is discarded once;
  - doctor/inspect JSON shape;
  - probes time out;
  - a harness before setup reports Unsupported.
