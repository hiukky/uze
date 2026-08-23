## 1. Rename the manifest contract

- [x] 1.1 `git mv` repo-root `marketplace.json` → `agents.json` (embedded `uze-official` snapshot source); shape unchanged
- [x] 1.2 Update `crates/uze-core/src/acquisition/marketplace.rs`: module docs, `parse_manifest` error path (`agents.json`), tests write `agents.json`
- [x] 1.3 Update `crates/uze-application`: `bootstrap.rs` (`extract_and_parse` reads `agents.json`), `application.rs` (`parse_marketplace_source`, `load_marketplace_manifest`), `build.rs` (collect `agents.json`, `rerun-if-changed`)
- [x] 1.4 Update `src/ui.rs` doc comment

## 2. Tests and fixtures

- [x] 2.1 Update `tests/cli_grammar.rs` and `tests/project_agent_environment.rs` fixtures to write `agents.json`
- [x] 2.2 Assert vendor-catalogue tests (`tests/shared_agent_skill_root_naming.rs`, `tests/plugin_first_vertical_slice.rs`) still read `.agents/plugins/marketplace.json` — unchanged, not "fixed"
- [x] 2.3 Dogfood: `uze market add ~/ai` succeeds against `~/ai/agents.json`; error for a root with only `marketplace.json` names `agents.json`

## 3. Docs and ADR

- [x] 3.1 `README.md` official-marketplace tree: `agents.json`
- [x] 3.2 `docs/architecture/invariants.md`: contract mentions → `agents.json`; vendor catalogue mentions untouched
- [x] 3.3 ADR-023 written (`docs/adr/023-marketplace-manifest-is-agents-json.md`), index entry added

## 4. Validation

- [x] 4.1 `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test --no-fail-fast`
- [x] 4.2 `openspec validate --all --strict`
