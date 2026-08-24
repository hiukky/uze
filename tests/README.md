# UZE test suite

This is the map of the whole suite: **what** is tested, **at which level**,
**against which environment**, **with which harness**, and **what evidence a
passing test provides**. A green `cargo test` is not the story: the story is
which cell of the matrices below is covered by which test file.

## Levels

| Level | What it proves | Environment | Where |
|---|---|---|---|
| **L0 — Unit** | pure logic: parsing, normalization, identity, deterministic helpers | nothing real; trivial temp only | `#[cfg(test)]` inside `crates/*/src/**`, `src/**`; `tests/packages/*`, `tests/projection/*` helpers |
| **L1 — Component/Contract** | a subsystem's contract on a real isolated filesystem, with a *fake* process boundary | isolated temp HOME/UZE_HOME, no developer state, fake harness CLIs only | `tests/{cli,memory,packages,workspace,lifecycle,projection}/**`, `tests/integrations/**` |
| **L2 — Harness Conformance** | real vendor binary semantics, isolated HOME/UZE_HOME, no model calls | real vendor binary, skipped cleanly when absent (`UZE_REAL_HARNESS_TESTS`-style probe-and-skip) | `tests/integrations/harness/codex.rs::real_codex_dogfood...`; the `e2e/` container lab (Tiers 1-2) |
| **L3 — Acceptance** | public user-level scenario end-to-end through the real `uze` binary | clean isolated `TestEnvironment` (real UZE binary, fake or controlled harness CLIs) | `tests/acceptance/**` |
| **L4 — Manual/Model behavioral** | model-invocation or interactive-only behavior | manual/agentic eval, never CI | `tests/fixtures/scenarios/eval/` (see `docs/capabilities/uze-skill.md`) |

Rules:

- **L3 exercises the public path**: `uze` binary → Application → Store/Engine
  → Integration. No test recreates the implementation.
- **Never call a real vendor CLI from ordinary CI**: L2 probes are
  skip-if-absent by design; the `e2e/` lab is the verdict where real vendor
  behavior matters.
- The same invariant at multiple levels is *good* (L0 exact-coverage
  helper + L3 "nothing missing after install"). Duplicates at the *same*
  level are not.

## Structure

```
tests/
├── cli/             CLI layer: grammar (ADR-019), machine-scoped commands
├── memory/          what UZE sees: context inspection, reconciliation, projections
├── packages/        acquisition, containment, Store/Engine, canonical model
├── workspace/       agents.lock consumer, marketplace (incl. malformed), root resolution
├── lifecycle/       install (application layer), future: update/remove/receipts/drift
├── projection/      exposure naming, invocation labels/policy, shared skill roots
├── integrations/    contract, capability+lifecycle conformance, runtime-shim boundary,
│   ├── harness/     per-harness invocation-policy semantics (claude/codex/opencode/antigravity)
├── acceptance/      L3 scenarios (see below)
└── fixtures/        canonical / foreign / scenarios / golden (see tests/fixtures/README.md)
```

Each `tests/<domain>/main.rs` is the Cargo test target (directory-`main.rs`
layout); the `.rs` files are its modules. `tests/` files that predate the
refactor (consumer.rs's historically-named tests, canonical_package.rs)
live inside their domain with behavior-descriptive test names.

## Support crate

`crates/uze-test-support` owns the shared infrastructure:

- `temp::TestEnvironment` — isolated HOME/UZE_HOME/PATH/cwd/fake-bin and
  per-harness config homes, with real-home safety guards. Every child
  process gets the environment through `env::command` (process env of the
  test binary is never mutated); in-process `PATH`/`HOME` mutation goes
  through `env::scope` (crate-wide mutex + RAII restore).
- `fake_harness::FakeHarness` — declarative fake CLI binaries (rule table +
  invocation log), including the vendor plugin-marketplace state machine
  (`Action::VendorMarketplace`) and Antigravity's stub-install lifecycle.
- `fixtures` — canonical `/foreign/`/`scenarios/`/`golden` resolution.
- `scenario::Scenario` — declarative system-state builder.
- `assertions` — context-carrying fs/process assertions + `snapshot_dir`.

## Commands

```bash
cargo test --workspace --no-fail-fast   # the full suite (includes acceptance)
cargo test -p uze --test acceptance      # L3 only (the release signal)
cargo test -p uze --test integrations    # conformance + per-harness semantics
cargo test -p uze --test projection      # naming/labels/shared roots
make test-acceptance / make test-conformance  # same as above
make test-real-harness                   # L2 probes that need real vendor binaries
```

Real-harness policy: a probe skips cleanly when the binary is absent
(`real_codex_dogfood...`); the `e2e/` lab (Docker, offline tiers) is the
place for real-vendor verdicts, and is never required for the ordinary
suite.

## Domain × level matrix (as of this refactor)

| Domain | L0 | L1 | L2 | L3 |
|---|---:|---:|---:|---:|
| Store / packages | ✓ | ✓ | — | ✓ |
| Acquisition (git, containment) | ✓ | ✓ | — | partial |
| Memory / context (inspect/reconcile) | ✓ | ✓ | — | ✓ |
| Skills (model + discovery) | ✓ | ✓ | partial | ✓ |
| Invocation policy | ✓ | ✓ | ✓* | ✓ |
| MCP | ✓ | ✓ | — | ✓ |
| Lifecycle (install/remove/receipts/drift) | ✓ | ✓ | ✓* | ✓ |
| Runtime shim | ✓ | ✓ | ✓* | ✓ |
| CLI (grammar + machine) | ✓ | ✓ | — | ✓ |
| Marketplace | ✓ | ✓ | partial | ✓ |
| Workspace (lock/root resolution) | ✓ | ✓ | — | ✓ |

`✓*` = covered by the skip-if-absent real-Codex dogfood (L2 evidence when a
binary exists) or the `e2e` lab (partial). `partial` = covered only at some
levels or through the e2e lab, not by an in-repo test at that exact tier.

`--` = deliberately absent: the invariant is proven at a lower tier and no
real-vendor evidence exists in-repo (see the harness matrix below).

## Harness evidence matrix

| Harness | Component (L1) | Real CLI (L2) | Acceptance (L3) |
|---|---:|---:|---:|
| Claude | ✓ | e2e lab (Tier 2) — no in-repo probe | ✓ |
| Codex | ✓ | ✓ (`real_codex_dogfood...`, zero model calls, skip-if-absent) | ✓ |
| OpenCode | ✓ | e2e lab (Tier 2) | ✓ |
| Antigravity | ✓ | e2e lab (Tier 2) | ✓ |

"e2e lab" = `e2e/` container lab Tiers 1-2 (offline, no credentials, runs
the real binaries) — the honest place for vendor-semantics verdicts.

## Acceptance scenarios (L3)

| Id | Scenario | Test |
|---|---|---|
| A1 | fresh machine + canonical plugin | `fresh_project::fresh_machine_installs_canonical_plugin_and_inspects_healthy` |
| A2 | fresh clone + agents.lock | `fresh_project::fresh_clone_with_lock_install_marks_environment_ready` |
| A3 | marketplace + consumer | `fresh_project::marketplace_resolution_installs_plugin_by_shorthand` |
| A4 | multi-harness projection, no duplicate delivery | `multi_harness::one_plugin_reaches_every_harness_with_no_duplicate_delivery` |
| A5 | invocation policy projection | `multi_harness::invocation_policy_projects_per_harness_classification` |
| A6 | remove lifecycle | `lifecycle::remove_lifecycle_cleans_artifacts_and_keeps_project_lock_untouched` |
| A7 | drift blocks destructive remove | `lifecycle::drift_blocks_destructive_remove_and_preserves_the_artifact` |
| A8 | runtime shim never recurses | `runtime_shim::runtime_shim_active_internal_calls_resolve_real_executable_without_recursion` |
| A9 | nested cwd → workspace root | `workspace_health::nested_cwd_resolves_workspace_root_and_installs` |
| A10 | workspace overview readiness | `workspace_health::workspace_overview_tracks_environment_readiness` |
| A11 | projection conflict honest failure | `multi_harness::projection_conflict_is_reported_honestly` |
| A12 | golden environment health (release signal) | `fresh_project::golden_environment_is_healthy` |

A12 update-lifecycle is *not* an acceptance scenario yet: update semantics
are L1 (`tests/packages/acquisition.rs` re-resolution tests) and the CLI
`plugin update` path is untested at L3 — that is the first gap to close
after this refactor.

## Isolation guarantees

- `TestEnvironment::isolated()` roots every path under a fresh temp dir;
  `assert_not_real_home` panics if any root could overlap `~/.uze`,
  `~/.claude`, `~/.agents`, `~/.codex`, `~/.gemini`,
  `~/.config/opencode` (or an ancestor of them).
- Child-process env is set per-`Command` (`env::command`): the test binary
  env is never mutated for L3.
- In-process `PATH`/`HOME`/cwd mutation goes through `env::scope`
  (serialized on a per-binary mutex, restored on drop — no ad hoc
  `set_var` anywhere: `grep -rn "set_var" tests/` finds only the support
  crate).
- L2 probes require the vendor binary and skip cleanly; nothing in the
  ordinary suite depends on developer-installed harnesses.
