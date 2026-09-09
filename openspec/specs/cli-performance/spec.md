# cli-performance Specification

## Purpose
Guarantees that UZE's CLI and TUI operations respond in milliseconds unless
an operation is explicitly justified to be slow (e.g. a network install),
by requiring expensive local probes to be cached, correctly invalidated,
and covered by tests that assert the budget rather than assume it.
## Requirements
### Requirement: CLI operations complete within a low-millisecond budget
Any UZE command whose work does not require an explicitly justified slow
operation (network installation/provisioning) SHALL complete in under 50
milliseconds when its underlying inputs (installed harnesses, packages,
project state) are unchanged since the previous invocation. No manual
flag or operator action SHALL be required to obtain this fast path.

#### Scenario: Repeated read-only commands are fast
- **WHEN** any read-only command that reports harness or plugin state
  (including but not limited to `status`, `doctor`, `list`, `inspect`,
  `context inspect`, `context plan`, `marketplace list`, `plugin list`) is
  run twice in a row with no change to installed harnesses or project
  state between runs
- **THEN** the second invocation completes in under 50 milliseconds and
  performs no external harness subprocess invocation

#### Scenario: TUI startup is fast on repeat launches
- **WHEN** the TUI is launched twice in a row with no change to installed
  harnesses between launches
- **THEN** the second launch's startup work completes in under 50
  milliseconds and performs no external harness subprocess invocation

#### Scenario: Installation remains exempt from the budget
- **WHEN** a command performs an explicitly justified slow operation, such
  as installing or provisioning a harness or plugin
- **THEN** the budget does not apply to that operation, and the command is
  not required to complete in milliseconds

#### Scenario: A justified-slow command still avoids redundant detection cost
- **WHEN** a command that performs a justified slow operation (such as
  `add`, installing a harness or plugin) also needs harness detection
  results as part of its work
- **THEN** it reuses cached detection results the same way a read-only
  command would, and does not perform additional uncached detection
  probes on top of the justified slow operation

### Requirement: Harness detection is cached and reused within one command
A single logical command invocation SHALL perform at most one live
detection probe per harness integration, regardless of how many internal
call sites need that integration's detection result.

#### Scenario: Multiple internal consumers share one probe
- **WHEN** a command's internal logic needs a given integration's
  detection result from more than one call site during the same invocation
- **THEN** only one external probe of that harness occurs, and every call
  site observes the same result

### Requirement: Harness detection is cached and reused across command invocations
Harness detection results SHALL be persisted so that a subsequent, separate
CLI invocation can reuse a still-valid result instead of performing a new
live probe.

#### Scenario: Second invocation reuses a persisted result
- **WHEN** `uze list` is run, followed by a separate `uze doctor` invocation
  shortly after, with no relevant change to the installed harnesses
- **THEN** the second invocation reuses the detection result recorded by
  the first instead of re-probing the harness

### Requirement: Cached detection results are invalidated when they no longer reflect reality
A cached detection result SHALL NOT be served once it may no longer reflect
the real state of the harness: after the harness is installed, removed, or
updated, or after a bounded maximum age has elapsed.

#### Scenario: Newly installed harness is detected without manual cache clearing
- **WHEN** a harness that was previously absent becomes installed, and a
  UZE command that reports harness presence is run afterward
- **THEN** the command reports the harness as present without any manual
  step to clear or reset a cache

#### Scenario: Updated harness version is reflected
- **WHEN** an installed harness is updated to a different version
- **THEN** the next UZE command that reports harness version reflects the
  updated version, not the previously cached one

#### Scenario: Removed harness is reflected
- **WHEN** a previously installed harness is uninstalled
- **THEN** the next UZE command that reports harness presence reports it
  as absent, not as still present from a stale cache entry

#### Scenario: Stale entries expire even without a detected change
- **WHEN** a cached detection result has exceeded its maximum allowed age
- **THEN** the next command needing that result performs a fresh live
  probe instead of serving the expired entry

### Requirement: UZE-driven changes to harness state refresh the cache automatically
When UZE itself performs an action that changes a harness's installed
state (installation or update), it SHALL update the cached detection
result for that harness as part of that action, without requiring a
separate probe or any operator action.

#### Scenario: Cache reflects a UZE-driven install immediately
- **WHEN** UZE installs or updates a harness through its own provisioning
  flow
- **THEN** the next command that reads that harness's detection result
  observes the fresh state immediately, with no stale window and no
  manual refresh needed

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

### Requirement: A missing or corrupt cache never breaks a command
A command that depends on cached detection SHALL still succeed, falling
back to a live probe, when the underlying cache is missing, unreadable, or
corrupted.

#### Scenario: Corrupted cache file does not fail the command
- **WHEN** the on-disk detection cache is missing or contains unreadable
  data
- **THEN** the command completes successfully by performing a live probe,
  rather than failing or surfacing an error about the cache itself

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
