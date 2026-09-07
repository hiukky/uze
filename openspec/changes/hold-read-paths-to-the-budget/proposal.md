## Why

Opening `uze` is fast; what the management screens show arrives slowly.
Measured on the maintainer's machine (WSL, four harnesses, one marketplace
registered from an SSH URL), release build, warm caches:

| Path | Measured | Where it went |
|---|---|---|
| `uze market list` | 3.4 s | a full clone of the marketplace, on every listing |
| management refresh (TUI) | ~7 s | the same clone twice: `marketplace().list()` and `marketplace().plugins()` |
| `uze theme list` | 45 ms, of which the command itself 0.1 ms | bootstrap: every harness re-recorded, every catalogue rewritten, each a synced atomic write |
| `uze doctor` | 44 ms, of which the report 10 ms | the same bootstrap |
| one absent harness, any command | 26 ms | a `PATH` walk stat-ing fifteen `/mnt/c/...` entries over 9p |
| `doctor` report, debug build | 64 ms | `PATH` walk per harness for the shim check, an fsync per mutation lock |

The clone is the regression the maintainer noticed: since a marketplace
became a Git repository (`db42e8c`), `load_marketplace_manifest` acquires a
Git source by cloning it into a scratch directory, and two read paths call
it per refresh. The rest predates it and was never measured: the one
budget test every `Budgeted` command pointed at timed `detect_cached`
alone, so nothing held a command's actual path.

## What Changes

- **A marketplace catalogue cache** (`application::marketplace_catalogue`):
  one checkout per Git marketplace under `cache/marketplaces/`, a one-hour
  TTL, filled by `market add` from the clone it already makes, dropped by
  `market remove`, read in place for a local source, and answering with the
  last catalogue seen when a refill fails. `market inspect <plugin>` reads
  the cached checkout instead of cloning.
- **The embedded snapshot is compared in memory.** `bootstrap::entries` and
  `has_update` read the manifest and the plugin's files from the binary;
  extraction to disk is now only what an install does.
- **A warm bootstrap writes nothing.** `state::record` skips an unchanged
  record; `ensure_default_plugins` republishes only an integration whose
  `publication` is `Unpublished`; Claude's and Codex's `publication` also
  notice a generated envelope missing from disk, so gating loses no
  healing.
- **A harness is looked for on this machine's own filesystems.**
  `harness_runtime::harness_search_path` drops `PATH` entries on a `9p`
  mount (a Windows drive inside WSL); `runtime_shim_is_active` walks it and
  stops at the shims directory.
- **The mutation lock is taken without an fsync**, and a profile listing
  takes it only when it has a default profile to create.
- **`MachineSnapshot`**: the management screens' data is one application
  read model; the TUI composes nothing.
- **Budget tests per command** (`performance_tests`): each `Budgeted`
  command's application call, on a fresh application, in a world whose
  harness sleeps half a second per probe and whose marketplace repository
  was deleted; ceiling 25 ms, best of three, debug build. Binary-level
  tests hold the two claims a timing cannot: a warm read-only command
  writes nothing under `UZE_HOME`, and a marketplace registered by URL is
  listed without its repository. `BUDGETED_COMMAND_TESTS` is cross-checked
  against the source it names.
- **`tracing` spans** on the bootstrap, the snapshot, a marketplace clone
  and the TUI's startup and refresh workers — the crate was already in the
  tree through `rmcp`; a subscriber and an exporter are a change of their
  own.

## Impact

- Specs: `cli-performance` (added requirements, one modified),
  `marketplace` (added requirement).
- Code: `uze-core` (`home`, `state`, `persistence`, `harness_runtime`),
  `uze-application` (`marketplace_catalogue`, `marketplace`, `bootstrap`,
  `overview`, `doctor`, `profile`, `application`, tests),
  `uze-integrations` (`claude`, `codex` publication), `src/ui/worker.rs`,
  `src/command_performance.rs`, `tests/cli/budget.rs`.
- Behaviour a person can see: a Git marketplace's listing can be up to an
  hour behind its remote, and registering the same source again refreshes
  it now. A tool installed only on a Windows drive is no longer resolved as
  a harness, by detection or by the runtime shim.
