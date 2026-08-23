## MODIFIED Requirements

### Requirement: Plugin install resolves marketplace entry and converges into acquisition
The system SHALL resolve `plugin install <name>@<marketplace>` by looking up the marketplace registry,
reading its `agents.json` to find the plugin entry's `source`, and then running the existing
`PackageSource` acquisition (Git/Local) → `Store` → `native projection` pipeline. The Store SHALL remain
unaware of marketplace.

#### Scenario: Install from local marketplace
- **WHEN** user runs `uze plugin install flow@ai` and `ai` was added via local path
- **THEN** system resolves `flow → ./plugins/flow`, materializes it, installs to Store, and attaches via
  native projection where applicable

#### Scenario: Install from Git marketplace
- **WHEN** user runs `uze plugin install flow@ai` and `ai` is a Git marketplace
- **THEN** system clones/marketplace-resolves the plugin subdirectory and installs it identically to a
  direct `uze add` of that subdirectory
