## 1. Manifest and model

- [ ] 1.1 Add `requirements: [{ executable, version?, purpose? }]` to the canonical `plugin.json` schema in `uze-core`; validate at discovery (name required, version constraint parseable); document the field as an extension of the `agent-plugins.org` manifest
- [ ] 1.2 Model `Requirement` and the package's effective set: declared entries plus entries contributed by generated artifacts, each attributed to its source (author / artifact)
- [ ] 1.3 Unit tests: valid/malformed declarations; effective set merges duplicates by executable and keeps the strictest version constraint

## 2. Detection (read-only)

- [ ] 2.1 Implement detection in `uze-core::machine`: `PATH` lookup plus a version probe with a per-executable table (flag and parse format); unknown format reports "present, version unknown"
- [ ] 2.2 Requirement status read model per package: met / too old / missing, with the closing command when an installer is known
- [ ] 2.3 `uze plugin list` shows requirement status per package; `uze doctor` re-verifies every installed package's effective set and reports unmet/drifted requirements and orphaned UZE-installed tools
- [ ] 2.4 Tests with a fake `PATH` (present, absent, too old); classify any new CLI leaf in `command_performance.rs`

## 3. Plan, confirmation, install

- [ ] 3.1 Installer selection in `uze-core::machine`: preference order (user-level manager in use → `brew` → system manager → `winget`), executable → package-name table per installer, manual-fallback message when none applies
- [ ] 3.2 `RequirementPlan` read model from install/update: missing and too-old requirements with purpose, installer and command line
- [ ] 3.3 CLI: show the plan and ask `y/N`; `--yes` accepts; non-interactive without `--yes` declines and names the flag; run the installer through the process runner and report success/failure per requirement
- [ ] 3.4 TUI: render the plan as a view (extension host, no domain reach) with one confirmation; same outcome reporting
- [ ] 3.5 Receipts: a UZE-installed executable gets a receipt (executable, installer, requiring package); an already-present one is recorded as observed; package removal drops records and never removes an executable
- [ ] 3.6 Tests: plan rendering; decline keeps the package installed with the gap reported; `--yes` path with a fake installer; receipts written and dropped; architecture suite still zero-debt (no `uze_core::` reach from `src/`)

## 4. Integrations contribute their requirements

- [ ] 4.1 Integrations declare the requirements of the artifacts they generate; the `sh` hook wrapper (from `native-first-hooks`) contributes `jq` attributed to the wrapper
- [ ] 4.2 Test: a package with hooks and no declared requirements shows `jq` as required by the wrapper on Claude/Codex/Antigravity and nothing on OpenCode

## 5. Conformance and docs

- [ ] 5.1 Fixture: a marketplace plugin declaring a requirement; one vertical proves the unmet report when the tool is absent and the hook wrapper's fail-closed behaviour in that state
- [ ] 5.2 Docs: `docs/capabilities/portable-hooks.md` `jq` note points to requirements; plugin authoring docs describe `requirements`; `uze doctor` docs list the new report
- [ ] 5.3 Full gate: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --no-fail-fast`, `openspec validate --all --strict`
