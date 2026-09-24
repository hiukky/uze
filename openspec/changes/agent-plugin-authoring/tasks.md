## 1. Core: the `package/authoring` concern

- [x] 1.1 Create `crates/uze-core/src/package/authoring.rs` (module doc stating the concern: producing the bytes a package is acquired from) with the marketplace scaffold: `marketplace.json` (name, owner, description, `plugins: []`), `plugins/` tree, rendered from inline `const` templates
- [x] 1.2 Add the plugin scaffold: `plugin.json` (name, description), `skills/<name>/SKILL.md` with the canonical `invoke: {model, user}` policy block and commented field documentation; `--hook`/`--mcp`/`--instructions` templates for `hooks.json`, `mcp.json`, and the instruction resource
- [x] 1.3 Add `authoring::check`: compose `read_plugin_manifest` + `validate_references`, `is_valid_name_component`, the `invoke:` policy parser, the `hooks.json` handler contract, the `mcp.json` model and `acquisition/marketplace.rs::parse_manifest` into a `ValidationReport` (findings with location + reason, plus what the artifact would deliver); marketplace check runs the plugin check per resolved entry
- [x] 1.4 Verify `parse_manifest` accepts an empty `plugins: []`; if it requires an entry, extend the parser for the born-empty state (never a fake plugin)
- [x] 1.5 The invariant test: for every scaffold flag combination, render into a scratch tree and run `authoring::check` over it — clean. Extend the guard to hand-edited breakages (bad name charset, `..` path reference, malformed `invoke:`, out-of-bounds hook timeout) each reported with a locating reason
- [x] 1.6 Create refuses to overwrite: an existing registered marketplace name, a non-empty `--at` directory, and an existing `plugins/<name>/` each fail before any write

## 2. Application orchestration

- [x] 2.1 Create `crates/uze-application/src/application/authoring.rs` beside `lifecycle/`: `marketplace create` = scaffold → `git init`/`add`/`commit` through `uze-git` → existing `marketplace add` (Local source) → existing `link`; one `MutationLock` around the registry mutation
- [x] 2.2 Git identity: detect a configured `user.name`/`user.email`; when absent, fail with the two `git config` commands for the author to run — never fabricate an author line
- [x] 2.3 `plugin create` = scaffold into the marketplace checkout → add the `plugins[]` entry to its `marketplace.json`; refuse when the name collides or the plugin exists
- [x] 2.4 Read models in `read_models.rs`: create answers (paths written, commit, registry record, link) and the `ValidationReport`; every verb's answer is JSON-renderable
- [x] 2.5 Containment: plugin create resolves and writes only inside its marketplace checkout; `--at` must be empty or absent

## 3. CLI grammar

- [x] 3.1 Add `AgentAction::Market(AgentMarketAction::Create)` and `AgentAction::Plugin(AgentPluginAction::{Create, Check})`, hidden from `--help` like the rest of the `agent` grammar, dispatched from `run_agent`
- [x] 3.2 `uze agent market check <path>` beside the plugin verb (spec requires the marketplace-level check answer)
- [x] 3.3 Classify every new leaf in `src/command_performance.rs` (`every_cli_command_is_classified` passes; check verbs read-only)

## 4. The `AGENTS.md` audience region and the Skill

- [x] 4.1 Grow the region UZE projects into `AGENTS.md` (`crates/uze-core/src/project/` context) with the authoring verbs and their loop, in the same voice as the existing `agent work name`/`agent context` documentation
- [x] 4.2 Author `plugins/uze/skills/author/SKILL.md` (`invoke: {model: true, user: true}`): the decision tree (create vs. select via `market list`), scaffold, capability flags, check-before-install, install from the linked marketplace, iterate, publish by pushing
- [x] 4.3 Update the root `marketplace.json` entry (the official plugin's description) for the authoring mention
- [x] 4.4 Regenerate the project's own projected `AGENTS.md` region and verify the Skill validates as canonical skill frontmatter (its own plugin passes `check`)

## 5. Tests and the journey

- [x] 5.1 `tests/packages/authoring.rs`: scaffold→register→link contract, born-linked delivery (install before a second commit), refusal cases, check reports (clean and each finding class)
- [x] 5.2 A product journey in `journeys/02-packages`: agent performs create → plugin create → check → install from the linked marketplace; every check reads the filesystem and the Store, never UZE's own report — written in the post-flatten grammar
- [ ] 5.3 `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, the full workspace suite, and the architecture suite green (no new spawn convention; no vendor names in core)
