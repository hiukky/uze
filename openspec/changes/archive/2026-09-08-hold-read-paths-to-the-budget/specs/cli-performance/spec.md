## MODIFIED Requirements

### Requirement: Every CLI command carries an enforced performance classification
Every command in UZE's CLI surface SHALL be explicitly classified as
either subject to the low-millisecond budget or exempt as a justified-slow
operation. This classification SHALL be enforced automatically: a command
added to the CLI without a corresponding classification SHALL cause the
test suite to fail, and a command classified as budget-bound SHALL have
its own automated test asserting that the application call it dispatches
to meets the budget. The classification SHALL name that test, and the
suite SHALL fail when the named test does not exist in the module it
names. It SHALL NOT be possible for a new command to ship
fast-by-assumption, silently regress past the budget, or remain
unclassified because a human reviewer did not think to check it.

#### Scenario: An unclassified new command fails the test suite
- **WHEN** a new command is added to the CLI without an explicit
  performance classification (budget-bound or justified-slow, with a
  reason)
- **THEN** the test suite fails, identifying the unclassified command by
  name

#### Scenario: A budget-bound command without its own performance test fails the test suite
- **WHEN** a command is classified as budget-bound
- **THEN** the test suite SHALL include a test that exercises that
  command's own cache-warm application path — not a shared probe the
  command happens to route through — and asserts it completes under the
  budget, and the suite fails if that test is missing

#### Scenario: A classification that names a test that no longer exists fails the test suite
- **WHEN** the test a budget-bound command's classification names is
  renamed or removed
- **THEN** the test suite fails, naming the command and the missing test

#### Scenario: A justified-slow command requires a stated reason
- **WHEN** a command is classified as exempt from the budget
- **THEN** the classification records a human-readable justification (e.g.
  "performs a network install"), so an exemption is a deliberate,
  reviewable decision rather than a silent default

## ADDED Requirements

### Requirement: A warm read-only command writes nothing
A command that only reports state SHALL leave every file under `UZE_HOME`
unchanged when run again with nothing changed in between: the bootstrap
that precedes every command SHALL record a harness, republish a derived
view, or write any other file only when the content would differ.

#### Scenario: A second run leaves UZE_HOME as it found it
- **WHEN** `status`, `doctor` or `plugin list` is run twice in a row with
  no change to installed harnesses, packages or project state between runs
- **THEN** every file under `UZE_HOME` has the same size and modification
  time after the second run as after the first

#### Scenario: A derived view that stopped matching is still rebuilt
- **WHEN** an integration's published catalogue or a generated package
  envelope no longer matches the installed package set, or is missing
- **THEN** the next command's bootstrap rebuilds that integration's view

### Requirement: A harness executable is looked for on this machine's own filesystems
When resolving a harness executable — for detection, for its cached
fingerprint, for the runtime shim's own resolution and for the check that
the shim is what `PATH` resolves to — UZE SHALL skip `PATH` entries that
live on a filesystem reached over a network protocol (`9p`, the mount a
Windows drive appears as inside WSL). A harness UZE integrates keeps its
state under `$HOME` on this machine; an executable on such a mount is not
it.

#### Scenario: A Windows drive on PATH costs nothing
- **WHEN** `PATH` carries entries under a `9p` mount and a harness is not
  installed
- **THEN** resolving that harness's name performs no filesystem access
  under those entries

#### Scenario: The shim check stops at the shims directory
- **WHEN** the shims directory is on `PATH`
- **THEN** whether a harness's shim is active is decided from the entries
  up to and including the shims directory, and no entry after it is probed

### Requirement: The management screens' data is one read model within the budget
The data every management screen of the TUI shows SHALL be composed by
the application into one read model, and refreshing it SHALL be held to
the budget by a test that times that one call on a fresh application.

#### Scenario: A refresh is one call
- **WHEN** the TUI's management worker refreshes
- **THEN** it obtains plugins, health, marketplaces, marketplace plugins,
  profiles, workspace, context status and prompt history from a single
  application call, and composes nothing of its own beyond presentation
  ordering

#### Scenario: A warm refresh meets the budget without probing anything
- **WHEN** the read model is produced again with nothing changed
- **THEN** no harness is probed, no marketplace is cloned, and the call
  completes within the budget
