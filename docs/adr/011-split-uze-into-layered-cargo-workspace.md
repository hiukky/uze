# Split UZE into a layered Cargo workspace

Status: Accepted

## Context

UZE's Core, vendor integrations, Application façade, terminal UI and
conformance lab have distinct dependency directions but previously compiled
as one crate. That allowed vendor/UI/test dependencies to leak into the Core
and made future expansion harder to reason about.

## Decision

We use a small layered workspace: `uze-core` is harness-agnostic;
`uze-integrations` depends on Core; `uze-application` composes Core and
integrations; root `uze` is the compatible installable CLI/TUI facade; `e2e`
is a workspace member outside product dependencies. The root reexports the
established public API during this v0 evolution.

## Consequences

The compiler now enforces the main architectural direction and Core consumers
avoid vendor/UI dependencies. The repository gains package metadata and some
cross-crate fixture-path care, but avoids microcrates and preserves existing
installation and import paths.

Source change: openspec/changes/modularize-uze-workspace/
