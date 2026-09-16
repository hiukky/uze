## Why

Every question of the form "which agent is this?" is answered today by the
directory the process stands in: the shim resolves the conversation to
resume, `uze agent task name` resolves the task to rename, and the
workspace client binds a pane to its task, all by matching the working
directory against `.worktrees/<id>`. That makes the isolation directory the
system's identity channel, not merely its isolation mechanism — and it has
two consequences that are already bugs. An agent that a harness launches
from inside another agent's pane stands in the same directory and resumes
the parent's conversation, so two processes share one `--resume`. And any
agent started outside an isolated checkout has no identity at all: no
conversation, no naming, no binding — which is what forbids placing an
agent anywhere but a slot.

The identity of an agent UZE launched is known at the moment UZE launches
it. It should travel with the launch, not be reconstructed from where the
launch happened to land.

## What Changes

- **A launch carries the agent's identity.** When the workspace client
  creates an agent's tab it stamps the agent's identifier into the pane's
  environment. The terminal runtime persists that environment beside the
  tab's command and hands it back on the session, so a pane the server
  respawns after a restart carries the same stamp and a client reads its
  own stamp back rather than probing the process.
- **Identity is verified, not trusted.** A stamp names an agent; the
  record for that agent names where it belongs. Every reader checks both:
  the identifier exists in the project's records, and the directory the
  process stands in is one that record allows (its own slot for an
  isolated agent). A stamp the record contradicts is ignored.
- **The shim resolves continuity by the stamp**, not by the directory. A
  launch carrying no stamp is an ordinary invocation. A launch made from
  inside an already-stamped agent — a harness started by a harness — is an
  ordinary invocation too, which closes the nested-resume defect.
- **`uze agent task name` resolves the task by the stamp** the agent's
  process inherited, so the refusal "this is not an agent's checkout" is
  replaced by "this process is not an agent UZE launched".
- **The workspace client binds panes to agents by the stamp** the session
  hands back, and reports which agents are still held by a live pane when
  it asks for slots to be reconciled — alongside, not instead of, the
  directories those panes occupy, which remain the fact that keeps a slot
  from being reused.
- **BREAKING**: a harness started by hand inside an isolated checkout no
  longer resumes that task's conversation. Continuity was always specified
  for the launches UZE composes; the directory match was a proxy for that,
  and it is the proxy that goes.

## Capabilities

### New Capabilities
- `agent-identity`: how an agent UZE launched is identified for the life of
  its process — the identifier stamped at launch, how it is carried and
  persisted, and the two checks every reader makes before acting on it.

### Modified Capabilities
- `agent-session-continuity`: the requirement `Relaunching an agent
  continues its task's conversation` resolves the task from the launch's
  identity rather than from the directory; `The operator's own invocation
  is never rewritten` gains the nested launch and the hand launch in a
  slot as invocations that stay ordinary.
- `terminal-runtime`: a tab carries the environment its launch put there —
  persisted with the tab, respawned with it, and reported back to clients
  as data the server never interprets.
- `agent-work-naming`: naming resolves the task from the agent's identity
  rather than from its checkout. Its spec lives in the still-open
  `name-an-agents-work` change and is edited there in place, with the
  task it reopens pointing here; no delta is carried in this change.

## Impact

- **`uze-terminal`** — `CreateTab` gains an environment; the tab persists
  and reports it; the spawn path applies it and strips it from the
  environment a pane inherits from the server. Protocol version bump.
- **Core** — `conversation::owner_of` takes a claim (identifier plus
  directory) instead of a directory, answered through one accessor over
  the store's records; `continuity::plan`/`refresh` take the claim and
  never read the environment. The variable's name is owned by
  `uze-terminal`'s launch vocabulary; core never names it.
- **Application** — `name_task` resolves by claim; occupancy reconciliation
  accepts the identifiers live panes hold.
- **CLI/TUI** — the launch path stamps the identifier; the pane→task
  binding reads the session's echo; the lexical `pane_checkouts` stays for
  slot occupancy only.
- **Tests and Lab** — `session_continuity.rs` and
  `conformance/contract/continuity.py` lay a stamp down instead of relying
  on a slot directory; `runtime_boundary.rs` gains the nested-launch case.
- **Docs** — ADR-047's mechanism evolves; the ADR is written at archive.
