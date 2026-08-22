# UZE — Agent Context

## Project overview

UZE is a local compatibility and distribution layer for the agent-plugin ecosystem.
Deterministic Rust pillars (Harness Manager / Plugin Manager / Context Manager) plus
an agentic `/uze` Skill. Package is the distribution unit; Store (`~/.uze/store`) owns
bytes; Integrations own harness delivery. See `README.md` and `docs/architecture/invariants.md`.

Workspace: `uze` (binary) + `crates/uze-core`, `crates/uze-integrations`,
`crates/uze-application`, `e2e`. Edition 2024, MSRV 1.97. Version is
`[workspace.package].version` in root `Cargo.toml` (currently `0.1.0-alpha.7`).

## Build / Test / Lint / Format

All commands are workspace-rooted. CI gates (`ci.yml`) are the source of truth.

```bash
make build        # cargo build --locked --bin uze  (debug)
make release      # cargo build --locked --release --bin uze
make install      # cargo install --path . --bin uze --locked --force  (into ~/.cargo/bin; set CARGO_INSTALL_ROOT to override)
make version      # cargo run --quiet --bin uze -- --version

make test         # cargo test --no-fail-fast
make fmt          # cargo fmt
make lint         # cargo clippy -- -D warnings   (CI uses --all-targets)
make check        # cargo fmt --check && cargo clippy -- -D warnings && cargo test --no-fail-fast

# Direct cargo equivalents
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --no-fail-fast
```

## Workspace conventions

- **Single version source:** bump `[workspace.package].version` before any binary is copied/installed/released (`docs/versioning.md`). Dev rebuilds need not bump; distributed builds must.
- **Store vs harness separation:** Store never writes harness artifacts; Integrations own vendor semantics (`docs/architecture/invariants.md`).
- **Acquisition never executes package code:** no hooks/submodules, consent boundary (`--trust`) only for remote MCP `command`.
- **Deterministic context:** `AGENTS.md` is the portable baseline. `CLAUDE.md`/`GEMINI.md` are bridged via `@AGENTS.md`; do not hand-edit managed regions.
- **Package vs context independence:** `uze add`/`remove`/`update` are global (Store); `uze context inspect|plan|reconcile` are project-scoped. Neither touches the other's state.
- **CLI commands must be fast, or explicitly justified otherwise:** a new command must be classified in `src/command_performance.rs` (`Budgeted` — low-millisecond, cache-backed — or `JustifiedSlow` with a stated reason); `cargo test --bin uze` fails by name if it's missing. See ADR 018 and `docs/adr/018-cache-harness-detection-with-fingerprint-ttl-invalidation.md`.

## Structure

- `src/` + `crates/uze-*/` — core/application/integrations
- `tests/` — vendor-neutral invariants (`tests/vendor_neutral_core.rs` etc.)
- `e2e/` — conformance fixtures
- `plugins/uze/` — the `/uze` Skill (ordinary local package)
- `playground/` — WSL/distro helpers (`make install-wsl-lab`)
- `docs/` — `adr/`, `capabilities/`, `architecture/invariants.md`

## UZE commands (project context)

```bash
uze status                  # is this project's context healthy?
uze context inspect         # read-only: what's here, is it portable?
uze context plan            # read-only: what would reconcile change?
uze context reconcile       # writes: compose AGENTS.md + harness bridges
uze list / uze inspect <id> / uze doctor
```
