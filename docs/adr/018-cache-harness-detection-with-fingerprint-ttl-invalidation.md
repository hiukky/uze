# Cache harness detection with fingerprint + TTL invalidation

Status: Accepted

## Context

`src/main.rs`'s `run()` calls `app.ensure_default_plugins()`
unconditionally before dispatching any subcommand, and `src/ui/
worker.rs` does the same on every TUI startup — meaning nearly every
command, not a hand-picked few, pays this cost. `ensure_default_plugins()`
calls `prepare_detected_integrations()`, which calls `IntegrationPort::
detect()` per harness (twice per integration in that one function alone),
which shells out to `<harness> --version` synchronously, with no caching
at any level. Measured: `uze status` 11.87s, `uze doctor` 11.34s,
`uze list` 8.41s, `uze marketplace list` 8.52s, `uze plugin list` 9.08s,
`uze inspect <id>` 9.1-9.7s — all local reads with no network dependency
of their own. A single `gemini --version` call alone costs 2-11s across
runs (a mise/node-installed binary UZE does not control the startup cost
of). `context.rs` separately calls `.detect()` up to three more times per
integration inside one `context inspect` run, on top of the shared
bootstrap's own two calls.

UZE's stated value is a fast, local Rust layer; ordinary read-only
commands paying multi-second, repeated external-process costs for no
correctness reason is a direct contradiction of that, and there was no
mechanism, and no test, guarding against it. A decision was needed on
*how* to cache this — and specifically how to invalidate it, since a
harness detection cache that goes stale silently (reporting a harness as
absent after install, or reporting an old version after an update) is a
correctness regression, not just a performance one.

## Decision

We will cache harness detection (`HarnessDetection`) results in two tiers:
an in-process memoization for the lifetime of one command (collapsing
redundant same-run calls to a single live probe), and an on-disk JSON
cache under `UzeHome::cache_dir()` that persists across separate CLI
invocations.

Cache entries are invalidated by a fingerprint of the resolved executable
path plus its file mtime, checked via a cheap `stat()` on every read — no
subprocess spawn required to validate freshness. This is backed by a
bounded 24h TTL as a safety net for cases the fingerprint cannot observe
(e.g. an npm-based install, such as `gemini`'s, preserving a packaged
mtime instead of stamping install time). Consistency requires no operator
action: whenever UZE itself changes a harness's installed state
(`provision()` succeeding), it writes the fresh detection result it
already obtained straight into the cache as part of that action. No
manual refresh flag is exposed anywhere — a design alternative
deliberately rejected in favor of keeping correctness fully automatic.

We considered TTL-only invalidation (rejected: a harness updated minutes
after a cache write would silently report the old version until the TTL
lapses — an explicit correctness requirement this change rules out) and
fingerprint-only invalidation (rejected: fingerprinting depends on `PATH`
resolution and mtime semantics holding for every installer, which UZE
does not control, so an unbounded worst case is unacceptable). We also
considered caching on-disk only, without in-process memoization
(rejected: it does not fix the structural bug of independent call sites
each redundantly invoking `.detect()` within the same run) and
in-process-only, without persistence (rejected: this does not address the
actual complaint, since every fresh CLI invocation would still pay one
full live probe). We also considered adding an explicit `--refresh` flag
to `status`/`list`/`doctor` (rejected: redundant given write-through on
UZE-driven changes plus fingerprint checks on out-of-band changes, and it
would reintroduce the exact failure mode this change removes — an
operator needing to know a cache exists to get correct behavior).

A missing, unreadable, or corrupted cache file is treated as fail-open —
an empty cache, not an error — consistent with UZE's existing fail-open
convention for detection-adjacent behavior (`HarnessRuntimeContribution`).

## Consequences

- Ordinary CLI/TUI operations that only need harness presence/version
  become fast (cache-hit path: a filesystem stat per integration, no
  subprocess spawn), directly addressing the measured multi-second
  latencies.
- Every future consumer of `detect()` inherits this caching and
  invalidation contract automatically, rather than each call site needing
  its own caching decision — but it also means any future change to how
  detection results should be considered "fresh" has to go through this
  cache's fingerprint/TTL model rather than a bespoke mechanism.
- Freshness has a bounded worst case (up to 24h under the TTL safety net,
  only for out-of-band changes the fingerprint fails to catch) rather
  than a guarantee of always-live data, and there is deliberately no
  manual override — if that bound proves too coarse, the fix is
  tightening the TTL or fingerprint, not adding an operator-facing flag.
- Introduces a new persistent, reconstructable state file
  (`cache/harness_detection.json`) that must be handled defensively
  (atomic writes, fail-open reads) since it is now read on nearly every
  command invocation — a bug in cache read/write becomes a much
  higher-blast-radius class of bug than before, mitigated by the
  fail-open design rather than eliminated.
- Establishes the precedent, and the concrete mechanism, for the broader
  project performance principle (`specs/cli-performance/spec.md`) that
  expensive local probes must be cached rather than re-paid per
  invocation — future probes with the same shape can reuse this pattern
  instead of inventing a new one.

Source change: openspec/changes/cache-harness-detection-for-fast-cli/
