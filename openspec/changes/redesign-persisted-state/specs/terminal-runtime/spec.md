# terminal-runtime Specification

## ADDED Requirements

### Requirement: The workspace claim names the process holding it

The server serving a workspace SHALL record its own process id in the
claim it holds, so that a client which cannot reach the endpoint can still
say — and end — what holds the workspace. A reader SHALL treat the
recorded id as a lead and corroborate it against the process table before
acting on it; the lock, not the record, remains the proof that a server is
alive.

#### Scenario: A client cannot reach the endpoint

- **WHEN** a client finds the workspace claimed and nothing answering at
  the endpoint this build computes
- **THEN** it SHALL allow the server the time its endpoint watch needs to
  restore a socket that was taken from it
- **AND THEN** failing that, it SHALL end the recorded holder and start a
  server that answers where this build looks
- **AND THEN** the spaces and panes the previous server was serving SHALL
  be restored by the one that replaces it

#### Scenario: The claim names nobody this process can act on

- **WHEN** the claim is held but records no id, or records one the process
  table no longer vouches for
- **THEN** the client SHALL report that the workspace is served by
  something it cannot reach, naming the endpoint it looked at and the
  command that ends a server
- **AND THEN** it SHALL NOT report only the operating system's error about
  the endpoint path

### Requirement: Stopping the runtime stops what holds the workspace

`uze terminal stop` SHALL end the server holding the workspace, whether or
not that server answers at the endpoint this build computes. "Nothing
answers here" SHALL NOT be reported as "nothing is running" while the
workspace is claimed.

#### Scenario: The server recorded itself and is on another endpoint

- **WHEN** `uze terminal stop` runs while a server that recorded its own
  id holds the workspace at an endpoint this build does not name
- **THEN** that server SHALL be ended
- **AND THEN** the workspace SHALL be free for the next server

#### Scenario: The server predates the record

- **WHEN** the workspace is claimed, nothing answers at this build's
  endpoint, and the claim records nobody — a server from a release older
  than the record itself
- **THEN** `uze terminal stop` SHALL report that, naming the endpoint it
  looked at and how to find the process
- **AND THEN** it SHALL NOT report success, and SHALL signal nothing

#### Scenario: Nothing is running at all

- **WHEN** `uze terminal stop` runs with no server holding the workspace
  and no endpoint present
- **THEN** it SHALL succeed, having nothing to do

### Requirement: The endpoint lives beside the workspace it serves

The endpoint SHALL be named from the workspace's own location, so that
every terminal of one machine computes the same endpoint for one
`UZE_HOME` whatever their session environment says, and no directory a
system cleaner owns can take it while the workspace itself survives. Where
that path cannot hold a socket, UZE SHALL fall back to the session's
runtime directory and the system temporary directories in turn.

#### Scenario: Two terminals with different session environments

- **WHEN** two terminals of one machine have different values for the
  session's runtime directory, or one has none
- **THEN** both SHALL compute the same endpoint for the same `UZE_HOME`
- **AND THEN** the second SHALL attach to the server the first started

#### Scenario: A home too long for a socket path

- **WHEN** the workspace's own directory would exceed the length a socket
  path allows
- **THEN** UZE SHALL use the first fallback directory that can hold one
- **AND THEN** the endpoint SHALL still be one per `UZE_HOME`

### Requirement: A server is never started from a binary that is gone

Starting a server SHALL use an executable that exists. Where the running
image has been replaced on disk — an install over a live session — UZE
SHALL start the server from `uze` as the path resolves it, and where
neither exists it SHALL say so rather than reporting a missing file.

#### Scenario: The binary was replaced while the client ran

- **WHEN** a client needs to start a server after its own binary has been
  replaced on disk
- **THEN** the server SHALL be started from the installed `uze`
- **AND THEN** no error naming a deleted path SHALL reach the operator

### Requirement: A workspace written by an earlier build opens on this one

The persisted workspace SHALL be carried across to the shape this build
reads, keeping every space, its root, its tabs and each tab's directory
and launch. A field an earlier shape carried that this one dropped SHALL
be the only thing lost. Spaces SHALL NOT be lost because the workspace was
written by an earlier build whose shape this one knows.

#### Scenario: Upgrading with spaces open

- **WHEN** UZE is upgraded and the persisted workspace was written by an
  earlier shape this build knows
- **THEN** the same spaces SHALL open, with the same roots, tabs and
  directories
- **AND THEN** nothing SHALL be reported

#### Scenario: The shape that carried a kind per space

- **WHEN** the workspace was written when a space carried a kind of its
  own
- **THEN** every space SHALL open without it
- **AND THEN** nothing else about the workspace SHALL change

### Requirement: The runtime says what it could not carry across

When the terminal runtime cannot carry the persisted workspace across and
starts from nothing, it SHALL tell the client, and the client SHALL show
the operator what happened and where the previous workspace was kept.

Reporting it only where a log would have to be turned on SHALL NOT satisfy
this: the runtime and the screen are different processes, and the operator
is at the screen.

#### Scenario: A workspace that could not be read

- **WHEN** the runtime starts from nothing because the persisted workspace
  could not be read or carried across
- **THEN** the operator SHALL be told on screen
- **AND THEN** they SHALL be told where the previous workspace is kept

#### Scenario: A first run

- **WHEN** the runtime starts with no workspace persisted at all
- **THEN** nothing SHALL be reported: there was nothing to lose
