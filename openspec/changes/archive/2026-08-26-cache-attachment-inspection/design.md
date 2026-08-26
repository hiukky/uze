## Context

Attachment inspection is read evidence, not a mutation — but it costs
seconds because it shells out to vendor CLIs per receipt. The previous
attempt at a quick TUI (shallow dashboard report + deferred deep load)
was masking: it left other surfaces ("unknown" plugin health) worse and
moved the delay instead of removing it. The project already has the
pattern for this exact problem: ADR 018's detection cache
(in-process + on-disk, fingerprint + TTL, fail-open).

## Decisions

### D1. Cache verdicts at the receipt level, on read paths only

`reconcile_cached_report` (used by `doctor()` and the plugin drawer's
managed state) consults `InspectionCache` keyed by ledger key; core
`reconcile_package` stays live for removal planning and detach — a stale
`Matched` must never authorize destroying a drifted vendor artifact.

### D2. Matched-only caching; anomalies always live

Only `Matched` verdicts are stored. Any anomaly is re-inspected on every
read, so the first glance at a problem is never stale and Doctor's
warnings always reflect the current vendor state.

### D3. Fingerprint (stat-able artifacts) + TTL (vendor-native) +
mutation invalidation

- `ManagedArtifact::SymlinkReference` → fingerprint of the link
  (existence/state + current target), re-checked on every read —
  including memo hits, since a fingerprint check is two stats, not a
  probe. This is what makes a hand-removed skill link visible
  immediately (regression test in `tests/exposure_naming.rs` depends on
  it).
- `IntegrationOwned`/`VendorConfigEntry` → no cheap fingerprint; bounded
  by the 24h TTL (same as the detection cache) plus invalidation on
  every mutation (`install_materialized`, `remove_plugin`).

### D4. Bootstrap attach becomes steady-state-skip

`ensure_default_plugins` runs on every CLI/TUI start. Its attach loop
previously re-ran `codex/claude plugin add` unconditionally (~3s). It
now skips when the integration already holds the package's receipts and
stat-able artifacts are still in place; a vanished link is re-attached
cheaply; a vendor-native loss surfaces as a read-time anomaly, healed by
the explicit setup path (setup/install/update attach unconditionally and
keep their healing ability).

### D5. No masking

The TUI no longer splits doctor into shallow/deep: every refresh loads
the full report (warm: ~5ms; cold: one probe per receipt, once per TTL
window — the honest price of evidence, shown as the normal refresh
spinner).

## Consequences

- Steady-state: `doctor` ~5ms, every screen real. Cold cache: seconds,
  once per 24h or after a mutation.
- A bounded (24h) staleness window exists for vendor-native verdicts
  whose state cannot be fingerprinted — documented in ADR 024.
- The `doctor_fast`/`RefreshDoctor` machinery is deleted rather than
  kept as dead code.
