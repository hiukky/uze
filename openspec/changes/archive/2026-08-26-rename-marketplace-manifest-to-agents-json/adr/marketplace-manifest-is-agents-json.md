# Marketplace Manifest Is agents.json

Status: Accepted

## Context

ADR-012 named the official UZE marketplace's registry manifest `marketplace.json` at the repo root, and
ADR-015 validated that same filename when registering discovery sources. `redesign-cli-project-machine-grammar`
design.md §4 documented that this filename names two unrelated things: UZE's own registry manifest
contract, and the vendor-dictated native catalogues (`.claude-plugin/marketplace.json`,
`.agents/plugins/marketplace.json`) that Claude/Codex integrations republish into vendor directories.
The same change (design.md D7) analyzed renaming UZE's manifest to `agents.json` and deferred it,
requiring the follow-up to explicitly decide rename-only vs. folding in the `AGENTS.md`/`agents.lock`/
`.agents/` naming family.

In the meantime, marketplaces in the wild already use the new name: `hiukky/ai` ships `agents.json` +
`plugins/std|flow`, and `uze market add ~/ai` fails with `bundle manifest is missing in
.../marketplace.json` while the correct manifest sits next to it under `agents.json`. A marketplace
repository — `AGENTS.md`, `agents.json`, `plugins/**` — is conceptually an agent registry, which is
exactly what the rename captures.

## Decision

UZE's marketplace-root registry manifest is named `agents.json`, with the identical schema
(`{name, plugins: [{name, source, description, keywords}]}`, `owner` optional) and the identical
parse/resolve primitives (`uze-core::acquisition::marketplace`). Specifically:

1. **Rename only.** The CLI verb `market`, the `Marketplace*` domain types, the state registry file
   `~/.uze/state/marketplaces.json`, and the `plugin_marketplaces.json` provenance map are unchanged —
   ADR-019 explicitly decided the domain name is a CLI-vocabulary matter, and `agents.lock`/`AGENTS.md`/
   `.agents/` are project-scoped artifacts, not distribution manifests; they share vocabulary, not
   mechanism.
2. **No fallback alias.** A marketplace root with only `marketplace.json` fails with an error naming the
   missing `agents.json`; the contract is one deterministic filename. Pre-1.0 alpha, so migration is a
   one-file rename per marketplace root (`git mv` + push for Git marketplaces).
3. **Vendor catalogues untouched.** `.claude-plugin/marketplace.json` and
   `.agents/plugins/marketplace.json` keep their vendor-dictated filenames; the rename touches no bytes in
   `crates/uze-integrations`.

This supersedes the filename clauses of ADR-012 (Decision: "`marketplace.json` at the root plus
`plugins/**`") and ADR-015 (Decision: "with `marketplace.json` validation at registration"). All other
clauses of both ADRs stand as accepted.

## Consequences

- **BREAKING (pre-1.0):** every existing marketplace root must rename its manifest file; UZE errors name
  `agents.json`, so the break is loud and self-describing.
- The two-unrelated-things ambiguity is resolved: `agents.json` is UZE's registry manifest,
  `marketplace.json` at vendor paths is vendor-dictated. Filename now signals ownership and scope.
- No schema versioning was introduced; if the manifest ever gains one, it is a separate decision.
- The embedded `uze-official` snapshot is renamed in-tree (`build.rs` + bootstrap read `agents.json`),
  so bootstrap needs no compatibility path; `~/ai/agents.json` works unmodified.

Source change: openspec/changes/rename-marketplace-manifest-to-agents-json.
