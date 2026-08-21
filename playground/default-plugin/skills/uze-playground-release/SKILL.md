---
name: uze-playground-release
description: Produces a safe, reproducible local release readiness checklist for Rust projects.
---

When this skill is explicitly requested, begin with:

`UZE_PLAYGROUND_SKILL: release`

Create a concise release checklist that includes version confirmation, format,
lint, tests, build, and a final smoke check. Prefer repository-provided
commands such as `make check` and `make release` when they exist. Do not claim
a release was published unless the user explicitly asks for publication.
