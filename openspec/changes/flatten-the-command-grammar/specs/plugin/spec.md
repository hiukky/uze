## MODIFIED Requirements

### Requirement: Plugin install resolves marketplace entry and converges into acquisition
The system SHALL resolve `uze <name>@<marketplace>` by looking up the marketplace registry, reading its `marketplace.json` to find the plugin entry's `source`, and then running the existing `PackageSource` acquisition (Git/Local) → `Store` → `native projection` pipeline. The Store SHALL remain unaware of marketplace.

#### Scenario: Install from local marketplace
- **WHEN** user runs `uze flow@ai` and `ai` was added via local path
- **THEN** system resolves `flow → ./plugins/flow`, materializes it, installs to Store, and attaches via native projection where applicable

#### Scenario: Install from Git marketplace
- **WHEN** user runs `uze flow@ai` and `ai` is a Git marketplace
- **THEN** system clones/marketplace-resolves the plugin subdirectory and installs it identically to a direct `uze add` of that subdirectory

### Requirement: Plugin install is idempotent and shows marketplace provenance
The system SHALL record installed plugins with their `{name, marketplace}` provenance and make installing idempotent.

A bare plugin name already active from a different marketplace SHALL be resolvable at the root install form: `--alias <name>` installs alongside it, `--replace` takes the other one's name once it is safe to (ADR-036).

#### Scenario: Re-install same plugin
- **WHEN** `flow@ai` is already installed and user runs `uze flow@ai` again
- **THEN** system reports already installed without re-cloning or duplicating

#### Scenario: A name already taken by another marketplace
- **WHEN** `uze flow@other` runs and `flow` is already active from `ai`
- **THEN** it is refused unless `--alias` or `--replace` says which resolution is wanted

### Requirement: Direct add remains shortcut
The system SHALL keep `uze add <path|git>` as a direct PackageSource install without requiring a marketplace entry, for single-plugin sources. It follows the same scope rule as every root verb: it declares in the project when there is one, and `-m` confines it to this machine.

#### Scenario: Direct add
- **WHEN** user runs `uze add ./plugins/flow --trust`
- **THEN** system installs `flow` directly, with `marketplace` shown as `-` or `direct` in the plugin listing
