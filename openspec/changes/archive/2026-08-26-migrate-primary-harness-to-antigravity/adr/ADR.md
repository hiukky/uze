# ADR-027

Recorded: **Antigravity CLI is the Google-family v0 harness; the prior
Google-family integration was removed.**

- File: `docs/adr/027-antigravity-primary-google-family-harness.md`
- Status: Accepted.
- Decision summary: new independent `AntigravityIntegration` (id
  `antigravity`); canonical package is a valid Antigravity plugin (explicit
  native route); staged plugin tree is a Derived Artifact with fingerprint
  ownership; Commands are Adapted (no explicit-only mechanism); Context is
  Native (`AGENTS.md` read directly); the replaced integration was removed
  from the codebase (history preserved in ADRs + migration audit); zero
  `uze-core` / `IntegrationPort` changes.
