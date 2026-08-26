## 1. Canonical marketplace contract

- [x] 1.1 Rename the repository-root and embedded manifest inputs to `marketplace.json`; update the core workspace and acquisition contract and diagnostics.
- [x] 1.2 Update application registration, embedded bootstrap, overview, and TUI model consumers to the single manifest name.
- [x] 1.3 Preserve vendor-owned marketplace catalogues and persisted marketplace state without renaming them.

## 2. Regression coverage

- [x] 2.1 Update testkit writers, deterministic fixtures, and Rust integration/UI tests to materialize `marketplace.json` roots.
- [x] 2.2 Add/retain negative coverage proving an `agents.json`-only root is rejected rather than accepted as a compatibility alias.
- [x] 2.3 Update conformance synthetic-world marketplace materialization and prove all four harness verticals retain their existing TUI-first coverage.

## 3. Durable documentation

- [x] 3.1 Update README, AGENTS.md, architecture invariants, and the marketplace/overview documentation to describe `marketplace.json` as the sole root manifest.
- [x] 3.2 Add ADR-032 in `docs/adr/`, update the ADR index, and mark ADR-023 superseded by ADR-032.

## 4. Verification

- [x] 4.1 Run focused marketplace/workspace/bootstrap/UI tests, then `cargo test --no-fail-fast`.
- [x] 4.2 Run `python3 conformance/lab.py --harness` for `claude`, `codex`, `opencode`, and `antigravity`.
- [x] 4.3 Run `openspec validate --all --strict` and a final reference scan proving no active UZE-owned `agents.json` marketplace input remains.
