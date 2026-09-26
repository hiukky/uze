# Design — agent-plugin-authoring

## Context

See proposal.md for motivation. Constraints that shape the approach:

- **The machinery to reuse already exists.** A marketplace registered from a
  local path (`marketplace.rs::add`, validating `marketplace.json`) and
  linked to a checkout (`marketplace.rs::link` → `mirror::materialize_linked`)
  is exactly the authoring state: installs read the author's working tree,
  including uncommitted files. The scaffold's job ends at producing that
  directory and calling what exists.
- **Git is already a transport, not a module to grow.** `uze-git` exposes
  generic `read`/`write` spawn entry points; `git init`, `add`, `commit` are
  argument lists, so authoring adds no spawn convention and no `uze-git`
  change. The architecture suite's spawn sanctions hold as-is.
- **The parsers to check with already exist.** `read_plugin_manifest`
  (store.rs) and its `validate_references`, `is_valid_name_component`
  (naming.rs), the `invoke:` policy parser (capability/skill.rs), the
  `hooks.json` handler contract (capability/hook.rs), the `mcp.json` model,
  and `acquisition/marketplace.rs::parse_manifest`. Check is composition,
  not a second grammar.
- **The agent-audience pattern is set.** `AgentAction` (Task, Artifacts,
  Context) is hidden from `--help`, documented in the region UZE projects
  into `AGENTS.md`; the authoring verbs join that grammar rather than
  becoming person-facing `market`/`plugin` actions.
- The change is machine-scoped end to end (ADR-019): it touches the machine
  registry and directories the agent names — never the current project's
  `agents.yaml` or `AGENTS.md` managed regions.
- **Lands after `flatten-the-command-grammar`.** That change (proposed, not
  yet implemented) removes the `uze plugin` namespace and moves machine
  scope onto the root verbs with `-m/--machine`. Every install this change
  performs — the journey, the Skill's script — is written in the post-flatten
  grammar, and the spec's scenarios are phrased in outcome terms rather than
  in the doomed spelling; the authoring verbs themselves join the `agent`
  grammar, which flatten does not touch.

## Goals / Non-Goals

**Goals:**

- One command per artifact: market, plugin, and each optional capability
  file — deterministic, non-interactive, parseable answer, non-zero exit with
  the reason on failure.
- A scaffolded marketplace is installable-from immediately (born registered
  and linked); a scaffolded plugin is installable immediately (manifest entry
  added at scaffold time).
- The `check` answer is the same validation install would apply, delivered
  offline — never a weaker duplicate grammar.
- The scaffold's embedded documentation cannot drift: every scaffolded
  layout must pass its own `check`.

**Non-Goals:**

- Publishing/remote push (Git the author runs), TUI wizardry, changes to what
  install delivers, any new external dependency.
- **A person-facing surface for authoring, now or later.** The operator's
  rule: the person's CLI carries only what a person performs by hand; what
  is agent control lives under `uze agent`. Authoring a plugin is agent
  work — a person who wants to browse marketplaces and their plugins uses
  the existing `uze market` surface. The two-surface split this avoids is
  the duplication the flatten change removes for the rest of the grammar;
  authoring simply never grows the second half.

## Decisions

### D1 — Authoring lives in a new `package/authoring` concern (core) + `application/authoring` (orchestration)

Scaffold templates and the check model go to `crates/uze-core/src/package/authoring.rs`
(+ `authoring/` if it outgrows one file): producing the bytes a package is
acquired *from* is the `package` concern's own question — "where a package's
bytes come from" — so it sits beside `acquisition` rather than a sixth
concern. Application orchestration (scaffold → register → link;
scaffold → manifest entry; check → report) is one `application/authoring.rs`
module beside `lifecycle/`, which stays the install/remove/update/delivery
quartet.

*Alternative considered*: a new top-level `authoring` concern in core —
rejected; nothing in it is outside `package`'s stated subject, and the
concern list is closed for good reason.

### D2 — CLI grammar: two hidden `AgentAction` variants, verbs mirroring the person's nouns

`AgentAction::Market(AgentMarketAction::Create { ... })` and
`AgentAction::Plugin(AgentPluginAction::{Create { ... }, Check { ... }})`,
dispatched from `run_agent`. The nouns match the person's `market`/`plugin`
verbs so an agent's output reads the same either side; the verbs stay
machine-scoped and never touch project state.

*Alternative considered*: a single `AgentAction::Author(AuthorAction)` —
rejected; it invents a third noun for surfaces the grammar already names.

### D3 — Create registers and links in one step, reusing `marketplace add` + `link`

After scaffolding files and committing, `UzeApplication` runs the existing
`marketplace add` (Local source, full manifest validation) then `link`. The
marketplace's bytes stay in the directory the agent named; only the record
lives in `$UZE_HOME`. `plugins: []` scaffolds to an empty array — `parse_manifest`
must accept it (a marketplace with no plugins is a valid born state);
verification is part of the scaffold's first test.

*Alternative considered*: a special-cased "authoring record" — rejected; two
registry paths for one record is the drift `market link` just removed.

### D4 — Git identity: detect, explain, hand over — never invent authorship

The initial commit runs with the machine's own Git identity. When none is
configured, the create fails with the two `git config` commands the author
runs — the same detect → explain → hand-the-command shape as
`plugin-requirements`. UZE never fabricates an author line in the author's
repository.

### D5 — Check composes the install-time parsers into one report

`authoring::check` walks the authored directory and collects findings
(location, reason) from the existing parsers, plus containment checks
(plugins resolve inside the marketplace; no absolute/`..` paths — reusing
`validate_references`). The read model is a `ValidationReport` (list of
findings + what the artifact would deliver); CLI exit is non-zero iff any
finding. Marketplace check runs the plugin check per resolved entry.

*Alternative considered*: a purpose-written lint DSL — rejected; a second
grammar is exactly the scattered-compatibility failure this workspace
refuses.

### D6 — Templates are constant strings guarded by the scaffold-passes-its-own-check invariant

Scaffold templates are inline `const` documents rendered with the standard
library. The load-bearing test: for every flag combination, render the
scaffold into a scratch tree and run `authoring::check` over it — clean.
The commented field documentation cannot drift from the parsers because a
drift fails the suite mechanically (the `agents.yaml` scaffold precedent,
made stronger: commentary is validated by the *check*, not by a string diff).

### D7 — The `uze:author` Skill is authored in `plugins/uze`, delivered by the normal pipeline

`plugins/uze/skills/author/SKILL.md` with `invoke: {model: true, user: true}`
and a body that conducts the loop (see spec). Root `marketplace.json`
description grows the authoring mention. No integration change: Skill
delivery already carries it everywhere.

### Mermaid diagrams

No new container, component or dependency edge: authoring is a module in
existing crates speaking to existing ones (`src/` → uze-application →
uze-core, Git via `uze-git`). No diagram under `docs/architecture/` changes.

## Candidate ADRs

- **Agent-first authoring surface** — authoring lives under the hidden
  `uze agent` audience, its bytes in a directory the author names, and only
  a registry record in `$UZE_HOME`; a scaffolded marketplace is born linked;
  and the person-facing CLI never gains authoring verbs (the person's
  surface is hand-work only; `uze market` answers browsing). Hard to reverse
  once agents are taught it (the Skill and the `AGENTS.md` region both
  encode it), and it fixes where authoring may *never* live: no authored
  marketplace bytes under `$UZE_HOME`, and no person-facing alias, ever.

## Risks / Trade-offs

- [Registry validation may reject an empty `plugins: []`] → verify in D3's
  first test; if the parser requires an entry, the manifest parser gains the
  born-empty case rather than the scaffold growing a fake plugin.
- [Authoring verbs imply filesystem writes outside any sandbox] → they are
  the author's own machine and the verbs are the existing `agent` trust
  class; containment is still enforced (plugins must resolve inside their
  marketplace; `--at` must be an empty or absent directory).
- [Scaffold overwriting user work] → every create refuses to overwrite
  (market name registered, directory non-empty, plugin exists) and fails
  before writing anything.
- [`check` must not outpace install] → check composes the *same* parsers
  install uses; a check-clean artifact that install then rejects is a bug
  naming the parser that disagreed (covered by the journey).

## Migration Plan

Additive only: new verbs, new module, one new official Skill. Rollback is
removing the verbs; scaffolded artifacts are the author's own text (authored
tier) and survive — the registry record is the only UZE-owned state, already
tiered under `state/`.

## Open Questions

None — location, audience, Git behaviour and the Skill were resolved with
the operator during proposal.
