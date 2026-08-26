# Research notes — transparent harness attachment (2026-08-20)

Supporting evidence for `proposal.md` / `design.md` / `adr/`. Not an OpenSpec
artifact — a durable record of what was checked, against which official
sources, and what was empirically verified on this machine, so findings can
be re-verified as both harnesses evolve. Two research passes ran in parallel
against current official documentation (Claude Code: `code.claude.com/docs`;
Codex CLI: `github.com/openai/codex`, `learn.chatgpt.com/docs` — the domain
`developers.openai.com/codex/*` was mid-migration to `learn.chatgpt.com/docs/*`
during this pass, 308-redirecting; treat exact URLs as unstable and prefer
`codex --help` / the in-repo docs as the durable source). One empirical test
was then run directly against the real `claude` CLI (2.1.237) in an isolated
`$HOME`.

## 1. The boxed question, answered for both harnesses

> After a one-time `uze setup`, is there an officially supported mechanism
> for the user to run plain `claude`/`codex` and have the UZE integration
> activate automatically?

**Claude Code: yes, via a documented but under-specified mechanism, now
empirically closed on the discovery/loading side.** **Codex: yes, directly
documented and explicit.**

## 2. Mechanism classification

| Mechanism | Claude | Codex | Classification | Notes |
|---|---|---|---|---|
| Global plugin install (marketplace + `claude plugin install` / `codex plugin add`) | exists, scriptable | exists, scriptable | `DOCUMENTED BUT UNPROVEN` as the *primary* path | Real, non-interactive CLI subcommands confirmed on both (`claude plugin install\|i`, `codex plugin add`) — a legitimate secondary path, but functionally routes to the same user-scope directory as the row below, so it is redundant as the primary mechanism. |
| User-scope skills directory, discovery-based (`~/.claude/skills/<name>/`, `~/.agents/skills/<name>`) | `claude plugin init --help`: "Scaffold a new plugin at `~/.claude/skills/<name>/` (auto-loads next session as `<name>@skills-dir`)" | official docs: USER scope, `$HOME/.agents/skills`, cwd-independent, scanned every session | `PROVEN` (mechanism itself) | Selected as the primary attachment location for both harnesses. |
| Symlink support at that location | **empirically verified this session**, see §3 | explicitly documented: "Codex supports symlinked skill folders and follows the symlink target when scanning these locations" | Claude: `PROVEN` at discovery level (see caveat below). Codex: `PROVEN`. | Claude's symlink support is not written down anywhere found in official docs — it was verified by direct, controlled experiment against the real binary, not by citation. |
| MCP (global config) | client-side, `~/.claude` config | client-side, `config.toml [mcp_servers.*]` | `NOT SUITABLE` | Different primitive (Tools, not `SKILL.md` Agent Skills); config is static, not dynamically cwd-aware on either harness. |
| Hooks / SessionStart | 29 events, text/context injection only, cannot register new invocable Skills | `SessionStart`/etc. exist, same limitation, plus a one-time interactive trust-by-hash step for non-managed commands | `NOT SUITABLE` as the primary mechanism | Real on both harnesses; kept in mind as a future self-healing/drift-check companion, not required for the baseline. |
| Config includes/extends | n/a | no `include`/`extends` key documented for `config.toml` | `UNSUPPORTED` (Codex); not applicable to Claude's JSON settings the same way | Not a viable indirection point for either harness. |
| Process wrapper on PATH | possible | possible | rejected outright per product constraint | Not evaluated further — out of scope by the product's own North Star. |

## 3. Empirical verification — Claude Code symlink support

Performed in a disposable, fully isolated `$HOME` (`/tmp/.../claude-symlink-test/`)
against the real Claude Code 2.1.237 binary. No file under the operator's
real `~/.claude` was read or written.

Layout created:
```
$UZE_HOME_STUB/store/uze-e2e/.claude-plugin/plugin.json   (skills: ["./"])
$UZE_HOME_STUB/store/uze-e2e/SKILL.md
$CLAUDE_HOME/.claude/skills/uze-e2e -> $UZE_HOME_STUB/store/uze-e2e   (symlink)
```

```
$ HOME=$CLAUDE_HOME claude plugin validate ~/.claude/skills/uze-e2e
✔ Validation passed with warnings

$ HOME=$CLAUDE_HOME claude plugin list
Skills-directory plugins (.claude/skills/*):
  ❯ uze-e2e@skills-dir
    Version: 0.1.0
    Scope: user
    Path: ~/.claude/skills/uze-e2e
    Status: ✔ loaded
```

**Control**: the same three commands (`validate`, `list`, `details`) were run
against a real, non-symlinked plugin scaffolded by `claude plugin init` in a
second isolated home. Output was byte-identical in shape, including a
`Skills (0)` line in `plugin details` for *both* the symlinked and the real
plugin — confirming that line is a general quirk of `plugin details`, not a
symlink-detection artifact.

**What this does and does not prove.** Claude's own introspection tooling
(`validate`, `list`, `details`) treats the symlinked skills-dir plugin
identically to a real one, with `Status: ✔ loaded`. It does **not** prove a
real `claude -p` session actually invokes the skill and returns its content
— that requires authentication, and no `ANTHROPIC_API_KEY` was available in
the isolated environment. Copying the operator's real OAuth credentials into
a throwaway home to close this gap was deliberately avoided (see
proposal.md's isolation constraint). This behavioral gap is closed later by
an opt-in, auth-gated runtime-phase conformance test (tasks.md §8), which
also serves as the project's existing pattern for `Unverified` → `Verified`
transitions.

## 4. Codex CLI — official symlink documentation

Confirmed directly from the primary Skills-discovery documentation
(`build-skills`, current at time of research): Codex scans four scopes at
session start — `SYSTEM` (OpenAI-bundled), `ADMIN` (`/etc/codex/skills`,
root-owned, not usable by a per-user tool), `USER` (`$HOME/.agents/skills`,
fixed path, cwd-independent), `REPO` (per-project `.agents/skills`, walked
from cwd to repo root). The `USER` scope is explicitly documented as
symlink-following. The Codex Plugins CLI (`codex plugin marketplace add`,
`codex plugin add <pkg>@<marketplace>`) is a real, scriptable, non-interactive
alternative front-end that, per its documented global install scope, writes
into the same `USER`-scope directory — confirming the plugins system is not
a separate mechanism, just an alternate installer for the same location.

No real-binary experiment was needed for Codex — the mechanism is directly
documented, unlike Claude's undocumented-but-observed symlink behavior.

## 5. Confirmed unknowns (do not treat as settled)

- Whether Claude Code's symlink-following for skills-dir plugins is
  officially supported/guaranteed or merely incidental current behavior
  (nothing in the docs states it either way; only empirically observed).
- Exact CLI version floor for Codex's Plugins CLI and `USER`-scope Skills
  directory — the Codex docs domain was mid-migration during this research
  pass and a precise GA date could not be extracted from primary sources.
- Whether a future Claude Code release could change plugin discovery timing
  (e.g. defer loading until first use) in a way that affects symlink
  resolution — flagged as a version-dependent risk in `design.md`.
