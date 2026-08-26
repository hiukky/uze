## 1. Core

- [x] 1.1 `UzeHome::inspection_cache_path` (cache_dir/inspection.json)
- [x] 1.2 `integration::managed_artifact_fingerprint` (SymlinkReference → link state + target; others None)
- [x] 1.3 `integration::managed_artifact_present` (SymlinkReference in-place check; others false)

## 2. Application

- [x] 2.1 `InspectionCache` (memo + on-disk, TTL 24h, Matched-only, invalidate) with unit tests
- [x] 2.2 `reconcile_cached_report` used by `doctor()` and plugin managed state; removal paths keep live `reconcile_package`
- [x] 2.3 Invalidate on `install_materialized` and `remove_plugin`
- [x] 2.4 Bootstrap guard: `ensure_default_plugins` skips effective re-attach, heals vanished stat-able artifacts

## 3. TUI (un-masking)

- [x] 3.1 Every refresh loads the full `doctor()`; shallow split, `RefreshDoctor`, deep-health prompt and related plumbing removed
- [x] 3.2 Plugins screen renders real attachment health after a refresh (no "unknown")

## 4. Tests

- [x] 4.1 Inspection cache: round-trip both tiers, fingerprint change invalidates, removed symlink detected, anomaly never stored, TTL expiry, invalidate, fail-open
- [x] 4.2 Application: matched cached across instances, anomalies re-inspected every time, installation invalidates
- [x] 4.3 TUI: entering Doctor needs no separate reload; attachment health rendered with real states
- [x] 4.4 Lifecycle regression (`tests/exposure_naming.rs`): manual symlink removal still surfaces as missing; setup re-heals

## 5. Validation

- [x] 5.1 `cargo fmt --check`
- [x] 5.2 `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [x] 5.3 `cargo test --workspace --no-fail-fast`
- [x] 5.4 `openspec validate --all --strict`
- [x] 5.5 `git diff --check`
