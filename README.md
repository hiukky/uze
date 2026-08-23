<div align="center">

# uze

**Install once. Native everywhere.**

[![CI](https://github.com/hiukky/uze/actions/workflows/ci.yml/badge.svg)](https://github.com/hiukky/uze/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/rust-1.97%2B-orange)](Cargo.toml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](Cargo.toml)
[![Status](https://img.shields.io/badge/status-alpha-important)](#alpha)

A compatibility and distribution layer for agentic tooling across harnesses.

</div>

Install a plugin once and uze exposes it through the most native surface
each harness supports — a real plugin where one exists, native capabilities
where it doesn't, a safe adapter only as a last resort.

One project context (`AGENTS.md`) replaces separately maintained `.claude`,
`.codex`, `.opencode`, and Gemini configuration. uze keeps a single source
of truth and projects it to each harness safely.

```text
Marketplace / Project
        │
        ▼
       uze
   ┌────┼────┬────────┐
   ▼    ▼    ▼         ▼
Claude Codex OpenCode Gemini
```

## Why uze

- Install a plugin once; uze delivers it through whatever each harness
  natively supports.
- Share one project context across every harness instead of maintaining
  four.
- Native delivery over lowest-common-denominator conversion — nothing is
  translated without proven equivalence.
- Lifecycle, drift, and ownership are managed centrally, with typed
  receipts, never silent overwrites.

## Compatibility

| Harness | Plugin delivery | Skills | MCP | Context | Runtime |
|---|---|---|---|---|---|
| Claude Code | Adapted | ✅ | ✅ | Bridged | ◌ Experimental |
| Codex | Native plugin | ✅ | ✅ | Native | — |
| OpenCode | Native capabilities | ✅ | ✅ | Native | — |
| Gemini CLI | Native extension | ✅ | ✅ | Bridged | — |

`✅` Ready/native · `◐` Partial/adapted · `◌` Experimental · `—` Not
implemented. "Native" means a real, first-class mechanism per harness, not
a shared name — see the [capability landscape](docs/capabilities/landscape.md)
for exactly what each cell means and its evidence, or
[`crates/uze-integrations/README.md`](crates/uze-integrations/README.md) for
the evidence-graded, per-harness compatibility audit (status per surface,
package-coverage safety, lifecycle, and each integration's own README).

| Capability | Status |
|---|---|
| Skills | Ready |
| MCP | Ready |
| Instructions | Ready |
| Agents | Research |
| Hooks | Research |

## Today

- Harness manager — detect, provision, and set up Claude Code, Codex,
  OpenCode, and Gemini CLI.
- Plugin manager — install once, store bytes, deliver natively per harness.
- Context manager — one `AGENTS.md`, projected to each harness's own
  mechanism.
- Official marketplace — `plugins/uze`, the `/uze` Skill.
- Terminal UI — browse plugins, harnesses, context, and diagnostics.
- Runtime integration *(experimental)* — a PATH shim that projects
  `AGENTS.md` into Claude Code without writing into the project.

## Quick start

```bash
cargo install --path .   # alpha: installs from source, no registry yet
uze setup
uze
```

```bash
uze doctor
```

## Terminal UI

```bash
uze
```

> Browse marketplaces, plugins, harnesses, project context, and
> diagnostics from one terminal UI.

## Native first

```text
native package → native capability → safe adapter → unsupported
```

uze preserves source plugins as-is and never translates semantics unless
equivalence is proven.

## Official marketplace

```text
agents.json
plugins/
  uze/
```

This repository is also the official uze marketplace; official plugins
live under `plugins/`.

## Project context

```text
AGENTS.md
```

`AGENTS.md` is the canonical project instruction surface. uze delivers it
through each harness's native mechanism, or a runtime projection where
native delivery isn't available yet.

## Roadmap

- [x] Harness management
- [x] Skills & MCP delivery
- [x] Project context (`AGENTS.md`)
- [x] Official marketplace
- [x] Terminal UI
- [ ] Native package delivery for Claude Code
- [ ] Runtime context projection beyond experimental
- [ ] Agent & hook portability

## Alpha

uze is alpha. APIs, schemas, and harness integration behavior may change
while the cross-harness model is being validated.

## Learn more

- [Architecture invariants](docs/architecture/invariants.md)
- [Capability landscape](docs/capabilities/landscape.md)
- [Architecture decisions](docs/adr/README.md)

## Development

```bash
cargo test --no-fail-fast
cargo clippy --all-targets -- -D warnings
```

Contributions and early feedback are welcome while uze is still validating
its compatibility model.

---

Built in Rust. MIT licensed.
