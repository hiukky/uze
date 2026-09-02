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

This repository is itself the official uze marketplace: `marketplace.json` +
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

### Test execution on WSL (history of the "session dies mid-test" bug)

Until 2026-09-01, running any suite that exercised a child-process timeout
killed the whole WSL login session (`[process exited with code 9]`, the
agent included). It was **not** OOM and not cargo parallelism:
`kill_process_group` in `crates/uze-core/src/subprocess.rs` shelled out to
`/bin/kill -KILL -<pid>`, and procps-ng `kill` parses a negative pid by its
first digit only (`pid = '0' - optopt`), so any child pid starting with `1`
(1000-1999, 10000-19999, ...) became `kill(-1, SIGKILL)`: every process the
user owns. The helper now issues `kill(2)` directly. Never reintroduce a
shell-out to `kill` for negative pids anywhere in the workspace; use
`libc::kill` (or `kill -- -<pid>` if a shell script truly needs it).

`CARGO_BUILD_JOBS=2` / `RUST_TEST_THREADS=4` (also `[build] jobs = 2` in
`.cargo/config.toml`) remain as a courtesy to a 4-vCPU VM, not as a
crash-avoidance measure. If a session still dies mid-run, look for a kill
by signal in `journalctl` (`user@1000.service: ... status=9/KILL` at the
same second as `Session N logged out`) before assuming memory pressure.

Run the CLI itself with `cargo run --bin uze -- <args>` or `./target/debug/uze <args>` after a build; `uze` with no args launches the terminal UI.

## Code style

Keep the code clean and self-documenting. Prefer expressive names and small,
focused functions over explanatory comments. Add a comment only when it
explains a non-obvious *why* — the rationale behind a decision, an invariant,
a workaround, or a subtle constraint the code alone cannot convey. Never
restate what the code already says; write intent, not implementation.

## Documentation hygiene

Do not create permanent Markdown files for implementation notes,
investigation logs, temporary plans, or task reports unless explicitly
requested — prefer ephemeral working notes. Research becomes documentation
only when the information is likely to remain useful, is not better
represented by code/tests/ADR/OpenSpec, and has a clear canonical owner.
Before adding a document, update the existing canonical one when
appropriate; new docs must state their durable purpose and owner, and
prefer updating an existing document over creating a new file.

## Workspace layout

Cargo workspace, edition 2024, MSRV 1.97. Single version source:
`[workspace.package].version` in the root `Cargo.toml` — every crate
inherits it; bump it before any binary is distributed (dev rebuilds don't
need to).

- `.` (binary crate `uze`) — CLI parsing (`src/main.rs`), the terminal UI
  (`src/ui.rs`, `src/ui/`), the runtime PATH shim (`src/shim.rs`), and
  `src/command_performance.rs`.
- `crates/uze-core` — harness-agnostic domain, organized into five
  concerns, each a module whose own doc says what belongs in it. Read the
  concern before the module: `hook` is a *capability*, and that is a
  different question from where its file sits.
  - `package/` — where a package's bytes come from and where they live:
    acquisition, trust, importers, bundle, naming, store.
  - `capability/` — what a plugin declares, portably: skill, hook.
  - `delivery/` — how a capability reaches a harness: integration,
    router, exposure, engine, state, persistence, reconciliation.
  - `project/` — what a project declares and what UZE writes into it:
    project_lock, worktree policy, context, text_region, workspace roots.
  - `machine/` — the local environment outside UZE's own state: home,
    detection cache, provisioning, subprocess, shell PATH, harness runtime.

  Public paths stay flat (`uze_core::store`, not `uze_core::package::store`)
  via re-exports at the crate root, which is also where a reader sees which
  concern each module belongs to. Depends on nothing harness-specific and
  must stay that way (see Architecture below).
- `crates/uze-application` — the product-facing facade
  (`UzeApplication`) that orchestrates Core + Integrations into
  install/remove/update/context lifecycle operations. `src/application.rs`
  is the large orchestration surface; `src/application/lifecycle/` holds
  the per-operation modules (add/install/remove/update/attach).
- `crates/uze-git` — the one transport for speaking to the Git binary:
  spawn convention, `read`/`write` entry points, and Git's exit code
  reported rather than classified (a non-zero exit is an answer for
  `diff`, `rebase` and `rev-parse --verify`, and a failure elsewhere —
  only the caller knows which). Carries no domain.
- `crates/uze-terminal` — the local terminal runtime: a server owning the
  pseudoterminals and a versioned client protocol, so a pane survives a
  client leaving. Depends on nothing else in the workspace.
- `crates/uze-extensions` — built-in TUI extensions. An extension answers
  with a `view::View` (what it has) and never draws, computes geometry, or
  names a colour; `src/ui/extension_view.rs` renders it. Presentation, one
  module per extension, one `ExtensionRegistry::builtin` entry.
- `crates/uze-integrations` — one module per harness
  (`claude`, `codex`, `opencode`, `antigravity`)
  implementing the shared `IntegrationPort` from `uze-core`, plus `shared/`
  for cross-vendor process/path helpers.
- `conformance/` — Harness Conformance Lab (Python): Real Harness +
  Synthetic World isolation evidence, vertical per harness
  (`conformance/harnesses/{antigravity,claude,codex,opencode}/`) in a
  disposable Docker environment — the real harness binary, a synthetic
  provider, zero Internet, zero tokens. Vendor-specific by design; never
  linked into the deterministic suite. Single fixture source:
  `tests/_fixtures`; per-harness synthetic seeds under
  `conformance/harnesses/<vendor>/fixtures/`. Run with
  `python3 conformance/lab.py --harness <h>`; replay a recorded run with
  `make lab-replay`. The CI `conformance` job runs all four verticals
  (matrix). Debugging a failure: see the `conformance-debug` skill (fast
  `--sandbox` reproduction loop, seconds not minutes) before iterating
  against the full gate run.
- `tests/` — domain-organized integration suites (one `main.rs` per
  domain: `cli/`, `memory/`, `packages/`, `workspace/`, `lifecycle/`,
  `projection/`, `integrations/`, `acceptance/`), shared test
  infrastructure in `crates/uze-testkit` (isolated `TestEnvironment`,
  `FakeHarness`, canonical/scenario fixtures), and the taxonomy documented
  in `tests/README.md` (L0-L4).
- `playground/` — WSL/distro install helpers (`make wsl-lab`) and a
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
CLI/TUI (src/)  ──uses──▶  uze-extensions   (presentation: an extension
      ↓                                      describes, src/ui renders)
uze-application  (orchestration: add/install/remove/update/context)
      ↓
uze-core         (domain contracts: Package, Store, Engine, Router,
      ↑           IntegrationPort, capability/exposure model)
uze-integrations (Claude, Codex, Antigravity, OpenCode — implement IntegrationPort)

uze-git          (transport, no domain — used by core and by extensions)
uze-terminal     (local terminal runtime, depends on nothing here)
```

**`src/` must not name `uze_core::` or `uze_integrations`.** Presentation
consumes read models from `uze-application`; reaching into the domain
directly makes every domain change ripple into the frontend and leaves no
single surface anything else could consume. Enforced by
`tests/architecture/layering.rs::architecture_rules_hold`, and the debt is
**zero** — every remaining reach is `sanctioned` and named. Need something
the domain has? Add it to `uze-application`: a read model, a method on a
service, or a re-export when it is vocabulary a read model is made of.
**Never raise a budget**, and add to `sanctioned` only for a file that is
genuinely not presentation, with the reason written down.

The compiler cannot enforce this in place of the test, because
`src/shim.rs` and `src/bin/uze-harness-matrix.rs` share the binary crate
and legitimately name the domain. Making `rustc` the enforcer would mean
giving them crates of their own — a decision about what the `uze` binary
is, not a tidy-up.

**An extension never draws, and reaches nothing it was not handed.** It
returns a `view::View` — the host owns rendering, geometry, hit-testing and
the palette — and every other capability (running Git, reading a file,
resolving `$HOME`) arrives through `uze_extensions::Host`, implemented in
`src/ui/extension_host.rs`. `uze-extensions` depends on no UZE crate at
all, and names no process, filesystem or environment API; the architecture
suite fails on each. An extension is code UZE runs in its own
process, which is a different trust class from plugin bytes a harness
reads; see the ADR in
`openspec/changes/enforce-architecture-seams/adr/`.

**Speak to Git through `uze-git`.** Never spawn `git` directly: two callers
with two exit-code conventions is what this replaced, and a repository
write lock cannot be complete if a module spawns Git around it. Reads go
through `read`, writes through `write`. The one exception is
`acquisition::git`, which clones *untrusted remote* repositories and
therefore strips the environment rather than inheriting it — a different
threat model, not a second convention. Both are sanctioned by name in the
architecture suite; a third spawn fails it.

`uze-core` production code never names a specific harness (Claude/Codex/
OpenCode/Antigravity) — enforced by
`tests/integrations/identity.rs::core_never_names_a_vendor_harness`; the
same neutrality holds for `uze-application` and `src/` (CLI/TUI),
enforced by `application_never_names_a_vendor_harness` and
`cli_and_tui_never_name_a_vendor_harness`. Vendor-specific knowledge lives
only in `uze-integrations`, whose `registry::IntegrationRegistry`
(`builtin`/`isolated`) is the single composition root that names the
concrete integration types — application, the runtime shim, and tooling
all consume the registry or the `IntegrationPort` contract. A new harness
should require no semantic change to Store, Engine, or Router and no
change to core/application/CLI/TUI — only a new integration vertical, one
registry entry, conformance, and docs. Antigravity CLI is the Google-family
v0 harness (ADR-027).

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
- **Portable Hooks (ADR-033)**: one authored `hooks.json` + shell-command
  ABI (normalized stdin/stdout, bounded output/timeout, first-deny-wins,
  fail-open for observational vs fail-closed for deny/ask/transform) is the
  canonical Hook surface; every harness projection (merged config entries,
  the generated Antigravity plugin, the owned OpenCode bridge) is
  receipt-owned, content-identity inspected, and never touches foreign
  hooks/plugins/order.
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
<!-- uze:begin project:worktree-policy/222da37836952335 -->
## Concurrent work isolation

- Isolated checkouts live in `.worktrees/<name>` under the primary checkout, one writer each, on branch `agent/<name>`.
- If your working directory is already inside `.worktrees/`, you are already isolated. Do not create another worktree, and do not switch branches.
- Before spawning parallel subagents that write files, give each its own checkout so they cannot collide:

```bash
git worktree add -b agent/<topic> "$(git rev-parse --path-format=absolute --git-common-dir)/../.worktrees/<topic>" HEAD
```

- The path above is resolved against the *primary* checkout on purpose — a path relative to your own would nest one worktree inside another.
- When work is done: leave your branch and its commits for review — never merge, rebase, or reset the primary branch.
<!-- uze:end project:worktree-policy/222da37836952335 -->
