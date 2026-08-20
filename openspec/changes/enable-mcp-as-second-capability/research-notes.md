# Research notes — MCP as a second capability (2026-08-20)

Supporting evidence for `proposal.md` / `design.md` / `adr/`. Two research
passes ran in parallel against current official documentation.

## 1. The gate question, answered for both harnesses

> After a one-time `uze setup`, can UZE register an MCP server once, at
> user/global scope, non-interactively, such that it's available to every
> future harness session in any project, with zero per-project config?

**Yes for both, symmetrically** — unlike Agent Skills (where Claude's
symlink-following was empirical, not documented), MCP registration is
directly, officially documented as global/user-scope on both harnesses.

## 2. Claude Code MCP

Source: `code.claude.com/docs` (`mcp-quickstart.md`, `mcp.md`,
`plugins.md`, `plugins-reference.md`).

- **Config location**: `~/.claude.json` (a different file from
  `~/.claude/skills/`, which Agent Skills uses), top-level `mcpServers` key.
  Respects `CLAUDE_CONFIG_DIR` like the skills-dir mechanism does. Scope
  precedence: local > project > user.
- **Registration**: `claude mcp add --scope user --transport stdio <name>
  -- <command> [args...]` — non-interactive, scriptable. `--env KEY=VALUE`
  writes a literal value; `${VAR}` / `${VAR:-default}` expansion (resolved
  from Claude Code's own OS environment at runtime) is documented for
  `command`/`args`/`env`/`url`/`headers` — the mechanism for referencing a
  secret without writing its value into the file.
- **Discovery/config check, no LLM turn**: `claude mcp list` (reports live
  stdio connectivity, e.g. "✔ Connected"), `claude mcp get <name>`.
- **Plugin-bundled `mcp.json`**: confirmed — a plugin can bundle an
  `mcp.json` (or inline `mcpServers` in `plugin.json`) that auto-registers
  when the plugin loads. This is the same Agent Plugins-shaped convention
  UZE's own package `mcp.json` reuses, independently confirmed from the
  Claude-specific side.
- **No keychain/secret-store integration** — plain OS environment variables
  only, confirmed no support for referencing an external secret manager.
- Classification: **PROVEN**.

## 3. Core MCP spec + Codex MCP

Sources: `modelcontextprotocol.io` spec (current: 2025-11-25; a 2026-07-28
release candidate exists, not yet final); `github.com/openai/codex`
`codex-rs/cli/src/mcp_cmd.rs`, `codex-rs/config/src/mcp_types.rs`,
`codex-rs/config/src/config_toml.rs`, `codex-rs/config/src/loader/
README.md` (latest tag checked: `rust-v0.149.0-alpha.4`; latest stable
`rust-v0.148.0`).

- **Core spec scope**: wire-protocol only (JSON-RPC 2.0, capability
  negotiation, Resources/Prompts/Tools/Sampling/Roots/Elicitation). The
  Skills-Over-MCP Working Group charter explicitly lists "Plugin/bundle
  packaging" as **out of scope**, deferred to Agent Plugins 1.0 (shipped
  2026-08-06). Confirms, independently of the prior change's research, that
  no `UZE MCP format` is needed — the packaging layer already exists
  elsewhere and UZE should not duplicate it.
- **Transports**: stdio and Streamable HTTP are current standard
  transports; plain SSE is deprecated but retained for backward
  compatibility; WebSocket is not part of the spec.
- **Codex config**: `~/.codex/config.toml`, top-level `mcp_servers` map
  (`McpServerConfig` in `mcp_types.rs`). A project-local `.codex/
  config.toml` can exist for "trusted projects" and would take precedence —
  UZE never creates one, by design (avoiding per-project config is the
  whole point).
- **Registration**: `codex mcp add <name> -- <command> [args...]` — no TTY
  prompts for a plain stdio server with no auth. **No `--scope` flag
  exists**; the command always writes to `find_codex_home()` (`$CODEX_HOME`
  or `~/.codex`) and prints "Added global MCP server". The write uses
  `ConfigEditsBuilder::replace_mcp_servers` over `toml_edit::DocumentMut` —
  format-preserving, atomic, touches only `[mcp_servers]`, confirmed safer
  than UZE hand-patching TOML directly.
- **Discovery/config check, no LLM turn**: `codex mcp list --json`
  (config-only read).
- **Env/secrets**: `env = {KEY="literal"}` (avoid) vs `env_vars =
  ["VAR_NAME"]` (name-only pass-through from Codex's own process
  environment) — the latter is **not reachable via the `--env` CLI flag**,
  only by hand-editing `config.toml`; irrelevant to UZE's first,
  secret-free fixture, but worth knowing before any future secret-carrying
  package.
- Classification: **PROVEN**.

## 4. Expected CapabilityRouter classification

Both harnesses land in `ADAPTABLE` for `CapabilityKind::Mcp` — the same
tier Agent Skills already occupies. MCP is not expected to be, and per this
research is confirmed not to be, the capability that exercises
`UNSUPPORTED` — that question is reserved for the still-unstarted
asymmetric-capability change (Hooks/Commands/Agents), which requires a
separate, explicit review before any implementation per the user's own
instruction.

## 5. Confirmed unknowns (do not treat as settled)

- `claude mcp add`'s/`codex mcp add`'s exact behavior when a name already
  exists with different `command`/`args` — not confirmed either way; UZE's
  design mitigates by never calling `add` for an already-existing name
  rather than depending on undocumented overwrite semantics.
- Exact CLI version floor for the `codex mcp` subcommand's JSON output
  shape and the `env_vars`/`source: remote` mechanism — both look
  newer/actively evolving (tied to Codex's remote-execution features);
  pin conformance tests against a specific `codex --version` rather than
  assuming stability across minor releases, same caution already recorded
  for the Skills change.
- The hosted Codex docs domain migration (`developers.openai.com` →
  `learn.chatgpt.com`) is real and in-flight — `codex mcp --help` and the
  `codex-rs` source remain the more durable ground truth than any specific
  hosted-docs URL.
