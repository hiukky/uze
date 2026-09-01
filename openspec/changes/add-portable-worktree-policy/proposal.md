## Why

`agents.lock` has carried a `worktrees_dir` field since the project agent
environment was introduced. It is parsed and validated, and then nothing
reads it: the only consumer in the workspace is the TUI's Git diff overlay,
which uses it to decide which checkouts to display. No integration is ever
told about it, and no harness ever learns where a project wants its
concurrent work isolated.

The policy's only real transport has been prose in the `uze` package's
`worktree` Skill — and prose is not a projection. The Skill also had to
*decide whether it should be used*, which is circular: a skill must be
selected before it can argue for its own selection, and skill descriptions
are matched against the user's request, where the conditions for isolation
("more than one agent will write here") never appear.

Meanwhile the harnesses moved. At least one ships a first-class worktree
primitive whose documented activation condition is the project's own
instructions — meaning the shared instruction baseline is not a fallback for
harnesses that lack a mechanism, it is the trigger for the one that has it.

UZE already solved this exact shape twice: `AGENTS.md` (one authored
instruction baseline, projected per harness) and portable Hooks (one authored
manifest, projected per harness, receipt-owned, honestly routed). Worktree
policy is the third instance, and the only one left as prose.

## What Changes

- **UZE isolates at launch — the seat rule.** The primary checkout seats one
  agent. The first agent in a repository starts there and sees the operator's
  uncommitted work. Every additional live agent starts in
  `.worktrees/<name>` on branch `agent/<name>`, created by UZE. Nobody is
  asked a question, nothing is configured, and no harness has to cooperate.
  Isolation impossible (not a repository, no commit to branch from, Git
  absent) degrades to seating the agent, never to refusing to launch it.
- **`agents.lock`: `worktrees_dir` → a `worktrees` block with one field**,
  `completion` (`handoff` | `merge`) — what happens to finished work, the
  only axis that is a team decision rather than infrastructure. The replaced
  key is rejected by name rather than silently ignored.
- **New `uze-core::worktree`** — the fixed layout, the completion vocabulary,
  primary-checkout resolution, seat membership, isolated-checkout creation
  (prune, collision-suffix, ignore-file maintenance), and the rendered text.
- **Projection into the shared baseline** — `uze context reconcile` writes a
  short statement into a marker-owned region of `AGENTS.md`: the layout, the
  fact that the reader is already isolated, how to isolate a subagent against
  the primary, and the completion rule. It never asks for a top-level
  worktree, which is what a harness's own worktree primitive activates on.
- **One repository, one terminal server** — the runtime is keyed on the
  resolved workspace root instead of the launch directory. Keyed on the raw
  cwd, launching UZE from a repository and from a subdirectory of it produced
  two servers over one checkout, each believing its seat was free.
- **The `worktree` Skill shrinks** to what no harness does: isolating
  subagents UZE cannot see, coordinating writers, and integrating branches.

## Non-goals

- UZE never removes a worktree. Checkouts are kept; a closing tab deletes
  nothing.
- No harness's native worktree settings are written, and no harness's own
  worktree primitive is triggered.
- No configurable path, branch prefix, base ref, or isolation trigger. The
  layout is infrastructure; a configuration axis is earned by demand.
- No `pull_request` completion, and no language- or ecosystem-specific
  handling of what a fresh checkout lacks.
- In-session subagents are outside UZE's reach: they are addressed by the
  projected text, which is delivery, not a guarantee.

## Capabilities

### New Capabilities
- `worktree-policy`: launch-time isolation of concurrent agents, plus the one
  declared completion behavior and its projection into the shared baseline.

### Modified Capabilities
- `project-agent-environment`: `agents.lock` gains `worktrees.completion` and
  loses `worktrees_dir`.
- `context`: `inspect`/`plan`/`reconcile` gain the policy region.

## Impact

- **Core** — new `worktree` module; `project_lock` schema change; one shared
  workspace-root resolver.
- **Integrations** — none. Isolation is performed before a harness starts, so
  no vertical needs worktree knowledge, and a fifth harness needs none either.
- **Application** — `context_inspect|plan|reconcile` compose the policy region.
- **CLI/TUI** — `context` output renders the policy; the workspace
  orchestrator applies the seat rule at both agent-creation sites.
- **Extensions** — the Git diff overlay reads the fixed layout.
- **Package** — the `uze` package's `worktree` Skill is rewritten.
