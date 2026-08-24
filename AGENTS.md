# AGENTS.md

This file provides guidance to agentic coding tools (Claude Code, Codex,
OpenCode, Antigravity CLI) when working with code in this repository.

## What this is

uze is a Rust CLI: a compatibility and distribution layer for agentic tooling
across harnesses (Claude Code, Codex, OpenCode, Antigravity CLI). You install a
plugin once; uze stores its bytes centrally and delivers it through each
harness's most native mechanism — a real plugin where one exists, native
capabilities where it doesn't, a safe adapter only as a last resort. It also
maintains one project context file, `AGENTS.md`, and projects it into each
harness's own bridge/config format instead of maintaining four separately.

This repository is itself the official uze marketplace: `agents.json` +
`plugins/**` at the root, with `plugins/uze` (the `/uze` Skill) as the one
official plugin today.

## Commands

```bash
cargo build --locked --bin uze                          # debug build
cargo build --locked --release --bin uze                 # release build
cargo install --path . --bin uze --locked --force         # install into ~/.cargo/bin (force-rebuild, no version bump)
cargo run --quiet --bin uze -- --version

cargo test --no-fail-fast                                 # full workspace suite
cargo test -p uze-core some_test_name                      # single crate / targeted test during iteration
cargo test --test package_containment                       # one top-level integration test file (tests/*.rs)

cargo fmt --check
cargo clippy --all-targets -- -D warnings                  # CI uses --all-targets; plain `clippy -- -D warnings` is the Makefile default
cargo llvm-cov --workspace --summary-only --fail-under-lines 64 --fail-under-regions 65 --html
```

`Makefile` wraps all of the above (`make build`, `make test`, `make check`,
etc. — run `make help` for the full list). `ci.yml` is the source of truth
for what actually gates a merge; treat `make check` as a close local proxy,
not a guarantee of parity.

Run the CLI itself with `cargo run --bin uze -- <args>` or `./target/debug/uze <args>` after a build; `uze` with no args launches the terminal UI.

## Code style

Keep the code clean and self-documenting. Prefer expressive names and small,
focused functions over explanatory comments. Add a comment only when it
explains a non-obvious *why* — the rationale behind a decision, an invariant,
a workaround, or a subtle constraint the code alone cannot convey. Never
restate what the code already says; write intent, not implementation.

## Workspace layout

Cargo workspace, edition 2024, MSRV 1.97. Single version source:
`[workspace.package].version` in the root `Cargo.toml` — every crate
inherits it; bump it before any binary is distributed (dev rebuilds don't
need to).

- `.` (binary crate `uze`) — CLI parsing (`src/main.rs`), the terminal UI
  (`src/ui.rs`, `src/ui/`), the runtime PATH shim (`src/shim.rs`), and
  `src/command_performance.rs`.
- `crates/uze-core` — harness-agnostic domain: package/capability model,
  Store, Engine, Router, exposure planning, reconciliation, acquisition,
  provisioning, trust. Depends on nothing harness-specific and must stay
  that way (see Architecture below).
- `crates/uze-application` — the product-facing facade
  (`UzeApplication`) that orchestrates Core + Integrations into
  install/remove/update/context lifecycle operations. `src/application.rs`
  is the large orchestration surface; `src/application/lifecycle/` holds
  the per-operation modules (add/install/remove/update/attach).
- `crates/uze-integrations` — one module per harness
  (`claude`, `codex`, `opencode`, `antigravity`)
  implementing the shared `IntegrationPort` from `uze-core`, plus `shared/`
  for cross-vendor process/path helpers.
- `e2e` — conformance fixtures and a real-binary test harness
  (`e2e/src/harness.rs`, `tier.rs`) for exercising uze against actual
  vendor CLIs, not just isolated unit state.
- `tests/` — domain-organized integration suites (one `main.rs` per
  domain: `cli/`, `memory/`, `packages/`, `workspace/`, `lifecycle/`,
  `projection/`, `integrations/`, `acceptance/`), shared test
  infrastructure in `crates/uze-testkit` (isolated `TestEnvironment`,
  `FakeHarness`, canonical/scenario fixtures), and the taxonomy documented
  in `tests/README.md` (L0-L4).
- `playground/` — WSL/distro install helpers (`make install-wsl-lab`) and a
  default local plugin used for manual dogfooding.
- `docs/adr/` — numbered architecture decision records (read before making
  a structural change; recent ones cover generated native-package
  projection, Skill invocation policy, and invocation labels).
- `docs/architecture/invariants.md` — properties the architecture actually
  holds today, each tied to the specific test that proves it. Treat this as
  the canonical list of "do not break this" behaviors.
- `openspec/` — active/archived change proposals (spec-driven work log);
  `openspec validate --all --strict` is part of the full gate set.

## Architecture

Dependency direction is one-way and enforced by tests, not just convention:

```
CLI/TUI (src/)
      ↓
uze-application  (orchestration: add/install/remove/update/context)
      ↓
uze-core         (domain contracts: Package, Store, Engine, Router,
      ↑           IntegrationPort, capability/exposure model)
uze-integrations (Claude, Codex, Antigravity, OpenCode — implement IntegrationPort)
```

`uze-core` production code never names a specific harness (Claude/Codex/
OpenCode/Antigravity) — enforced by
`tests/integration_conformance.rs::core_never_names_a_vendor_harness`.
Vendor-specific knowledge lives only in `uze-integrations`. A new harness
should require no semantic change to Store, Engine, or Router — only a new
`IntegrationPort` implementation. Antigravity CLI is the Google-family v0
harness (ADR-027).

Key domain concepts (see `docs/architecture/invariants.md` for the guarded
properties):

- **Store** (`uze-core::store`) owns installed package bytes and is the
  single source of truth; it never writes anything a harness reads, and
  integrations never mutate it.
- **One Skill capability; invocation policy is its semantics**: explicit
  user action vs. background knowledge is the `invoke: {model, user}`
  block in SKILL.md (ADR-030), never a second capability kind — no
  canonical `Command`, no `commands/` surface. Integrations translate the
  policy into vendor-specific encodings; Store bytes stay verbatim.
- **Package vs. project context are independent**: `uze add`/`remove`/
  `update`/`market`/`plugin`/`harness` are machine-scoped (`~/.uze`);
  `uze context inspect|plan|reconcile` are project-scoped. Neither touches
  the other's state — see `docs/adr/019-explicit-project-machine-boundary-in-cli-command-grammar.md`.
- **Native > Generated Native > Safe Adaptation > Unsupported** delivery
  precedence per capability per harness. "Native" means the harness offers
  an officially supported mechanism preserving canonical semantics — not
  necessarily the same physical primitive across vendors. The canonical
  capability is the Skill; its portable semantics are *who may invoke it*
  (invocation policy, ADR-030), and each integration translates that policy
  into the vendor's own encoding.
- **Derived artifacts are non-authoritative and rebuildable** — anything an
  integration generates (a generated native package, a projected bridge
  file) can be safely deleted and regenerated from the Store + Engine
  alone.
- **Receipts drive lifecycle safety**: every managed filesystem artifact
  (symlink, generated directory, config entry) is tracked by a typed
  receipt; drift or an unreadable ledger blocks destructive mutation rather
  than authorizing one; removal always inspects current state before
  detaching (inspect-before-detach).
- **The runtime PATH shim** (`src/shim.rs`, `uze-core::harness_runtime`) is
  an experimental mechanism that projects `AGENTS.md` into a harness
  without writing into the project; it must never recursively invoke
  itself — this is a named, tested boundary
  (`tests/runtime_shim_boundary.rs`).
- **`command_performance.rs`** enforces that every CLI leaf command is
  classified as `Budgeted` (low-millisecond, cache-backed via
  `UzeApplication::detect_cached`) or `JustifiedSlow` with a stated reason;
  an unclassified command fails `cargo test` by name
  (`tests::every_cli_command_is_classified`). Classify any new command you
  add here.
- **Project context bridging**: `AGENTS.md` is the portable baseline for
  project instructions. `CLAUDE.md` is the one generated bridge
  (`@AGENTS.md`) produced by `uze context reconcile` — don't hand-edit
  their managed regions in a *project uze manages*; this repository's own
  root `CLAUDE.md`/`AGENTS.md` are the exception, maintained directly since
  this is uze's own source, not a uze-managed target project.

`IntegrationPort` (in `uze-core::integration`) is intentionally kept as one
trait proven by conformance tests across all four harnesses, rather than
split into per-capability traits (`PackageDelivery`, `SkillDelivery`, …) —
that fragmentation has been considered and rejected absent a concrete
implementation problem forcing it.
