# Implement UZE in Rust as a layered Cargo workspace

Status: Accepted
Consolidates: ADR-011 (split UZE into a layered Cargo workspace) — see the
"Consolidated records" section of `README.md`.

## Context

UZE needed an implementation language, and the prior design left it open.
The requirements were safe path handling, deterministic resolution and
reporting, portable CLI execution, and a single distributable binary a user
can put on `PATH`.

Once implemented, a second problem appeared: Core, vendor integrations, the
Application facade, the terminal UI, and the conformance harness all
compiled as one crate. Nothing stopped vendor, UI, or test dependencies from
leaking into the Core, and the dependency direction the architecture depends
on was convention rather than a compiler-checked fact.

## Decision

**Rust, on the active stable toolchain**, library-first: typed domain
models, deterministic ordering, explicit diagnostics, and tests next to the
behavior they verify. TypeScript/Node was considered for a fast prototype
and rejected — it would introduce a second runtime for the user to install
and does not give a single static binary.

**A small layered Cargo workspace**, where the compiler enforces the
dependency direction:

```
uze (root binary: CLI, TUI, PATH shim)
      ↓
uze-application   (orchestration)
      ↓
uze-core          (harness-agnostic domain)
      ↑
uze-integrations  (per-vendor, implements IntegrationPort)
```

`uze-core` is harness-agnostic and depends on nothing vendor-specific.
`uze-integrations` depends on Core. `uze-application` composes both. The
root `uze` crate is the installable CLI/TUI facade. Test and conformance
support are workspace members outside the product dependency graph.

Microcrates are deliberately avoided: the split follows the architectural
boundaries that are actually enforced by tests, not every module seam.

## Consequences

Easier: the compiler enforces the main architectural direction, and Core
consumers cannot pick up vendor or UI dependencies by accident. Library-first
boundaries make resolver, router, and report behavior testable without
subprocesses. A single static binary ships with no runtime prerequisite.

Harder: contributors need Rust and Cargo; crate versions must be maintained
across the workspace; and cross-crate fixture paths need care. Compiling the
full workspace is memory-hungry enough to need explicit job limits on
constrained hosts (see `AGENTS.md`).
