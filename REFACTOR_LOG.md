# Refactor log

Newest first. Each entry: what changed, why, what was rejected, numbers,
remaining risk.

## 2026-09-17 — Conformance Lab green on the refactored integrations

- **Why:** the integrations consolidation changed several things that only real harness binaries can check:
  - probes, which now have a timeout and require `--version` to succeed;
  - OpenCode MCP, which now has a single file route;
  - the generated Claude/Codex marketplace;
  - hook and skill plans.
- **Setup:** the cached Lab image carried `uze 0.0.0-alpha.4`, so a first run tested old code. The image was rebuilt from this branch.
- **Result:**

  | Harness | Asserted PASS |
  |---|---|
  | Claude Code 2.1.274 | 38/38 |
  | Codex 0.154.0 | 50/50 |
  | OpenCode 2.0.5 | 44/44 (6 ADAPTED) |
  | Antigravity 1.2.4 | 48/48 |

- **Harness versions:** each is newer than the version the Lab recorded, which ADR-035 reports as an explicit event rather than a failure.

## 2026-09-17 — Audit: shells, channels, panic restore (no change needed)

- **Shell invocations:** `run_shell_bounded` runs only the project's own `setup`/`gate` steps, verbatim from `agents.yaml`. No agent-, branch- or task-derived value is interpolated into a shell. Git calls separate refs with `--`. The installers' `sh -c` strings are constants.
- **Unbounded channels left in production:**
  - `damage` carries pane ids to a broadcaster that no longer waits on anything.
  - The UI's socket-reader channel stops with the whole process when the TUI is suspended, which fills the socket; the server's outbox then bounds that client.
  - The terminal's reply channel carries answers to a program's own queries.
  None can grow without a thread that is already stuck, so they stay unbounded.
- **Panic restore:** the TUI's hook restores the terminal before the previous hook prints, including for background threads.

## 2026-09-17 — Frame cost measured: no work warranted

- **Measurement:** a whole workspace frame rendered through `TestBackend`, in release mode. Averaged over 300 frames after a 20-frame warm-up.
  - Workload: 10 spaces (worktree and workspace kinds), 80 agent tabs, 80 panes at 200×60, and a 260×70 screen.
  - Result: **0.73 ms per frame**, about 22× under a 16 ms budget. Frames are drawn only when the model is dirty.
- **Decision:** no rendering optimization. The benchmark was a local probe and is not committed, since it asserts nothing.

## 2026-09-17 — Appearance no longer freezes; naming journey runs on macOS

- **Bug (UI freeze):** the Appearance list opens with a heading. Moving up from the first choice clamped the index back onto that heading at every step and never left the loop.
  - Found while consolidating the four selection movers.
  - `moving_up_from_the_first_appearance_choice_stays_put` runs the move on a thread with a timeout. It fails without the fix.
  - The walk now uses checked arithmetic bounded by the list, and the movers share `step_within`.
- **CI:** macOS `E2E - UZE` failed on `03-naming-the-work`, the only red job on `8be9c88`. The journey read the agent's stamp from `/proc`, which macOS lacks; it now uses `ps eww`, verified on the journey image.
  - Gate: 1 903/0.
  - Journeys: 23/23.

## 2026-09-17 — `clippy::pedantic` evaluated; only defect-finding lints acted on

- **Survey:** `-W clippy::pedantic` reports 1 220 warnings. The bulk are stylistic:
  - `must_use_candidate` 106
  - `missing_errors_doc` 59
  - `doc_markdown` 19
  - and similar.
  Enabling pedantic workspace-wide would add noise without finding defects, so it stays off.
- **Acted on:** `match_wildcard_for_single_variants` (6 sites).
  - In the four integrations' `exposure_plan`, a `_` arm over `CapabilityKind` meant a new capability kind compiled straight into "unsupported" for every harness. Naming `Instruction` makes adding a kind a compile error in each integration, which is where the decision belongs.
  - Same in `doctor.rs` and `input.rs`.
- **Inspected, not changed:**
  - 91 `usize→u16` casts are widths bounded by the terminal.
  - The `clamp(0, len - 1)` selection movers, which would panic on an empty list, are each guarded. Their duplication is logged as BACKLOG #9.

## 2026-09-17 — Landing tests use the shared origin fixture

- **Change:** the three bare-origin setups in `landing.rs` tests now use `Repository::with_origin`/`clone_origin` (−45 lines).
- **Why:** these were the last of eight hand-rolled copies of one fixture.
- **Proof:** tests only, and the 28 landing tests are unchanged.

## 2026-09-17 — A stopped pane takes its process group with it

- **Bug (orchestration):**
  - Stopping a pane signalled only the program. Workers it started that ignore SIGHUP survived as orphans.
  - Those orphans also held the PTY slave open, so the pane's reader thread never saw EOF and leaked too.
  - On shutdown, reapers were not awaited before the process exited.
- **Change:**
  - After the leader is handled, the reaper SIGKILLs the pane's process group.
  - `own_process_group` guards the signal: the group must be the pid itself, `> 1`, and not UZE's own group.
  - `stop_panes` starts every reaper and joins them all.
- **Rejected:**
  - A process tree walk via `/proc`: platform-bound and racy against forks.
  - Leaving it to the TTY hangup: it never reaches a worker that ignores SIGHUP, and the orphan keeps the master from closing.
- **Proof:** `a_stopped_pane_takes_its_process_group_with_it` fails without the group signal, leaving an orphaned `sleep`. It observes the worker's death as its FIFO hanging up, with no polling. Gate 1 902/0.
- **Risk:** a job an interactive shell moved into a group of its own is outside the pane's group. It still receives the terminal hangup.

## 2026-09-17 — A client that stops reading is bounded

- **Bug (robustness):** each attached client had an unbounded event queue. A client that stopped reading, such as a suspended `uze` or a stalled socket, made the server buffer every repaint of every pane for as long as it stayed attached.
- **Change:**
  - A per-client `Outbox` bounded at 256 events. Broadcasts `offer` without waiting; on overflow the client is marked stale and sent nothing more.
  - Once its queue has drained, the broadcaster resyncs it with a `Snapshot` and whole-pane repaints built from `snapshot()`, which leaves the shared damage baseline untouched.
  - Direct answers to a client's own request `reply`, which may wait, but only on that client's own thread.
- **Rejected:**
  - Disconnecting a lagging client: a suspended TUI would be thrown out.
  - A per-client damage baseline: that rewrites the shared diff model for the same outcome.
- **Proof:**
  - `a_client_that_stops_reading_is_bounded_and_resynchronized` checks three things: pending ≤ capacity after 16 384 broadcasts, the client is marked stale, and it gets a `Snapshot` after draining.
  - Full test suite 1 901/0; gate journeys 23/23.
- **Risk:** errors broadcast to a stale client are dropped; the resync does not carry them.

## 2026-09-17 — Every production `unsafe` states its contract

- **Change:**
  - `SAFETY:` comments on `localtime_r`'s zeroed `tm` and on `retire`'s `kill`, which relies on `signalable` refusing 0 and negative pids;
  - one `current_uid()` helper for the three `getuid` calls;
  - `Route::index`'s bare `unwrap` now states its invariant.
- **Not changed:** SIGPIPE in `main.rs` already carries its justification.

## 2026-09-17 — Stopped panes are reaped (`cd4b12d`)

- **Bug:** `portable-pty`'s `kill` is SIGHUP, a ≤200 ms poll, then an
  unawaited SIGKILL. A harness deaf to SIGHUP stayed `<defunct>` under the
  server; the request that closed the tab waited out the grace period.
- **Change:** `PaneRuntime::stop` kills and waits on its own thread.
- **Rejected:** a `SIGCHLD` reaper calling `waitpid(-1)`, which would steal
  exit statuses from Git and harness subprocesses waited on elsewhere.
  Waiting inline, which ties a request thread to a process in uninterruptible I/O.
- **Proof:** `a_stopped_pane_leaves_no_zombie` fails without the wait. It
  synchronizes on the program's own output before signalling; an earlier
  version passed without the fix because SIGHUP beat the trap.
- **Risk:** the leader is reaped, but its children outside the foreground
  group are not signalled (BACKLOG #2).

## 2026-09-17 — Panes flushed on registration (`c64d5a7`)

- **Bug:** the PTY reader starts before `spawn_pane` registers the runtime,
  and damage for an unregistered pane is dropped. A program that printed
  once and then waited on input was never drawn ("starting shell…" for
  good). The gate journeys hit it intermittently on every launch path.
- **Change:** registration enqueues one notification. The first damage for
  a pane is a full repaint.
- **Rejected:** registering before the reader starts, which needs the
  runtime split in two for one line's benefit. Retrying unknown panes in
  the broadcaster, which keeps state for panes that may never exist.
- **Proof:** `a_spawned_pane_is_flushed_once_it_is_registered` fails
  without the send. Full gate journeys: 22/23 before, 23/23 after.

## 2026-09-17 — Tenants end when their tab closes (`0021bbd`)

- **Bug:** occupancy reconciliation ran only on the first sweep or when a
  slot lost its last pane. A tenant holds no slot, so it stayed live.
- **Change:** the client also reconciles the space roots when an agent it
  echoed loses its last tab.
- **Journeys:** fixed four journeys that had drifted from the design they
  prove. That brings CI's `E2E - UZE` from 4 failing journeys (before this
  refactor) to 0.
  - a regex-escaped click target the runner matches literally;
  - a flat tenant row sought by a glyph the selected row omits by spec;
  - naming run from a shell with no launch stamp;
  - ledger fields the core no longer writes.

## 2026-09-16 — Codebase-wide simplification (merges `94654e4`…`ac7c580`)

Nine area reviews against the settled design (AGENTS.md, ADRs, openspec),
then seven isolated implementations merged here. 112 commits, 216 files,
+13 086 / −18 201.

- **Deleted, dead in production:**
  - project resource composition;
  - the importer/bundle pipeline;
  - verification and representation types;
  - per-session exposure;
  - `plugin_marketplaces.json`;
  - `Layout::Split`/`Focus`;
  - the tenant read model;
  - the pid-file protocol version;
  - the bootstrap attach loop, which never ran.
- **Consolidated:**
  - the Claude/Codex generated marketplace, as one dialect;
  - hook, MCP and skill plans across four harnesses;
  - the slot-owner rule (4 copies) and the slot path (5);
  - the locked-not-installed computation (4);
  - management drawers (5), confirmations (9) and list screens (4).
- **Types carrying invariants:**
  - `Launch { Shell, Program }`;
  - `SpaceSeat`;
  - `HookTarget`;
  - `ExposureMechanism::Managed(ManagedArtifact)`.
- **Bugs fixed:**
  - resuming a task opened an unstamped tab;
  - notices typed into a shell by directory match;
  - Codex provisioned through UZE's own shim;
  - Codex coverage ignored invocation policy;
  - Claude's shim skipped sibling files;
  - the diff interleaved replacements;
  - restore misaligned tabs after an empty space;
  - a server of another build or of a deleted `UZE_HOME` was attached to.
- **Behaviour changes, pre-1.0 and accepted:**
  - doctor/inspect JSON shape;
  - a harness before setup reports Unsupported;
  - a stale `workspace.json` is discarded;
  - probes time out.

  Details in `BREAKING_CHANGES.md`.
