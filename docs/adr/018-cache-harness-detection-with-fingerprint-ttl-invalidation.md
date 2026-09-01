# Cache expensive read paths with fingerprint + TTL invalidation

Status: Accepted
Consolidates: ADR-024 (cache attachment inspection on read paths) — see the
"Consolidated records" section of `README.md`.

## Context

Two independent probes dominated the cost of nearly every UZE command, and
both were paid live, every time.

**Harness detection.** `ensure_default_plugins()` runs before every CLI
dispatch and on every TUI startup, and it calls `IntegrationPort::detect()`
per harness — shelling out to `<harness> --version` synchronously, with no
caching at any level. Measured: `uze status` 11.87s, `uze doctor` 11.34s,
`uze list` 8.41s, `uze plugin list` 9.08s, `uze inspect <id>` 9.1–9.7s — all
local reads with no network dependency of their own. A single npm-installed
harness's `--version` call alone cost 2–11s across runs.

**Attachment inspection.** Per-receipt `IntegrationPort::inspect_receipt` is
the expensive half of `doctor`: several integrations verify their deliveries
by running vendor CLIs (`codex plugin marketplace list` + `codex plugin
list`, `claude plugin …`), each a subprocess of a slow install-managed
binary. With N packages and four integrations that is seconds per run — and
`doctor` is the read model behind the TUI's dashboard, Plugins, and Doctor
screens, so every refresh and every screen switch paid it.

UZE's stated value is a fast, local Rust layer; ordinary read-only commands
paying multi-second external-process costs for no correctness reason
contradicts that directly. The hard part is not caching but *invalidation*:
a cache that goes stale silently — reporting a harness absent after install,
an old version after an update, or an attachment healthy after it drifted —
is a correctness regression, not merely a performance one.

An earlier attempt to keep the TUI fast by splitting `doctor` into a shallow
"dashboard" report and deferring the full one to the Doctor screen was
rejected as masking: the Plugins screen showed attachment health as
"unknown" and the delay just moved around. The fix has to make the probe
itself cheap in steady state — inside the cache engine, not around it.

## Decision

Both probes share one caching contract, with on-disk state under
`UzeHome::cache_dir()` (`harness_detection.json`, `inspection.json`).

### 1. Two tiers

In-process memoization for the lifetime of one command — collapsing
redundant same-run calls to a single live probe — plus an on-disk JSON cache
that persists across separate CLI invocations. In-process-only was rejected
(every fresh invocation still pays a full live probe); on-disk-only was
rejected (it leaves the structural bug of independent call sites each
redundantly probing within one run).

### 2. Fingerprint first, TTL as the safety net

Anything cheaply stat-able is fingerprinted and re-checked on **every read**,
with no subprocess spawn:

- **Detection**: the resolved executable path plus its file mtime.
- **Inspection**: for receipts whose artifact has stat-able presence
  (`ManagedArtifact::SymlinkReference`), the link state plus its target —
  two `stat` calls, so a hand-removed or re-pointed skill link is detected
  immediately, even within one invocation.

A bounded **24h TTL** backs this for what a fingerprint cannot observe: an
npm-based install preserving a packaged mtime instead of stamping install
time, and verdicts for artifacts whose state lives inside vendor files
(`IntegrationOwned` catalogues, vendor config entries).

TTL-only was rejected — a harness updated minutes after a cache write would
silently report the old version until the TTL lapses. Fingerprint-only was
rejected — fingerprinting depends on `PATH` resolution and mtime semantics
holding for every installer, which UZE does not control, so an unbounded
worst case is unacceptable.

### 3. Only healthy verdicts are cached

Inspection anomalies (`Missing`/`Drifted`/`Conflict`/`Blocked`) are always
re-inspected live. A warning is never stale.

### 4. Mutations invalidate; UZE-driven changes write through

Every Store or vendor-state mutation clears both tiers, so a verdict never
outlives the change that produced the state it describes. Symmetrically,
whenever UZE itself changes a harness's installed state (a successful
`provision()`), it writes the fresh detection result it already obtained
straight into the cache as part of that action.

**No manual refresh flag is exposed anywhere.** An explicit `--refresh` on
`status`/`list`/`doctor` was considered and deliberately rejected: it is
redundant given write-through plus fingerprint checks, and it reintroduces
the exact failure mode being removed — an operator needing to know a cache
exists in order to get correct behavior.

### 5. Read-path-only scope

Removal planning and detach keep using the live core `reconcile_package`. A
stale `Matched` must never authorize destroying a vendor artifact that
drifted.

### 6. Steady-state bootstrap skip

`ensure_default_plugins` no longer re-runs the vendor-CLI attach pass when
the integration already holds the package's receipts and stat-able artifacts
are still in place. A vanished skill link is still re-attached cheaply; a
vendor-native loss surfaces as a read-time anomaly and is healed by the
explicit setup path. Setup, install, and update keep attaching
unconditionally — only the every-invocation bootstrap re-attach is skipped.

### 7. Fail-open

A missing, unreadable, or corrupt cache file is an empty cache, not an
error, and a failed write never fails a command. The cache is a
reconstructable optimization, consistent with UZE's existing fail-open
convention for detection-adjacent behavior.

## Consequences

- Steady-state `doctor` drops from ~6s to ~5ms (two packages, four
  integrations); ordinary CLI/TUI operations needing only harness
  presence/version become a filesystem stat per integration with no
  subprocess spawn. A cold cache costs one probe per receipt, once per TTL
  window or after a mutation — the honest price of first evidence.
- Every screen shows real attachment health on every refresh; the
  "unknown"/deferred-load masking is gone.
- Freshness has a bounded worst case (up to 24h, only for out-of-band
  changes no fingerprint can catch) rather than a guarantee of live data.
  Anomalies UZE itself observes are never stale. If that bound proves too
  coarse, the fix is tightening the TTL or the fingerprint, not adding an
  operator-facing flag.
- Every future consumer of `detect()` or `inspect_receipt` inherits this
  contract automatically rather than making its own caching decision — but
  any future change to what counts as "fresh" must go through this
  fingerprint/TTL model rather than a bespoke mechanism.
- Two new persistent, reconstructable state files are read on nearly every
  invocation, so a bug in cache read/write has a much larger blast radius
  than before — mitigated by atomic writes and fail-open reads rather than
  eliminated.
- Establishes the concrete mechanism behind the project's CLI performance
  principle (`specs/cli-performance/spec.md`): expensive local probes are
  cached, not re-paid per invocation. Future probes of the same shape reuse
  this pattern instead of inventing a new one.
