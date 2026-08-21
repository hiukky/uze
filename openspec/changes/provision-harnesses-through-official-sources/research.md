# Official provisioning routes (2026-08-21)

This record limits v0 automation to Unix and WSL. A route is invoked only by
the owning integration during explicit `uze setup`; `uze add` never invokes
one.

| Harness | Missing executable | Existing executable | Verify | Evidence |
|---|---|---|---|---|
| Claude Code | `curl -fsSL https://claude.ai/install.sh | bash` | `claude update` | `claude --version` | [Anthropic setup](https://docs.anthropic.com/en/docs/claude-code/getting-started), [CLI reference](https://docs.anthropic.com/en/docs/claude-code/cli-usage) |
| Codex | `curl -fsSL https://chatgpt.com/codex/install.sh | sh` | `codex --upgrade` | `codex --version` | [OpenAI Codex README](https://github.com/openai/codex/blob/main/README.md), [OpenAI Help](https://help.openai.com/en/articles/11096431) |
| OpenCode | `curl -fsSL https://opencode.ai/install | bash` | `opencode upgrade` | `opencode --version` | [OpenCode install](https://opencode.ai/docs), [OpenCode CLI](https://dev.opencode.ai/docs/cli/) |
| Gemini CLI | `npm install -g @google/gemini-cli@latest` | same idempotent npm command | `gemini --version` | [Gemini installation](https://geminicli.com/docs/get-started/installation/), [Gemini FAQ](https://geminicli.com/docs/faq/) |

The Windows-specific official PowerShell routes are intentionally deferred
until their command contracts are tested in a Windows runner. macOS follows
the Unix routes above where vendors document them. An unsupported platform is
reported as `BLOCKED`, not routed through another package manager.

Installer output is never persisted. UZE records only action, result,
official-method label, observed version, and timestamp under
`$UZE_HOME/state/provisioning.json`.
