# Agent Skill Conformance Playground

This isolated project is a standard Agent Plugin package and the single source
of the `uze-e2e` Agent Skill used by UZE's import, contract, and opt-in
real-harness probes. `plugin.json` and `skills/` follow Agent Plugins v1. The
`.agents/skills/uze-e2e` entry is an internal symlink to that same skill;
there is one physical `SKILL.md` and no `.claude/`, `.codex/`, or other vendor
directory.

The test asks a harness to activate `uze-e2e` without revealing the proof
token. A successful response proves the harness exposed the standard project
skill. The importer test also verifies that the plugin package resolves the
same physical `SKILL.md`. It does not prove generic Agent Plugin installation,
MCP, or ACP.

Run an explicit probe only with a locally authenticated harness:

```text
UZE_E2E_REAL_HARNESSES=claude cargo test --test real_harness_conformance -- --ignored --nocapture
UZE_E2E_REAL_HARNESSES=codex cargo test --test real_harness_conformance -- --ignored --nocapture
UZE_E2E_REAL_HARNESSES=opencode cargo test --test real_harness_conformance -- --ignored --nocapture
```

The default `cargo test` skips these probes. A failed probe is meaningful
evidence: the integration must remain `UNVERIFIED` or be reported as
`NOT_EXPOSED`; it must not cause UZE to generate a vendor-specific copy.

## Recorded baseline

On 2026-08-20, the opt-in probes found:

| Harness | Version | Result for `.agents/skills/uze-e2e` |
| --- | --- | --- |
| Claude Code | 2.1.237 | `UNVERIFIED` — the latest probe was blocked by a session-limit API error. |
| Codex CLI | 0.148.0 | `VERIFIED` |
| OpenCode | 1.18.18 | `UNVERIFIED` — it documents `.agents/skills` discovery, but this machine has no configured provider for a real probe. |

Claude Code is now `UNVERIFIED`: a subsequent probe returned an API session
limit, which must never be interpreted as a capability result. The test rejects
structured API errors before it evaluates the absence of the proof token.
