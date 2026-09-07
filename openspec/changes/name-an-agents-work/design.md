## Context

See proposal.md — Why. Three constraints shape the approach, and all three
are already load-bearing elsewhere in the codebase:

- **The four harnesses agree on very little.** Only `PreToolUse` is claimed
  by all four (`hooks.rs`), and of those four only three claim the `deny`
  effect — OpenCode's hook route carries `observe`/`allow` alone. Any
  mechanism that must work everywhere cannot be a harness feature.
- **`TaskId` keys the checkout and the persisted state**, and `task.branch`
  is read by readiness, delivery, sync and the sidebar. Making the branch
  mutable is safe only because identity is not the name — a separation
  `task.rs` already states in its own module doc.
- **The projected `AGENTS.md` region is the one instruction surface every
  harness reads**, because it is a file in the repository rather than a
  vendor capability. It is also, per `project-agent-environment` §12, only
  as current as the last reconciliation.

## Goals / Non-Goals

**Goals:**
- One name, authored once by the party that knows the work, reaching both
  the branch and the label.
- Enforcement where a harness can express it; an honest, recorded
  degradation where it cannot.
- A manual rename that is respected everywhere, by the same mechanism that
  makes it visible.

**Non-Goals:**
- Deriving a good name from text UZE happens to hold. The reconstructed
  prompt history exists and is best-effort by contract; a name derived from
  it reads like a sentence with hyphens, which is what this change exists
  to stop producing.
- A general agent-facing RPC. `uze agent` gains exactly the verbs a
  workflow step needs; a namespace is not an invitation to fill it.
- Naming a task before it exists. There is nothing to name at launch, which
  is why launch is not where naming happens.

## Decisions

### 1. The agent names the work, because only the agent knows it

The alternatives were measured rather than assumed.

*Derive from the user's prompt.* UZE already reconstructs submitted prompt
lines client-side (`PromptBuffer`) and stores them, harness-agnostically —
so this is genuinely available. Rejected on quality: a slug of a sentence
("resolver-a-questao-das-branchs-dos-agents") is the failure mode, not the
fix. It is also best-effort by contract — a chord, a history recall or a
paste marks the buffer untrusted and it is discarded — so it could not be
the only mechanism anyway.

*A harness hook that returns metadata.* The portable Hook ABI has no output
channel at all ("its stdout carries nothing"), and no prompt event exists in
the vocabulary. This would need a new event and a new data channel, and
would then work only where all four harnesses have both.

*A Skill instructing the agent.* ADR-030 is explicit that `invoke:` answers
who **may** invoke, never that anything **will**; on Antigravity neither
switch exists at all. A Skill is a suggestion gated on discovery.

**Chosen: a command the agent runs, enforced by a hook and instructed by the
projected region.** The command is the mechanism, the projected text is how
the agent learns it, and the hook is how it stops being optional.

### 2. `uze agent` is an audience, not a category

ADR-019 gave the grammar one axis: root is project-scoped, `market`/`plugin`
are machine-scoped. `uze agent` adds a second axis — who reads the command —
and it does not blur the first, because everything under it is
project-scoped.

What it buys is a matching pair: commands whose audience is the agent are
hidden from `uze --help` and documented in the projected region. Each
audience reads one surface, and neither is polluted by the other's
vocabulary. `hook-exec` is the existing precedent for a hidden,
machine-audience command; this names the pattern rather than inventing it.

Rejected: sorting the human help by frequency (treats a discovery symptom,
leaves the agent's commands in the person's list) and putting the verb at
the root (the most-typed command in the flow becomes indistinguishable from
the person's commands).

### 3. First-writer-wins, which removes a rule rather than adding one

A name that exists is never a candidate for renaming. The predicate is one
comparison for the branch (`task.branch == task.id.branch()`) and the
existing `is_generated_label` for the label — the rule `adopt_agent_labels`
already applies to tab labels, generalized rather than invented.

Three consequences, all simplifications: the precedence ladder collapses
(only one automatic rename ever happens, so there is no "when does renaming
stop" question); the `!pushed` guard becomes unnecessary; and adopting a
manual rename automatically protects it, because the adopted branch is no
longer the generated one. Reflecting a rename and refusing to overwrite it
stop being two mechanisms.

### 4. The vocabulary is closed, and the project closes it

A proposed name must be validated, and only a closed set is validatable —
which is the whole point, since the name comes from a model. `conventional`
is the market's most adopted vocabulary (Conventional Commits: spec'd,
tooled, and already what every branch and commit in this repository uses),
`gitflow` the runner-up, `flat` for GitHub Flow, `agent` for today.

A project may also declare its own list instead of a preset, because a
preset that almost fits invites misuse: `style` in Conventional Commits
means formatting, not visual design, and a team wanting `ui` should declare
`ui` rather than mislabel work as `style`. Presets are named lists; the
validation is identical either way.

### 5. The automatic half is a Git fact, not a harness feature

The design carried a `PreToolUse` `deny` here, and building it produced
three findings that removed it — kept in the record because the reasoning
is the useful part:

*It could not cover the four.* OpenCode claims `observe`/`allow` only, and
`assess` routes an unpreservable `deny` as `Unsupported` rather than
degrading it. So the mechanism chosen to answer "it has to work always"
was the one part of the design that could not.

*It cost a trust decision.* A Hook is an executable capability, and the
default plugin is bootstrapped onto every machine with `NoTrustAuthority`
— the bootstrap refuses the package outright (`TRUST_REQUIRED`). Shipping
it as a second plugin made enforcement an install and a prompt, for a
guarantee that held on three harnesses out of four.

*Its handler depended on `uze`.* ADR-040 took the binary off the hook
execution path deliberately, and the handler put it back — a plugin whose
bytes only work where UZE is installed is not a portable package.

**Chosen instead: derive at the first commit, on the evaluation pass that
already runs.** It is a Git fact, so it holds on every harness and on the
next one; it needs no plugin, no trust and no ABI; and it covers every
completion behaviour, where the publish-time fallback only ever covered
`pr` — which was the actual hole. The model still authors the name: it
wrote the commit message.

What it gives up is deliberateness — `feat/answer-ping-with-pong` instead
of the two words an agent would have chosen — which is exactly why the
projected clause says so, and why `uze agent task name` arriving first
wins.

### 6. The publish-time fallback stays, demoted

`readable_branch_name` exists and is the right shape; what was wrong was its
input (a label that is always the identifier). Re-sourced from the first
commit's subject, it becomes the answer for a task nobody named — including
every task on a harness that cannot deny. The guarantee it backs is narrow
and worth stating: no pull request ever carries a generated identifier.

## Candidate ADRs

- **An agent-audience command namespace** — `uze agent` establishes that a
  command's audience decides its surface (hidden from human help,
  documented in the projected region), which refines ADR-019's grammar and
  is expensive to move once agents are instructed to call into it.

## Risks / Trade-offs

- **[A model proposes a bad name that passes validation]** → Mitigation: the
  operator renames, and first-writer-wins guarantees nothing overwrites the
  correction. Validation constrains the shape, not the judgment.
- **[Renaming a branch under a running agent]** → Mitigation: the rename
  happens on the agent's own explicit call, from its own checkout; Git
  renames the branch HEAD follows, so its next `git` command is unaffected.
  The projected text tells it to ask Git rather than remember the name.
- **[A hook that denies commits is a hook that can block work]** →
  Mitigation: it denies exactly one condition, and the reason carries the
  one command that clears it. The group is `deny`, so a handler that cannot
  run is fail-closed by contract — which is why the handler does nothing
  but read the task's own recorded state.
- **[The projected instruction is stale, so the agent reads the wrong
  vocabulary]** → Mitigation: this is `project-agent-environment` §12, and
  the dependency is why that change lands first.
- **[`agent/` stops being how UZE's branches are found]** → Mitigation:
  `prune_integrated_branches` scans the prefix today; it must also consider
  the branches the task store names, or a renamed branch is never collected.
