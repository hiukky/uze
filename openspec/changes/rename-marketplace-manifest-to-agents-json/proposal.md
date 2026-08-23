## Why

The marketplace-root manifest is still called `marketplace.json`, but that rename to `agents.json` was
analyzed and explicitly deferred (`redesign-cli-project-machine-grammar` design.md D7: "analysis only, no
rename"). It stays pending while the ecosystem naming family a marketplace repo belongs to already speaks
`agents` (`AGENTS.md`, `agents.lock`), and real marketplaces already use it: `hiukky/ai` ships
`agents.json` + `plugins/`, and `uze market add ~/ai` fails today with `bundle manifest is missing in
.../marketplace.json` even though the repo has the correct manifest under the correct name.

There is a second reason, documented in D7 Context §4: `marketplace.json` names two unrelated things —
UZE's own registry manifest and the vendor-dictated native catalogues (`.claude-plugin/marketplace.json`,
`.agents/plugins/marketplace.json`) Claude/Codex integrations republish. Renaming UZE's manifest removes
the file-identity ambiguity without touching a single vendor-owned byte.

## What Changes

- **BREAKING** — UZE's marketplace-root manifest file is renamed `marketplace.json` → `agents.json`:
  `market add`, `market inspect`, `plugin install/update <name>@<market>`, and the embedded `uze-official`
  snapshot all read `agents.json`.
- Schema unchanged: `{name, plugins: [{name, source, description, keywords}]}` (owner optional) is the
  same contract, only the filename changes.
- No fallback alias: a marketplace root with only `marketplace.json` now fails with an error naming the
  missing `agents.json`; migration is a one-file rename in the marketplace root.
- The vendor-owned `.claude-plugin/marketplace.json` / `.agents/plugins/marketplace.json` catalogues SHALL
  remain named `marketplace.json` — they are not UZE's files.
- Domain names stay: CLI verb `market`, `Marketplace*` internal types, `~/.uze/state/marketplaces.json`
  registry, and `plugin_marketplaces.json` provenance are unchanged (ADR-019 decided "domain name ...
  unchanged"; only the manifest filename changes).

## Capabilities

### New Capabilities

- (none)

### Modified Capabilities

- `marketplace` — the manifest contract a marketplace root answers: filename `agents.json`, no fallback
  alias, same schema; embedded `uze-official` pre-registration names `agents.json` at the repo root.
- `plugin` — `plugin install <name>@<marketplace>` resolves the plugin entry's `source` from `agents.json`
  (unchanged otherwise); convergence, provenance, lifecycle unaffected.

## Impact

- Core: `crates/uze-core/src/acquisition/marketplace.rs` (docs, parse error path, tests)
- Application: `src/bootstrap.rs` (embedded snapshot read), `application.rs`
  (`parse_marketplace_source`, `load_marketplace_manifest`), `build.rs` (embedded manifest collect +
  rerun-if-changed)
- CLI: none (help/completions never name the manifest file); one comment in `src/ui.rs`
- Repo root: `marketplace.json` → `agents.json` (the embedded `uze-official` snapshot)
- Tests: `tests/cli_grammar.rs`, `tests/project_agent_environment.rs`
- Docs: `README.md`, `docs/architecture/invariants.md`, ADR-023 (supersedes the filename clauses of
  ADR-012/ADR-015), ADR index
- Not impacted (vendor-owned, must stay `marketplace.json`): `.claude-plugin/marketplace.json`,
  `.agents/plugins/marketplace.json`, `crates/uze-integrations/src/claude|codex/**` catalogue code and
  their tests
