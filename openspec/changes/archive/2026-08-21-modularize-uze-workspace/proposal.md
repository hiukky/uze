## Why

UZE's package lifecycle, peer integrations, application facade, terminal UI,
and conformance tooling have grown into distinct responsibilities inside one
crate. The compiler cannot enforce the dependency direction that ADR-008 and
ADR-009 require, and UI/test-only dependencies leak into ordinary Core use.

## What Changes

- Establish a small Cargo workspace with `uze-core`, `uze-integrations`,
  `uze-application`, the existing `uze` presentation facade, and the existing
  `e2e` conformance package as members.
- Move harness-agnostic package/domain/lifecycle/contracts into `uze-core`.
- Move named peer integrations into `uze-integrations`, which depends only on
  Core.
- Move package-centric application orchestration into `uze-application`,
  which composes Core and integrations.
- Keep the root `uze` package and its public reexports so `cargo install
  --path .`, CLI/TUI use, and existing library imports remain compatible.

## Capabilities

No product behavior changes. This is a dependency-boundary refactor, so
`skip_specs: true` is intentional.

## Impact

- Cargo workspace/package metadata, module paths, test fixture paths, LikeC4,
  and architecture documentation.
- No new harness behavior, plugin format, attachment mechanism, or CLI command.
