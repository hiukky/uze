## ADDED Requirements

### Requirement: A space is a root, and the runtime knows nothing else about it
The terminal runtime SHALL identify a space by its root alone. It SHALL
NOT carry, persist or report a kind for a space, and nothing it does with
panes, tabs or processes SHALL depend on one. When asked to open a space
for a root, the runtime SHALL reuse the existing space for that root, and
create one only when there is none.

A persisted workspace written by a version that recorded a kind per space
SHALL be read by the rule the `upgrade-resilience` capability states for a
document of an older version.

#### Scenario: A root names one space
- **WHEN** a client opens a space for a root that already has one
- **THEN** the existing space is selected and none is created

#### Scenario: The space round-trips through a restart
- **WHEN** a space is created and the server is restarted
- **THEN** the restored space is the same space, at the same root

#### Scenario: A workspace persisted by an older version
- **WHEN** the runtime restores a persisted workspace whose spaces carry a kind
- **THEN** it reads it as the older document it is, and the operator is told what was done about it
