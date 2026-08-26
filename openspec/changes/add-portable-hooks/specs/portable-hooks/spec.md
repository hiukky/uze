## ADDED Requirements

### Requirement: UZE discovers a canonical Hook manifest
The system SHALL discover a package-root `hooks.json` as portable Hook
resources without changing its bytes in the Store. The manifest SHALL accept
only the documented portable command-hook schema and SHALL report malformed,
duplicate, or unsafe declarations before attachment.

#### Scenario: One authored manifest creates independent hook resources
- **WHEN** a package contains one valid `hooks.json` with two declared hook groups
- **THEN** the Engine composes two stable Hook resources in manifest order
- **AND** each resource identity remains stable across a reinstall

#### Scenario: Invalid declaration is not silently projected
- **WHEN** a manifest contains an unknown event, malformed matcher, unsupported handler type, or timeout outside the allowed range
- **THEN** UZE reports a validation error and creates no Hook attachment receipt

### Requirement: UZE calculates Hook compatibility semantically
The system SHALL calculate compatibility from event, effect, matcher,
available data, transformation, handler type, and ordering, not merely from
matching vendor event names. It SHALL report `native`, `adapted`, `degraded`,
or `unsupported` with a reason and produced artifacts.

#### Scenario: Stop has no false OpenCode equivalence
- **WHEN** a portable Stop hook is assessed for OpenCode
- **THEN** the result is degraded or unsupported with an explanation
- **AND** UZE SHALL NOT claim that a session or tool callback is a native Stop hook

### Requirement: Command hooks use one portable ABI
The system SHALL normalize command-hook input to JSON stdin and parse an
optional JSON stdout decision. It SHALL support observation, allow, deny,
ask, and safe input replacement where the target supports them; it SHALL
document and diagnose any target that cannot preserve an effect.

#### Scenario: First deny wins in deterministic order
- **WHEN** multiple matching pre-tool command handlers are declared
- **THEN** adapters execute them in manifest order
- **AND** a deny stops later handlers and the intercepted operation where the target supports blocking

### Requirement: Managed Hook delivery preserves user configuration
The system SHALL add only receipt-owned Hook artifacts and configuration
entries. Reconciliation SHALL inspect the exact managed content and removal
SHALL refuse drift/conflicts and preserve unrelated user hooks/plugins.

#### Scenario: Existing OpenCode plugins survive bridge delivery
- **WHEN** OpenCode configuration already contains a foreign plugin entry
- **THEN** UZE adds its distinct generated bridge entry without reordering or replacing the foreign entry
- **AND** removal deletes only the matching UZE bridge entry
