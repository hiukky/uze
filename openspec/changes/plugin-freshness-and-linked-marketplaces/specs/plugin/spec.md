## MODIFIED Requirements

### Requirement: Plugin list shows marketplace and update availability
The system SHALL list installed plugins with their marketplace and their
freshness state, and `plugin list` SHALL show the marketplace column.
Freshness SHALL be one of **up to date**, **behind**, **linked** or **not
checked** (see the `plugin-freshness` capability); no version number SHALL
be shown, because nothing in the chain carries one.

#### Scenario: List plugins
- **WHEN** user runs `uze plugin list`
- **THEN** output includes `name`, `marketplace` (e.g., `ai`,
  `uze-official`), and `freshness`

#### Scenario: An uncompared plugin is not reported as current
- **WHEN** an installed plugin's freshness has never been established
- **THEN** `uze plugin list` reports it as **not checked**, distinct from
  a plugin reported as **up to date**

### Requirement: Plugin update and remove use marketplace-resolved source
The system SHALL update a marketplace-installed plugin by re-resolving its
marketplace entry and running the existing update pipeline, and remove SHALL
detach via native projection then delete Store bytes, respecting ADR-009.

An update SHALL be refused, with the installed revision left in place, when
the revision it would install introduces an executable capability the
installed one did not have and the operator has not authorized it.

#### Scenario: Update plugin
- **WHEN** user runs `uze plugin update flow@ai` and the marketplace's plugin has a newer commit
- **THEN** system re-acquires and replaces the Store package

#### Scenario: Remove plugin
- **WHEN** user runs `uze plugin remove flow`
- **THEN** system detaches via `Integration` (native or capability) per ADR-009 and removes Store bytes; `marketplace remove ai` remains blocked while plugins from `ai` are installed

#### Scenario: Update introducing execution is refused without authorization
- **WHEN** the newer revision of `flow@ai` declares a hook handler or MCP
  server command the installed revision did not, and the operator has not
  authorized it
- **THEN** the installed revision stays in place and the reason is reported
