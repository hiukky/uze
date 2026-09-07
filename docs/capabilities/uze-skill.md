# UZE's own Skills

`plugins/uze` is the one official plugin, and it is an ordinary Agent Plugins
1.0 package: `plugin.json` plus a `skills/` directory. It carries two Skills.

| Skill | What it reasons about |
|---|---|
| `uze:init` | Portable project context. Delegates every managed mutation to the deterministic Context Manager (`uze context inspect \| plan \| reconcile`) and never bypasses it — see [context-manager.md](context-manager.md). |
| `uze:worktree` | Git workspace ownership: when to isolate concurrent writes, how to hand off a branch, and when it is safe to integrate. It reads the project's `worktrees:` policy from `agents.yaml` before creating anything, uses Git directly, and has no Context Manager mutation authority. |

**The Skill reasons; `uze` mutates.** That boundary lives in each SKILL.md's
own "Hard boundaries" section, not in Rust: `uze-core` has no idea these
Skills exist. A Skill may read files, analyze a project, propose content, ask
questions, and write *user-owned* content with its own file tools. It may never
touch anything between `<!-- uze:begin -->` / `<!-- uze:end -->` markers, invent
its own marker or receipt mechanics, or apply a change without confirmation.

The enforcement that actually holds is unchanged by any of that:
`uze context reconcile` is still the only code path that writes a managed
region, so a misbehaving invocation cannot corrupt state any worse than a human
running shell commands already could.

## How each harness invokes them

Delivery is `ManagedUserScopeReference` — the same path every Skill-only package
uses. The label is the stable plugin-qualified name (ADR-026); the *prefix* is
the harness's own.

| Harness | Discovery path | Explicit invocation |
|---|---|---|
| Claude Code | `~/.claude/skills/uze:init/` | `/uze:init` |
| Codex | `~/.agents/skills/uze:init/` | `$uze:init` |
| OpenCode | `~/.agents/skills/uze:init/` (shared root with Codex) | `@uze:init` — a **mention**, not a slash command |
| Antigravity | its global skills root | `/uze:init` |

All four also select the Skill autonomously from its `description`, which is
the trigger that is genuinely uniform. The invocation *syntax* is a
harness-owned fact and uze does not pretend otherwise — OpenCode V2 moved from
`/name` to `@name`, and the Lab types what each harness's own user types
(`conformance/harnesses/<vendor>/bindings.py::invoke`) rather than reading a
catalog, which is what caught it — the generated
[compatibility matrix](../../web/content/docs/harnesses.mdx) is derived from
each integration's own `invocation_prefix()`, so it cannot drift from the code.

Every harness runs its shell tool with cwd set to the session's working
directory, so `uze context inspect` with no path argument already resolves
correctly, and every harness can ask the user a question in ordinary
conversation. Neither needed new integration surface.

## No special treatment, proven structurally

`tests/uze_skill.rs::the_package_receives_no_special_treatment_a_renamed_copy_behaves_identically`
installs a byte-identical copy of the SKILL.md under a *different* package id
and asserts it is discovered exactly the same way. Nothing in the Store, the
router, or any integration references `"uze"` as a package identity.

## What is not tested

- **Reasoning quality** — whether the Skill drafts a *good* `AGENTS.md`, or
  classifies content well, has no automated eval. The fixture set and rubric a
  manual pass should use are in `tests/_fixtures/scenarios/eval/` (the L4 tier
  in `tests/README.md`).
- **End-to-end inside a live session** — running the Skill inside a real
  credentialed Claude/Codex/OpenCode session is not exercised anywhere. The
  deterministic contract around it (install, discovery, JSON shape, ownership)
  is.

## `uze status` vs `uze doctor`

Kept separate on purpose. `doctor` has no `project_root` and never will: it is
a statement about *this machine's* uze installation — Store health, harness
detection and provisioning, package-level attachment state. `status` is always
project-scoped and composes `context_inspect` with the Store's package count,
introducing no health-detection logic of its own.

```
$ uze status
Project
  Context       PORTABLE
Harnesses
  claude-code  bridged (Matched)
  codex        native
  opencode     native
Packages
  1 installed
  1 contributing here
Health
  no issues
```
