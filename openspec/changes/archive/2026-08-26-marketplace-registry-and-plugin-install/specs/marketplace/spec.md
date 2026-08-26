## Purpose

Registry of marketplace discovery sources that maps a marketplace name to a generic Git/local source and resolves `marketplace.json` for plugin discovery.

## ADDED Requirements

### Requirement: Marketplace registry stores generic Git/local sources
The system SHALL store marketplace entries as `{name, source: Git|Local}` in `~/.uze/state/marketplaces.json`, where Git is a generic URL (not GitHub-specific) and Local is a filesystem path. The registry SHALL NOT copy plugin bytes.

#### Scenario: Add local marketplace
- **WHEN** user runs `uze marketplace add /home/hiukky/ai`
- **THEN** system records `ai → Local{path:/home/hiukky/ai}` and `marketplace.json` is readable

#### Scenario: Add Git marketplace
- **WHEN** user runs `uze marketplace add https://github.com/hiukky/ai`
- **THEN** system records `ai → Git{url:https://github.com/hiukky/ai}` without cloning plugins

### Requirement: Marketplace add validates marketplace manifest
The system SHALL validate that the marketplace source contains a readable `marketplace.json` with `plugins[]` entries (`name`, `source`).

#### Scenario: Valid marketplace
- **WHEN** `marketplace.json` exists and is well-formed
- **THEN** `marketplace add` succeeds

#### Scenario: Invalid marketplace
- **WHEN** `marketplace.json` is missing or malformed
- **THEN** `marketplace add` fails with a clear error and records nothing

### Requirement: Marketplace list and remove manage registry
The system SHALL list registered marketplaces and remove a marketplace only when no installed plugin still references it (or with explicit force handling).

#### Scenario: List marketplaces
- **WHEN** user runs `uze marketplace list`
- **THEN** system shows `name`, `source` (path/URL), and plugin count

#### Scenario: Remove marketplace
- **WHEN** user runs `uze marketplace remove ai` and no plugin from `ai` is installed
- **THEN** registry entry is removed

### Requirement: Embedded official marketplace is pre-registered
The system SHALL treat the embedded official marketplace (`plugins/uze`, `marketplace.json` at repo root) as a pre-registered marketplace `uze-official` without requiring `marketplace add`.

#### Scenario: Official marketplace available
- **WHEN** system starts with no registry
- **THEN** `uze marketplace list` shows `uze-official` and `uze plugin list` can resolve `uze@uze-official`
