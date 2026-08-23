## MODIFIED Requirements

### Requirement: Marketplace root manifest is agents.json
The system SHALL treat `agents.json` at a marketplace source's root as the registry manifest contract
(`{name, plugins: [{name, source, description, keywords}]}`, `owner` optional, no duplicate plugin
names). The previous filename `marketplace.json` SHALL NOT be read or validated at a marketplace root.

#### Scenario: Add local marketplace
- **WHEN** user runs `uze market add /home/hiukky/ai`
- **THEN** system records `ai → Local{path:/home/hiukky/ai}` and `agents.json` is readable

#### Scenario: Add Git marketplace
- **WHEN** user runs `uze market add https://github.com/hiukky/ai`
- **THEN** system records `ai → Git{url:https://github.com/hiukky/ai}` without cloning plugins

### Requirement: Marketplace add validates marketplace manifest
The system SHALL validate that the marketplace source contains a readable `agents.json` with `plugins[]`
entries (`name`, `source`), with the same schema as before the rename.

#### Scenario: Valid marketplace
- **WHEN** `agents.json` exists and is well-formed
- **THEN** `market add` succeeds

#### Scenario: Invalid marketplace
- **WHEN** `agents.json` is missing or malformed
- **THEN** `market add` fails with a clear error naming `agents.json` and records nothing

### Requirement: Marketplace list and remove manage registry
The system SHALL list registered marketplaces and remove a marketplace only when no installed plugin still
references it (or with explicit force handling).

#### Scenario: List marketplaces
- **WHEN** user runs `uze market list`
- **THEN** system shows `name`, `source` (path/URL), and plugin count

#### Scenario: Remove marketplace
- **WHEN** user runs `uze market remove ai` and no plugin from `ai` is installed
- **THEN** registry entry is removed

### Requirement: Embedded official marketplace is pre-registered
The system SHALL treat the embedded official marketplace (`plugins/uze`, `agents.json` at repo root) as a
pre-registered marketplace `uze-official` without requiring `market add`.

#### Scenario: Official marketplace available
- **WHEN** system starts with no registry
- **THEN** `uze market list` shows `uze-official` and `uze plugin list` can resolve `uze@uze-official`

### Requirement: Manifest filename is agents.json, independent of CLI verb and vendor catalogues
The manifest filename and the CLI verb are independent concerns: renaming the CLI verb `marketplace` →
`market` (ADR-019) SHALL NOT imply a manifest change, and this change's manifest rename is its own
recorded decision. The vendor-owned `.claude-plugin/marketplace.json` / `.agents/plugins/marketplace.json`
catalogues Claude and Codex integrations generate SHALL remain named `marketplace.json`; UZE's rename
SHALL NOT touch them.

#### Scenario: `market add` validates against `agents.json`
- **WHEN** the user runs `uze market add <source>`
- **THEN** the system looks for and validates `agents.json` at that source's root

#### Scenario: Vendor catalogue filenames unchanged
- **WHEN** UZE republishes installed packages for Claude or Codex
- **THEN** the derived native catalogues remain `.claude-plugin/marketplace.json` /
  `.agents/plugins/marketplace.json`, unchanged

## ADDED Requirements

### Requirement: Rename provides no fallback alias for the old filename
The system SHALL NOT read `marketplace.json` at a marketplace root as a fallback when `agents.json` is
absent; the manifest filename is unambiguously `agents.json`.

#### Scenario: Old-filename root fails loudly
- **WHEN** the user runs `uze market add <source>` and only `marketplace.json` exists at the root
- **THEN** the command fails with an error naming the missing `agents.json` and records nothing
