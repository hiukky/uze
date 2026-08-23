## 1. Application layer — new thin read models (no new invariants)

- [x] 1.1 Add `UzeApplication::market_inspect(&self, name: &str) -> Result<MarketplaceSummary>` in
      `crates/uze-application/src/application/marketplace.rs`, filtering the same per-entry computation
      `marketplace_list()` already does down to one named entry; `UnknownPackage`-style error when absent.
- [x] 1.2 Add `UzeApplication::harness_list(&self) -> Vec<HarnessHealth>` and
      `harness_inspect(&self, name: &str) -> Result<HarnessHealth>` in
      `crates/uze-application/src/application/doctor.rs`, slicing `doctor()`'s existing `harnesses` field —
      no new detection/provisioning call.
- [x] 1.3 Unit tests for both: `harness_inspect` known-id and unknown-name in `application.rs`'s own test
      module (`harness_inspect_finds_by_id_or_display_name_and_errors_on_unknown`); `market_inspect`
      unknown-name (`market_inspect_errors_on_an_unregistered_marketplace`) — the known-marketplace case is
      exercised end-to-end by the dogfood run and `tests/cli_grammar.rs::market_add_never_touches_the_project_lock`
      rather than duplicated as a unit fixture.

## 2. CLI grammar — `src/main.rs` rewrite

- [x] 2.1 Restructured `Command` enum: removed root `Add`/`List`/`Inspect`/`Update`; kept `Install`,
      `Status`, `Context`, `Doctor`, `Setup`; renamed `Marketplace`→`Market` (and `MarketplaceAction`→
      `MarketAction`); added `Harness { action: HarnessAction }` with `List`/`Inspect { name }`/
      `Setup { name: Option<String> }`.
- [x] 2.2 Root `Remove { plugin, format }` now calls `app.remove_project_plugin` only: `Removed` → success;
      `NoLock`/`NotInLock` → `uze::UzeError::{NoProjectEnvironment, PluginNotUsedByProject}` (new, minimal
      variants in `uze_core::error`, both naming `uze plugin remove <plugin>`) — the fallback to
      `app.remove_plugin` is gone.
- [x] 2.3 Added `#[command(external_subcommand)] External(Vec<String>)` and `ShorthandArgs` (a real
      `#[derive(Parser)]`, not the planned hand-rolled struct-with-manual-loop).
- [x] 2.4 `run_project_shorthand` and the pre-`clap` `argv[1].contains('@')` check are deleted. New
      `run_shorthand` reuses `parse_plugin_marketplace_spec` for classification and `ShorthandArgs::try_parse_from`
      for flags; a no-`@` token reuses `Cli::command().error(ErrorKind::InvalidSubcommand, ...)` for a
      genuine `clap`-shaped error with a `@market` hint.
- [x] 2.5 Added `grammar_tests` module in `src/main.rs`: walks `Cli::command()`'s full tree (not a
      hand-maintained list) asserting no name contains `@`, plus a check that `external` never surfaces as a
      discoverable leaf name.
- [x] 2.6 `Command::Market` wired to `marketplace_add/list/remove` (unchanged) + `market_inspect` (new).
- [x] 2.7 `Command::Harness` wired to `harness_list`/`harness_inspect` (new) and `HarnessAction::Setup` to a
      shared `run_setup()` helper also used by root `Command::Setup` — one implementation, two call sites.
- [x] 2.8 `print_colored_help` rewritten for the Project/Machine grouping; namespace `--help` (`market`,
      `plugin`, `harness`) confirmed self-contained via `clap`'s own nested-subcommand help (no hand
      rendering needed) — see `tests/cli_grammar.rs::market_help_is_self_contained`.

## 3. Tests

- [x] 3.1 `tests/cli_grammar.rs` (new) + `src/main.rs::grammar_tests`: built-in precedence, `--help`/`-V`
      short-circuit unchanged, unrecognized flag after the shorthand rejected by `clap`
      (`shorthand_rejects_an_unknown_flag_instead_of_ignoring_it`).
- [x] 3.2 `tests/cli.rs::root_remove_no_longer_falls_back_to_global_removal` +
      `tests/cli_grammar.rs::remove_is_the_builtin_not_shorthand`: the machine package survives a failed
      project-scoped removal, both outside a project and when the plugin isn't in the lock.
- [x] 3.3 Regression-only, confirmed by the full existing suite passing unchanged (`uze-core`/
      `uze-application` lifecycle/ADR-009 tests untouched by this change) — no new test needed since no
      behavior changed.
- [x] 3.4 `tests/cli_grammar.rs::market_add_never_touches_the_project_lock` and
      `::plugin_install_by_direct_path_never_touches_the_project_lock`, plus the dogfood run's `agents.lock`
      before/after (see final report) for the `<plugin>@<market>` side.
- [x] 3.5 `tests/project_agent_environment.rs::install_project_environment_reproduces_a_lock_on_a_fresh_machine`
      passes unchanged; re-confirmed live in the dogfood run with a third, brand-new `UZE_HOME`.
- [x] 3.6 `tests/cli_grammar.rs::help_names_the_project_machine_split` and `::market_help_is_self_contained`.
- [x] 3.7 Confirmed by the full workspace suite: `uze-core`/`uze-integrations` were not touched by this
      change (an unrelated, concurrent trait-widening fix from a peer session in `uze-integrations` was
      already in flight and is orthogonal — see the final report's "unrelated in-flight work" note).
- [x] 3.8 Alias window: **declined**, per explicit instruction in this change's approval — "prefer removing
      debt... não deixar duas gramáticas coexistindo." No aliases were added for `add`/`list`/`inspect`/
      `update`; they simply no longer exist at the root.

## 4. Docs and ADR

- [x] 4.1 `docs/adr/019-explicit-project-machine-boundary-in-cli-command-grammar.md` exists;
      `docs/adr/016-project-agent-environment.md` carries the `Status note: partially superseded by ...` line.
- [x] 4.2 `AGENTS.md`'s "UZE commands" section rewritten for the new grammar. `README.md` had no stale
      command references (its one `uze marketplace` mention is prose about the marketplace *concept*, not a
      CLI invocation).
- [x] 4.3 No manual edit: `CHANGELOG.md` is generated from Conventional Commits by `git-cliff` — the commit
      message for this change is the changelog entry.

## 5. Out of scope — explicitly not tasked here

- Renaming `marketplace.json` → `agents.json` (design.md D7) — separate change, not started.
- GitHub `owner/repo` shorthand for `market add` (design.md D6) — separate, small follow-up.
- A CLI/application path to lock a bare local or direct-Git plugin source without a marketplace
  (design.md Context §6) — orthogonal pre-existing gap, unaffected by this change.
- TUI Project-vs-Machine views (design.md D8) — consequence documented for a future change; `src/ui/*` is
  not touched by this task list.
