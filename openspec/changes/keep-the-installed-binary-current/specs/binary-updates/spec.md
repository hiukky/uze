## Purpose

How an installed `uze` learns that a newer release exists, replaces itself
when it is the binary the installer placed, and tells the operator what
happened — without ever replacing a binary something else installed.

## ADDED Requirements

### Requirement: Only the installer's binary is replaced

The installer SHALL record the file it placed and the release it was. UZE
SHALL replace a binary only when the running binary is that recorded file, and
SHALL NOT replace a binary running from any other path, whatever installed it.

#### Scenario: A binary the installer placed

- **WHEN** the running `uze` is the file the installer recorded and a newer
  release is published
- **THEN** that file is replaced with the newer release, and the record names
  the newer release

#### Scenario: A binary something else placed

- **WHEN** the running `uze` is not the file the installer recorded — or no
  record exists — and a newer release is published
- **THEN** no file is replaced, and the operator is told the newer release
  exists

### Requirement: A replacement is verified before it is placed

UZE SHALL verify a downloaded release against the published checksums, SHALL
run the binary it contains and confirm it reports the release it was
downloaded as, and SHALL leave the installed file untouched when either check
fails. The replacement SHALL be a single rename on the installed file's own
filesystem, so no process can start a partially written binary.

#### Scenario: A checksum that does not match

- **WHEN** the downloaded archive's digest differs from the published one
- **THEN** the installed binary is unchanged and no notice claims an update

#### Scenario: A binary that is not the release it claims

- **WHEN** the unpacked binary does not report the release it was downloaded as
- **THEN** the installed binary is unchanged

### Requirement: What is running is never interrupted by an update

Replacing the installed binary SHALL NOT stop, restart or signal any running
process. The new release SHALL first run at the next launch.

#### Scenario: An update lands with the workspace open

- **WHEN** the binary is replaced while the terminal workspace is open
- **THEN** the open workspace and every pane in it continue unaffected, and
  the sidebar says the update is installed and takes effect on restart

### Requirement: The workspace sidebars announce releases

Both sidebars SHALL show, above the sections at their foot, a notice for an
update that was installed and not yet running, for a release the updater
installed that has not yet been acknowledged, or for a newer release this
binary will not install itself. The notice SHALL name the version and SHALL
keep its dismissal visible at any sidebar width. Activating the notice SHALL
open that release's notes; dismissing it SHALL put away the notice for that
release in every client and every later run.

#### Scenario: Opening the notes

- **WHEN** the operator activates the notice's row
- **THEN** the release notes for the version it names open in their browser

#### Scenario: Dismissing it

- **WHEN** the operator dismisses the notice
- **THEN** it disappears from both sidebars and does not return for that
  release

### Requirement: A CLI command never waits on a release check

A CLI command SHALL NOT perform network access to check for releases. When the
last answer is stale it SHALL hand the check to a separate process and return.
A command whose reader is a person at a terminal SHALL mention a given release
at most once, after its own output and on stderr, and only when it succeeded.
Commands whose reader is an agent, a hook or the terminal server SHALL NOT
mention releases.

#### Scenario: A stale answer

- **WHEN** a CLI command finishes and the last release check is over an hour
  old
- **THEN** the command returns without waiting, and a separate process asks
  for the latest release

#### Scenario: A release already mentioned

- **WHEN** a release was already mentioned by an earlier command
- **THEN** later commands say nothing about it

### Requirement: The operator can turn it off

UZE SHALL stop checking when told to, SHALL check without replacing when told
to, and SHALL NOT check at all in continuous integration unless told to.

#### Scenario: Turned off

- **WHEN** `UZE_AUTOUPDATE` is `off`
- **THEN** no release is asked for, replaced or announced

#### Scenario: Notify only

- **WHEN** `UZE_AUTOUPDATE` is `notify`
- **THEN** a newer release is announced and never installed

#### Scenario: In CI

- **WHEN** `CI` is set and `UZE_AUTOUPDATE` is not
- **THEN** no release is asked for
