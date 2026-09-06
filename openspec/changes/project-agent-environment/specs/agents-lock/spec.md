## Purpose

Defines `agents.lock`'s schema and durability guarantees: a versioned,
deterministic, atomically-written YAML file recording what resolving
`agents.yaml` actually produced, so `install_project_environment`
reproduces that exact environment on another machine without resolving
anything again.

The lock is derived. Intent lives in `agents-manifest`; nothing a person
decides is stored only here.

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

### Requirement: A locked plugin pins the bytes it resolved to
Every plugin entry SHALL carry an `integrity` value: a content hash over
the bytes acquisition ingested into the Store, written when the entry is
resolved. A revision names where bytes came from; the hash is what makes
the lock verifiable rather than merely descriptive.

#### Scenario: Resolution records the hash of what was ingested
- **WHEN** a plugin is resolved and ingested
- **THEN** its lock entry carries an `integrity` value derived from the
  ingested bytes

#### Scenario: Installing bytes that do not match the pin fails
- **WHEN** `install_project_environment` acquires a plugin whose bytes
  hash to a value other than the entry's `integrity`
- **THEN** the install fails naming both hashes, and nothing is delivered
  to any harness

#### Scenario: A source with no stable bytes records no pin
- **WHEN** a plugin resolves from a local path
- **THEN** `integrity` is absent rather than fabricated, and the entry is
  reported as non-reproducible

### Requirement: A malformed lock is repaired by regeneration, not by hand
A lock that fails to parse SHALL be reported, naming the path and the
reason, and SHALL be treated as regenerable rather than as data to
protect: it holds no intent, so resolving `agents.yaml` again replaces
it. No command SHALL ask a person to repair a lock by editing it, and no
command SHALL refuse to proceed solely because the existing lock is
unreadable.

This replaces the original "never overwritten by a subsequent write"
rule, which was right while the lock was the only declaration a project
had and wrong once it became derived.

#### Scenario: Malformed YAML is reported and regenerable
- **WHEN** `agents.lock` contains invalid YAML
- **THEN** reading it fails with a malformed-lock error naming the path
  and reason
- **AND** resolving the manifest again replaces the file without asking

#### Scenario: A malformed lock never reproduces an environment
- **WHEN** `install_project_environment` finds an unparseable lock
- **THEN** it installs nothing from it, rather than applying the part
  that happened to parse

#### Scenario: A hand-edited lock is replaced without ceremony
- **WHEN** a person edits `agents.lock` and the environment is resolved
  again
- **THEN** the edit is replaced by what resolution produced, reported as
  a regenerated file rather than as drift to reconcile

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
Adding a plugin under a marketplace name already declared with a
different source SHALL fail rather than silently repoint the manifest.

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

### Requirement: No lock until something resolves
A lock SHALL exist only once the project has a marketplace or a plugin to
resolve. Declaring only an isolation policy SHALL produce no lock, and a
lock left holding no entries SHALL be removed rather than written empty —
there is nothing to reproduce, and an empty lock invites the belief that
resolution happened.

#### Scenario: A policy-only project has no lock
- **WHEN** a manifest declares an isolation policy and no marketplaces or
  plugins
- **THEN** no `agents.lock` is written

#### Scenario: Removing the last plugin removes the lock
- **WHEN** the last plugin is removed from a project's manifest
- **THEN** `agents.lock` is deleted rather than left declaring nothing

### Requirement: Deterministic key ordering
Marketplace and plugin entries SHALL serialize in a deterministic
(alphabetical) key order, independent of insertion order.

#### Scenario: Multiple entries serialize in sorted order
- **WHEN** a lock contains marketplaces `{local, ai}` and plugins `{uze,
  flow}` (inserted in that order)
- **THEN** the serialized file lists `ai` before `local` and `flow`
  before `uze`

### Requirement: The lock is derived and rebuildable
The lock SHALL be a derived artifact: deleting it and resolving
`agents.yaml` again SHALL produce an equivalent lock, and no command
SHALL ask a person to edit it. Its header SHALL say so.

#### Scenario: A deleted lock is regenerated equivalently
- **WHEN** `agents.lock` is deleted and the environment is resolved again
  from an unchanged `agents.yaml` against unchanged sources
- **THEN** the regenerated lock is byte-identical to the deleted one

#### Scenario: The lock states that it is generated
- **WHEN** `agents.lock` is written
- **THEN** its first line is a comment naming `agents.yaml` as the file to
  edit instead

### Requirement: The lock carries no intent
The lock SHALL record only resolution. It SHALL NOT carry the worktree
isolation policy, which is a declaration and belongs to `agents.yaml`.
A key the lock once carried and no longer may SHALL be rejected by name,
saying where the declaration lives now — never dropped silently, which
would turn a declared policy into no policy with nothing said.

#### Scenario: A policy block in the lock is rejected
- **WHEN** `agents.lock` carries a `worktrees` block
- **THEN** the lock is reported as malformed, naming `agents.yaml` as the
  file that declares the policy
- **AND** no environment is loaded from it

#### Scenario: The superseded directory key is still rejected by name
- **WHEN** `agents.lock` carries the retired `worktrees_dir` key
- **THEN** the lock is reported as malformed, naming the key and
  `agents.yaml`

#### Scenario: An unknown key at the top level still loads
- **WHEN** `agents.lock` carries a key from a newer UZE that this version
  does not know
- **THEN** the lock still loads, so a newer lock is readable by an older
  binary

### Requirement: The lock records what the manifest asked for
Each entry SHALL carry the request it satisfies — the manifest
declaration it was resolved from — so staleness is decidable by comparing
the two files, with no network access and no re-resolution.

#### Scenario: A changed manifest is detected as stale offline
- **WHEN** `agents.yaml` changes a plugin's requested reference and the
  environment is inspected with no network available
- **THEN** the lock is reported as stale for that entry, naming the
  requested and locked values

#### Scenario: An unchanged manifest is not stale
- **WHEN** `agents.yaml` is unchanged since the lock was written
- **THEN** inspection reports the lock as current without resolving
  anything
