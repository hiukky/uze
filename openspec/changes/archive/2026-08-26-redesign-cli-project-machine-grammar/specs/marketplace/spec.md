## MODIFIED Requirements

### Requirement: Marketplace registry stores generic Git/local sources
The system SHALL store marketplace entries as `{name, source: Git|Local}` in `~/.uze/state/marketplaces.json`, where Git is a generic URL (not GitHub-specific) and Local is a filesystem path. The registry SHALL NOT copy plugin bytes. The CLI verb for this is `uze market` (previously `uze marketplace`); no domain name, state filename, or internal type changes.

#### Scenario: Add local marketplace
- **WHEN** user runs `uze market add /home/hiukky/ai`
- **THEN** system records `ai → Local{path:/home/hiukky/ai}` and `marketplace.json` is readable

#### Scenario: Add Git marketplace
- **WHEN** user runs `uze market add https://github.com/hiukky/ai`
- **THEN** system records `ai → Git{url:https://github.com/hiukky/ai}` without cloning plugins

### Requirement: Marketplace add validates marketplace manifest
The system SHALL validate that the marketplace source contains a readable `marketplace.json` with `plugins[]` entries (`name`, `source`).

#### Scenario: Valid marketplace
- **WHEN** `marketplace.json` exists and is well-formed
- **THEN** `uze market add` succeeds

#### Scenario: Invalid marketplace
- **WHEN** `marketplace.json` is missing or malformed
- **THEN** `uze market add` fails with a clear error and records nothing

### Requirement: Marketplace list and remove manage registry
The system SHALL list registered marketplaces and remove a marketplace only when no installed plugin still references it (or with explicit force handling).

#### Scenario: List marketplaces
- **WHEN** user runs `uze market list`
- **THEN** system shows `name`, `source` (path/URL), and plugin count

#### Scenario: Remove marketplace
- **WHEN** user runs `uze market remove ai` and no plugin from `ai` is installed
- **THEN** registry entry is removed

### Requirement: Embedded official marketplace is pre-registered
The system SHALL treat the embedded official marketplace (`plugins/uze`, `marketplace.json` at repo root) as a pre-registered marketplace `uze-official` without requiring `market add`.

#### Scenario: Official marketplace available
- **WHEN** system starts with no registry
- **THEN** `uze market list` shows `uze-official` and `uze plugin list` can resolve `uze@uze-official`

## ADDED Requirements

### Requirement: Marketplace-level inspect shows one marketplace's own detail
The system SHALL provide `uze market inspect <name>`, showing that single marketplace's source, resolved
plugin count, and (for `uze-official`) that it is embedded — distinct from inspecting one plugin within a
marketplace, which remains reachable through `uze plugin inspect <plugin>`.

#### Scenario: Inspect a registered marketplace
- **WHEN** the user runs `uze market inspect ai` and `ai` is registered
- **THEN** the output shows `ai`'s source (path or URL) and plugin count

#### Scenario: Inspect an unknown marketplace
- **WHEN** the user runs `uze market inspect nope` and no marketplace named `nope` is registered
- **THEN** the command fails with an error naming the missing marketplace, distinct from a plugin-not-found
  error

### Requirement: The registry manifest filename and CLI verb are independent concerns
Renaming the CLI verb from `marketplace` to `market` SHALL NOT imply, require, or silently trigger any
change to the `marketplace.json` manifest filename a marketplace root is expected to contain, nor to the
unrelated, vendor-owned `.claude-plugin/marketplace.json` / `.agents/plugins/marketplace.json` catalogues
Claude and Codex integrations generate for their own native plugin systems.

#### Scenario: `market add` still validates against `marketplace.json`
- **WHEN** the user runs `uze market add <source>`
- **THEN** the system still looks for and validates `marketplace.json` at that source's root, exactly as
  `uze marketplace add` did before the verb rename
