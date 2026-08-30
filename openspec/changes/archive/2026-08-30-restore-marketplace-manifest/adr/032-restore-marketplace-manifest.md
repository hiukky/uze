# Restore `marketplace.json` as the Marketplace Manifest

Status: Accepted

## Context

ADR-023 renamed UZE's marketplace-root registry manifest from
`marketplace.json` to `agents.json`, while preserving the manifest schema and
leaving vendor-owned catalogues alone. That decision made the distribution
registry look related to `AGENTS.md`, `agents.lock`, and `.agents/`, even
though those are project instruction, dependency, and vendor-configuration
artifacts with different owners and lifecycles.

The active marketplace specification and the conventions of the harnesses UZE
integrates with use `marketplace.json` for a marketplace catalogue. UZE is
pre-production and has one operator, so retaining an alias or migration path
would add permanent ambiguity without protecting deployed users.

This decision supersedes [ADR-023](../../../../docs/adr/023-marketplace-manifest-is-agents-json.md).
It restores only the marketplace-root filename clauses of
[ADR-012](../../../../docs/adr/012-model-a-marketplace-contract-for-the-embedded-default-plugin.md)
and [ADR-015](../../../../docs/adr/015-marketplace-as-discovery-registry.md);
their remaining decisions stay accepted.

## Decision

UZE's marketplace-root registry manifest is exactly `marketplace.json`. Its
schema remains `{name, plugins: [{name, source, description, keywords}]}`
with optional `owner`, and the existing marketplace parse and resolution
primitives remain the authority for it.

1. Every UZE-owned marketplace root, including the repository root, embedded
   official marketplace, local/Git acquisition inputs, workspace detection,
   fixtures, and conformance seed, uses `marketplace.json`.
2. `agents.json` is not accepted as an alias. A root that contains only that
   file is not a marketplace and must receive the normal missing-manifest
   failure that names `marketplace.json`. UZE does not rename it, warn about
   it, or persist a compatibility setting.
3. This decision does not rename vendor-owned catalogue files such as
   `.claude-plugin/marketplace.json` and `.agents/plugins/marketplace.json`,
   nor UZE state such as `~/.uze/state/marketplaces.json`. It also does not
   change `AGENTS.md`, `agents.lock`, package manifests, or the marketplace
   domain vocabulary.
4. ADR-023 is marked superseded by this record. ADR-012 and ADR-015 retain
   their historical text; this record is the current filename authority.

## Options considered

### Restore `marketplace.json` with no compatibility path (chosen)

This matches the marketplace specification and familiar harness catalogue
terminology, while clearly separating distribution discovery from project
agent-environment files. It requires a deliberate one-file rename for any
external pre-production marketplace, but the known deployment has one
operator and no production consumers.

### Keep `agents.json`

This avoids another source rename, but preserves the misleading naming family
and contradicts the current marketplace specification.

### Accept both filenames

This would ease a hypothetical transition, but makes discovery precedence,
diagnostics, fixtures, and documentation permanently less deterministic. It
is rejected because there is no deployed compatibility requirement.

## Consequences

- The source tree and all UZE-facing diagnostics must change atomically so
  there is one canonical root-manifest contract.
- Tests must prove both the positive `marketplace.json` path and that an
  `agents.json`-only root is rejected. A repository scan must distinguish
  allowed historical ADR/OpenSpec references and vendor-owned paths from
  accidental active UZE references.
- Vendor integration paths and persisted marketplace state retain their names
  and bytes; broad filename replacement is explicitly unsafe.
- Pre-production marketplace authors must rename their root file before using
  it with this revision. There is intentionally no runtime migration.

## Implementation plan

1. Rename the repository root manifest and update
   `crates/uze-core/src/workspace.rs` plus
   `crates/uze-core/src/acquisition/marketplace.rs` to expose and consume
   `marketplace.json`.
2. Update the embedded manifest pipeline in `crates/uze-application/build.rs`
   and `crates/uze-application/src/bootstrap.rs`, then update marketplace
   registration and overview consumers in
   `crates/uze-application/src/application.rs` and
   `crates/uze-application/src/application/overview.rs`.
3. Update the UI model and UI tests in `src/ui/model.rs` and `src/ui.rs`, plus
   testkit marketplace/scenario writers in `crates/uze-testkit/src/`.
4. Rename test fixtures and update deterministic suites under `tests/`, then
   update the synthetic marketplace materialization in
   `conformance/shared/common.py` without touching per-harness vendor
   catalogues.
5. Update `README.md`, `AGENTS.md`, architecture invariants, the ADR index,
   and the archived-decision status. Do not add a filename fallback anywhere.

## Verification

- [ ] `uze market add` and installation resolve a local root containing only
  `marketplace.json`.
- [ ] A local root containing only `agents.json` is rejected with a diagnostic
  naming the missing `marketplace.json`.
- [ ] Workspace overview distinguishes `agents.lock` from a
  `marketplace.json` marketplace and recognizes their hybrid state.
- [ ] The embedded official marketplace is built and bootstrapped from
  `marketplace.json`.
- [ ] Deterministic Rust tests pass: `cargo test --no-fail-fast`.
- [ ] All four real-harness conformance verticals pass through
  `python3 conformance/lab.py --harness <harness>`.
- [ ] `openspec validate --all --strict` passes.
- [ ] A final active-source reference scan finds no UZE-owned
  `agents.json` marketplace input outside historical decision records or
  explicitly vendor-owned paths.
