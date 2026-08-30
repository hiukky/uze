# overview Specification

## Purpose
TBD - created by archiving change overview-workspace-semantic-health. Update Purpose after archive.
## Requirements
### Requirement: Overview reports semantic project state
The system SHALL report the workspace project half as semantic states, not file facts: `Environment` ∈ {NotConfigured, Invalid, InstallRequired, Ready}, `Memory` ∈ {None, Ready, Issue}, and a `Plugins` quantity. The state SHALL be computed by the Application layer; the TUI SHALL render it verbatim and never re-derive it from `agents.lock` bytes.

#### Scenario: Plain directory
- **WHEN** the cwd contains neither `agents.lock` nor `marketplace.json`
- **THEN** Environment is `NotConfigured`, Memory is `None`, Plugins shows none, and no MARKETPLACE column is rendered

#### Scenario: AGENTS.md only
- **WHEN** the cwd contains `AGENTS.md` and no anchors
- **THEN** Environment is `NotConfigured` and Memory is `Ready`

#### Scenario: Valid lock, everything installed
- **WHEN** `agents.lock` parses and every declared plugin is installed in the Store
- **THEN** Environment is `Ready` and Plugins shows the installed quantity with no warning indicator

#### Scenario: Valid lock, nothing installed
- **WHEN** `agents.lock` parses and no declared plugin is installed
- **THEN** Environment is `InstallRequired` and Plugins shows a `! 0/N installed` divergence

#### Scenario: Valid lock, partially installed
- **WHEN** `agents.lock` parses and some declared plugins are missing
- **THEN** Environment is `InstallRequired` and only the missing plugins are reported as missing

#### Scenario: Malformed lock
- **WHEN** `agents.lock` exists but cannot be parsed (malformed or unsupported version)
- **THEN** Environment is `Invalid`, Plugins is unknown, and `Ready` is never emitted

#### Scenario: Ready is never emitted with a known unsatisfied requirement
- **WHEN** any declared plugin is missing from the Store
- **THEN** Environment SHALL NOT be `Ready`

### Requirement: Overview reports marketplace state
The system SHALL report a marketplace workspace as `Name` / `Plugins` / `Status`, where Status is `valid`, `N invalid` (declared sources missing/escaping), or `invalid manifest`. Marketplace health SHALL NOT depend on whether its packages are installed globally.

#### Scenario: Valid marketplace
- **WHEN** `marketplace.json` parses and every declared source directory exists
- **THEN** Status is `valid` and the marketplace name and package count are reported

#### Scenario: Marketplace with missing package sources
- **WHEN** `marketplace.json` parses but a declared package's source directory is missing
- **THEN** Status reports the invalid count without declaring the manifest invalid

#### Scenario: Invalid manifest
- **WHEN** `marketplace.json` is missing, malformed, or structurally invalid
- **THEN** Status is `invalid manifest` and the marketplace name is unknown

### Requirement: Overview mixes project and marketplace contextually
The system SHALL show PROJECT and/or MARKETPLACE blocks based on the workspace kind: consumer → PROJECT only, marketplace → MARKETPLACE only, both anchors → both blocks, neither → the PROJECT three-row empty state. The blocks SHALL always render vertically (PROJECT above MARKETPLACE), never side-by-side. The empty state SHALL be compact (no verbose onboarding).

#### Scenario: Hybrid workspace
- **WHEN** both `agents.lock` and `marketplace.json` exist at the workspace root
- **THEN** both blocks are rendered, each with its own honest state, PROJECT above MARKETPLACE

#### Scenario: Marketplace-only workspace hides PROJECT
- **WHEN** only `marketplace.json` exists
- **THEN** no PROJECT column is rendered

#### Scenario: Nested workspace nearest anchor wins
- **WHEN** the cwd is inside a nested consumer under a marketplace root
- **THEN** the consumer state is reported for the nearest anchor

### Requirement: Overview indicators have one meaning
The system SHALL use `✓` only for a verified healthy condition, `!` for
attention/actionable state, `×` for error/invalid state and `—` for
absent/not applicable/not configured. Quantities SHALL NOT carry a check
mark; color divergence instead. `Environment ready` SHALL only be shown
when the Application reports `Ready`.

#### Scenario: Quantity without check
- **WHEN** all declared plugins are installed
- **THEN** Plugins renders as `N installed` without a `✓`

#### Scenario: Divergent quantity is actionable
- **WHEN** some declared plugins are missing
- **THEN** Plugins renders with `!` and the divergence, and the Overview offers `i install`

#### Scenario: Invalid state is never actionable as install
- **WHEN** Environment is `Invalid` or `NotConfigured`
- **THEN** no `i install` action is offered

