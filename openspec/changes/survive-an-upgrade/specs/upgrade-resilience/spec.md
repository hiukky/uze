# upgrade-resilience Specification

## ADDED Requirements

### Requirement: Every document UZE owns declares its version, read before the document

Every file UZE writes for itself SHALL carry a schema version, and the
system SHALL read that version out of a shape every version of the file
shares, *before* reading the document. A document from a version this
build does not know SHALL be reported as that, never as a parse failure.

#### Scenario: A document from an older schema

- **WHEN** UZE reads a document whose declared version is not the one this
  build writes
- **THEN** it SHALL report an unsupported version, naming the version
  found and the version expected
- **AND THEN** it SHALL NOT report a field-level parse error about a file
  the operator never wrote

#### Scenario: Bytes that are not the document at all

- **WHEN** the file cannot be parsed far enough to find a version
- **THEN** UZE SHALL treat it as unreadable by the rule its class carries
  below, not as a document of the current version

#### Scenario: A document written before its kind declared a version

- **WHEN** a document carries no version field at all
- **THEN** UZE SHALL read it as version 1 of its kind
- **AND THEN** it SHALL NOT be reported unreadable for that reason alone

### Requirement: Only a version this build is ahead of may be set aside

Recovery moves in one direction. A document from a version older than this
build's is this build's to set aside and record again; a document from a
version *newer* than this build's SHALL be left exactly as it is, and the
operation that needed it SHALL be refused with that reason.

Two builds on one machine is an ordinary state — a release beside a
development build — and a rule that set aside whatever it could not read
would have them take turns destroying each other's records, each saying
it had recovered.

#### Scenario: An older build meets a newer build's document

- **WHEN** a build reads a document whose version is newer than its own
- **THEN** the document SHALL NOT be moved, rewritten or emptied
- **AND THEN** the operation SHALL be refused, naming the version found
  and the version this build writes

#### Scenario: A newer build meets an older build's document

- **WHEN** a build reads a document whose version is older than its own
- **THEN** the rule for the document's class applies: a rebuildable one is
  set aside and recorded again, an irreplaceable one refuses

### Requirement: Rebuildable state is set aside, never a dead end

State UZE can reconstruct from the machine — the record of a project's
agents, a client's remembered layout, a cache — SHALL never stop the
operator from working. When this build cannot read it, UZE SHALL move the
bytes aside under a name that is no longer that document, start the record
again from what is on disk, and say so once.

#### Scenario: A task document this build cannot read

- **WHEN** an operation reads a project's task document and cannot
  understand it
- **THEN** the bytes SHALL be kept beside the document, under a name
  nothing reads as one
- **AND THEN** the operation SHALL proceed against an empty record
- **AND THEN** the checkouts the repository still registers SHALL be
  adopted into it
- **AND THEN** the operator SHALL be told once, with the path the bytes
  were moved to

#### Scenario: The same document is unreadable on a read-only path

- **WHEN** a surface that only reads the document meets one it cannot
  understand
- **THEN** it SHALL NOT move or rewrite the document
- **AND THEN** it SHALL report that the document could not be read, rather
  than drawing the project as having no agents

### Requirement: Irreplaceable state is never destroyed to make an operation succeed

State UZE cannot reconstruct — a receipt for a managed artifact, a ledger
proving ownership — SHALL block the operation that depends on it and name
the remedy. UZE SHALL NOT set it aside, overwrite it, or guess at it.

#### Scenario: An unreadable ownership ledger

- **WHEN** a destructive operation needs a ledger this build cannot read
- **THEN** the operation SHALL be refused
- **AND THEN** the refusal SHALL name what could not be read and what the
  operator can do about it

### Requirement: What UZE owns stays addressable when the rules that locate it change

A resource whose location is computed — a socket, a generated directory, a
cache path — SHALL be reachable through something whose own location does
not depend on those rules, so that a build which computes a different
location can still find, report, and end what a previous build left
running.

#### Scenario: A server at an endpoint this build does not compute

- **WHEN** a live server that recorded itself holds a workspace at an
  endpoint named by rules this build no longer uses
- **THEN** `uze terminal stop` SHALL end that server
- **AND THEN** opening UZE SHALL replace it with one that answers where
  this build looks, restoring the spaces and panes it was serving
- **AND THEN** neither SHALL require the operator to find a process or
  restart the machine

#### Scenario: The resource predates the anchor that would name it

- **WHEN** what holds the resource was left by a version that recorded
  nothing for this build to act on
- **THEN** UZE SHALL report the situation, the resource it looked for and
  how to find what holds it
- **AND THEN** it SHALL NOT report the operation as having succeeded

### Requirement: An upgrade failure names what happened and what to do

Where UZE cannot recover on its own, the operator SHALL be told which
state was involved, what UZE did about it, and the one action that
resolves it. A bare operating-system error about a path the operator never
typed SHALL NOT be the whole message.

#### Scenario: Something holds a resource UZE cannot reach

- **WHEN** an operation fails because of state or a process left by
  another version
- **THEN** the message SHALL name the artifact or process involved
- **AND THEN** it SHALL name the command that resolves it

### Requirement: Upgrade leftovers are reported before they are hit

`uze doctor` SHALL report the state a previous version left that this
build did not adopt: documents set aside, state from an unknown schema,
and a server holding a workspace at an endpoint this build cannot reach —
each with its remedy.

#### Scenario: A document was set aside earlier

- **WHEN** `uze doctor` runs on a machine where a document was set aside
- **THEN** it SHALL name the document, when it was set aside, and that
  nothing reads it
- **AND THEN** it SHALL say what was rebuilt in its place

### Requirement: A release proves its own upgrade against the previous one

The suite SHALL include a scenario per class of state in which the
*previously released* UZE creates the state and this build is then run
against it, checked by what it leaves on the machine rather than by what
it reports.

#### Scenario: The previous release's workspace is opened by this build

- **WHEN** the last released binary has created a workspace, agents and
  their checkouts on a machine
- **AND WHEN** this build is opened on the same machine
- **THEN** the agents' branches and commits SHALL still be there
- **AND THEN** the operator SHALL be able to create an agent without any
  intervening step

#### Scenario: The previous release predates the recovery being proven

- **WHEN** the released binary is older than the mechanism a scenario
  needs — it records no claimant, or writes a document of a schema this
  build also writes
- **THEN** the scenario SHALL prove what this build *reports* in that
  state, which is what the operator meets
- **AND THEN** it SHALL NOT be written as though the recovery ran, and
  SHALL name the release from which the recovery itself becomes provable

#### Scenario: A new state document has no upgrade scenario

- **WHEN** a change introduces a document UZE persists for itself
- **THEN** the change SHALL be accompanied by the scenario that proves
  what this build does with the previous version of it
