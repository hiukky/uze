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

- **`agents.lock`** — new project-scoped file declaring desired agent environment (marketplaces + plugins with resolved sources)
- **`uze <plugin>@<marketplace>`** — new project shorthand that writes `agents.lock` (requires `@`)
- **`uze install`** — new command that consumes `agents.lock` to reconstruct environment on fresh machine
- **`uze remove <plugin>`** — disambiguated: removes from project lock if present, else delegates to global `remove_plugin`
- **Project root resolution** — deterministic walk upward for `agents.lock` > `AGENTS.md` > `.git`
- **Application API** — `project_environment()`, `plan_project_environment()`, `add_project_plugin()`, `remove_project_plugin()`, `install_project_environment()`
- **Error variants** — `UnsupportedLockVersion`, `MalformedLock`, `MarketplaceSourceConflict`, `MarketplaceMismatch`
- **Dependency** — `noyalib` (maintained YAML, replacing deprecated `serde_yaml`)

## Capabilities

### New Capabilities
- `project-agent-environment`: project-scoped desired state (`agents.lock`), global vs project separation, reproducible install
- `agents-lock`: YAML schema v1, deterministic serialization, reproducible source identity, trust boundary preservation

### Modified Capabilities
(none — existing `add/remove/update` global commands unchanged; new commands are additive)

## Impact

- **CLI** — new shorthand `uze <plugin>@<marketplace>`, `uze install`, `uze remove` disambiguation
- **Core** — new `project_lock` module (parser/serializer), `project_root` module (resolution), error variants
- **Application** — new `project_environment` use cases (plan/add/remove/install)
- **TUI** — future `Installed/Used` toggle reads `project_environment()` API (same use case as CLI)
- **Store/Engine/Integration** — unchanged (vendor neutrality preserved)
- **Dependencies** — `noyalib` (YAML serialization, replacing deprecated `serde_yaml`)
- **Docs** — ADR-016 (Project Agent Environment, including the `agents.lock` schema)
