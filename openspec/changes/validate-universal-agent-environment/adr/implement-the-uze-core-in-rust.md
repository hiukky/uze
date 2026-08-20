# Implement the UZE core in Rust

Status: Accepted

## Context

UZE is ready to move from architecture to a local PoC, but the prior design
left its implementation language intentionally open. The PoC needs safe path
handling, deterministic resolution/reporting, portable CLI execution, and a
future Client ↔ Agent boundary that can use the official ACP Rust SDK when a
concrete integration requires it. The project currently has no application
code or established runtime.

## Decision

We will implement the UZE core as a Rust CLI and library on the active stable
toolchain. The initial package will expose the resolver and report model as a
library, with a thin `uze` binary for user-facing commands. The core will use
typed domain models, deterministic ordering, explicit diagnostics, and tests
next to the behavior they verify.

The first implementation may depend on Rust crates for CLI parsing,
serialization, configuration parsing, traversal, and diagnostics. It SHALL
NOT add the ACP Rust SDK, an ACP Proxy, or a Conductor merely because Rust is
selected. When a concrete Client ↔ Agent integration is implemented, it SHALL
use the official ACP SDK rather than a UZE wire protocol, and any Proxy or
Conductor use SHALL be an explicit, reported concern under ADR-003.

## Consequences

Easier: the implementation has a robust local CLI toolchain, safe filesystem
APIs, and a direct path to the official ACP SDK for a future bounded runtime
integration. Library-first boundaries make resolver, classifier, and report
behavior testable without subprocesses.

Harder: contributors need Rust and Cargo; crate versions must be maintained;
and the team must not let the availability of the ACP Rust SDK expand UZE's
scope beyond its project-composition responsibility. TypeScript/Node was
considered for a fast CLI prototype but rejected because it would not align as
directly with the official ACP SDK and would introduce a second runtime choice
without an existing project codebase.

## Implementation Plan

- **Affected paths:** create `Cargo.toml`, `src/lib.rs`, `src/main.rs`,
  `src/project/`, `src/capability/`, `src/runtime/`, `src/report/`, and
  integration tests under `tests/`.
- **Dependencies:** use maintained crates with compatible stable-Rust versions
  for `clap`, `serde`, `serde_json`, `toml`, `thiserror`, and `walkdir`.
  Do not add `agent-client-protocol` until a concrete ACP handshake or adapter
  task requires it.
- **Patterns to follow:** library-first API; structured errors; deterministic
  collections and report serialization; no mutation of a inspected project
  unless a future explicit enhancement-application command requests it.
- **Patterns to avoid:** vendor-directory synchronization, a UZE protocol
  schema, implicit adapters, global mutable configuration, and direct ACP SDK
  use outside a dedicated runtime-integration module.

### Verification

- [ ] `cargo test` exercises portable-core discovery and report serialization.
- [ ] `cargo clippy -- -D warnings` passes for the workspace.
- [ ] `cargo fmt --check` passes for the workspace.
- [ ] The initial CLI resolves a fixture project without writing a vendor
      directory or requiring ACP.
- [ ] Any later ACP SDK integration is isolated to `src/runtime/` and reports
      the selected integration path.

## More Information

Implements the language choice anticipated by ADR-003 and the Rust-evaluation
task in `openspec/changes/validate-universal-agent-environment/tasks.md`.
