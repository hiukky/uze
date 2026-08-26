# 024 — Cache attachment inspection on read paths with fingerprint + TTL invalidation

- **Status** — Accepted
- **Context**

  Attachment health (per-receipt `IntegrationPort::inspect_receipt`) is
  the one expensive half of `doctor`: several integrations verify their
  deliveries by running vendor CLIs, each a subprocess of a slow
  install-managed binary. With N packages and four integrations that is
  seconds per run, and `doctor` was the read model behind the TUI's
  dashboard, Plugins, and Doctor screens. ADR 018 had already solved the
  other expensive half (harness detection) with a two-tier on-disk
  cache; a first attempt at keeping the TUI fast by splitting `doctor`
  into a shallow report was rejected as masking.

- **Decision**

  Per-receipt inspection verdicts are cached for read paths, mirroring
  the detection cache's contract (ADR 018): two tiers
  (in-process + on-disk under `UzeHome::cache_dir()`), fingerprint + TTL
  invalidation, fail-open reads, best-effort writes. Only `Matched`
  verdicts are stored — anomalies are always re-inspected live.
  Fingerprints exist for stat-able artifacts (`SymlinkReference`:
  link state + target, re-checked on every read); vendor-native
  catalogue verdicts are bounded by a 24h TTL plus clear-on-mutation
  (install/remove/update). Scope is read paths only: removal planning
  and detach keep the live core reconciliation. `ensure_default_plugins`
  skips the every-invocation vendor-CLI re-attach when the integration
  already holds effective receipts; vanished stat-able artifacts are
  still re-attached cheaply, and vendor-native losses surface as
  read-time anomalies healed by the explicit setup path.

- **Consequences**

  Steady-state `doctor` is ~5ms (was ~6s with two packages and four
  integrations); a cold cache costs one vendor-CLI probe per receipt,
  once per TTL window or after a mutation. A bounded 24h window exists
  for vendor-native verdicts whose state cannot be fingerprinted —
  equivalent to what the detection cache already accepts, and anomalies
  seen by UZE are never stale. Setup/install/update keep their healing
  ability; only the every-invocation bootstrap re-attach is skipped.
