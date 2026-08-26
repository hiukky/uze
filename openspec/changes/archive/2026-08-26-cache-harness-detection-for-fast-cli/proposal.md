## Why

`src/main.rs`'s `run()` calls `app.ensure_default_plugins()`
**unconditionally, before dispatching to any subcommand** (`src/
main.rs:364`), and `src/ui/worker.rs:192` does the same on TUI startup —
already flagged in a code comment at the TUI call site as having "left the
terminal looking frozen for however long harness detection took."
`ensure_default_plugins()` calls `prepare_detected_integrations()`, which
loops over every registered integration and calls `IntegrationPort::
detect()` — **twice per integration** in that one function alone (once for
`configured`, once again to build `ProvisioningResult::verified(...,
integration.detect())`). `detect()` shells out to `<harness> --version`
synchronously, with no timeout and no caching anywhere. The practical
result: this cost is paid by nearly every command, not a specific subset —
measured on a real dev machine with all four harnesses installed:

| Command | Measured |
|---|---|
| `uze status` | 11.87s |
| `uze doctor` | 11.34s |
| `uze list` | 8.41s |
| `uze marketplace list` | 8.52s |
| `uze plugin list` | 9.08s |
| `uze inspect <id>` | 9.1–9.7s |

None of these six do anything that requires network access or an external
install — they are local reads. `gemini --version` alone (a mise/
node-installed binary UZE does not control the startup cost of) accounts
for most of it, ranging 2-11s across runs depending on OS-level cache
state; `claude`/`codex`/`opencode --version` each take <0.2s.
`crates/uze-application/src/application/context.rs` separately calls
`.detect()` on the same integration up to three more times inside one
`context inspect` run, on top of the shared bootstrap's own two calls per
integration. The result is that ordinary, read-only CLI/TUI operations —
effectively all of them, via this one shared bootstrap call site — are
dominated by the startup cost of external tools UZE does not control,
paid repeatedly and redundantly, for no correctness reason. This directly
contradicts UZE's value proposition as a fast, local Rust layer, and
there is currently no project-level guarantee — and no test — that CLI
operations stay fast.

Genuinely network-bound operations (`add`, `update`, `plugin install`,
`plugin update`, `setup`'s provisioning step, `install` reconstructing
from `agents.lock`) are explicitly out of scope for the performance
budget — they depend on official third-party installers UZE does not
control. But the detection overhead layered on top of those operations by
the same shared bootstrap call is not itself justified, and this change
removes it there too.

## What Changes

- Introduce a harness-detection cache: `IntegrationPort::detect()` results
  (`HarnessDetection { present, version }`) are cached per integration id
  and reused instead of re-invoking `<harness> --version` on every call.
- De-duplicate the redundant in-process `.detect()` calls — most
  importantly `prepare_detected_integrations()` (`application.rs:511-531`,
  which itself calls `.detect()` twice per integration and runs on
  *every* CLI invocation and TUI startup via `ensure_default_plugins()`
  at `main.rs:311,364` and `ui/worker.rs:192`), plus `context.rs` lines
  ~79, ~160, ~203, `doctor.rs:43`, `application.rs:192,202`, and
  `lifecycle/install.rs:87` — so each logical command performs at most one
  detection probe per integration, cached and shared across all call
  sites within that run. Because the shared bootstrap call site is
  reached by nearly every command, fixing it is what makes the fast path
  apply broadly rather than to a hand-picked subset of commands.
- Persist detection results across CLI invocations with a safe invalidation
  strategy (TTL and/or binary-identity based, e.g. resolved executable path
  + mtime) so a newly installed/updated/removed harness is picked up without
  requiring the cache to be manually cleared, while still avoiding a fresh
  subprocess spawn on every command.
- Keep the cache consistent automatically, with no manual flag required:
  whenever UZE itself changes a harness's installed state (install/update
  via `provision()`), it writes the fresh detection result straight into
  the cache as part of that operation. Out-of-band changes (the operator
  installs/updates/removes a harness outside UZE) are caught by the
  fingerprint check on the next read, with the TTL as a bounded backstop
  for the rare case the fingerprint can't observe the change.
- Establish a project-wide performance budget as an explicit, testable
  goal: CLI/TUI operations that are not inherently justified to be slow
  (network installs, plugin installation) must complete in under 50
  milliseconds, with zero manual action required to get that fast path.
  Ship tests that assert this budget for the shared bootstrap
  (`ensure_default_plugins`) plus `status`, `doctor`, `context inspect`,
  `marketplace list`, and `plugin list` using a fake slow harness/process
  runner, so a regression that reintroduces an uncached subprocess spawn
  on any of these paths fails CI.
- Record the caching/invalidation strategy as an ADR (`docs/adr/`) per this
  repo's convention that hard-to-reverse, cross-cutting mechanisms get a
  numbered decision record.

No user-facing command surface changes; this is a purely internal
performance and correctness-of-freshness change to existing command
behavior — no new flag, no manual step, no cache the operator ever has to
know exists.

## Before/After Latency Projection

**Update — measured after implementation, not just projected:** on the
same dev machine, every `Budgeted` command (`status`, `doctor`, `list`,
`inspect`, `marketplace list`, `plugin list`, `context inspect`) now
completes in **~50-58ms warm** (debug build, full process including
startup — not an isolated microbenchmark), down from the 8-12s measured
below. The dominant remaining cost after the `detect_cached` wiring
turned out not to be `detect()` at all, but `IntegrationPort::install()`
independently calling `self.detect()` a second, uncached time — found by
timing the real binary, not by re-reading the call sites (see design.md
decision 7). The projection below predated that fix and undersold the
`install()` cost; the final numbers are better than projected because
that second bug is gone too.

Originally measured on a real dev machine with all four harnesses installed
(`claude`, `codex`, `gemini`, `opencode`). "After" applies uniformly
because the dominant cost sits in the one shared bootstrap call
(`ensure_default_plugins`) that nearly every command and the TUI startup
path already goes through — this is not six separate fixes, it is one
shared fix plus de-duplicating a few extra per-command probes:

| Command                  | Before (every run, current) | After — first run post-upgrade (cold) | After — subsequent runs (warm cache) |
|---------------------------|-----------------------------|----------------------------------------|----------------------------------------|
| `uze status`               | 11.87s | unchanged (still one live probe per integration, now cached afterward) | ~single-digit ms |
| `uze doctor`                | 11.34s | unchanged | ~single-digit ms |
| `uze list`                  | 8.41s  | unchanged | ~single-digit ms |
| `uze marketplace list`      | 8.52s  | unchanged | ~single-digit ms |
| `uze plugin list`           | 9.08s  | unchanged | ~single-digit ms |
| `uze inspect <id>`          | 9.1-9.7s | unchanged | ~single-digit ms |
| `uze context inspect\|plan\|reconcile` | ~10.5s (measured directly — not the ~0.00s an earlier, cache-skewed measurement in this same investigation had assumed) | unchanged | ~single-digit ms |
| TUI startup                | multi-second freeze (see the existing code comment at `ui/worker.rs:192` acknowledging this) | unchanged | ~single-digit ms |
| after `uze setup gemini` (install/update) | n/a | n/a — cache is populated as part of the provisioning result itself, no extra probe | ~single-digit ms immediately after, no stale window |
| `uze add` / `update` / `plugin install` / `install` (network-bound portion) | dominated by the network operation itself | unchanged — explicitly out of scope, depends on the official installer | the network operation stays as-is; only the redundant detection overhead layered on top of it disappears |

The "warm" projection is grounded in an already-measured baseline from
this same machine: `uze --help` (the one path that returns before
`ensure_default_plugins` runs at all) completes in ~0.00s per
`/usr/bin/time`, i.e. process startup plus ordinary filesystem work is
already negligible once no subprocess is spawned. The warm path adds, per
command, N `stat()` calls (one per integration — 4 today) plus parsing a
small JSON file (a handful of entries); both are sub-millisecond
operations. The realistic warm-path budget is therefore low tens of
milliseconds at most, dominated by process startup rather than the cache
itself — roughly a **200-1000x reduction** versus the current 8-12s
range, and this projection is exactly what the performance-budget test
(tasks.md §4.6) turns into an enforced, measured assertion rather than a
claim.

Cold-start (the very first run after upgrading, or the first run for a
never-before-detected integration) is unchanged by this proposal — the
cache cannot make a probe that has never happened before free. It only
removes the *repeated* cost on every subsequent command, and the
*redundant* multiple-probes-per-run cost this same investigation found in
`prepare_detected_integrations` (two calls per integration) and
`context.rs` (three more).

## Capabilities

### New Capabilities
- `cli-performance`: establishes the project-wide guarantee that CLI/TUI
  operations without an explicit justification (e.g. plugin install)
  complete in under 50ms, with no manual action required, backed by a
  cached, automatically-invalidated harness-detection mechanism and tests
  that measure the budget. Applies universally through the shared
  `ensure_default_plugins` bootstrap nearly every command and the TUI
  startup path already go through, not to a hand-picked subset of
  commands.

### Modified Capabilities
<!-- No existing capability in openspec/specs/ currently specifies detection
     latency or caching behavior; openspec/specs/ has no baseline yet for
     harness detection (all prior harness-provisioning work is still
     in-flight, unarchived). Nothing to modify here. -->

## Impact

- **Code**: `crates/uze-core/src/integration.rs` (`IntegrationPort` trait,
  `HarnessDetection`), `crates/uze-integrations/src/{claude,codex,gemini,
  opencode}/provision.rs` (`detect_binary`), `crates/uze-application/src/
  application.rs` (`ensure_default_plugins`, `prepare_detected_
  integrations`), `crates/uze-application/src/application/{doctor.rs,
  context.rs, lifecycle/install.rs}`, `src/main.rs` (the shared
  `ensure_default_plugins()` call at line 364 that every dispatched
  subcommand goes through, and the `add`-specific one at line 311),
  `src/ui/worker.rs` (TUI startup, line 192).
- **New code**: a detection-cache module (in-process memoization +
  on-disk persistence under the existing UZE home directory), wired into
  the `IntegrationPort::detect()` call path.
- **Tests**: new performance-budget tests using a fake/slow `ProcessRunner`
  to assert cached commands stay under 50ms, plus cache-invalidation
  correctness tests (new install, removed binary, updated version, all
  triggered automatically with no manual action).
- **Docs**: new ADR under `docs/adr/` documenting the caching/invalidation
  decision; `docs/adr/README.md` index updated.
- **No breaking changes** to existing command output, exit codes, or CLI
  surface — no new flag is introduced.
