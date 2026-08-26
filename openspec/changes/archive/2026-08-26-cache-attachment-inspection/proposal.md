## Why

`doctor()` — the read model behind the TUI dashboard, the Plugins
screen, and the Doctor screen — spent seconds per invocation on
per-receipt attachment inspection: several integrations verify their
deliveries by running vendor CLIs (`codex plugin list`, `claude plugin
list`, …), each a subprocess of a slow install-managed binary. With two
packages and four integrations the measured cost was ~6s per run, paid
by every refresh and every screen.

ADR 018 had already solved the other expensive half (harness detection)
with a two-tier on-disk cache. A first attempt to keep the TUI fast by
splitting `doctor` into a shallow dashboard report and deferring the
full one to the Doctor screen was rejected as masking: the Plugins
screen showed attachment health as "unknown", and the delay just moved
around.

This change fixes the inspection itself — inside the cache engine, not
around it — and removes the masking.

## What Changes

- Application gains an `InspectionCache` (two tiers, on-disk under
  `~/.uze/cache/inspection.json`) serving `Matched` verdicts on read
  paths: `doctor()` and `PluginInspection` managed state.
- Invalidation: artifact fingerprint (symlink state + target, re-checked
  on every read — a hand-removed link is detected with two stats, no
  vendor CLI) + 24h TTL + clear-on-mutation (install/remove/update).
- Anomalies (Missing/Drifted/Conflict/Blocked) are never cached — always
  re-inspected live, so warnings are never stale.
- Read-path-only scope: removal planning and detach keep the live core
  `reconcile_package`.
- Steady-state bootstrap: `ensure_default_plugins` skips the vendor-CLI
  re-attach when the integration already holds the package's effective
  receipts; vanished stat-able artifacts are still re-attached cheaply.
- TUI masking removed: every refresh carries the full `doctor` (cache
  makes it ~5ms warm); no shallow report, no deferred deep reload, no
  "unknown" placeholder.

## Impact

`crates/uze-core` (artifact fingerprint/presence helpers, cache path),
`crates/uze-application` (inspection cache + read paths + bootstrap
guard), TUI worker/model/views (un-masking). No Store or vendor
semantics change. ADR 024 documents the mechanism.
