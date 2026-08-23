# 024 — Cache attachment inspection on read paths with fingerprint + TTL invalidation

- **Status** — Accepted
- **Context**

  Attachment health (per-receipt `IntegrationPort::inspect_receipt`) is
  the one expensive half of `doctor`: several integrations verify their
  deliveries by running vendor CLIs (`codex plugin marketplace list`
  + `codex plugin list`, `claude plugin …`, `gemini plugin …`), each a
  subprocess of a slow install-managed binary. With N packages and four
  integrations that is seconds per run, and `doctor` was the read model
  behind the TUI's dashboard, Plugins, and Doctor screens — so every
  refresh, every screen switch, paid it.

  ADR 018 had already solved the *other* expensive half (harness
  detection) with a two-tier on-disk cache. A first attempt to keep the
  TUI fast by splitting `doctor` into a shallow "dashboard" report
  (no inspections) and deferring the full one to the Doctor screen was
  rejected as masking: the Plugins screen showed attachment health as
  "unknown", and the delay just moved around. The fix must make the
  *inspection itself* cheap in steady state — inside the cache engine,
  not around it.

- **Decision**

  Per-receipt inspection verdicts are cached for read paths, mirroring
  the detection cache's contract (ADR 018):

  1. **Two tiers**: in-process memoization per `UzeApplication`
     invocation plus an on-disk JSON file under `UzeHome::cache_dir()`
     (`inspection.json`).
  2. **Fingerprint + TTL invalidation**: receipts whose artifact has a
     cheaply stat-able presence (`ManagedArtifact::SymlinkReference`)
     carry a fingerprint (link state + target). It is re-checked on
     every read — two `stat` calls, no subprocess — so a hand-removed or
     re-pointed skill link is detected immediately, even within one
     invocation. Verdicts for artifacts whose state lives inside vendor
     files (`IntegrationOwned` catalogues, vendor config entries) are
     bounded by a 24h TTL plus mutation invalidation.
  3. **Only `Matched` verdicts are cached**: anomalies
     (Missing/Drifted/Conflict/Blocked) are always re-inspected live, so
     a warning is never stale.
  4. **Mutations invalidate**: every Store/vendor-state mutation clears
     both tiers, so a verdict never outlives the change that produced
     the state it describes.
  5. **Read-path-only scope**: removal planning and detach keep using
     the live core `reconcile_package`. A stale `Matched` must never
     authorize destroying a vendor artifact that drifted.
  6. **Steady-state bootstrap skip**: `ensure_default_plugins` no longer
     re-runs the vendor-CLI attach pass when the integration already
     holds the package's receipts and stat-able artifacts are still in
     place. A vanished skill link is still re-attached cheaply; a
     vendor-native loss surfaces as a read-time anomaly and is healed by
     the explicit setup path.
  7. **Fail-open, best-effort**: unreadable/corrupt cache file or a
     failed write never fails a command — the cache is a reconstructable
     optimization.

- **Consequences**

  - Steady-state `doctor`: ~5ms (was ~6s on this machine with two
    packages and four integrations); cold cache costs one vendor-CLI
    probe per receipt, once per TTL window or after a mutation — the
    honest price of first evidence.
  - Every screen shows real attachment health on every refresh; the
    "unknown"/deferred-load masking is gone (no shallow doctor).
  - A 24h window exists for vendor-side changes whose fingerprints we
    cannot compute (native catalogue state) — bounded, documented, and
    equivalent to the detection cache's own accepted tradeoff; anomalies
    seen *by UZE* are never stale.
  - Setup/install/update paths keep their healing ability (they attach
    unconditionally); only the every-invocation bootstrap re-attach is
    skipped.
