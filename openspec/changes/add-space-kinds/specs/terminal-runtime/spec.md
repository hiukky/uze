## ADDED Requirements

### Requirement: A space carries a kind the server keeps and never reads
The terminal runtime SHALL accept a kind when a space is created, SHALL
persist it with the space, SHALL restore it with the space, and SHALL
report it to clients as part of the space. The runtime SHALL NOT interpret
the kind: nothing it does with panes, tabs or processes depends on it.
When asked to open a space for a root, the runtime SHALL reuse an existing
space only when both root and kind match, and SHALL create a new space
otherwise.

#### Scenario: The kind round-trips through a restart
- **WHEN** a space is created with a kind and the server is restarted
- **THEN** the restored space reports the same kind

#### Scenario: Same root, different kind, different space
- **WHEN** a client opens a space for a root that already has a space of the other kind
- **THEN** a new space is created and the existing one is untouched

#### Scenario: Same root, same kind, same space
- **WHEN** a client opens a space for a root that already has a space of that kind
- **THEN** the existing space is selected and none is created
