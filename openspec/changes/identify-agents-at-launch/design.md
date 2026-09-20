## Context

See proposal.md — Why. The facts that shape the approach:

- The shim already stamps `UZE_SHIM_NAME` and `UZE_SHIM_PID` into the
  harness's environment (`src/shim.rs`), and the terminal server reads
  them back from the pane's process group leader, accepting the name only
  when the stamped pid *is* the leader (`runtime.rs::shim_launched_name`).
  The server also strips both from the environment it seeds every pane
  with (`SHIM_IDENTITY_VARIABLES`), because the server is routinely
  started from inside an agent's pane.
- The terminal persists a tab's `command` and respawns it on restart
  (`PersistedTab`, `restore_finished_agent_panes`); a tab whose agent
  exited is respawned as a plain shell with `command: None`.
- `conversation::owner_of(home, cwd)` is the one resolver, lexical on
  `.worktrees/<id>` plus one small read of the task store; ADR-047 fixed
  its budget at "one lexical path match, one small JSON read".
- `Workspace::name_task` resolves the task the same way, deliberately from
  the directory alone so one agent cannot name another's work.
- The workspace client binds panes to tasks through `pane_checkouts`
  (directory, from the probed cwd) and `slot_claims` (a placement's
  promise until the evaluation lists the task).

## Goals / Non-Goals

**Goals:**
- One channel answers "which agent is this?" for every reader, and it
  works in a directory several agents share.
- The channel is at least as trustworthy as the directory match it
  replaces: a process cannot adopt another agent's identity by editing its
  own environment.
- The nested-launch defect closes as a consequence, not as a special case.
- The terminal runtime learns nothing about agents: it carries an
  environment the way it carries a command.

**Non-Goals:**
- Changing how slot *occupancy* is decided. A pane standing in a slot
  keeps the slot from being reused; that is a fact about the directory
  and stays lexical.
- Signing or encrypting the stamp. See Risks.
- Any change to how a harness names or resumes its own conversation
  (`IntegrationPort::session_continuity` is untouched).

## Decisions

**The identity travels in the pane's environment, stamped by the client at
`CreateTab`.** The alternatives were a wrapper in the command (`env
UZE_AGENT=… <shim>`), rejected because it depends on an external binary,
breaks `relaunch_command_for_process`'s reading of what runs, and
survives nowhere the command does not; and a server-side probe of the
running process's environment, rejected below. `CreateTab` gains
`env: Vec<(String, String)>`; the runtime applies it after stripping the
inherited identity variables, persists it with the command as one
`Launch::Program { argv, env }`, respawns with it, and reports it on
`Tab.env`. A shell is `Launch::Shell`, which has no environment to carry,
so a tab respawned as a shell carries and reports nothing — the existing
lifecycle already says "this pane stopped being an agent" and the
environment rides on it.

**The client binds a pane to its agent by the session's echo, never by
probing the process.** The echo is the server's own record of what it
launched and the pane cannot rewrite it; a value read from `/proc` is
whatever the pane's processes chose to put there. "Which agent was this
pane launched for" is answered by the echo; "is a harness still running
in it" is already answered by the probed `process`, and the client already
combines the two (`space_context_agent` treats a harness that exited and
left a shell as no agent). Two questions, two existing sources, no new
probe and no agent vocabulary in the server beyond one variable name in
the strip list.

**A reader verifies the stamp against the record and against the
directory.** `owner_of` takes `Claim { id, cwd }` and answers only when
the store names `id` *and* `cwd` is inside the directory the record
names as the agent's own: the task's slot today. The stamp says *which*,
the directory says *where*, and they must agree. The store answers
through one accessor, `TaskStore::agent(id) -> Option<AgentRecord<'_>>`,
whose variants each know their own directory, so `owner_of` matches on
nothing and a later record kind adds a variant, never a reader. This is
what keeps "rename another agent's branch" and "resume another agent's
conversation" impossible for a process editing its own environment, at no
new cost — the store read already existed. Residual risk stated below.

**The client's refresh of a conversation builds the claim from the
launch.** `spawn_conversation_refresh` passes the identifier the tab
echoes and the directory the tab was launched in, never the probed
`pane.cwd`, which reports a removed checkout with ` (deleted)` appended.

**The environment is a launch's, so a shell never carries one.** `CreateTab`
refuses an environment without a command, bounds its entries and size for
the persisted file's sake (the wire carries the full `Session` only on
attach, create and select), and refuses a name that is empty or contains
`=`. The persisted file has no defaults: one written before a launch
carried an environment reads as nothing persisted, not as a launch with
none.

**An identity has an owner, and the shim decides ownership.** The owner is
the process whose pid `UZE_SHIM_PID` names, stamped by the shim at
`exec`. Before any shim runs there is no owner. The first shim to read an
identity with no owner takes it; a shim that finds an owner other than
itself is inside that owner's launch, treats the identity as absent and
the invocation as ordinary. This is the rule the terminal already applies
when it reads `UZE_SHIM_NAME` (`shim_launched_name` accepts the name only
from the pid that stamped it), applied by the shim to itself, and it
closes the nested-resume defect the lexical match could never close (both
processes share the directory). There is no form without "absence": the
shim cannot remove the variable, because the harness's children —
`uze agent task name` among them — must inherit it, so "nobody owns it
yet" is necessarily the state before the first `exec`. The rule belongs to
the shim because only a launch can be nested; a command is not: `uze agent
task name` is a child of the owner and accepts the inherited identity
without applying the rule. `continuity::plan` receives a claim or nothing
and never reads the environment.

**The lexical match is removed, not kept as a second channel.** Keeping it
for slots would mean two mechanisms answering one question, with the
directory one silently winning for hand launches. The one behaviour lost
is a harness started by hand inside `.worktrees/<id>` resuming that task's
conversation. ADR-047 already scoped continuity to "a bare launch of an
agent UZE created"; the directory match was an over-approximation of that
scope, and `session_continuity.rs::a_relaunched_agent_resumes…` was
proving the over-approximation. It is rewritten to lay a stamp down.

**Occupancy stays lexical for slots and becomes the stamp for everything
else.** `spawn_occupancy_reconcile` already carries `held: Vec<PathBuf>`
from the client; it gains the identifiers live panes echo. Three
questions, three answers, no overlap: identity by the stamp, slot
occupancy by the directory, presence of an agent without a directory (a
later change's tenants) by the stamp.

**The variable has one owner: the terminal's launch vocabulary.** The
name is transport, not domain: it belongs to the set of variables a launch
stamps and a pane never inherits, which `uze-terminal` already owns
(`UZE_PANE`, the strip list). `uze_terminal::launch::AGENT_IDENTITY_VARIABLE`
is that owner; the strip list is built from it; the server never reads the
value. The two readers, the shim and `uze agent task name`, live in the
root crate, which already names `uze_terminal`, and read the name from
there. Core receives `Claim { id, cwd }` and never names the variable;
`uze-application` re-exports nothing. The alternative — a constant in
core mirrored by a literal in `runtime.rs`, the arrangement `UZE_SHIM_PID`
has today — is two owners for one name, and that precedent is debt rather
than a model. `src/shim.rs` is sanctioned by name in the architecture
suite for reaching core; the sanction grows to name the terminal, with the
reason: the shim is the launch boundary, not presentation.

**No diagram update.** No container or component is added or removed and
no relationship changes: the client already creates tabs on the terminal
and the shim already reads core.

## Candidate ADRs

- **An agent is identified by what its launch carried, verified against
  its record and its directory** — supersedes the mechanism half of
  ADR-047 (lexical match) while keeping its decision (the launch boundary
  owns continuity). Hard to reverse: it fixes what the terminal persists
  per tab, what the shim reads, and what the Lab lays down.

## Risks / Trade-offs

- [Two agents in one directory can swap identities by editing their own
  environment] → Both checks pass for either stamp, because both records
  allow the same directory. Accepted and written into the ADR: it is the
  same trust already extended to `UZE_SHIM_NAME`, and the alternative — a
  server-minted secret verified by the shim — puts the secret into the
  same environment. Naming is not exposed to it: a later change's tenants
  are refused naming by type.
- [A respawn that falls back to a shell leaves an agent's record live with
  no pane] → Already true today for tasks; the next occupancy sweep sees
  no pane echoing the identifier and ends the record. The echo being empty
  is the evidence the sweep uses.
- [The Lab's continuity scene lays a slot down by hand and relies on the
  directory] → It lays a stamp down instead, on the relaunch command:
  `UZE_AGENT=<id> <launcher> <args>` twice, with no `UZE_SHIM_PID`, so the
  first shim takes ownership; with `id` equal to the checkout name and the
  cwd inside the slot, the cross-check passes and the scene keeps
  measuring the harness rather than the engine. `$$` from a shell is not
  the harness's pid, which is why "present with no owner" must mean
  composed, and only "owned by another pid" means nested.
- [Old task-state files carry no stamp and old sessions carry no
  `Tab.env`] → Pre-1.0: schema and protocol versions bump, stale state is
  cleaned, no compatibility path is written.
