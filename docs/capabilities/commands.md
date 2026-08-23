# Commands / Actions — capability spec

Companion to [landscape.md](landscape.md). **Supersedes the 2026-08-21
research pass** (which recommended not modeling Commands; see the ADR
record and below for why that recommendation is withdrawn). Implemented
per [ADR-025](../adr/025-commands-as-first-class-capability.md).

## Model

**Skill** — model-discoverable reusable knowledge/workflow; may not be
directly user-invokable.

**Command** — explicitly user-invokable prompt/action surface (`/name` or
equivalent in a harness), with its own identity and a distinct UX.

They may share content patterns; they are **not** the same capability. UZE
never reinterprets a Skill as a Command and never claims Command delivery
as Native through a Skip-only collapse.

Canonical command (v0, minimal by design):

```
plugin/
├── plugin.json
├── skills/
│   └── review/SKILL.md      ← Skill `review`
└── commands/
    └── review.md            ← Command `review` (file stem = name)
├── mcp.json                 ← MCP, unchanged
```

- One file = one command; flat `commands/` directory (nested namespaces
  are a vendor naming concern, not canonical).
- Optional YAML-style frontmatter; the only consumed field is
  `description`. The body is the prompt, preserved **verbatim**.
- No universal argument placeholder in v0 (see Arguments).

## Harness support matrix (official docs, verified 2026-08-23)

| Harness | Native custom commands | Format | Arguments | Delivery | UZE route |
|---|---|---|---|---|---|
| Claude Code | Yes (plugin `commands/` dir + manifest `commands` field; `~/.claude/commands` legacy still works; vendor has *merged* the concept into Skills — a command file and a skill file both create `/name`) | flat `.md` | `$ARGUMENTS` | explicit/generated native plugin | **NATIVE** (package) |
| OpenCode V2 | Yes (`~/.config/opencode/commands/`, `.opencode/commands/`, config `commands` key) | `.md` + frontmatter (`description`, `agent`, `model`, `subtask`=ignored) | `$ARGUMENTS`, `$1..$N`; `` !`cmd` `` shell interpolation | managed symlink to canonical file | **NATIVE** (direct standard) |
| Gemini CLI | Yes (`~/.gemini/commands/*.toml`, project-level, extension `commands/` dir) | TOML (`prompt` required, `description` optional) | `{{args}}`; `!{...}` (shell, confirmed before run); `@{...}` (file) | generated user-scope TOML; generated extension commands | **NATIVE** (generated) |
| Codex | **No custom-command format** — `~/.codex/prompts/*.md` officially **deprecated** in favor of Skills — but an official **explicit-invocation-only Skill** mechanism exists | `SKILL.md` + `agents/openai.yaml` (`policy.allow_implicit_invocation: false`) | — (out of v0 scope) | generated user-invokable Skill with explicit-only policy | **NATIVE** (official explicit-only Skill) |

Per-harness classification: Claude `NATIVE` (package), OpenCode `NATIVE`
(direct), Gemini `NATIVE` (generated), Codex `NATIVE` (official
explicit-invocation-only Skill mechanism — semantics preserved through a
supported primitive, per the definition below; the physical artifact is a
Skill, the canonical identity stays `Command`). Never `UNKNOWN`-by-default:
each is stated from current official behavior.

## Routing & coverage

```
Native Package (explicit) > Generated Native Package > Native Capability
> Safe Adaptation (explicitly marked) > Unsupported
```

Exact package coverage is mandatory and per-surface:

- Claude generated envelope covers `skills/` + `commands/` + `mcp.json`
  servers — nothing else; an explicit envelope covers only what its
  `commands` field / conventional `commands/` directory declares.
- Gemini generated extension covers commands it can represent (canonical
  `.md` translated to `.toml`); an **explicit** extension never claims
  canonical commands (its commands are vendor TOML) — those fall back to
  capability-level generated user-scope TOML.
- Codex generated plugin cannot represent commands at all (its native
  format has no command surface); commands fall back to the NATIVE
  explicit-invocation-only Skill route — never blanket-covered.

## Arguments

Not portable in v0, and deliberately not translated:

| Harness | Placeholder |
|---|---|
| OpenCode | `$ARGUMENTS`, `$1..$N` |
| Gemini | `{{args}}` (shell-escaped inside `!{...}`) |
| Claude | `$ARGUMENTS` |

No proven, safe universal mapping exists; a canonical placeholder would be
premature portability. Bodies are delivered verbatim; argument-less
commands work everywhere via each harness's default argument appending.
Generated artifacts never synthesize vendor placeholders.

## Codex explicit-only mechanism (Command semantics audit)

A canonical Command must not be auto-selected by the model. Codex's
official mechanism for that is skill metadata:

```
~/.agents/skills/<name>/
├── SKILL.md               # name + description + verbatim command body
└── agents/openai.yaml     # policy.allow_implicit_invocation: false
```

Per Codex's Build skills documentation, `allow_implicit_invocation`
(default `true`), when `false`, means *"Codex won't implicitly invoke the
skill based on user prompt; explicit `$skill` invocation still works."*
UZE generates exactly this file for every Command adaptation — without it
the generated artifact would be an ordinary model-discoverable skill, a
silent semantic loss.

Empirically verified against codex-cli 0.149.0 (deterministic, zero model
calls; `codex debug prompt-input` with an isolated HOME):

| Setup | Model-visible skills list |
|---|---|
| plain Skill (no metadata) | skill present (model-invocable) |
| skill + valid `allow_implicit_invocation: false` | skill **absent** (model cannot see it) |
| skill + malformed `openai.yaml` | skill present again (control: file is genuinely read) |

Codex supports symlinked skill folders and follows the link target, so the
managed `~/.agents/skills/<name>` reference works. Explicit user invocation
(`$review` mention or `/skills` selector) is official and documented;
proving it headlessly would require the interactive TUI — recorded as
UNVERIFIED, not invented.

## Semantic matrix (UZE Command vs harness surfaces)

| Semantic property | UZE Command | Claude (plugin `commands/`) | Codex (explicit-only Skill) | OpenCode (`.md` command) | Gemini (`.toml` command) |
|---|---|---|---|---|---|
| Explicit invocation | required | ✓ `/name` | ✓ `$name` / `/skills` (official; TUI UNVERIFIED headless) | ✓ `/name` | ✓ `/name` |
| Auto model invocation disabled | required | ✓ `disable-model-invocation` (documented for plugin skills/commands) | ✓ `allow_implicit_invocation: false` (proven) | ✓ commands are a user-typed registry, not model-discovered | ✓ commands are a user-typed registry |
| Stable identity | required | ✓ | ✓ (skill name = command name; UZE identity stays `Command`) | ✓ | ✓ |
| Prompt body | required | ✓ verbatim | ✓ verbatim | ✓ verbatim | ✓ verbatim (TOML) |
| Description | desired | ✓ | ✓ | ✓ frontmatter | ✓ TOML |
| Structured args | out of scope v0 | — | — | — | — |

Under the semantic definition — *NATIVE = the harness provides a
first-class supported mechanism that preserves the canonical capability
semantics, even if the primitive is named differently* — every row above
is NATIVE; Codex's explicit-only Skill is the official Codex mechanism for
*explicit user workflows* and UZE uses it directly, with no emulation or
degradation of any current canonical semantic. This definition is
formalized in ADR-025, in `docs/architecture/invariants.md`, and on
`CompatibilityRoute::Native` itself.

## Security

Generated commands are **prompt-oriented only**. UZE never produces:
- OpenCode shell interpolation (`` !`cmd` `` — runs outside the agent
  permission flow per official docs);
- Gemini `!{...}` shell execution or `@{...}` file injection.

Vendor-specific syntax an author ships inside a canonical body is preserved
verbatim as author evidence (never generated, never rewritten). This is
future Security Gate territory: any later automatic interpolation requires
an explicit trust policy.

## Lifecycle

Commands use the existing AttachmentReceipt/reconciliation architecture —
no separate ledger. Package-level delivery (Claude generated/explicit
plugin, Gemini generated extension) → one package receipt, no
capability-level receipts; standalone delivery (OpenCode symlink, Gemini
user TOML, Codex adapted Skill) → normal capability receipts with
attach/inspect (Matched/Drifted/Missing)/detach semantics. Generated
artifacts are Derived Artifacts (ADR-013 §4) under `$UZE_HOME`, never the
Store; delivery/receipts never mutate Store bytes.

## Naming

Same principles as Skills: bare logical name first where the harness UX is
the user-typed name (OpenCode `/review`, Gemini `/review`, Codex skill
name), fully package-qualified on collision (`beta-review`). The file
extension for file-based registries (`.md`, `.toml`) is a vendor naming
constraint owned by each integration, never by Application. Resolution is
deterministic via the existing first-unclaimed-candidate algorithm (see
`tests/command_capability_conformance.rs`).

## Conformance

`tests/command_capability_conformance.rs` covers the 14-command contract
(discovery as Command; Skill discovery unchanged; native routes; explicit-only
marking; unsupported stays; explicit precedence; deterministic generation;
exact coverage incl. Command only when delivered; fallback; single receipt;
collision determinism; attach→Matched→detach→Missing; Store bytes
unchanged; existing Skill/MCP behaviour green), plus the real-Codex dogfood
regression proving Skill implicit discoverability / Command explicit-only /
store immutability.
