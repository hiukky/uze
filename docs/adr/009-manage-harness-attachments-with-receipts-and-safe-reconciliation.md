# Manage harness attachments with receipts and safe reconciliation

Status: Accepted

## Context

ADR-013 established that a package can be delivered as one native plugin or
decomposed into capability attachments. Those delivery paths create external
state owned by a harness: discovery symlinks, MCP configuration entries, and
Codex native-plugin installations. Store provenance alone cannot establish
that an artifact still belongs to UZE at removal time: a user may repoint a
symlink, edit a configuration entry, disable a plugin, or replace a
marketplace source.

The alternatives were to trust a UZE-only ledger during detach, to give Core
knowledge of each vendor configuration format, or to make integrations prove
current ownership. A ledger-only approach can delete user changes; a shared
vendor parser would violate the integration boundary.

## Decision

Every persistent attachment returns an `AttachmentReceipt`. The receipt
records package identity, optional resource identity, integration, delivery
strategy, and one typed artifact:

- `SymlinkReference { path, target }`
- `VendorConfigEntry { entry_name, command, args }`
- `MarketplacePlugin { selector, marketplace_root, package_root }`

`$UZE_HOME/state/attachments.json` persists these receipts as secret-free
ownership intent. It is not the source of truth for a live harness. Before
detach, the owning `IntegrationPort` inspects its receipt and returns exactly
one of `MATCHED`, `MISSING`, `DRIFTED`, `CONFLICT`, or `BLOCKED`.

Only `MATCHED` permits detach. `MISSING` needs no detach; `DRIFTED`,
`CONFLICT`, and `BLOCKED` preserve external state and block package removal.
Reconciliation aggregates ledger receipts with integration inspections; it
does not repair anything automatically.

Mutation prefers harness CLIs/APIs. Read-only vendor configuration fallback
is permitted only inside its integration when no sufficient structured API
exists. Claude MCP writes/removes through `claude mcp add/remove` and reads
only the necessary fields of `~/.claude.json`; Codex MCP uses `codex mcp get
--json` and `codex mcp remove`; Codex plugins use `codex plugin ... --json`
and `codex plugin remove`; OpenCode owns its config parsing and edits only a
matched MCP entry. UZE never rewrites Claude or Codex configuration formats.

## Consequences

The future remove command and doctor can share one deterministic inspection
model. A corrupt ledger blocks destructive work rather than being rebuilt.
Codex's marketplace remains shared: removing a managed plugin never removes
the marketplace itself. The Core, Store, CapabilityRouter, and
`PackageExposurePlan` remain vendor-schema-free and do not gain lifecycle
semantics beyond existing anti-duplication planning.

This adds a small persistent compatibility obligation: receipt shapes must be
backward-compatible or explicitly migrated. It deliberately does not add
automatic repair, `--force`, public removal, application services, or a TUI.

## Implementation Plan

- **Affected paths:** `src/integration.rs`, `src/state.rs`,
  `src/reconciliation.rs`, the Claude/Codex/OpenCode integrations, and
  lifecycle contract tests.
- **Pattern:** integrations create, inspect, and safely detach typed receipts;
  `state` persists only ownership intent; reconciliation calls integrations
  through `IntegrationPort` and computes a conservative removal plan.
- **Avoid:** raw Codex TOML mutation, Claude config reserialization, global
  discovery-directory deletion, ledger-based blind deletion, Store-owned
  harness artifacts, and auto-repair.

### Verification

- [x] Symlink, vendor-config, and native marketplace receipts are typed and
      persisted under `$UZE_HOME/state`.
- [x] Ledger persistence, duplicate-key idempotency, multi-receipt packages,
      native-package-only receipts, and corrupt-ledger blocking are tested.
- [x] Claude MCP inspection is read-only and detects missing, drifted,
      malformed, and unknown-field cases.
- [x] Codex MCP reads the official `get --json` response and Codex plugin
      inspection verifies marketplace, plugin, source, installed, and enabled
      identity before its CLI detach path.
- [x] OpenCode removes only a matched MCP entry and preserves unrelated data.
- [x] Reconciliation and removal planning block drift, conflicts, blocked
      inspection, and corrupt ownership state.

Source change: openspec/changes/consolidate-plugin-first-v0-experience/
