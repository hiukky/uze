# plugin Specification

## Purpose
Install, update, remove, and list plugins via `name@marketplace` through the existing acquisition → Store → native projection pipeline, keeping `uze add <path>` as a direct-source shortcut.
## Requirements
### Requirement: Plugin install resolves marketplace entry and converges into acquisition
The system SHALL resolve `plugin install <name>@<marketplace>` by looking up the marketplace registry, reading its `marketplace.json` to find the plugin entry's `source`, and then running the existing `PackageSource` acquisition (Git/Local) → `Store` → `native projection` pipeline. The Store SHALL remain unaware of marketplace.

#### Scenario: Install from local marketplace
- **WHEN** user runs `uze plugin install flow@ai` and `ai` was added via local path
- **THEN** system resolves `flow → ./plugins/flow`, materializes it, installs to Store, and attaches via native projection where applicable

#### Scenario: Install from Git marketplace
- **WHEN** user runs `uze plugin install flow@ai` and `ai` is a Git marketplace
- **THEN** system clones/marketplace-resolves the plugin subdirectory and installs it identically to a direct `uze add` of that subdirectory

### Requirement: Plugin install is idempotent and shows marketplace provenance
The system SHALL record installed plugins with their `{name, marketplace}` provenance and make `plugin install` idempotent.

#### Scenario: Re-install same plugin
- **WHEN** `flow@ai` is already installed and user runs `uze plugin install flow@ai` again
- **THEN** system reports already installed without re-cloning or duplicating

### Requirement: Plugin list shows marketplace and update availability
The system SHALL list installed plugins with their marketplace and update availability, and `plugin list` SHALL show the marketplace column.

#### Scenario: List plugins
- **WHEN** user runs `uze plugin list`
- **THEN** output includes `name`, `marketplace` (e.g., `ai`, `uze-official`), `version`, and `update_available`

### Requirement: Plugin update and remove use marketplace-resolved source
The system SHALL update a marketplace-installed plugin by re-resolving its marketplace entry and running the existing update pipeline, and remove SHALL detach via native projection then delete Store bytes, respecting ADR-009.

#### Scenario: Update plugin
- **WHEN** user runs `uze plugin update flow@ai` and the marketplace's plugin has a newer commit
- **THEN** system re-acquires and replaces the Store package

#### Scenario: Remove plugin
- **WHEN** user runs `uze plugin remove flow`
- **THEN** system detaches via `Integration` (native or capability) per ADR-009 and removes Store bytes; `marketplace remove ai` remains blocked while plugins from `ai` are installed

### Requirement: Direct add remains shortcut
The system SHALL keep `uze add <path|git>` as a direct PackageSource install without requiring a marketplace entry, for single-plugin sources.

#### Scenario: Direct add
- **WHEN** user runs `uze add ./plugins/flow --trust`
- **THEN** system installs `flow` directly, with `marketplace` shown as `-` or `direct` in `plugin list`

