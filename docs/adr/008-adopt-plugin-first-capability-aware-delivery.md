# Adopt Plugin First, Capability Aware delivery

Status: Accepted

## Context

ADRs 006 and 007 proved Agent Skill and MCP attachment separately. That
resource-first model could not express the distinction between a package a
harness installs as one native plugin and a package whose portable components
must be delivered individually. It also copied only portable files, losing
source-provided vendor envelopes.

The combined fixture establishes a real asymmetry: Codex accepts the
source-provided `.codex-plugin/plugin.json` package through its documented
local marketplace; Claude Code does not consume that envelope and retains the
existing Skill/MCP attachments; OpenCode does not consume it either, but does
discover a user-scope Skill and run MCP configured through its own config.
Marketplace provenance is not a compatibility rule.

## Decision

UZE is **Plugin First, Capability Aware**. A preserved external package is
the distribution unit; a resource/capability is the compatibility unit; an
integration selects delivery. `StoredPackage` and Plugin remain one-to-one in
this slice and `PackageId` remains the installed identity.

The Store preserves the complete validated source tree and publishes only a
standard Codex local marketplace catalog for packages that already contain a
Codex manifest. It never creates a UZE plugin format. A package plan lists
the resource identities consumed by native package delivery, so individual
fallback attachment cannot duplicate them. Without an exact native envelope,
UZE decomposes conservatively: Claude uses ADR-006/007 fallbacks; OpenCode
uses global `~/.agents/skills` discovery and safely adapts stdio command/args
to its documented global MCP config.

## Consequences

The same Skill+MCP package can be delivered once to Codex as a native plugin
and decomposed for peers without `codex -> claude` conversion. The native
Codex CLI/cache behavior remains `UNVERIFIED` until an opt-in real-harness
run. OpenCode configuration conflicts fail rather than overwrite unrelated
user entries. Hooks, Agents, Commands, remote sources, TUI, proxy/runtime,
and a registry remain out of scope.

## Implementation Plan

- **Affected paths:** Store/home, exposure/integration, Codex/OpenCode
  integrations, CLI, combined fixture, and shared-store test.
- **Pattern:** preserve the external tree; generate only vendor-standard
  catalog/config at the integration edge; use consumed resource identities
  instead of adding a global compatibility enum.
- **Avoid:** UZE manifests, wrapper runtimes, vendor-to-vendor conversion,
  duplicate attachment, and behavioral claims from planning evidence.

### Verification

- [x] One fixture holds Skill and stdio MCP under one PackageId.
- [x] One Store installation preserves the envelope and derives both resources
      in one EffectiveEnvironment.
- [x] Codex native package planning consumes both identities.
- [x] Claude retains adaptable Skill/MCP delivery.
- [x] OpenCode receives native global Skill discovery plus safe MCP adaptation.

Source change: openspec/changes/validate-plugin-first-portability-across-peer-and-adversarial-harnesses/

## More Information

2026-08-20: A real Codex CLI configuration probe ran with an isolated
temporary `$HOME` and `$UZE_HOME`. `uze add` created the UZE standard local
marketplace, `codex plugin marketplace list --json` reported `uze-local`,
and `codex plugin list --json` reported the installed, enabled
`uze-plugin-first-conformance@uze-local` at its cache path. No model turn or
MCP invocation was made, so this is native package configuration/install
evidence, not behavioral capability verification.
