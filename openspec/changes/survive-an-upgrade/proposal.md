## Why

Every UZE on a machine meets state a previous UZE left there, and the last
weeks have shown it does not meet it well. Three failures in one day, all
from the same shape of mistake, and each one reached the operator as an
error about a file or a path they never wrote:

- a task document written under an older schema failed `serde` before the
  schema guard could read the version, so the sidebar said `tasks
  unreadable` and **no agent could be created at all** in that repository;
- the endpoint's own rules changed, so a live server held the workspace at
  a path the new build does not name — `uze terminal stop` reported
  nothing to stop, and **restarting the machine was the only way out**;
- `make install` over a running client left `current_exe()` resolving to
  `<path> (deleted)`, and starting a server from it answered `No such file
  or directory`.

Each was fixed where it was found. What is missing is the rule that would
have stopped all three from being written: UZE's own persisted state is a
*compatibility surface*, and today every subsystem re-decides on its own
what to do when it meets a version of that state it does not understand.
Before v1 this costs a developer an afternoon; after it, it is a user
whose agents are unreachable and whose remedy is a reboot.

## What Changes

- **A stated rule set for state UZE owns across versions** — what must
  self-heal, what must be reported, what may never be destroyed, and what
  a person is told in each case. Named once, so a new state file inherits
  it instead of relearning it.
- **An audit of every document and endpoint UZE persists** against that
  rule set, with the gaps closed: each one declares its version, reads
  that version before the document, and answers an unreadable version in
  the way its kind says it must.
- **Every artifact UZE owns is addressable by something whose location
  does not depend on rules that can change** — the way the workspace claim
  now names its holder, so a server at an endpoint this build cannot
  compute is still something `stop` can stop.
- **`uze doctor` reports upgrade leftovers**: documents set aside, state
  from a schema this build does not know, a server holding a workspace at
  an endpoint nobody can reach. Today they are invisible until something
  fails.
- **The previous release becomes part of the test suite** — a journeys
  chapter that runs the *last released binary* to create state on a
  machine, then runs this build against it and checks what the operator is
  left with. No unit test can prove this: the whole claim is about two
  binaries meeting on one disk.
- **BREAKING** (for state, not for API): a document this build cannot read
  is set aside and its project recorded again from the machine. What is
  lost is UZE's own labels and bookkeeping, never a branch, a commit, or a
  plugin's bytes — but it *is* lost, and the rule says so out loud rather
  than leaving each subsystem to decide quietly.

## Capabilities

### New Capabilities

- `upgrade-resilience`: what UZE does when it meets state a previous
  version wrote — the classes of state, the answer each class owes, what
  the operator is told, and how a release proves it.

### Modified Capabilities

- `terminal-runtime`: the endpoint is addressable across versions — the
  claim names its holder, so a server this build cannot reach is still one
  `stop` ends and `attach` replaces, rather than a workspace shut until
  the machine restarts.

## Impact

- `crates/uze-core`: every persisted document (`task`, `client_layout`,
  `prompt_history`, `conversation`, `delivery::state`, `package::store`,
  `theme_state`, `profile_state`) audited against the rule set.
- `crates/uze-terminal`: the claim, the endpoint, and what `stop` and
  `attach` act on.
- `crates/uze-application`: `doctor` gains the upgrade-leftovers report.
- `journeys/`: a new chapter that runs two binaries against one machine.
- `docs/architecture/invariants.md`: the rule set's properties, each tied
  to the test that holds it.
- No change to plugin bytes, to `agents.yaml`/`agents.lock`, or to any
  file a harness reads: this is about state UZE writes for itself.
