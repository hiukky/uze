## Why

An agent UZE manages survives everything except the one thing it is made
of. Its slot, its branch, its task record and its tab all come back after
the terminal server restarts — the conversation does not. The pane is
respawned with the argv it was created with (`claude`, `codex`, …), which
starts a harness that has never heard of the work in the checkout it is
standing in, while the real conversation sits intact in the vendor's own
session store, reachable by a verb UZE never says.

Every harness UZE supports can pick a conversation up again, and the
operator does it by hand today — leaving the TUI, finding the id, typing
the vendor's resume verb. The one fact needed to do it for them, the
harness's session identifier, is the one fact UZE never writes down.

## What Changes

- **A task remembers its conversation.** A managed task records, per
  harness, the session identifier of the conversation started for it, in
  UZE's own state beside the task itself — never inside a checkout, so a
  slot reset cannot take it, and keyed by task, so recycling a slot for the
  next task cannot leak the previous one's conversation into it.
- **A relaunch resumes rather than restarts.** An agent launched into a
  task that already has a conversation for that harness starts with the
  vendor's own resume verb. A task with none starts a fresh conversation
  and records it. Both decisions are made at the moment the process starts,
  by the launch boundary UZE already owns (the PATH shim), so a pane
  respawned by the terminal server after a restart resumes with no client
  present and no protocol change.
- **Continuity becomes a harness capability.** `IntegrationPort` gains the
  session-continuity contract: whether UZE may name a conversation at
  launch or must read the name back afterwards, the argv that starts a
  named conversation, the argv that resumes one, and the observation that
  finds the identifier for a harness that names its own. Vendor knowledge
  stays in `uze-integrations`; core, application and TUI say only
  "continue this task's conversation".
- **A harness that cannot do it says so.** Continuity is declared, never
  assumed: a harness with no mechanism UZE can drive reports it, the agent
  starts fresh, and the tab says the conversation could not be carried
  over — the same "declare, never omit" discipline the Lab contract uses.
- **The Lab proves it per harness.** One outcome-stated contract check —
  a conversation started in a slot, the process ended, an agent launched
  into the same task, and the earlier turn present in the new process —
  runs against every real harness binary, or is declared unsupported with
  a reason.

Not in scope: carrying a conversation across harnesses (a codex session is
not resumable by claude), carrying one across machines, and resuming an
agent that never got an isolated checkout — an unisolated agent has no
task to key a conversation on, and the tab already says why.

## Capabilities

### New Capabilities
- `agent-session-continuity`: what UZE records about a managed agent's
  conversation, when it resumes one instead of starting one, how a harness
  declares what it can do, and what the operator is told when it cannot be
  done.

### Modified Capabilities
<!-- None. `worktree-policy` (task, slot, delivery) is unarchived and
     unchanged by this: a task gains a recorded conversation, no
     requirement of that capability changes. -->

## Impact

- `uze-core`: a session record beside the task store (`project/`), and the
  vendor-neutral continuity contract on `IntegrationPort` (`delivery/`).
- `uze-integrations`: continuity per vendor — assignment for the harness
  that lets UZE name a conversation, observation for those that name their
  own, unsupported where neither exists.
- `src/shim.rs`: the launch decision, on the same boundary that already
  contributes runtime arguments; the caller's own argv is never rewritten
  and an explicit session flag always wins.
- `src/ui/`, `uze-application`: agents are launched through the shim by
  path rather than by name, so continuity never depends on the operator's
  PATH; the tab says when a conversation could not be carried over.
- `conformance/`: one contract check plus per-harness bindings.
- `journeys/`: one scene in `06-recovery` — an agent's conversation
  survives the terminal server's death.
- No new dependency.
