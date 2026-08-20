# Research notes — cross-harness capability investigation (2026-08-19)

Supporting evidence for `proposal.md` / `design.md`. Not an OpenSpec artifact — a
durable record of what was actually checked, against which harness docs, on
which date, so findings can be re-verified as the ecosystem moves. Four
research passes were run in parallel against primary/official documentation
only; anything not directly confirmed is marked `Unknown`, not guessed.

> **Scope note (added after this research):** Devin Desktop (formerly
> Windsurf) is deliberately excluded from UZE's active target list for
> this phase, per project decision — not because of anything found here.
> All Devin Desktop / Windsurf findings below are kept as-is for when it's
> picked back up; every other document in this change (`proposal.md`,
> `design.md`, specs, ADRs, `tasks.md`) now scopes to four active target
> harnesses: Claude Code, Cursor, Codex, OpenCode.

## 0. The single biggest finding: the target list itself changed mid-research

**Windsurf no longer exists as a product name.** Cognition AI renamed it
**Devin Desktop** (OTA update, 2026-06-02); `windsurf.com` and
`docs.windsurf.com` now redirect to `devin.ai/desktop` / `docs.devin.ai`. The
legacy **Cascade** agent (what the original brief's assumptions were about)
is on a deprecation ramp that may already be complete. The new default
runtime, **Devin Local**, connects via the open **Agent Client Protocol
(ACP)** — the same protocol reportedly used to plug Codex, Claude Agent, and
OpenCode into one editor. ACP's origin/spec authority was not verified in
this pass (flagged `Unknown`) and deserves a dedicated follow-up before the
PoC's runtime-bridge design is finalized, since it may already do part of
what UZE's own bridge layer is meant to do.

Everywhere below, "Windsurf" means "Devin Desktop (formerly Windsurf)."

## 1. Master capability matrix

Legend: **Std** = covered by an external open standard directly (no UZE
involvement needed) · **Native** = harness-specific but first-class ·
**Bridge** = documented format, harness-specific semantics, needs adaptation
· **Gap** = no stable file/API surface to target · **Unknown** = not
confirmed against primary docs this pass.

| Capability | Claude Code | Cursor | Codex | OpenCode | Devin Desktop (Windsurf) |
|---|---|---|---|---|---|
| Instructions | **Gap** — reads `CLAUDE.md`, not AGENTS.md (import/symlink shim required) | **Std** — native AGENTS.md + `.cursor/rules/*.mdc` | **Std** — AGENTS.md is OpenAI-stewarded, root→cwd merge algorithm | **Std** — native AGENTS.md, `/init` generates one | **Std** — native AGENTS.md feeds unified Rules engine |
| Skills | **Std** (own dirs only: `.claude/skills/`, `~/.claude/skills/`) | **Std** (`.agents/skills/` + `.cursor/skills/`) | **Std** (agentskills.io referenced explicitly, + `agents/openai.yaml` extension) | **Std** (`.opencode/`, `.claude/skills/`, `.agents/skills/`) | **Std** (`.windsurf/skills/`, `.agents/skills/`, `.claude/skills/`) |
| Agents/subagents | **Native** — `.md`+frontmatter, `agents/`, concurrent bg execution | **Native** — `.md`+frontmatter; reads `.claude/agents/` and `.codex/agents/` directly | **Gap** — no stable on-disk definition format; dynamically spawned | **Native** — `.md`+frontmatter, `subagent_depth` cap | **Native** — Devin Local supports sub-sessions; def. format unconfirmed |
| Actions/commands | **Native** — `commands/*.md` → `/name` | **Unknown** — no distinct primitive beyond Skills confirmed | Folded into Skills invocation (`$skill`, `/skills`) | **Native** — `commands/*.md`, rich `$ARGUMENTS`/`@file` templating | **Native** — `.windsurf/workflows/*.md` → `/name` |
| Hooks | **Native, richest** — 29 events, 4 handler types (command/http/mcp_tool/prompt/agent), exit 2 = block | **Bridge** — ~20 events (camelCase), command-only handlers, exit 2 = deny; subset of Claude's | **Bridge** — 11 events, naming converges on Claude's; only `command` handler executes today | Via **code plugins only** (JS/TS hook functions), not a declarative file | **Native** — 12 events, `pre_*` can veto (exit code), `post_*` observe-only |
| MCP | **Std** — client, `.mcp.json` | **Std** — client, `.cursor/mcp.json` | **Std** — client, but config lives in `config.toml` (TOML, not JSON) | **Std** — client, `opencode.json` | **Std** — client, `mcp_config.json`, OAuth supported |
| Plugins | **Native** — `.claude-plugin/plugin.json`, bundles skill+cmd+agent+hook+MCP+LSP+themes | **Two formats**: native Cursor Plugins (rich) **+** claims to load the open Agent Plugins spec unmodified | **Native** — `.codex-plugin/plugin.json` + `skills/`, universal ChatGPT+Codex directory | **Gap for filesystem import** — plugin = executable JS/TS module (npm), not a declarative manifest | **Ambiguous** — public "plugins" = IDE editor integrations, not agent bundles; separate enterprise-preview "Devin plugin system", bundling scope unconfirmed |
| Permissions | **Native** — `settings.json` allow/deny globs, scopes merge (managed>CLI>local>project>user) | **Native** — `permissions.json`, classifier-based "auto-review" | **Native, beta** — two parallel systems (legacy sandbox_mode + new `[permissions.*]` profiles) | **Native, fine-grained** — per-tool × allow/ask/deny, per-agent override | **Unknown** — RBAC docs exist, enterprise-focused, not read in depth |
| Memory | **Native** — auto-memory (`~/.claude/projects/<repo>/memory/`), machine-local | **Unknown/likely absent** | **Unsupported natively** — hooks doc suggests bridging via hook, not built-in | **Unknown** (AGENTS.md/Rules likely substitute) | **Legacy-only** — "Memories" tied to deprecated Cascade, **not available in Devin Local**, not git-committed |
| Workspace discovery | CWD + tree walk for CLAUDE.md, `--add-dir` | Unknown (presumed: open folder) | Git root, walked from cwd; `codex status` reports it | Git worktree root, walked from cwd | Git repo root (consistent pattern) |
| Distribution | Git-native marketplaces (`marketplace.json`), decentralized | Centralized, review-gated marketplace (cursor.com), Git-shaped underneath | Universal plugin directory shared with ChatGPT, or local marketplace for testing | npm (plugins), local files/git otherwise | Not directly researched |

## 2. Standards layer — what's actually settled

- **AGENTS.md** — genuinely multi-vendor (OpenAI-led, Linux Foundation
  Agentic AI Foundation as of Dec 2025). Deliberately unstructured: no
  schema, no validator, standardizes only "this file, this location, plain
  markdown." **Claude Code is the outlier that does not read it natively**
  despite being listed as a supported client on agents.md's own site — that
  claim is unverified against Claude's own docs and should be treated as
  aspirational, not confirmed.
- **Agent Skills (SKILL.md)** — open spec at agentskills.io /
  github.com/agentskills/agentskills, ~45 listed adopters, reference
  validator (`skills-ref validate`). Genuinely settled and genuinely
  adopted — the strongest interoperability win available today, requiring
  no UZE involvement beyond helping developers put files in the right
  conventional directories (which still differ per harness — see §3).
- **MCP** — current spec `2026-07-28`, moving to a stateless core with an
  **extensions framework**. Confirmed scope: connects tools/resources/
  prompts to models. Confirmed non-scope (explicit in the MCP "Skills over
  MCP" working-group charter): plugin/bundle packaging is **explicitly
  deferred to a separate packaging effort** — i.e., MCP's own WG treats
  Agent Plugins as the correct home for bundling, not MCP itself.
- **Agent Plugins** — the standards fork found a fresh (2026-08-06) v1.0.0
  spec at agent-plugins.org, co-authored by AWS/Cursor/Microsoft/OpenAI/
  Vercel/Google (**Anthropic not listed**). The Claude+Cursor fork
  independently found Cursor's own docs claiming Anthropic *is* a
  co-author, and a search snippet naming a different lineup entirely.
  **This is an unresolved, conflicting finding — do not treat Agent
  Plugins' authorship or Claude's participation as settled.** What both
  passes agree on: v1.0 scope is `plugin.json` + `skills/` + `mcp.json`
  only, and **explicitly excludes hooks, subagents, commands, and
  permissions as "too client-specific" for v1.**

## 3. Where "standard" still doesn't mean "zero-config"

Even fully standardized primitives are not automatically portable in
practice:

- **Skill locations differ per harness.** `.agents/skills/` is emerging as
  a shared convention (Cursor, OpenCode, Devin Desktop all read it) — but
  **Claude Code itself does not**, only `.claude/skills/` /
  `~/.claude/skills/`. A developer still needs a skill placed at (or
  symlinked into) more than one conventional path today.
- **MCP config format differs per harness** despite the protocol being
  standard: JSON `mcpServers` object (Claude, Cursor, Devin Desktop,
  OpenCode) vs. TOML `[mcp_servers.*]` entries (Codex `config.toml`). The
  wire protocol is portable; the *config file that points at it* is not.

This directly supports the brief's "runtime first, filesystem last"
principle while also showing why a thin, honest projection layer still
earns its place even for nominally "standard" primitives — see ADR-002.

## 4. The residual gap (what standards do not cover, confirmed twice over)

Both the MCP working-group charter and the Agent Plugins v1.0 spec
independently name the same exclusion list: **hooks/lifecycle events,
subagents, commands/actions, and permissions/policy.** Add **memory**
(no standard even attempts it; native support is inconsistent — rich on
Claude, absent/unconfirmed on Cursor and OpenCode, legacy-only and
non-portable on Devin Desktop). This is the evidence basis for scoping
UZE's capability model to exactly this set (ADR-002) rather than the
brief's original open-ended primitive list (§15).

## 5. Corrections to the brief's §24 speculative table

- Claude's "native/vendor format" instructions row understated the gap —
  it has **no native AGENTS.md support at all**, unlike all four other
  harnesses, which are AGENTS.md-native.
- Windsurf/Devin subagents: brief guessed "possible gap" → refuted, native.
- Windsurf/Devin hooks: brief guessed "available" (implied lesser tier) →
  actually rich and explicitly blocking-capable via exit codes.
- Windsurf/Devin plugins: brief guessed "different model" → true, but for
  a different reason (its public "plugins" are IDE-editor integrations,
  not agent-capability bundles at all).
- Plugins/extensions generally: the brief didn't anticipate that a
  cross-vendor Agent Plugins standard would exist by the time of this
  research — its ratification (2026-08-06) is more recent than the brief.
- Subagent format convergence is already happening organically: Cursor
  reads Claude's and Codex's agent directories directly, without any
  UZE-like tool in between — real existence-proof for the thesis, and a
  reason to explicitly check (next phase) whether harnesses cross-read
  each other's skill directories too, not just `.agents/skills/`.

## 6. Confirmed Unknowns (do not treat as settled)

- ACP's spec authority/governance and exact primitive coverage.
- Agent Plugins' true author list and whether Anthropic is a signatory.
- Whether Claude Code's own `.claude-plugin/plugin.json` is compatible
  with, a superset of, or simply different from, the Agent Plugins v1.0
  `plugin.json`.
- Cursor's exact workspace-root detection algorithm.
- OpenCode's and Cursor's native memory story, if any.
- Windsurf/Devin's subagent definition file format and RBAC/permission
  detail.
- Claude Code's MCP server-hosting capability (only client-side confirmed).
