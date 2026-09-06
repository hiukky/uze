## Why

UZE has global machine state (`~/.uze/store`, `marketplaces.json`, `attachments.json`) and project-scoped context (`AGENTS.md`, `.agents/`, harness bridges), but projects lack a reproducible declaration of which plugins compose their agent environment.

A contributor cloning a repo has no way to reconstruct the author's agent environment without manually running `uze marketplace add` and `uze plugin install` for every dependency. Global state is not versionable; project state must be.

The North Star experience:
```bash
# author
uze flow@ai
git add agents.lock
git commit

# contributor
git clone <repo>
cd <repo>
uze install
```

Result: **Same project. Same agent environment. Any supported harness.**

## What Changes

- **`agents.yaml`** — new project-scoped file where a person declares the desired agent environment (marketplaces, plugins, isolation policy), edited in place so comments survive
- **`agents.lock`** — generated from `agents.yaml`: resolved sources, pinned revisions, verified `integrity`, and the request each entry satisfies. Carries no intent, and is safe to delete
- **`uze <plugin>@<marketplace>`** — new project shorthand that writes `agents.lock` (requires `@`)
- **No new command** — `uze install` creates `agents.yaml` when a project has none, carrying every key the schema accepts with only the default policy live and the rest commented; opening the client never creates anything
- **Isolation policy in the workspace client** — the agent context popup names the completion behavior, target and gate in force and their provenance, and changes them on click
- **`uze install`** — new command that consumes `agents.lock` to reconstruct environment on fresh machine
- **`uze remove <plugin>`** — disambiguated: removes from project lock if present, else delegates to global `remove_plugin`
- **Project root resolution** — deterministic walk upward for `agents.yaml` > `AGENTS.md` > `.git`; `agents.yaml` replaces `agents.lock` as the consumer workspace anchor
- **Application API** — `project_environment()`, `plan_project_environment()`, `add_project_plugin()`, `remove_project_plugin()`, `install_project_environment()`
- **Error variants** — `UnsupportedLockVersion`, `MalformedLock`, `MarketplaceSourceConflict`, `MarketplaceMismatch`
- **Dependency** — `noyalib` (maintained YAML, replacing deprecated `serde_yaml`)

## Capabilities

### New Capabilities
- `project-agent-environment`: project-scoped desired state (`agents.lock`), global vs project separation, reproducible install
- `agents-lock`: YAML schema v1, deterministic serialization, reproducible source identity, verified integrity, offline staleness detection, trust boundary preservation
- `agents-manifest`: YAML schema for authored intent — marketplaces, plugin requests, isolation policy — and the guarantee that deleting the lock loses nothing

### Modified Capabilities
(none — existing `add/remove/update` global commands unchanged; new commands are additive)

## Impact

- **CLI** — new shorthand `uze <plugin>@<marketplace>`, `uze install`, `uze remove` disambiguation
- **Core** — `project_lock` splits into a manifest module (authored, comment-preserving) and a lock module (derived); `project_root` and `workspace` anchor on `agents.yaml`; error variants
- **Formats out of scope** — `plugin.json`, `marketplace.json`, `hooks.json` and `mcp.json` stay JSON (rationale in ADR-017 §10)
- **Application** — new `project_environment` use cases (plan/add/remove/install)
- **TUI** — future `Installed/Used` toggle reads `project_environment()` API (same use case as CLI)
- **Store/Engine/Integration** — unchanged (vendor neutrality preserved)
- **Dependencies** — `noyalib` (YAML serialization, replacing deprecated `serde_yaml`)
- **Docs** — ADR-016 (Project Agent Environment, including the `agents.lock` schema)
