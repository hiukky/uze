## 1. The name and what validates it

- [x] 1.1 `worktree::BranchVocabulary` — a preset (`conventional`,
  `gitflow`, `flat`, `agent`) or a project's own list, deserialized from
  one `worktrees.branch` key that accepts either form. A preset is a named
  list; validation does not know which spelling it came from.
- [x] 1.2 `worktree::validate_name` — the type is in the vocabulary, the
  subject is one well-formed segment (lowercase, `[a-z0-9-]`, no separator,
  bounded length, no double hyphen). It returns *which half* failed: a
  refusal an agent cannot act on is a refusal it will retry wrong.
- [x] 1.3 The label is the subject with hyphens as spaces. One derivation,
  in Core, so the sidebar and the branch cannot disagree about what a name
  means.
- [x] 1.4 `WorktreePolicy` carries the vocabulary and `deny_unknown_fields`
  still holds; an undeclared `branch:` resolves to `agent`, which is
  today's behavior exactly.

## 2. Naming a task

- [x] 2.1 `Task` gains no field: `branch` and `label` already exist and
  become writable. The generated-name predicate is
  `task.branch == task.id.branch()` for the branch and the existing
  `is_generated_label` shape for the label — no third notion of "unnamed".
- [x] 2.2 `Workspace::name_task(cwd, name)` — resolve the task from the
  checkout (`isolated_checkout` → `CheckoutId` → the slot's current owner,
  by `max_by_key(created_at_unix)` as `reconcile` already does), validate,
  `git branch -m` under the repository write lock, persist. Refuses when
  the task already carries a chosen name, when the branch exists, and when
  the checkout is mid-rebase.
- [x] 2.3 First-writer-wins is one guard in one place, not a check repeated
  at each call site. Every later mechanism asks the same predicate.

## 3. The checkout's HEAD is the truth

- [x] 3.1 `evaluate_tasks` re-reads `current_branch` for each live task's
  checkout and adopts it when it differs and is not detached. This is the
  fix for the real defect: a hand-renamed branch makes
  `commits_ahead` answer `0` through its `unwrap_or`, so the task never
  reaches `Ready` and delivery is never offered.
- [x] 3.2 `checkout::prune_integrated_branches` stops assuming the `agent/`
  prefix: it scans the prefix *and* the branches the task store names, or a
  renamed branch is never collected.
- [ ] 3.3 `reconcile`'s adoption of an unrecorded checkout already takes the
  branch as it finds it — confirmed by reading, **not yet pinned by a
  test**. It is covered incidentally (the acceptance test names an adopted
  checkout), never directly.

## 4. The agent surface

- [x] 4.1 `uze agent task name <type>/<subject>` — `Command::Agent` with a
  `task` noun, `hide = true`, project-scoped. No identifier argument: one
  agent must not be able to rename another's branch.
- [x] 4.2 Classify it in `command_performance.rs`. It is `Budgeted` (a task
  store read, a validation, one Git rename) — and check
  `every_cli_command_is_classified` treats a hidden nested leaf the way this
  assumes before relying on it.
- [x] 4.3 The refusal messages are written for a model: they name the
  vocabulary in force and the exact command form. This is the only feedback
  channel a denied agent has.

## 5. Enforcement, and its honest coverage

- [x] 5.1 A new official plugin, `plugins/uze-naming`: one `PreToolUse`
  group, effect `deny`, matched to the commit-shaped tool call, with a
  handler that exits `DENY_EXIT_CODE` when the task owning its checkout is
  unnamed. **Its own plugin, not `plugins/uze`** — a Hook is an executable
  capability, and the default plugin is bootstrapped onto every machine, so
  shipping one there means every machine authorizing one without being
  asked. The bootstrap proved it by refusing to install the package at all.
- [x] 5.2 *(built differently from the plan)* The handler shells out to
  `uze agent task guard` rather than reading recorded state itself. The
  plan wanted no subprocess; what it did not account for is that the
  question is three facts — does this project name its work, is this an
  agent's checkout, is this task still generated — and a POSIX `sh` script
  answering them would re-derive all three from files it has no business
  parsing, wrongly and differently per harness. The subprocess is the
  smaller risk, and it is guarded: no `uze` on `PATH` exits `0`, so the
  guard fails **open**. A guard that blocks work because UZE moved is worse
  than a guard that misses a commit.
- [x] 5.3 It scopes itself to an isolated checkout: the operator's own
  commits in the primary are never the subject.
- [x] 5.4 Coverage is recorded, not claimed: Claude, Codex and Antigravity
  honor `deny` on `PreToolUse`; OpenCode claims `observe`/`allow` only, so
  it degrades to the projected instruction with the reason stated (ADR-033).

## 6. The fallback nothing should need

- [x] 6.1 `landing::readable_branch_name` is re-sourced from the first
  commit's subject on the task's branch, not from the label. Conventional
  type becomes the segment where the subject carries one; otherwise the
  project's vocabulary decides.
- [x] 6.2 It applies only to a task nobody named, and never to a branch
  already published.

## 7. Projection

- [x] 7.1 `WorktreePolicy::instructions()` gains the naming clause, naming
  the command and *this project's* vocabulary — so the instruction an agent
  reads is the one its project will accept. `region_identity()` is a digest
  of these bytes, so a vocabulary change re-projects on its own.
- [x] 7.2 The clause tells the agent to ask Git for its branch rather than
  remember it, because the name can change under it.
- [x] 7.3 Depends on `project-agent-environment` §12: a policy that does not
  reach the projected region is a policy agents never read.

## 8. Tests

The same invariant at more than one level is deliberate here (`tests/README.md`):
the naming rule is a pure function at L0, a lifecycle at L1, and a user's
flow at L3.5 — and each catches a different way of being wrong.

- [x] 8.1 **L0 — validation and derivation** (`uze-core`, `#[cfg(test)]`):
  every preset accepts its own types and refuses the others; a project list
  behaves identically to a preset of the same members; a malformed subject
  is refused per half (empty, separator, over-length, double hyphen, upper
  case); the label derivation is the inverse of the subject spelling.
- [x] 8.2 **L0 — first-writer-wins**: the generated-name predicate is true
  exactly for `agent/<id>` and the generated label shapes, and false for
  everything else, including a name that merely *looks* generated
  (`agent/fix-thing`).
- [x] 8.3 **L1 — naming a task** (`tests/workspace/`): naming from a nested
  directory inside the checkout names the right task; naming from the
  primary fails and renames nothing; a slot that served an earlier task
  names its current owner; a second naming call is refused and the first
  name stands; a name colliding with an existing branch is refused and the
  branch is untouched.
- [x] 8.4 **L1 — the HEAD is the truth** (`tests/workspace/`): a branch
  renamed with Git outside UZE is adopted by the next evaluation; a task
  whose branch was renamed by hand still reaches `Ready` and is delivered
  (the regression test for the `unwrap_or(0)` defect); a checkout mid-rebase
  leaves the recorded branch alone.
- [x] 8.5 **L1 — delivery and collection** (`tests/lifecycle/`): a named
  task publishes under its own name; an unnamed one publishes under the name
  derived from its first commit; a published branch is never renamed again;
  `prune_integrated_branches` collects a renamed, integrated branch that no
  longer carries the `agent/` prefix.
- [x] 8.6 **L1 — the hook** (`tests/integrations/`): the group is emitted
  for the three harnesses that honor `deny` and recorded as degraded for the
  one that does not; the handler denies an unnamed task and allows a named
  one; it stays silent outside an isolated checkout.
- [~] 8.7 **L0 — projection** (`uze-core`, not `tests/projection/`): the
  projected region carries the declared vocabulary, and changing the
  vocabulary changes the region's identity — both written. The third claim,
  that two machines with the same declaration project the same bytes, is
  **not written**: `add-portable-worktree-policy` §10.4 already pins it for
  the policy as a whole, and the vocabulary rides in the same bytes.
- [x] 8.8 **UI (`src/ui/`, `TestBackend`)**: the sidebar renders the label
  and the adopted branch, and a long name is elided rather than cut. This is
  where a claim about what the screen *says* belongs — a journey may only
  gate on screen text, never assert it.
- [x] 8.9 **L3 — acceptance** (`tests/acceptance/`): through the real
  binary, `uze agent task name` in a placed agent's checkout renames the
  branch and the recorded label, and the command is absent from `uze --help`
  while still working — the pair that proves "hidden" means hidden from the
  person, not disabled.
- [x] 8.10 **L3.5 — journey**, `journeys/suites/04-workspace/`, a new file
  rather than a scene on `01-agents-and-slots`: it needs a world of its own
  (a manifest declaring a vocabulary), and its claim does not depend on that
  story. Scenes: an agent is placed and its branch is the generated one;
  the agent names its work and `git` in the checkout reports the new branch
  while the task store records the same name and the slot directory is
  unchanged; the operator renames the branch by hand and the next evaluation
  records *that* name, unchanged by anything automatic. Every `then` reads
  Git, the filesystem or the task store — never UZE's own report; screen
  text appears only as `expect`. Tagged `gate`.
- [x] 8.11 **L3.5 — journey**, `journeys/suites/05-delivery/`: a task
  nobody named is delivered, and the branch the remote receives carries a
  readable name rather than the identifier — read from the bare remote with
  Git, not from UZE's delivery report. A separate file from 8.10: different
  world, different cadence, and a shared file would mean a shared fate.
- [x] 8.12 Both journeys name the page they back in `proves:`, and
  `journey validate` passes — every gesture states its `expect`.

## 9. Documentation

- [x] 9.1 `docs/architecture/invariants.md`: first-writer-wins, and the
  checkout's HEAD as the truth about a task's branch, each tied to the test
  that proves it.
- [x] 9.2 The `worktrees.branch` vocabulary in the manifest scaffold
  `manifest::ensure_exists` writes, spelled out and commented like
  `completion` already is — the knobs are discoverable by opening the file.
- [x] 9.3 The page the journeys name in `proves:` describes naming, or
  `journey validate` fails on a `proves:` that does not resolve.
