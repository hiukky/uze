# transparent-harness-attachment Specification

## Purpose
Lets a user run `uze setup` once per machine and `uze add <package>` once per
package, then use `claude` and `codex` normally, with the installed Agent
Skill available in every session — no per-invocation flags, no launcher, and
no manual per-project vendor configuration.
## Requirements
### Requirement: Machine-level setup is idempotent and harness-scoped
`uze setup` SHALL detect each supported harness independently (binary
present, version when cheaply obtainable) and install or refresh that
harness's UZE integration without requiring the other harness to be present.
Running `uze setup` more than once SHALL NOT duplicate installed state,
duplicate managed artifacts, or fail because a prior run already completed.
`uze setup` for one harness SHALL NOT write to another harness's
configuration.

#### Scenario: Second run is a no-op beyond refreshing state
- **WHEN** `uze setup` runs a second time with no change to the installed
  packages or the harness's own configuration
- **THEN** no new managed artifact is created
- **AND THEN** previously recorded integration state is left equivalent, not
  duplicated

#### Scenario: One harness is absent
- **WHEN** only one of the supported harnesses is installed on the machine
- **THEN** `uze setup` installs the integration for the detected harness
- **AND THEN** it reports the absent harness as not configured rather than
  failing the whole command

### Requirement: Package installation triggers attachment refresh, not a second install
When a package is installed with `uze add`, every harness with a completed
`uze setup` SHALL have its managed attachment for that package's Agent Skill
created or refreshed as part of that same `uze add` operation. The user
SHALL NOT need to run a separate sync, prepare, or per-harness command
afterward for the skill to become visible to a normal harness invocation.

#### Scenario: Adding a package after setup requires no further action
- **WHEN** `uze setup` has already completed for Claude Code and Codex
- **AND WHEN** the user runs `uze add <package>` for a package containing one
  Agent Skill
- **THEN** both harnesses' managed attachments for that skill exist
  immediately after `uze add` returns
- **AND THEN** no `uze sync` or equivalent command exists or is required

### Requirement: Attachment is a persistent, UZE-managed, user-scope reference
A transparent attachment SHALL be a reference (such as a filesystem symlink)
UZE creates under the harness's own user-scope discovery location, pointing
at the package's content inside the UZE store, rather than a copy of that
content. UZE SHALL own the reference's lifecycle: it MAY refresh it when the
store package changes and SHALL remove it on `uze remove`/uninstall. UZE
SHALL NOT duplicate the store's package content as a second permanent
installation.

#### Scenario: Store update is reflected without a rewrite
- **WHEN** a UZE-managed attachment references a package already installed in
  the UZE store
- **AND WHEN** that store package's content changes through UZE
- **THEN** the harness resolves the updated content without UZE recreating
  the attachment

#### Scenario: Removing a package removes its attachment
- **WHEN** a package with an active transparent attachment is removed from
  the UZE store
- **THEN** UZE removes the managed reference for every harness that had it
- **AND THEN** no dangling reference is left in any harness's discovery
  location

### Requirement: Attachment never disturbs unrelated entries in a shared discovery location
A harness's user-scope discovery location MAY already contain entries UZE
did not create. UZE SHALL namespace and track only the entries it manages,
SHALL NOT modify, move, or remove an entry it did not create, and SHALL
detect and avoid name collisions with existing entries.

#### Scenario: Pre-existing unrelated entry is left untouched
- **WHEN** a harness's user-scope discovery location already contains an
  entry not created by UZE
- **AND WHEN** `uze setup` or `uze add` runs
- **THEN** that pre-existing entry is unchanged
- **AND THEN** UZE's own entries are distinguishable as UZE-managed

### Requirement: Real project cwd is preserved and no manual per-project vendor config is required
Because attachment lives at harness user scope, resolving a project through
an attached harness SHALL use the real project working directory. The user
SHALL NOT be required to create, edit, or maintain any vendor-specific file
or directory inside the project for the attached skill to be available.

#### Scenario: Project directory contains no UZE-authored vendor files
- **WHEN** a package's Agent Skill is transparently attached through user
  scope
- **AND WHEN** the user opens a project directory that never received
  explicit UZE project-level exposure
- **THEN** the project directory contains no UZE-authored `.claude` or
  `.agents` vendor configuration
- **AND THEN** the harness still resolves the attached skill through its own
  user-scope discovery

### Requirement: No launcher and no wrapper are required for normal invocation
The system SHALL NOT require `uze claude`, `uze codex`, or any UZE-specific
subcommand to start a harness with the attached capability available. The
system SHALL NOT install a process wrapper that replaces the harness's own
executable on the user's PATH.

#### Scenario: Plain harness invocation is sufficient
- **WHEN** `uze setup` and `uze add` have completed for a harness
- **THEN** starting that harness with its own normal command and no UZE
  arguments makes the attached skill available
- **AND THEN** no UZE-provided executable is required to be on PATH ahead of
  the harness's own executable

### Requirement: Minimal integration state is recorded without harness secrets
UZE SHALL record, per harness, at least: harness identifier, detected
version (when available), integration strategy, whether setup completed, and
the managed artifact paths it created. UZE SHALL NOT store harness
authentication credentials or other harness secrets in this state.

#### Scenario: Doctor reports integration state
- **WHEN** `uze doctor` runs after `uze setup` has completed for a harness
- **THEN** it reports that harness's installed/configured status without
  printing any credential material

### Requirement: Setup and runtime transparency are verified as distinct phases
An opt-in conformance test for transparent attachment SHALL distinguish a
setup phase (`uze setup` completing and producing the expected managed
state, verified independently of process invocation) from a runtime phase (a
plain harness invocation with no UZE-specific arguments and no test-authored
preparation step executed immediately before that invocation). A test SHALL
NOT claim runtime transparency was verified solely because setup succeeded.
Real-harness conformance SHALL run against a temporary, isolated home
directory and UZE home, never the operator's real harness configuration.
Authentication or quota failures during a runtime probe SHALL be classified
as an environment block, never as incompatibility.

#### Scenario: Setup-only success is not reported as runtime-verified
- **WHEN** a setup-phase test confirms the managed attachment exists on disk
- **AND WHEN** no runtime-phase invocation of the real harness has run
- **THEN** the capability's transparent-attachment verification remains
  unverified, not verified

#### Scenario: Quota failure during a runtime probe is an environment block
- **WHEN** a runtime-phase probe invokes the real harness and the harness
  reports an authentication or quota condition
- **THEN** the probe's verification result is an environment block
- **AND THEN** the capability is not reported as incompatible or unsupported

