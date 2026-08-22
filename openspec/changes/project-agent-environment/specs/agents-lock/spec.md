## Purpose

Defines `agents.lock`'s schema and durability guarantees: a versioned,
deterministic, atomically-written YAML file that is the single source of
truth `install_project_environment` reproduces an environment from.

## ADDED Requirements

### Requirement: Schema version
The lock SHALL carry an explicit `version` field, currently `1`, and
parsing SHALL reject any other value rather than guess a compatible
shape.

#### Scenario: Supported version is accepted
- **WHEN** a lock with `version: 1` is parsed
- **THEN** parsing succeeds

#### Scenario: Unsupported version is rejected
- **WHEN** a lock with `version: 99` is parsed
- **THEN** parsing fails with an error naming both the found and expected
  version, and the lock is not treated as valid

### Requirement: YAML format
The lock SHALL be serialized as YAML.

#### Scenario: Lock round-trips through YAML
- **WHEN** a lock is written to disk and then read back
- **THEN** the read-back value is equal to the original

### Requirement: Deterministic serialization
Writing the same logical lock content twice SHALL produce byte-identical
output.

#### Scenario: Repeated writes with no change are byte-identical
- **WHEN** `uze flow@ai` is run twice with nothing changing between runs
- **THEN** `agents.lock`'s bytes are identical after the second run

### Requirement: Marketplace source types
A locked marketplace's source SHALL be one of `git`, `path`, or
`embedded`, mirroring the acquisition mechanisms the rest of UZE
supports.

#### Scenario: Each source type parses to its own variant
- **WHEN** a lock declares a marketplace with `type: git`, `type: path`,
  or `type: embedded`
- **THEN** it parses into the corresponding source variant, carrying that
  variant's own fields (`url`/`reference`/`subdirectory` for `git`,
  `path` for `path`, `id` for `embedded`)

### Requirement: Resolved revision reflects what was actually acquired
A locked marketplace's or plugin's `resolved.revision` SHALL be derived
from what acquisition actually observed, not fabricated — present for a
Git commit or the embedded snapshot, absent for a local path (which has
no stable revision to pin).

#### Scenario: Git source records a commit
- **WHEN** a marketplace or plugin is acquired from a Git source
- **THEN** `resolved.revision` holds that acquisition's resolved commit

#### Scenario: Embedded source records its fixed identity
- **WHEN** a marketplace or plugin is acquired from the embedded snapshot
- **THEN** `resolved.revision` holds the literal `embedded`

#### Scenario: Local source records no revision
- **WHEN** a marketplace or plugin is acquired from a local path
- **THEN** `resolved.revision` is absent, not a fabricated value

### Requirement: Plugin source types
A locked plugin's source SHALL be either `marketplace` (a named
marketplace plus a plugin name within it) or `git` (a direct repository
reference).

#### Scenario: Marketplace-sourced plugin
- **WHEN** a lock declares a plugin with `type: marketplace, marketplace:
  ai, plugin: flow`
- **THEN** the plugin resolves through marketplace `ai`'s manifest

### Requirement: Integrity field is reserved, not enforced
The lock format SHALL accept an optional `integrity` field on a plugin
entry without validating it — reserved for a future content hash, not
implemented today.

#### Scenario: Integrity field round-trips without validation
- **WHEN** a lock entry carries an `integrity` value
- **THEN** it is preserved through parse/serialize but does not affect
  whether install succeeds

### Requirement: Malformed lock handling
A lock that fails to parse SHALL be reported as an error, never silently
ignored and never overwritten by a subsequent write.

#### Scenario: Malformed YAML is rejected without data loss
- **WHEN** `agents.lock` contains invalid YAML
- **THEN** every read of it fails with a malformed-lock error naming the
  path and reason, and no code path overwrites the file in response

### Requirement: Non-UTF-8 lock is rejected
The lock file SHALL be valid UTF-8; a lock that isn't SHALL fail to parse
with a clear reason rather than a generic decode panic.

#### Scenario: Invalid UTF-8 is a malformed-lock error
- **WHEN** `agents.lock` contains bytes that are not valid UTF-8
- **THEN** parsing fails with a malformed-lock error stating the encoding
  problem

### Requirement: Atomic write
The lock SHALL be persisted atomically (temp file plus rename), matching
every other piece of durable UZE state.

#### Scenario: A write is never observed partially applied
- **WHEN** `agents.lock` is written
- **THEN** any concurrent reader observes either the previous complete
  content or the new complete content, never a partial file

### Requirement: Marketplace source conflict is rejected
Adding a plugin under a marketplace name already locked to a different
source SHALL fail rather than silently repoint the lock.

#### Scenario: Same name, different source, is rejected
- **WHEN** `add_project_plugin` is called for a marketplace name already
  present in the lock with a different source than what's being added
- **THEN** the call fails, naming both the lock's existing source and the
  newly requested one, and the lock is not modified

### Requirement: Plugin marketplace mismatch is rejected
Adding a plugin already locked under a different marketplace SHALL fail
rather than silently move it.

#### Scenario: Same plugin, different marketplace, is rejected
- **WHEN** `add_project_plugin("flow", "other")` is called and the lock
  already has `flow` sourced from a different marketplace
- **THEN** the call fails, naming the expected and found marketplace, and
  the lock is not modified

### Requirement: Empty lock is valid
A lock declaring no marketplaces and no plugins SHALL be valid — the
state before anything has been added.

#### Scenario: A lock with only a version parses successfully
- **WHEN** a lock file contains only `version: 1`
- **THEN** it parses successfully into an entry with no marketplaces or
  plugins

### Requirement: Deterministic key ordering
Marketplace and plugin entries SHALL serialize in a deterministic
(alphabetical) key order, independent of insertion order.

#### Scenario: Multiple entries serialize in sorted order
- **WHEN** a lock contains marketplaces `{local, ai}` and plugins `{uze,
  flow}` (inserted in that order)
- **THEN** the serialized file lists `ai` before `local` and `flow`
  before `uze`
