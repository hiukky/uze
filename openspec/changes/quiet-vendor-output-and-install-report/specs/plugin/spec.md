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

### Requirement: Plugin install reports UZE-owned, compact output
The system SHALL render the install/add report itself, one line per harness (`route` + attachment
location when recorded), and SHALL NOT pass vendor CLI output (Codex/Claude/Gemini progress text) to the
user's terminal. Vendor command failure SHALL surface the vendor's own last words in the error. The
`--verbose` flag SHALL additionally show the delivery evidence for each harness.

#### Scenario: Compact default report
- **WHEN** user runs `uze plugin install flow@ai` and delivery to claude-code/codex/gemini succeeds
- **THEN** output shows `Installed plugin`, the Store path, and one compact line per harness — and no
  vendor CLI progress text

#### Scenario: Verbose report includes evidence
- **WHEN** user runs `uze plugin install flow@ai --verbose`
- **THEN** each harness line is followed by UZE's delivery evidence explaining the route

#### Scenario: Vendor failure is surfaced
- **WHEN** a vendor CLI command fails during attachment
- **THEN** the install fails with an error carrying the vendor command's exit status and the vendor's
  own last output lines, instead of a bare status
