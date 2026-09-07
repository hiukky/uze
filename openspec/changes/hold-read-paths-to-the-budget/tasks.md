## 1. Marketplace catalogues

- [x] 1.1 `UzeHome::marketplace_cache_dir` (`cache/marketplaces`).
- [x] 1.2 `application::marketplace_catalogue::MarketplaceCatalogues`: read
      (local in place, Git from cache within TTL, refill by `acquire`,
      expired fallback on a failed refill), `store_from`, `invalidate`,
      in-process memo; unit tests for each.
- [x] 1.3 `marketplace().list()`, `plugins()`, `inspect_plugin()` read the
      catalogue; `add` stores the clone it made; `remove` invalidates.

## 2. Costs that grew around the read paths

- [x] 2.1 `bootstrap::entries` and `has_update` read the embedded snapshot
      in memory; `contained_relative_path` holds the manifest's `source`
      to the snapshot.
- [x] 2.2 `state::record` writes only a changed record;
      `ensure_default_plugins` republishes only `Unpublished` views; Claude
      and Codex `publication` notice a missing generated envelope.
- [x] 2.3 `harness_runtime::harness_search_path` drops `9p` mounts, read
      once from `/proc/self/mounts`; `resolve_real_executable` and
      `runtime_shim_is_active` walk it, the latter stopping at the shims
      directory.
- [x] 2.4 `MutationLock::acquire` without fsync; `profiles().list()` locks
      only to create the default profile.

## 3. The TUI's refresh as one read model

- [x] 3.1 `overview::MachineSnapshot` and `UzeApplication::machine_snapshot`.
- [x] 3.2 `src/ui/worker.rs::load_refresh_data` maps it; startup and
      refresh workers open `tracing` spans.

## 4. Holding it

- [x] 4.1 `application::performance_tests`: a world with a slow-probe
      harness and a deleted marketplace repository; one test per budgeted
      command path, the bootstrap (with a writes-nothing check) and the
      snapshot; 25 ms best of three.
- [x] 4.2 `command_performance::BUDGETED_COMMAND_TESTS` names them;
      `every_named_performance_test_exists` reads the source they name.
- [x] 4.3 `tests/cli/budget.rs`: a warm read-only command writes nothing
      under `UZE_HOME`; a marketplace registered by URL is listed and
      inspected without its repository.
- [x] 4.4 `docs/architecture/invariants.md` records the properties and the
      tests; `tests/README.md` lists the suite.
