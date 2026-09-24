## Why

Authoring a plugin today is entirely manual and undocumented as a flow: the
author writes `plugin.json`, `skills/*/SKILL.md` (and optionally `hooks.json`,
`mcp.json`, `AGENTS.md`) by hand, assembles a Git repository with a
`marketplace.json`, commits it, registers it with `market add`, and only then
runs an install — where the *first* feedback about a malformed manifest, an
invalid package name or an unsafe path reference arrives. There is no
scaffold, no offline validation, and no preview of what a plugin would deliver.
The intended operator of this flow is an agent: it needs deterministic,
scriptable commands that ask for and produce exactly one artifact each, not
prose instructions it has to interpret. Agent-driven authoring is the
mechanism that makes "install once, write your own plugin" a five-command
journey instead of a page of documentation read from `uze-testkit` sources.

## What Changes

- **A hidden, agent-facing authoring surface** — new deterministic verbs under
  `uze agent …` (the audience already documented in the region UZE projects
  into `AGENTS.md`; hidden from `uze --help` by the same rule that hides
  `agent task name` and `agent context`). Every verb is deterministic: same
  inputs, same artifacts, no interactive prompt, JSON or plain output, and a
  non-zero exit carrying the reason when validation fails.
- **`uze agent market create <name>`** — scaffolds a new marketplace as a Git
  repository at a directory the agent names (`--at <dir>`): `marketplace.json`
  (name, owner, description, an empty `plugins: []`), `plugins/`,
  `git init` and an initial commit through `uze-git`. The scaffold then
  **registers and links** the marketplace in one step, reusing the existing
  machine registry and `market link` machinery — the marketplace's bytes stay
  where the agent wrote them; only the record lives in `$UZE_HOME`. A linked
  marketplace delivers the author's working tree, including not-yet-committed
  files, so the edit → install → test loop needs no publish step.
- **Existing marketplace selection needs no new verb**: `market list` already
  answers which marketplaces exist; every authoring verb takes
  `--market <name>` and fails with the registry's own answer when the name is
  unknown. The authoring flow is: create *or* select, then author into it.
- **`uze agent plugin create <name>`** — scaffolds one plugin inside a
  marketplace's checkout (`plugins/<name>/` by convention): `plugin.json`
  (name, description) and `skills/<name>/SKILL.md` with the canonical
  `invoke: {model, user}` policy block commented for editing. `--hook`,
  `--mcp` and `--instructions` add `hooks.json`, `mcp.json` and an
  instruction-resource file respectively; each file carries commented
  documentation that must not drift from what the parser accepts (the
  `agents.yaml` scaffold precedent). Adds the matching entry to the
  marketplace's `marketplace.json` so the plugin is installable immediately.
- **`uze agent plugin check <path>`** — full offline validation of an authored
  plugin directory (and `uze agent market check <path>` for the marketplace
  wrapping it): manifest parse, package-name charset, unsafe path references,
  `SKILL.md` frontmatter and invocation policy, `hooks.json` handler contract,
  `mcp.json`, and the marketplace entry that resolves the plugin. This is the
  feedback an author currently gets only at install time, delivered before any
  install and without touching the Store or any harness.
- **The `uze:author` Skill** — added to the official plugin
  (`plugins/uze/skills/author/SKILL.md`, `invoke: {model: true, user: true}`):
  the guided script for the authoring loop — marketplace create or select,
  plugin scaffold, capability flags, *check before install*, install from the
  linked marketplace, iterate, publish by pushing. The verbs are the
  deterministic API; the Skill is what teaches an agent to conduct them in
  the right order and is delivered to every harness through the official
  plugin's normal delivery.
- **Classification**: every new leaf command is classified in
  `command_performance.rs` (create/scaffold verbs are `Budgeted`-adjacent file
  writes plus one Git commit each; check verbs are read-only).

Out of scope: publishing to a remote (pushing to a Git host is the Git the
author already knows — the scaffolded marketplace is a normal repository the
moment it is born), interactive wizards in the TUI, a person-facing wrapper
for these verbs (the agent audience is the only reader this change commits
to), and any change to what install delivers.

## Capabilities

### New Capabilities

- `plugin-authoring`: the authoring surface UZE exposes to an agent —
  scaffolding a marketplace (and registering it linked), scaffolding a plugin
  and its capabilities inside a chosen marketplace, and offline validation
  (`check`) of authored plugins and marketplaces; each as a deterministic
  verb with a machine-readable answer, hidden from the person's `--help` and
  documented in the projected `AGENTS.md` region; the guided script for that
  loop ships as the `uze:author` Skill in the official plugin.

### Modified Capabilities

<!-- Registration reuses the marketplace registry's existing add-and-validate
     requirement and link machinery verbatim; no requirement of an existing
     capability changes. All new externally observable behavior belongs to
     plugin-authoring. -->

## Impact

- `crates/uze-core`: a small `authoring` concern beside `package/` — scaffold
  templates (marketplace + plugin + capability files), the check/validation
  model over the existing manifest, skill, hook and mcp parsers, and no
  vendor names. Marketplaces are written through `uze-git` (one Git write
  per create; no new spawn convention — the architecture suite already
  sanctions `acquisition::git` and `uze-git` only).
- `crates/uze-application`: an `authoring` lifecycle module beside
  `lifecycle/` — `marketplace create` (scaffold → register → link),
  `plugin create` (scaffold → entry in `marketplace.json`), `check` (validate
  → read-model report). Read models for the reports in `read_models.rs`.
- `src/main.rs`: new `AgentAction` variants behind the existing hidden `uze
  agent` grammar; `command_performance.rs` classification for each new leaf.
- `plugins/uze`: the new `author` Skill (`skills/author/SKILL.md`) and its
  entry in the root `marketplace.json` (the official plugin's own description
  may grow the authoring mention); delivered through the existing Skill
  pipeline, no integration change.
  here. `tests/`: `tests/packages/` (scaffold + check contracts),
  `crates/uze-testkit` fixtures where a scaffolded layout doubles as a
  fixture generator; a product journey in `journeys/02-packages` covering
  create → check → install from the linked marketplace.
- No new external dependency: templates are rendered with the standard
  library, Git speaks through `uze-git`, and validation reuses the existing
  parsers. No dependency additions.
