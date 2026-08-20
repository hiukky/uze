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
```

The default `cargo test` skips these probes. A failed probe is meaningful
evidence: the integration must remain `UNVERIFIED` or be reported as
`NOT_EXPOSED`; it must not cause UZE to generate a vendor-specific copy.

## Recorded baseline

On 2026-08-20, the opt-in probes found:

| Harness | Version | Result for `.agents/skills/uze-e2e` |
| --- | --- | --- |
| Claude Code | 2.1.237 | `NOT_EXPOSED` |
| Codex CLI | 0.148.0 | `VERIFIED` |

The Claude result is a supported negative conformance case, not a reason to
project the skill into `.claude/`. The probe will fail if that observed result
changes, requiring the integration capability declaration to be reviewed.
