# The `/uze` Skill — agentic context orchestrator

Status: **first vertical slice, implemented, 2026-08-21.** Companion to
[context-manager.md](context-manager.md), which this Skill sits entirely on
top of and never bypasses.

```
              /uze (this Skill — packages/uze/skills/uze/SKILL.md)
                 reasoning / orchestration
        ┌────────────────┼────────────────┐
        ▼                ▼                ▼
     analyze          propose          confirm
        │                                  │
        └────────────────┬─────────────────┘
                          ▼
                  Context Manager
                deterministic layer (unmodified)
        ┌────────────────┼────────────────┐
        ▼                ▼                ▼
     inspect            plan          reconcile
```

The Skill reasons. `uze` mutates. That boundary is enforced by the Skill's
own instructions (`packages/uze/skills/uze/SKILL.md`'s "Hard boundaries"
section), not by anything in `uze-core` — the Core has no idea this Skill
exists, and never will (see the vendor-neutrality/no-hardcoding proof
below).

## Fase 1 — how Skill invocation actually works, per harness

Researched at three distinct confidence levels, per the brief's own
instruction not to blur them: **OFFICIAL** (vendor docs), **EMPIRICAL**
(observed against a real binary in this session, credential-free where
possible), **SOURCE_CONFIRMED** (read directly from first-party source when
docs didn't cover it).

| Harness | Discovery path (matches UZE's existing delivery unchanged) | How a user invokes it | Evidence |
|---|---|---|---|
| Claude Code | `~/.claude/skills/<entry>/SKILL.md` | Directory-name-driven: `/<entry-name>`. **Not** `/uze` — see the finding below. Also: autonomous, description-matched, no typing required. | OFFICIAL (code.claude.com/docs/en/skills) + **EMPIRICAL**: this session's own live Skill listing showed the installed package exactly as `uze-uze-uze`, matching the doc's stated rule before any assumption was made |
| Codex | `$HOME/.agents/skills/<entry>/SKILL.md` | `$uze` (frontmatter `name`, not the delivery directory) or autonomous. `/skills` lists, does not force. | OFFICIAL (learn.chatgpt.com/docs/build-skills) + **EMPIRICAL**: `codex debug prompt-input` (real binary, no credential) listed the installed skill as `- uze:` — the clean frontmatter name, confirmed identical to what the doc predicted |
| OpenCode | `~/.agents/skills/<entry>/SKILL.md` (one of several aliases) | Model-invoked tool call `skill({ name: "uze" })`, autonomous only — no manual command exists at all. | OFFICIAL (opencode.ai/docs/skills) + **EMPIRICAL**: `opencode debug skill` (real binary) listed `"name": "uze"` against `"location": ".../skills/uze-uze-uze/SKILL.md"` — confirms the tool-facing identifier is the clean frontmatter name even though the delivery path is namespaced |
| Gemini CLI | `~/.agents/skills/<entry>/SKILL.md` alias (confirmed in `skillManager.ts`) | Autonomous; `/skills` is a management command (list/link/enable/disable per `skillsCommand.ts`), not per-skill invocation. | SOURCE_CONFIRMED (`packages/core/src/skills/{skillLoader,skillManager}.ts`, `packages/cli/src/ui/commands/skillsCommand.ts`) — frontmatter `name`/`description` parsed directly, independent of directory, by the same code path OpenCode's pattern uses; **not independently re-run live this session**, so the exact tool-facing identifier for Gemini specifically is treated as a strong hypothesis, not confirmed to the same empirical standard as Codex/OpenCode |

**The one real, load-bearing finding this research produced:** UZE's
existing, general-purpose collision-avoidance naming
(`uze-<package_id>-<skill_name>`, unchanged since M1, used for *every*
package's Skill so two unrelated packages can't clobber each other in a
shared discovery directory) means Claude Code alone resolves the delivered
directory name — `uze-uze-uze`, given this package's id and this skill's
folder are both literally "uze" — into the command a user would have to
type. **`/uze` does not work as literally typed in Claude Code.** Codex and
OpenCode are unaffected because both key their user-facing/tool-facing
identifier off the SKILL.md frontmatter `name:` field, not the delivery
directory.

This was not assumed — it surfaced from dogfooding the package through the
real, unmodified pipeline (Fase 2, next section) and was then confirmed
against Claude Code's own documented naming rule
(`code.claude.com/docs/en/skills`, "How a skill gets its command name":
*"In a personal or project skill, `name` sets only the display label shown
in skill listings, and the command still comes from the directory name."*).

**No stop condition triggered.** Every harness genuinely supports the
Skill — discovery works everywhere, autonomous natural-language triggering
works everywhere, and three of four also support a clean explicit mention.
This is exactly the "harnesses differ in exact syntax" case the brief
explicitly permitted, not a "harness refuses" case. What it rules out is
**marketing `/uze` as a universal literal command** — the honest, uniform
UX across all four harnesses is *describing intent in natural language*
("check if my project's context is portable"), which every harness's
autonomous-triggering path already handles via the Skill's `description`
field. The Skill's frontmatter (`packages/uze/skills/uze/SKILL.md`) was
written accounting for this from the start: its `description` is
front-loaded with concrete trigger phrases specifically because that field,
not any slash command, is what's actually uniform.

CWD/project root, shell execution, and interactive confirmation were also
confirmed for all four: every harness runs its shell/bash tool with cwd set
to the session's working directory (so `uze context inspect` with no path
argument already resolves correctly with no extra plumbing), every harness
has bash/shell tool access (so calling the `uze` CLI needs no new
integration surface), and every harness supports the model asking the user
questions in ordinary conversation (OpenCode additionally has a dedicated
`question` tool/permission).

## Fase 2 — dogfooding proof: no special treatment anywhere

`packages/uze/` is an ordinary Agent Plugins 1.0 package: `plugin.json` +
`skills/uze/SKILL.md`. It was installed with the exact same `uze add`
command as any other package, in a fully isolated `$HOME`/`$UZE_HOME`, and
delivered identically to Claude Code, Codex, OpenCode, and Gemini CLI's
existing Skill delivery mechanisms — the same `ManagedUserScopeReference`
path every other Skill-only package already used before this milestone.

```
$ uze add ./packages/uze --trust
Installed plugin: uze
Attached to claude-code: <home>/.claude/skills/uze-uze-uze
Attached to codex: <home>/.agents/skills/uze-uze-uze
Attached to opencode: <home>/.agents/skills/uze-uze-uze
Attached to gemini: <home>/.agents/skills/uze-uze-uze
```

**Structural proof it needed no hardcoding**
(`tests/uze_skill.rs::the_package_receives_no_special_treatment_a_renamed_copy_behaves_identically`):
a byte-identical copy of the SKILL.md, installed under a *different*
package id, installs and is discovered exactly the same way. Nothing in the
Store, router, or any integration references the string `"uze"` as a
package identity.

```
$ grep -rn '"uze"' crates/uze-core/src/store.rs crates/uze-core/src/engine.rs crates/uze-core/src/router.rs
(no matches)
```

## Fase 3 — responsibilities (enforced in the Skill's own text, not in code)

The full boundary lives in `packages/uze/skills/uze/SKILL.md`'s "Hard
boundaries" section. Summary: the Skill may read files, analyze the
project, propose content, ask questions, and write *user-owned* content
(new `AGENTS.md` prose before it exists, content moved within a vendor file
with the user's approval) using its own normal file tools. It may never
touch anything between `<!-- uze:begin -->`/`<!-- uze:end -->` markers, invent
its own marker/receipt mechanics, or apply any change without explicit
confirmation. This is prompt content, not Rust — `context.rs`/`text_region.rs`
gained zero new lines to support this Skill.

## Fase 10 — `uze status` vs `uze doctor`: kept separate

`doctor` has no `project_root` parameter and never will — it is a pure
statement about *this machine's UZE installation* (Store health,
harness detection/provisioning, package-level attachment state), and giving
it a project-scoped mode would blend two genuinely different questions the
way earlier milestones were careful to keep apart (installation health vs.
project state). `status` (`UzeApplication::status`, `uze status [path]`) is
new, always project-scoped, and built almost entirely by composing
`context_inspect` (already read-only) with the Store's own package count —
it introduces no new health-detection logic of its own:

```
$ uze status
Project
  Context       PORTABLE
Harnesses
  claude-code  bridged (Matched)
  codex        native
  opencode     native
  gemini       bridged (Matched)
Packages
  1 installed
  1 contributing here
Health
  no issues
```

## Fase 13 — self-hosting proof

| Step | Claude Code | Codex | OpenCode | Gemini CLI |
|---|---|---|---|---|
| Discovers | `~/.claude/skills/uze-uze-uze/` | `$HOME/.agents/skills/uze-uze-uze/` | `~/.agents/skills/uze-uze-uze/` (one of several alias roots) | `~/.agents/skills/uze-uze-uze/` alias |
| Identifies itself as | `uze-uze-uze` (directory-derived) | `uze` (frontmatter-derived) | `uze` (frontmatter-derived) | `uze` (frontmatter-derived, by source inspection) |
| User can explicitly invoke via | `/uze-uze-uze` | `$uze` | *(no manual form exists)* | *(no manual form exists)* |
| User can invoke via natural language | Yes | Yes | Yes (primary mechanism) | Yes (primary mechanism) |
| Can call `uze` CLI | Yes (bash tool) | Yes (shell tool) | Yes (bash tool) | Yes (shell tool) |
| Project root available | Yes, session cwd | Yes, session cwd | Yes, session cwd | Yes, session cwd |

**What UZE can hide:** the delivery mechanism (identical `Skill` resource,
identical `ManagedUserScopeReference` attachment, one package, one
install). **What UZE cannot and should not hide:** the exact
invocation syntax a human types, or whether a manual form exists at all —
those are real, harness-owned facts, and the Skill's `description` was
written to make natural-language triggering the thing that's actually
uniform, rather than pretending a literal `/uze` command is universal when
it demonstrably isn't.

## Limitations

- `/uze` is not a literal, universal command. It works in Codex (`$uze`)
  and as natural-language auto-trigger everywhere; in Claude Code the exact
  typed command is `/uze-uze-uze`, a direct, unavoidable consequence of
  UZE's existing (correct, necessary) collision-avoidance naming applied to
  a package and skill both named "uze." No Core or naming-scheme change was
  made to paper over this — see stop-condition review below.
- Gemini CLI's exact tool-facing identifier was not independently
  re-confirmed live this session (unlike Codex/OpenCode); the claim rests
  on source-code inspection, one level below the empirical standard applied
  to the other three.
- The agentic reasoning quality (does the Skill draft a *good* `AGENTS.md`,
  classify content well) has no automated eval yet — see
  `tests/fixtures/context-eval-scenarios/` for the fixture set and rubric a
  future runner (or manual pass) should use.
- No cross-harness automated test exercises the Skill actually running
  inside a live Claude/Codex/OpenCode/Gemini session end-to-end (that would
  require credentials); the deterministic contract (install, discovery,
  JSON shape, ownership) is tested, the reasoning is not.

## Stop conditions reviewed

None triggered. In particular: the Store never learned this Skill is
"official" (proven by the renamed-copy test); `/uze` never gained the
ability to edit managed regions (enforced only in the Skill's own prompt
text, which is not a way to grant capability, only to request behavior —
the actual technical enforcement is unchanged: `uze context reconcile` is
still the only code path that writes a managed region, so a
misbehaving/adversarial Skill invocation still cannot corrupt state any
worse than a human running arbitrary shell commands already could); no
reasoning entered `context.rs`; no integration gained a `/uze`-specific
API; `uze add` was not touched.
