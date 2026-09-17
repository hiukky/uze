# Refactor log

Newest first. Each entry: what changed, why, what was rejected, numbers,
remaining risk.

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
