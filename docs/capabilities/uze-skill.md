# The `/uze` Skill — agentic context orchestrator

Status: **first vertical slice, implemented, 2026-08-21.** Companion to
[context-manager.md](context-manager.md), which this Skill sits entirely on
top of and never bypasses.

```
              /uze (this Skill — plugins/uze/skills/uze/SKILL.md)
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
own instructions (`plugins/uze/skills/uze/SKILL.md`'s "Hard boundaries"
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
| Claude Code | `~/.claude/skills/<entry>/SKILL.md` | Directory-name-driven: `/<entry-name>` (personal/project skills). After naming refactor, `uze` package delivers `uze` bare → `/uze`. Also autonomous, description-matched. | OFFICIAL (code.claude.com/docs/en/skills) + **EMPIRICAL** (pre-refactor listing was `uze-uze-uze`, post-refactor `uze`; verified via `exposure_name_candidates = [logical, qualified]` `claude.rs:152` + `TESTED` `exposure_naming:158`) |
| Codex | `$HOME/.agents/skills/<entry>/SKILL.md` | `$uze` (frontmatter `name`, not the delivery directory) or autonomous. `/skills` lists, does not force. | OFFICIAL (learn.chatgpt.com/docs/build-skills) + **EMPIRICAL**: `codex debug prompt-input` (real binary, no credential) listed the installed skill as `- uze:` — the clean frontmatter name, confirmed identical to what the doc predicted |
| OpenCode | `~/.agents/skills/<entry>/SKILL.md` (one of several aliases) | V1: model-invoked `skill({ name: "uze" })` autonomous only. **V2: `/uze` slash (skills listed as commands with `(Skill)` label) + autonomous.** | OFFICIAL (opencode.ai/docs/skills + opencode.ai/v2/docs/skills) + **EMPIRICAL**: `opencode debug skill` pre-refactor listed `"name": "uze"` against `".../skills/uze-uze-uze"`; V2 `slash` frontmatter `v2/docs/skills` + PR #11390 feat skills as slash commands |

**Update pós-refactor (2026-08-21):** O naming colision-avoidance original
(`uze-<package>-<skill>` → `uze-uze-uze` para este pacote) foi substituído por
`short-or-qualified` via `IntegrationPort::exposure_name_candidates`
(`crates/uze-core/src/integration.rs:359`, `crates/uze-integrations/src/claude.rs:152`).
Claude tenta `[logical, "id-logical"]` → `uze` bare vence quando livre; demais
integrações usam `["id-logical"] → uze-uze`. Antes, `uze-uze-uze` era o único nome
possível para Claude e `/uze` não funcionava (`TESTED` `exposure_naming` legacy);
depois, `uze-uze-uze` só persiste como **receipt legado** reutilizado verbatim
(`application.rs:996` existing-receipt-wins), novos installs obtêm `/uze`. A
uniformidade via `description` (autonomous trigger) permanece a mais portável,
mas agora `/uze` literal **funciona em Claude Code** sem alias.

**Observação:** Codex/OpenCode continuam usando frontmatter `name: uze`
para invocação tool (`$uze` / `skill({name:"uze"})`), independentemente do
diretório (`uze-uze` após refactor), e agora OpenCode V2 expõe Skills como
`/uze` slash também (ver Fase 1 atualizada).

CWD/project root, shell execution, and interactive confirmation were also
confirmed for all four: every harness runs its shell/bash tool with cwd set
to the session's working directory (so `uze context inspect` with no path
argument already resolves correctly with no extra plumbing), every harness
has bash/shell tool access (so calling the `uze` CLI needs no new
integration surface), and every harness supports the model asking the user
questions in ordinary conversation (OpenCode additionally has a dedicated
`question` tool/permission).

## Fase 2 — dogfooding proof: no special treatment anywhere

`plugins/uze/` is an ordinary Agent Plugins 1.0 package: `plugin.json` +
`skills/uze/SKILL.md`. It was installed with the exact same `uze add`
command as any other package, in a fully isolated `$HOME`/`$UZE_HOME`, and
delivered identically to Claude Code, Codex, OpenCode, and Antigravity CLI's
existing Skill delivery mechanisms — the same `ManagedUserScopeReference`
path every other Skill-only package already used before this milestone.

```
# antes do refactor (legado, ainda reutilizado verbatim se já instalado)
$ uze add ./plugins/uze --trust
Installed plugin: uze
Attached to claude-code: <home>/.claude/skills/uze-uze-uze
Attached to codex: <home>/.agents/skills/uze-uze-uze
Attached to opencode: <home>/.agents/skills/uze-uze-uze

# após refactor (builtin:uze, short-or-qualified)
# novos installs: claude → uze bare (/uze), demais → uze-uze
$ uze doctor
plugins: [uze builtin:uze]
Attached to claude-code: <home>/.claude/skills/uze -> state/attachments/claude/uze
Attached to codex: <home>/.agents/skills/uze-uze -> store/.../skills/uze
Attached to opencode: <home>/.agents/skills/uze-uze
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

The full boundary lives in `plugins/uze/skills/uze/SKILL.md`'s "Hard
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
Packages
  1 installed
  1 contributing here
Health
  no issues
```

## Fase 13 — self-hosting proof

| Step | Claude Code | Codex | OpenCode |
|---|---|---|---|---|
| Discovers (pós-refactor) | `~/.claude/skills/uze/` (legado `uze-uze-uze` ainda reutilizado se já existe) | `$HOME/.agents/skills/uze-uze/` | `~/.agents/skills/uze/` (V2 `slash:true`, legado `uze-uze` ainda reutilizado)
| Identifies itself as | `uze` (directory-derived bare, legado `uze-uze-uze` se receipt legado) | `uze` (frontmatter-derived) | `uze` (frontmatter, também `/uze` slash em V2)
| User can explicitly invoke via | `/uze` (legado `/uze-uze-uze` ainda funciona se instalado antes) | `$uze` | **V1:** *(autonomous only, skill tool)* / **V2:** `/uze` (skill listed as command `(Skill)`)
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

## Limitations (atualizado pós-refactor/builtin)

- Pós-refactor, `/uze` **é literal em Claude Code** (`uze` bare via `[logical, qualified]` `claude.rs:152`, `TESTED` `exposure_naming:158`). Legado `uze-uze-uze` persiste só como receipt reutilizado verbatim. Em Codex `$uze`, OpenCode V1 autônomo `skill({name:"uze"})` / **V2 `/uze` slash** (`slash: true` default, PR #11390). Natural-language trigger via `description` permanece o mais uniforme. Nenhum Core change escondeu a assimetria anterior — o naming foi corrigido para short-or-qualified.
- The agentic reasoning quality (does the Skill draft a *good* `AGENTS.md`,
  classify content well) has no automated eval yet — see
  `tests/fixtures/context-eval-scenarios/` for the fixture set and rubric a
  future runner (or manual pass) should use.
- No cross-harness automated test exercises the Skill actually running
  inside a live Claude/Codex/OpenCode session end-to-end (that would
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
