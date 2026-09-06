## ADDED Requirements

### Requirement: A journey is one declarative scenario file

A product journey SHALL be a single declarative file naming the world it
needs, the gestures it performs, and the checks that must hold afterwards.
Adding a journey SHALL require no new code: the step and check vocabulary is
closed, and an unknown verb SHALL fail validation rather than be interpreted.

#### Scenario: A new journey adds no code

- **WHEN** a contributor adds a journey file using existing verbs
- **THEN** the runner executes it with no change to the runner or to any
  step-definition module

#### Scenario: An unknown verb is refused

- **WHEN** a journey uses a verb the vocabulary does not define
- **THEN** `journey validate` fails naming the verb, and the run does not
  start

### Requirement: One scenario language drives both the CLI and the TUI

A journey SHALL be able to perform command invocations and TUI gestures in
the same scene, in order, against the same world.

#### Scenario: A scene mixes a command and a gesture

- **WHEN** a scene runs a `uze` command and then clicks a cell in the running
  TUI
- **THEN** both act on the same sandbox world, and the checks that follow see
  the result of both

### Requirement: Every assertion reads the machine, never UZE's own report

A `then` check SHALL be satisfied by reading the filesystem, a config
document, Git state, or the process table. UZE's own output SHALL be
assertable only as the subject of a `cmd` step, never as the source of truth
for another check.

#### Scenario: A command that reports success but writes nothing fails

- **WHEN** a command exits zero and reports the artifact it claims to have
  written, and the artifact is absent on disk
- **THEN** the journey fails on the filesystem check, regardless of the
  command's report

#### Scenario: Screen text never satisfies a check

- **WHEN** a journey states a `then` check
- **THEN** the check reads the machine; screen text appears only as an
  `expect` gate inside `when`

### Requirement: A gesture states what proves it landed

Every TUI gesture in a journey SHALL declare an `expect` condition, and
validation SHALL fail a gesture without one. Synchronization SHALL be by
`expect` (screen) or `until` (shell predicate) with a timeout; a fixed sleep
SHALL NOT be a synchronization primitive.

#### Scenario: A click with no expectation is refused

- **WHEN** a journey declares a click, double-click or key with no `expect`
- **THEN** `journey validate` fails naming the gesture

#### Scenario: A gesture that did not land fails the scene

- **WHEN** a gesture's `expect` is not satisfied within its timeout
- **THEN** the scene fails immediately, and no later gesture in it is
  performed

### Requirement: Journeys name product outcomes, not vendor paths

A journey SHALL express harness delivery in product terms. Where a check
needs a vendor-owned location, that location SHALL be declared in a
per-harness atlas file, and the check SHALL read the bytes at the resolved
location.

#### Scenario: A vendor path moves in one place

- **WHEN** a harness changes where it reads a delivered capability
- **THEN** only that harness's atlas entry changes, and no journey file is
  edited

#### Scenario: An unresolved term is refused

- **WHEN** a journey names a delivery term no atlas defines
- **THEN** validation fails naming the term and the harness

### Requirement: A lifecycle journey proves nothing was left behind

The vocabulary SHALL include a check that compares the sandbox tree before a
scene with the tree after it, so a removal journey proves the world returned
to its prior state rather than proving a receipt was deleted.

#### Scenario: Removal leaves no orphan

- **WHEN** a journey installs a plugin, removes it, and asserts `no_orphans`
- **THEN** any file, link or config entry that survives the removal fails the
  journey and is named in the evidence

### Requirement: Journeys run in a disposable world that can never be the developer's

A journey SHALL execute against a sandbox HOME and UZE_HOME created for that
run. The runner SHALL refuse to start if any sandbox root overlaps, or is an
ancestor of, the developer's real harness or UZE directories.

#### Scenario: A sandbox that could touch real state refuses to run

- **WHEN** a resolved sandbox root overlaps a real `~/.uze`, `~/.claude`,
  `~/.codex`, `~/.agents` or `~/.config/opencode`
- **THEN** the runner exits with an error before performing any gesture

### Requirement: Journeys run offline in the synthetic world

The synthetic world SHALL provision generated harness binaries and require no
network, no credentials and no model calls; it is the world CI gates on. The
real world SHALL use harness binaries present on the machine and SHALL skip
cleanly per harness when one is absent.

#### Scenario: A PR gate needs nothing but the repository

- **WHEN** the synthetic world runs on a clean machine with no harness
  installed and no network
- **THEN** every journey tagged for the gate runs to a verdict

#### Scenario: A missing real harness skips, never fails

- **WHEN** the real world runs and a harness binary is absent
- **THEN** that harness's journeys are recorded as skipped with the reason

### Requirement: A failed journey is debuggable from its evidence alone

A run SHALL write a verdict per journey. A failed scene SHALL record the
captured screen, the check's expectation against what was found, and the diff
of the sandbox tree across the scene.

#### Scenario: A failure is reproduced without rerunning by hand

- **WHEN** a scene fails in CI
- **THEN** the evidence shows the frame at failure, the check that failed with
  both sides, and what the scene wrote to the world

### Requirement: An unstable journey is quarantined with an owner, never retried

A journey known to be unstable SHALL be registered with a reason and an owner
and reported as such. The runner SHALL NOT re-run a failed assertion to make
it pass, and a registered journey that starts passing SHALL be reported so the
registration can be removed.

#### Scenario: A flaky journey is visible

- **WHEN** a registered unstable journey fails
- **THEN** the run reports it as a registered instability with its reason and
  owner, and does not silently retry it

#### Scenario: A recovered journey is escalated

- **WHEN** a registered unstable journey passes
- **THEN** the run reports that its registration is no longer justified
