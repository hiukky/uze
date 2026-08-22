## 1. Detection cache foundations

- [x] 1.1 Add `DetectionCache` to `crates/uze-core` (in-process
      `RefCell<HashMap<&'static str, HarnessDetection>>` keyed by
      integration id) — `crates/uze-core/src/detection_cache.rs`.
- [x] 1.2 Implement fingerprint resolution (`Fingerprint::resolve`: PATH
      lookup + mtime stat, no subprocess) — same file.
- [x] 1.3 Implement on-disk persistence: JSON at
      `UzeHome::harness_detection_cache_path()`
      (`cache/harness_detection.json`), atomic write via the existing
      `persistence::write_atomic`, fail-open on missing/corrupt read.
- [x] 1.4 Implement the read path (`DetectionCache::get`): in-process hit
      => return; else on-disk hit with valid fingerprint and within TTL
      (24h) => return and populate in-process; else `None` (live probe is
      the caller's job, via `detect_cached` in section 3).
- [x] 1.5 Implement the write-through path (`DetectionCache::put`).

## 2. Make detection fakeable for tests

- [x] 2.1-2.3 REVISED during implementation (see design.md decision 4):
      threading `&dyn ProcessRunner` into `detect()` was reverted after
      starting it — `detect()`'s trait signature has no runner parameter
      and is called from `install()` and read paths that don't have one
      in scope the way `provision()` does; the ripple cost outweighed the
      benefit for what this change actually needs. Instead: a test-only
      `IntegrationPort` fake (sleeps + counts calls in `detect()`) is the
      seam `detect_cached` is tested through — sufficient because the
      cache's correctness doesn't depend on *how* `detect()` gets its
      answer. Add `FakeIntegration` (or similarly named) test support in
      `crates/uze-application/src/application.rs`'s test module: id,
      configurable `HarnessDetection`, configurable `detect()` delay, and
      an `AtomicUsize` call counter. The real `detect_binary`
      implementations are untouched.

## 3. Wire the cache into all call sites

- [x] 3.1 Added `UzeApplication::detect_cached(&self, integration: &dyn
      IntegrationPort) -> HarnessDetection` (`application.rs`), backed by
      `DetectionCache`.
- [x] 3.2 Replaced direct `integration.detect()` calls with
      `self.detect_cached(integration.as_ref())` in `doctor.rs:43`,
      `context.rs` (all three sites), `application.rs`'s
      `ensure_default_plugins` and `prepare_detected_integrations`
      (collapsed to one shared result), and `lifecycle/install.rs:87`.
- [x] 3.3 Audited: no other reachable `.detect()` call sites needed
      routing beyond what 3.2 and 3.4a below cover; provisioning's own
      pre/post-install verification (`provision()` implementations)
      remains a live probe deliberately, since provisioning is the
      justified-slow case.
- [x] 3.4 Write-through lands in `UzeApplication::provision_and_prepare`
      (`application.rs`, right after `integration.install(...)` inside
      the existing `integration.provision(...)` success branch) — one
      shared call site, not one per integration.
- [x] 3.4a **Found during end-to-end timing, not in the original plan**:
      `IntegrationPort::install()` (every integration) called
      `self.detect()` directly and uncached, purely to get a version
      string to record — a second live probe stacked on top of
      `detect_cached`'s result computed one line earlier by the caller.
      This was the actual dominant remaining cost after 3.1-3.4 (real
      commands still took ~2s with 100% cache hits logged). Fixed by
      changing `IntegrationPort::install`'s signature to
      `fn install(&self, home: &UzeHome, detection: &HarnessDetection)`
      so callers (`provision_and_prepare`, `prepare_detected_
      integrations`) pass their already-known result instead of `install`
      re-deriving it. See design.md decision 7 for the full account —
      this is the concrete case for decision 6 (classification +
      exhaustiveness test): grep-driven call-site auditing missed this;
      end-to-end measurement caught it.

## 4. Tests

- [x] 4.1 `detection_cache::tests::put_then_get_within_one_instance_hits_in_process_tier`
      (in-process tier) + `application::tests::detect_cached_calls_detect_at_most_once_per_command`
      (application-layer, `FakeIntegration`).
- [x] 4.2 `detection_cache::tests::a_fresh_instance_reuses_the_on_disk_entry`
      + `application::tests::detect_cached_reuses_the_on_disk_result_across_separate_uze_application_instances`.
- [x] 4.3 `detection_cache::tests::fingerprint_change_invalidates_the_entry`
      and `..._expired_ttl_invalidates_the_entry_even_with_a_matching_fingerprint`.
- [x] 4.4 `application::tests::provision_and_prepare_writes_through_the_cache_on_success`.
- [x] 4.5 `detection_cache::tests::missing_cache_file_is_fail_open` and
      `corrupted_cache_file_is_fail_open`.
- [x] 4.6 `application::tests::cache_warm_detect_cached_meets_the_performance_budget`:
      `FakeIntegration` with a 500ms simulated delay, asserting both the
      in-process-warm and cross-invocation-warm reads complete under
      50ms, and the delay is paid exactly once across cold + both warm
      reads. Scoped to `detect_cached` itself (the mechanism every
      budgeted command routes through) rather than duplicating the same
      assertion once per CLI command — section 5's classification
      registry is what ties each real command to this guarantee.
- [x] 4.7 `application::tests::prepare_detected_integrations_probes_each_integration_at_most_once`
      — this is the test that caught decision 7's bug (`install()`'s own
      uncached `detect()`) when it was written against the pre-fix code.
- [x] 4.8 `cargo fmt --check`: clean. `cargo test --workspace
      --no-fail-fast`: every test binary green (0 failed). `cargo clippy
      -p uze-core -p uze-integrations -p uze-application --all-targets -D
      warnings`: clean. `cargo clippy --all-targets -D warnings` for the
      full workspace fails only on 3 pre-existing dead-code warnings in
      `src/progress.rs` (an untracked file with no git history, present
      before this change started, unrelated to CLI performance — not
      touched by this change). Also fixed two integration test doubles
      (`tests/exposure_naming.rs`, `tests/shared_agent_skill_root_naming.rs`)
      whose `IntegrationPort::install` overrides needed the new
      3-parameter signature from decision 7.

## 5. Command performance classification registry (never again)

- [x] 5.1 Added `PerformanceClass` (`Budgeted`/`JustifiedSlow(&'static
      str)`) and `CLASSIFICATION: &[(&str, PerformanceClass)]` in the new
      `src/command_performance.rs` (test-only, `#[cfg(test)] mod
      command_performance;` in `main.rs` — see 5.5).
- [x] 5.2 Classified all 19 current leaf commands: `Budgeted` — `list`,
      `inspect`, `remove`, `status`, `doctor`, `context inspect`,
      `context plan`, `context reconcile`, `marketplace list`,
      `marketplace remove`, `plugin list`, `plugin remove`.
      `JustifiedSlow("<reason>")` — `add`, `update`, `install`, `setup`,
      `marketplace add`, `plugin install`, `plugin update`.
- [x] 5.3 `command_performance::tests::every_cli_command_is_classified`
      walks `Cli::command()`'s clap-generated tree via a recursive
      `leaf_command_paths` helper (excludes clap's implicit `help`
      subcommand), asserting no unclassified and no stale entries.
- [x] 5.4 `command_performance::tests::every_budgeted_command_has_a_named_performance_test`,
      cross-checked against a `BUDGETED_COMMAND_TESTS` list (all pointing
      at `cache_warm_detect_cached_meets_the_performance_budget`, since
      every `Budgeted` command routes through the one `detect_cached`
      mechanism that test exercises). Also added
      `every_justified_slow_command_states_a_reason`.
- [x] 5.5 `command_performance` is declared `#[cfg(test)]` in `main.rs` —
      compiled and run only for the test target, never linked into the
      real binary or consulted by any command handler; confirmed via
      `cargo build --bin uze` producing no dead-code warnings for it.

## 6. Documentation

- [x] 6.1 `docs/adr/018-cache-harness-detection-with-fingerprint-ttl-
      invalidation.md` exists, indexed in `docs/adr/README.md`.
- [x] 6.2 `IntegrationPort::detect()`'s doc comment
      (`crates/uze-core/src/integration.rs`) now points callers at
      `UzeApplication::detect_cached`.
- [x] 6.3 Added a note to `AGENTS.md` (Workspace conventions).
