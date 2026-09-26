## Context

See proposal.md — Why. Three facts decide the shape.

**ADR-019 made scope structural for a real reason.** `uze remove flow` used
to fall back from project to machine depending on whether a lock happened to
mention `flow` — a command whose target scope depended on ambient, invisible
state. That fallback is what this change must not reintroduce.

**What is proposed is not that fallback.** It is a superset: the root verbs
always do the machine half, and additionally maintain the project's files
when there is one. There is no fork to guess at; the only variable is
whether a file exists to write. What ADR-019 actually wanted — that scope is
never silent — is kept by reporting the scopes touched, rather than by
encoding them in the command's position.

**Nothing on this machine knows which projects declare which plugins.**
`record::ensure` is called only from `task.rs:590`, `conversation.rs:256`
and `prompt_history.rs:185` — all workspace-client concerns. A contributor
who clones and runs `uze install` without ever opening the client leaves no
entry. Any safety rule built on that registry would under-report and delete
bytes another project declares.

## Goals / Non-Goals

**Goals:** one spelling per operation; a scope that is either obvious from
where you stand or stated with one flag, and always reported; a project root
resolution that can answer "none".

**Non-Goals:**

- **Inferring who else wants a package.** Out, for the reason above.
  Machine removal stays something the operator asks for.
- **Aliases for the removed verbs.** ADR-019 chose a clean break pre-1.0
  and this follows it.
- **Changing what any operation does.** Only who says it, and how scope is
  chosen.

## Decisions

### The flag is `-m` / `--machine`, not `--store` and not `-g`

`uze remove <p> -m` touches three things: the Store's bytes, the receipts,
and the artifacts in each harness. Two of the three are not the Store —
AGENTS.md defines it as owning package bytes and says it *"never writes
anything a harness reads"* — so `--store` would name a third of the
operation, and would invite the reading "store it but do not deliver it",
which never happens. `--global` is vague about what it is global to.
"Machine" is the word this product already uses on exactly this axis: the
AGENTS.md scope table, and ADR-019's own title.

### It is a scope selector, not a `--no-save`

An earlier draft proposed `--no-save` on install alone. That is a special
case of one verb; this is the same word on every verb that has two scopes,
which is why it stays one thing to learn. It also has a caller that cannot
express intent any other way: the conformance Lab drives all four verticals
with `uze plugin install` and records, at `conformance/DECISIONS.md:529`,
that *"machine scope is what `uze plugin install` promises — a project file
is a different capability's decision."* Left to "where you run it", that
guarantee becomes ambient: adding a `.git` to the Lab world would silently
start writing manifests and change what is under test with no failure.

### `AGENTS.md` stops outranking a repository root

The current closure remembers the nearest `AGENTS.md` and returns it *in
place of* the repository root, so `repo/docs/AGENTS.md` makes `repo/docs`
the project. `AGENTS.md` is a file UZE writes, so finding one in a
subdirectory says something ran there, not that a project begins there. It
stays as the last anchor — a directory with agent instructions and no Git is
still a project — but below the repository, never above it.

Two named tests encode the old behavior: `fallback_is_cwd_when_no_markers`
(`project_root.rs:81`) and `prefers_agents_md_over_git` (`:110`). The second
passes either way, because it puts both markers in the *same* directory —
the discriminating case, a nested `AGENTS.md` inside a repository, has no
test at all. It is the defect being fixed and gains one.

### "Not a project" is a return value, and five call sites must answer it

`resolve_project_root` returns `Result<PathBuf>` and ends in
`.unwrap_or(start)`. Making absence real changes the signature, and the
seven callers split: `lock_status` and `services/artifacts.rs` already treat
failure as absence, and the five `?` sites in `project_environment.rs` each
need a decision — machine-only, or refuse. Those five are where "where it
runs decides" is actually implemented; they are five decisions, not one, and
the task list names them individually.

### The renames ride the same pass

Two renames join this change because both are grammar work, and this is the
grammar pass:

- **`agent task` → `agent work`** — the word `name-an-agents-work` already
  speaks ("an agent's work carries two names"); the checkout lives in a
  worktree; the branch is `agent/<id>`. `task` is the word left without an
  owner. Only the noun moves: `name` and its argument grammar are untouched,
  so the rename is a spelling sweep (`AgentTaskAction`, the projected
  `AGENTS.md` region, `docs/`, the naming journey) with the same behavior.
  *Alternative considered*: renaming inside `name-an-agents-work` before its
  archive — rejected; that change is implemented and its archive should
  carry the name it was built with, so the rename lands as its own grammar
  decision here.
- **`theme` → `config`** — the verb's subject outgrew its name; it chooses
  the machine's authored configuration, which is `config.toml`'s tier. The
  sub-surfaces are typed (`theme`, `icons`, `notification`), never a generic
  key/value store — authored files grow by additive keys with unknown
  reading as default, and a generic `get/set` would promise more than the
  tier rules keep. `notification` is the one new behavior: the chime's
  first CLI surface, riding on the `agent-finished-chime` choice (this
  change carries the grammar; that one carries the choice — its delta was
  extended in step).

### Ordering against the freshness change

`plugin-freshness-and-linked-marketplaces` also modifies the `plugin`
capability — the *listing* and the *update/remove* requirements — while this
change modifies *install*, *idempotence* and *direct add*. The two do not
overlap at requirement level, but whichever archives second must carry the
other's edits forward into the main spec. This change should land **after**
it: the freshness work is unblocked today, and rewriting 19 journeys and 21
Lab call sites underneath it would strand that work mid-flight.

## Candidate ADRs

- **Scope is reported, not positional** — supersedes ADR-019 §1–§3 and
  returns `inspect`/`update` to the root; expensive to reverse once scripts
  and muscle memory follow it.
- **A project is `agents.yaml`, else the repository, else `AGENTS.md`, else
  none** — changes what every project-scoped command resolves against.

## Risks / Trade-offs

- **The reported-scope rule is weaker than a positional one**: a person who
  does not read the output can still be surprised. → The surprise is bounded
  in a way ADR-019's case was not: the machine half always happens, so there
  is no "I thought I removed it from the machine and did not". The only
  variable is whether a project file moved, and that shows in `git status`.
- **`uze <p>@<m>` in a cloned third-party repository creates `agents.yaml`
  there.** → Visible, untracked, reversible; and the alternative — declaring
  nothing in a repo without a manifest — silently fails the common case of
  adopting UZE in an existing project, which is the worse error because it
  is invisible.
- **The break is wide**: 19 occurrences in `journeys/`, 21 in
  `conformance/`, 34 in `docs/`, and `06-recovery/01` proves machine-scope
  removal, which only survives because `-m` exists. → Sequenced after the
  freshness change, and the Lab keeps an explicit spelling rather than an
  ambient one.
- **`uze remove` gains a project-registry-free rule but keeps a footgun**:
  `-m` removes bytes another project may declare. → Unchanged from today,
  where `uze plugin remove` does exactly this; ADR-009's inspect-before-
  detach is the safety that exists, and no new claim is made beyond it.
