## Why

An agent's work carries two names, and today both are generated noise. The
task's label is derived from a launch prompt the workspace client never
supplies (`Task::new(None, …)`), so it is always the identifier; the branch
is `agent/<id>` by construction, and the readable name produced at publish
time is derived from that same label — so a pull request opened from an
agent's work is titled `agent/zulqgq`. A reviewer meeting that branch
learns nothing, and the operator reading three agents in a sidebar cannot
tell them apart.

Naming cannot be solved by deriving harder. The only party that knows what
the work is, is the agent doing it — but nothing lets it say so portably,
and a name a harness volunteers is not a name every harness volunteers.
What is missing is a surface an agent can call and a moment it must call
it.

A second defect falls out of the same gap: nothing re-reads a task's branch
after it is recorded, so an operator renaming a branch by hand is invisible
to the sidebar — and worse, `commits_ahead` then asks Git about a branch
that no longer exists and answers `0` through its `unwrap_or`, leaving the
task `Running` forever and never offering delivery.

## What Changes

- **`uze agent <noun> <verb>`** — a new command namespace whose audience is
  the agent rather than the person: an ABI, hidden from `uze --help` and
  documented in the projected `AGENTS.md` region, which is where an agent
  reads. First verb: `uze agent task name <type>/<subject>`, resolved from
  the working directory with no identifier argument
- **`worktrees.branch`** — a new policy in `agents.yaml` declaring the
  project's branch vocabulary: a preset (`conventional`, `gitflow`,
  `flat`, `agent`) or the project's own list of types. Closed either way,
  because a proposed name is validated against it. Default `agent` — today's
  behavior unchanged until a project declares otherwise
- **First-writer-wins naming** — a generated name may be replaced once; a
  name anybody chose is never overwritten, extending to the branch and the
  task label the rule the tab label already follows
- **The checkout's HEAD is the truth** — each evaluation adopts the branch
  the task's checkout is actually on, so a manual rename reaches the
  sidebar, delivery and sync (**BREAKING** for nothing: `task.branch`
  becomes a cache of a Git fact)
- **A portable hook enforces it** where the harness can — `PreToolUse`
  `deny` on a commit from an unnamed task, shipped as its own official
  plugin (`uze-naming`) rather than in `plugins/uze`: a hook is an
  executable capability, and the default plugin is installed on every
  machine by UZE's own bootstrap, so putting one there would mean every
  machine silently authorizing one.
  Claude, Codex and Antigravity honor the denial; OpenCode claims only
  `observe`/`allow`, so it degrades to the projected instruction and the
  downgrade is recorded rather than hidden (ADR-033)
- **A safety net at publish** — a project that declares no vocabulary names
  nothing, so its branch is still published under a name derived from the
  first commit rather than under the generated identifier

## Capabilities

### New Capabilities
- `agent-work-naming`: how the work an agent does acquires its two names —
  the branch reviewers read and the label the operator reads — who may
  write them, what validates them, and what never overwrites them

### Modified Capabilities
- `worktree-policy`: the requirement `A task's identity is immutable and
  its name is derived` changes on the name half only. Identity, keying and
  atomic state are unchanged; the label stops coming from a launch prompt,
  the branch becomes renameable once, and the publish-time readable name
  becomes a fallback rather than the mechanism. Its base spec lives in the
  still-open `add-portable-worktree-policy` change

## Impact

- **Core** — `task` (the label's source, the branch as a mutable attribute),
  `worktree` (the branch vocabulary and its validation, the projected
  instruction), `checkout` (adopting the checkout's HEAD; `agent_branches`
  stops assuming the `agent/` prefix), `landing` (the publish-time fallback)
- **Application** — a naming use case on `Workspace`, and the evaluation
  pass that adopts a renamed branch
- **CLI** — the `agent` namespace, hidden from help; a new leaf command to
  classify in `command_performance.rs`
- **TUI** — the sidebar reads the adopted branch; the task label follows the
  name; the existing rename gesture gains the branch
- **Docs** — the projected `AGENTS.md` region gains the naming clause;
  `docs/architecture/invariants.md` gains first-writer-wins
- **Depends on** — `project-agent-environment` §12: a policy that does not
  reach the projected region is a policy agents never read
