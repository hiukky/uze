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
make install                                              # install into ~/.cargo/bin (force-rebuild, no version bump, --features telemetry)
cargo run --quiet --bin uze -- --version

cargo test --workspace --no-fail-fast                     # full workspace suite (default-members is the root crate only)
cargo test -p uze-core some_test_name                      # single crate / targeted test during iteration
cargo test --test package_containment                       # one top-level integration test file (tests/*.rs)

cargo fmt --check
cargo clippy --all-targets -- -D warnings                  # CI uses --all-targets; plain `clippy -- -D warnings` is the Makefile default
cargo llvm-cov --workspace --summary-only --fail-under-lines 68 --fail-under-regions 69 \
  -- --skip foreground_status_reports

cargo deny check                                           # licences, advisories, bans, sources (deny.toml); part of `make check`
make attributions                                          # regenerate CREDITS.md (about.hbs + Cargo.lock); CI fails when it drifts

UZE_LOG=info uze doctor                                    # spans and events as text (see docs/observability.md)
make observe                                               # local Jaeger (UI :16686); then OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318 cargo run --features telemetry --bin uze -- doctor
```

`Makefile` wraps all of the above (`make build`, `make test`, `make check`,
etc. — run `make help` for the full list). `ci.yml` (the fast gate) and
`conformance.yml` (the Lab, on Lab-affecting paths and nightly) are the
source of truth for what actually gates a merge; treat `make check` as a
close local proxy, not a guarantee of parity.

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

## Dependencies

The dependency surface is small on purpose: ~22 direct external crates
across the workspace. Every addition is a permanent maintenance liability
and part of the binary's supply chain, so choose by **provenance**, not by
whichever crate name matched the search.

Prefer, in this order, and stop at the first tier that can do the job:

1. **The standard library, or a crate already in the workspace.** Ask this
   first. Most additions lose here.
2. **Foundation** — `rust-lang`, `serde-rs`, `tokio-rs`, dtolnay:
   `serde`, `serde_json`, `thiserror`, `libc`, `tokio`.
3. **A de-facto standard with an organization behind it** — `clap`
   (clap-rs), `ratatui`, `crossterm` (crossterm-rs), `toml_edit`
   (toml-rs), `indicatif`/`dialoguer` (console-rs), `anstyle` (rust-cli),
   `unicode-width` (unicode-rs), `alacritty_terminal` (alacritty),
   `portable-pty` (wezterm), `rmcp` (the official MCP Rust SDK).
4. **A single maintainer, but mature and widely adopted** — `syntect`,
   `comfy-table`, `schemars`. Acceptable, with the trade-off stated in the
   PR.

Refuse, unless there is no alternative and the reason is written down:

- **A `0.0.x` crate.** In semver that range promises nothing; every
  release may break.
- **A crate that has renamed itself.** The identity of a dependency
  moving under the project is a supply-chain event, not branding.
- **A recently published crate with no adoption**, however well its README
  reads. A README is written by the person asking for your trust.
- **A crate whose author publishes many similarly-branded crates** with
  claims disproportionate to their version number.

Always check, and write the answer in the PR that adds it:

- **Who publishes it.** `cargo info <crate>` shows `repository`, but that
  field is set by the publisher — confirm the crate is genuinely published
  from the repository it names.
- **Whether it compiles C.** A build script compiling C costs real money
  here: `release.yml` cross-compiles four targets, two of them musl, and
  already carries a workaround for one such dependency (`onig_sys`, see
  the comment at `release.yml:206`). If a feature flag avoids the C path,
  take it.
- **`cargo deny check` passes.** A new advisory ignore needs a reason *and*
  a way out — `deny.toml`'s existing ignores name the condition that
  removes them, and a new one must do the same. An ignore with no exit is
  a decision to carry the problem forever.
- **Transitive weight.** `cargo tree -p <crate>` before, not after.

None of this applies to a crate the workspace already depends on: use it.

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
- `crates/uze-theme` — the design vocabulary: colour `Token`s, named
  `Symbol`s, the theme file schema, and the resolver that completes a
  partial theme from the built-in default. A leaf crate — it resolves no
  path, reads no environment, and names no rendering library, so every
  consumer adapts `uze_theme::Rgb` to what it draws with (`src/ui/theme.rs`
  for ratatui, `src/progress.rs` for the CLI). **Nothing outside
  `src/ui/theme.rs` may name a colour value or write a chrome glyph
  inline** — two rules in `tests/architecture/layering.rs` fail the build
  over it. Name the meaning (`Token::TextMuted`, `Symbol::MarkOfficial`)
  and let the active theme decide what it looks like; add a token or a
  symbol when none of the existing ones says what yours means. Colour that
  genuinely comes from content rather than from the design system — a
  pane's own output, syntax highlighting an extension ships — goes through
  `theme::content`.
- `crates/uze-extensions` — built-in TUI extensions. An extension answers
  with a `view::View` (a full-frame surface) or a `view::Section` (a
  collapsible block of one of the host's own columns) and never draws,
  computes geometry, or names a colour; `src/ui/extension_view.rs` renders
  both. Presentation, one directory per extension (`code/` is the only
  one today) with the extension's own surface in the file beside it, one
  `ExtensionRegistry::builtin` entry, one `ExtensionHit` variant per
  surface it draws. What more than one extension needs lives under
  `shared/`, and only once a second one actually needs it; `view.rs` and
  `Host` are the two contracts and sit at the crate root. `Host` is where
  every capability is granted — including the two that write, which the
  code surface's save and delete are the only callers of.
  `code` is **one** extension covering the checkout's changes, its files
  and its history, because the selection has to survive a switch between
  them and a selection cannot live above two extensions. Its rule: no
  handler branches on the mode to decide what the state *means* — the
  mode decides who is *asked*, and each half answers about the same path
  knowing nothing about the other.
- `crates/uze-integrations` — one module per harness
  (`claude`, `codex`, `opencode`, `antigravity`)
  implementing the shared `IntegrationPort` from `uze-core`, plus `shared/`
  for cross-vendor process/path helpers.
- `conformance/` — Harness Conformance Lab (Python): Real Harness +
  Synthetic World isolation evidence. `contract/` states what **every**
  harness must prove, in outcome terms, and names no vendor;
  `harnesses/<vendor>/bindings.py` says how that harness is driven and
  carries no assertion; `scenarios.py` keeps only what is genuinely unique
  to the vendor. A harness that cannot deliver part of a contract declares
  it through `bindings.unsupported` with a reason, which the run records —
  never by omitting the check. Vertical per harness
  (`conformance/harnesses/{antigravity,claude,codex,opencode}/`) in a
  disposable Docker environment — the real harness binary, a synthetic
  provider, zero Internet, zero tokens. Vendor-specific by design; never
  linked into the deterministic suite. Three fixture trees, by owner: the
  deterministic suites' inputs in `tests/_fixtures`; the Lab's own
  marketplace in `conformance/_fixtures/marketplace/`; per-harness session
  and provider state under `conformance/harnesses/<vendor>/fixtures/`. Run with
  `python3 conformance/lab.py --harness <h>`; replay a recorded run with
  `make lab-replay`. `conformance.yml` runs all four verticals (matrix) —
  its own workflow, on the paths that reach the Lab image plus nightly, so
  a docs or web change no longer pays 34 runner-minutes to prove nothing.
  `conformance-stability.yml` is ADR-035's promotion gate (3 clean runs per
  vertical), nightly only.
  Debugging a failure: see the `conformance-debug` skill (fast `--sandbox`
  reproduction loop, seconds not minutes) before iterating against the full
  gate run.
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
- `docs/appearance.md` — the two appearance axes: the palette (where a theme
  lives, the colour forms, what it cannot change) and the glyph set (the
  three UZE carries, and why the font is never detected). The canonical
  guide; `crates/uze-theme`'s module docs carry the reasoning.
- `docs/architecture/invariants.md` — properties the architecture actually
  holds today, each tied to the specific test that proves it. Treat this as
  the canonical list of "do not break this" behaviors.
- `openspec/` — active/archived change proposals (spec-driven work log);
  `openspec validate --all --strict` is part of the full gate set.

## Product journeys

`journeys/` is the third test tier: a user's flow performed through the real
CLI and the real TUI in a disposable world, then checked against the machine
it left behind. See `journeys/README.md` for the vocabulary and the
`journey-author` skill for how to write one; the rules that decide *what* a
journey is are here because they govern the shape of the suite.

- **Never validate UZE with UZE.** Every `then` check reads the filesystem,
  Git, the recorded task state or the process table. UZE's own report is the
  subject of a `cmd` step, asserted *against* those checks — never the source
  of truth for another one. This tier exists to catch "exited zero, reported
  the artifact, wrote nothing", and an assertion on UZE's own output cannot.
- **Screen text is a gate, never an assertion.** `expect` proves a gesture
  landed. A claim about what the screen *says* belongs in `src/ui/`'s own
  `TestBackend` tests, where it is cheaper and more precise.
- **Every gesture states its `expect`** — `journey validate` fails a click
  that does not. An untimed gesture is where a suite like this dies, so the
  linter refuses it rather than the reviewer.
- **Chapters are the order a person meets the product**, not subsystems:
  `01-first-run`, `02-packages`, `03-context`, `04-workspace`,
  `05-delivery`, `06-recovery`. The numbers are reading order only — every
  journey builds its own world, so none depends on a lower number having run.
  A chapter appears when its first journey does.
- **A scene continues the story; a file starts one over.** Joined by "and
  then" it is a scene; by "also" it is a file. Four things force a file: a
  different world, a claim that does not depend on the story so far, a
  different cadence, and — a property of the runner, not of taste — that a
  run stops at the first failure, so claims sharing a file share a fate.
- **A scene must be load-bearing.** Delete it: a later scene must break, or
  its checks must be the only proof of its own claim. Otherwise it was a
  step, not a claim, and belongs folded into its neighbour.
- **Tags say when it runs, numbers say where it sits.** `gate` is the
  pull-request set; everything runs nightly. A journey that becomes slow
  changes its tag, never its chapter.
- **The index is `journey list`**, read from the files themselves. Do not
  keep a table of what the suite proves — that is the trap the per-harness
  conformance matrix was in until it was parameterized.

A journey that backs a user-facing claim names the page it backs in its
`proves:` field, the way `docs/architecture/invariants.md` names the test
behind each property. `journey validate` fails a `proves:` that no longer
resolves, so a page that moved or was deleted surfaces as a red build, and
whoever changes a flow is told which page to re-read. It catches structural
drift and directs attention; it cannot tell you the prose went wrong, and
nothing can.

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
returns a `view::View` or a `view::Section` — the host owns rendering,
geometry, hit-testing and the palette — and every other capability
(running Git, reading a file, resolving `$HOME`) arrives through
`uze_extensions::Host`, implemented in
`src/ui/extension_host.rs`. `uze-extensions` depends on no UZE crate at
all, and names no process, filesystem or environment API; the architecture
suite fails on each. An extension is code UZE runs in its own
process, which is a different trust class from plugin bytes a harness
reads; see ADR-041 in `docs/adr/`.

**Nothing the workspace client draws waits on a repository.** Every Git
read it makes — the badge, the sidebar timeline, a commit's account, the
changes overlay and its per-file diff — runs on a thread and answers
through a channel, and so do the two other unbounded operations: placing a
new agent (`git worktree add` plus the project's `setup`) and slot
reconciliation. Two tests hold it: `src/ui/orchestrator/` (the render and
input halves) may not name `WorkspaceHost` at all, and every mention of it
in `src/ui/orchestrator.rs` must sit inside a `thread::spawn`. Every answer
carries the question it was asked, so one that arrives after the viewer
moved on is dropped rather than drawn. Adding a read means a
`spawn_*`/`absorb_*` pair beside the existing ones, never an inline call.

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
- **Portable Hooks (ADR-033, compiled per ADR-040)**: one authored
  `hooks.json` is the canonical Hook surface, and its handler contract is
  `HOOK_*` environment in, exit code out (0 allows, 3 denies with the
  reason on stderr, anything else fails by the group's effect), bounded
  output and a per-handler timeout, first-deny-wins, fail-open for
  observational vs fail-closed for deny/ask/transform. The contract is
  compiled at install time into the delivered artifact — a generated POSIX
  `sh` wrapper for the command-hook harnesses, the owned OpenCode bridge —
  with no `uze` on the execution path; a platform with no template is
  reported Unsupported, never adapted. Every projection (merged config
  entries, the generated Antigravity plugin, the OpenCode bridge) is
  receipt-owned, content-identity inspected, and never touches foreign
  hooks/plugins/order.
- **Machine and project scope are independent**: `uze setup`, `uze doctor`,
  `uze theme`, `uze market …` and `uze plugin …` are machine-scoped
  (`~/.uze`); `uze <plugin>@<market>`, `uze install` (aliased `uze i`),
  `uze remove`, `uze status`, `uze context inspect|plan|reconcile` and
  `uze agent …` are project-scoped (`agents.yaml`, `agents.lock`,
  `AGENTS.md`). Neither touches the other's state — see
  `docs/adr/019-explicit-project-machine-boundary-in-cli-command-grammar.md`.
  `uze install` converges the manifest in both directions — a plugin dropped
  from `agents.yaml` leaves `agents.lock`, while the Store and every harness
  keep it, because other projects share those and taking it off the machine
  is `uze plugin remove` — and leaves the project context reconciled, so
  declaring an environment and projecting it are one command rather than
  two.
- **`uze agent …` is an audience, not a category**: its reader is an agent
  UZE launched, not a person, so it is hidden from `uze --help` and
  documented in the region UZE projects into `AGENTS.md` — each audience
  reads one surface. `uze agent task name <type>/<subject>` is how work
  acquires the branch a reviewer sees and the label an operator reads; the
  vocabulary it is judged against is `worktrees.branch` in `agents.yaml`.
  Work that reaches its first commit still unnamed is named from that
  commit's subject, judged against the same vocabulary — a Git fact read on
  the evaluation pass, never a harness feature. A name anybody chose is
  never replaced.
- **`agents.yaml` is authored, `agents.lock` is derived**: the manifest holds
  what the project declared (marketplaces, plugins, the `worktrees:` policy);
  the lock holds only what resolving it produced — a commit per marketplace and
  a digest per package — so it is regenerated, never repaired, and deleting it
  loses nothing. A marketplace is a Git repository whether it is spelled as a
  URL or as a local path.
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
  (`tests/integrations/runtime_boundary.rs`).
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
<!-- uze:begin project:worktree-policy/f917b2e7340b7f72 -->
## Concurrent work isolation

- Name the work as your first action, before reading a file, planning or editing: `uze agent task name <type>/<subject>`. Types this project accepts: `feat|fix|docs|refactor|perf|test|build|ci|chore|style|revert`. The subject is one or two words naming the intention, not a description of the task — `fix/branch-naming`, not `fix/correct-the-problem-with-agent-branch-names`. The request you were given is where the intention comes from, so nothing you read later makes the name easier to choose. Work that reaches a commit still unnamed is named by UZE from that commit's subject, which is a worse name than the one you would have chosen. Either way your branch is renamed, so ask Git for its name rather than remembering it; a name you or the operator already chose is never replaced.
- Every agent UZE launches works in a checkout of its own under `.worktrees/<id>`, on branch `agent/<id>`. If your working directory is inside `.worktrees/`, you are already isolated; do not switch branches.
- Commit your work on your own branch, as you go. Never commit to, merge into, rebase, or reset the target branch: delivery is UZE's — UZE rebases your branch onto the target, runs the project's checks and publishes it, then asks you to open the request for it; commit on your branch and stop until it does.
- If UZE tells you a rebase is paused in your checkout, resolve the conflicts preserving the intent of your change, run `git rebase --continue`, run the project's checks, and end your turn.
- Before spawning parallel subagents that write files, give each its own checkout so they cannot collide:

```bash
git worktree add -b agent/<topic> "$(git rev-parse --path-format=absolute --git-common-dir)/../.worktrees/<topic>" HEAD
```

- The path above is resolved against the *primary* checkout on purpose — a path relative to your own would nest one worktree inside another.
<!-- uze:end project:worktree-policy/f917b2e7340b7f72 -->
