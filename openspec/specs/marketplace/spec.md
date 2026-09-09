# marketplace Specification

## Purpose
Registry of marketplace discovery sources that maps a marketplace name to a generic Git/local source and resolves `marketplace.json` for plugin discovery.
## Requirements
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

### Requirement: A marketplace's catalogue is served from a cache
What a marketplace registered from a Git source offers SHALL be read from
a cached checkout under `UZE_HOME`'s cache directory rather than by
cloning the source: one checkout per marketplace, recording the source it
was read from and when. A catalogue SHALL stand for a bounded time, after
which the next read clones the source again; registering the same source
again SHALL refresh it immediately; removing the marketplace SHALL drop
it. A marketplace registered from a local path SHALL be read in place on
every read. The cache is reconstructable and never authoritative: deleting
it costs one clone, never correctness.

#### Scenario: A listing does not need the repository
- **WHEN** a marketplace was registered from a Git URL and its repository
  is later unreachable or gone
- **THEN** `market list`, `market inspect`, the plugin listing and
  inspecting one of its plugins still answer from the cached catalogue,
  with no clone attempted while the entry stands

#### Scenario: A catalogue is refreshed once per window, not once per screen
- **WHEN** a cached catalogue is older than its maximum age
- **THEN** the next read clones the source once and every read after it
  is served from the new checkout until it ages out in turn

#### Scenario: Registering the same source again refreshes the catalogue
- **WHEN** `market add` is run with a source already registered under the
  same name
- **THEN** the registry is unchanged and the cached catalogue is replaced
  by the clone that registration made

#### Scenario: A refill that fails answers with the last catalogue seen
- **WHEN** a cached catalogue has aged out and the source cannot be cloned
- **THEN** the read answers with the expired catalogue rather than nothing,
  and the refill is attempted again on the next read

#### Scenario: A local marketplace is never cached
- **WHEN** a marketplace was registered from a local path
- **THEN** every read parses its `marketplace.json` where it is, and no
  cache entry is written for it

#### Scenario: Removing a marketplace drops its catalogue
- **WHEN** `market remove` succeeds
- **THEN** the marketplace's cache entry is removed with its registration
