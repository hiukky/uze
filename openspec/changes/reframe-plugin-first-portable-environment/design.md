## Context

The UZE Store presently imports an Agent Plugins-shaped directory into
`$UZE_HOME/store/packages/<PackageId>/`, preserving `plugin.json`, `skills/`,
and optional `mcp.json`. `UzeEngine` then flattens the installed package into
one `Resource` per skill and MCP server. `CapabilityRouter` selects an
integration route per resource. This model was intentionally sufficient for
the two tracer bullets, and ADR-006/ADR-007 prove its fallbacks work.

It loses package identity after composition, however: an integration cannot
currently decide to expose the *whole installed package* natively before it
sees `Resource`s. It also cannot retain an unmodified vendor extension such
as Claude agents or hooks as a package-level unit. This is a real gap only if
native plugin attachment is preferred and supported; the research concludes
that it is, but only for explicit package formats and capability subsets.

## Research conclusion

**Verdict: PLUGIN-FIRST VIABLE WITH IMPORTANT GAPS.**

Agent Plugins 1.0 is a strong canonical external envelope for exactly its
portable core: root `plugin.json`, immediate-child Agent Skills in `skills/`,
and MCP configuration in root `mcp.json`. It intentionally does not
standardize Commands, Hooks, Agents, Rules, distribution catalogs, trust,
secret storage, lifecycle, or vendor UI. Native envelopes are therefore not
interchangeable, and UZE must not claim they are.

The correct preference order is:

```text
native external plugin (same supported envelope)
  -> native attachment of an individual standard capability
  -> safe, documented adaptation
  -> explicit unsupported
```

“Bridge” is reserved for a genuine runtime data-path requirement; none is
assumed by this design.

## Target conceptual model (not implemented)

```text
External plugin
      |
      v
UZE local store  -- preserves original package roots and provenance
      |
      +--> InstalledPlugin / package metadata
      |        |
      |        +--> capability graph (derived, not converted)
      |
      v
EffectiveEnvironment
      |
      v
Compatibility router
      |
      +--> ClaudeIntegration  -- native Claude plugin if compatible
      +--> CodexIntegration   -- native Agent Plugin/Codex plugin if compatible
      +--> Other integration  -- native package or capability fallback
```

The graph must retain the package-to-component relation. A plugin containing
`review`, `security`, `github`, `before-tool`, `architect`, and `ship` must
report those as distinct components while retaining one original package
root. It must preserve the source documents rather than translating them to a
UZE proprietary resource format.

## Identity recommendation (not implemented)

Keep `PackageId` as the local installed identity for now, but stop treating
the terms as interchangeable in the next design phase:

```text
InstalledPackage (local/provenance/installation identity)
  └── Plugin (external declared envelope and root)
        └── Component/Resource (one material item)
              └── Capability (semantic compatibility classification)
```

Initially `InstalledPackage` and `Plugin` may be a one-to-one relation. Do
not introduce separate persisted IDs until an actual input proves one package
contains multiple independently installable plugins (a marketplace is the
likely first such input). The present `PackageId` is deliberately not changed
in this research phase.

## Integration interface direction (not implemented)

Do not blindly change `IntegrationPort`. Current `exposure_plan(&Resource)`
has no package-level dispatch point, but an eventual design needs to answer
two questions independently:

```text
plugin_support(plugin/envelope) -> native / partial / no
attach_plugin(plugin)           -> optional package-level attachment
capability fallback             -> existing Resource exposure plan
```

The likely shape is an additive package/envelope planning layer above the
existing resource route, not replacing `exposure_plan`. The native plan must
also return which components it consumed so the router only falls back for
the remainder. This avoids duplicate Skills/MCP registrations.

## Existing fallback mechanisms

`ManagedUserScopeReference` remains the static capability fallback for a
Skill when whole-plugin exposure is unavailable or unnecessary. It leaves the
UZE Store authoritative through a user-scope symlink; the harness owns
runtime discovery.

`ManagedVendorConfig` remains the generated per-harness configuration route
for an MCP server when a package cannot be consumed as a native plugin. It
does not make UZE a runtime proxy: Claude/Codex start and use their own MCP
configuration at runtime.

Neither mechanism is removed, generalized, or changed by this proposal.

## Native marketplace/source finding

Claude Code and Codex both have a local marketplace-source concept, but the
catalog and envelope are vendor-specific. Claude supports a local
`.claude-plugin/marketplace.json` and caches installed plugin copies. Codex
supports a local/Git marketplace (`codex plugin marketplace add`) and now
recognizes portable root Agent Plugin manifests in its current source.

Thus “register UZE once as a marketplace” is technically plausible as a
future experiment for both, but is **not yet a common implementation
strategy**:

- their marketplace manifest schemas, source semantics, cache lifecycle,
  enablement, and trust policy differ;
- a source must be refreshed/reloaded before newly added packages appear;
- a local catalog may be a copy/install path, not a persistent reference to
  the UZE Store;
- user action and vendor-specific installation policy may be required.

It is therefore a hypothesis for a focused future tracer bullet, not a
replacement for the two proven fallbacks.

## Current-code impact if accepted later

| Module | Later responsibility | Why no change now |
|---|---|---|
| `src/store.rs`, `src/home.rs` | preserve package root, source/provenance, and later package catalog state | Current copy/store is enough for Skills/MCP. |
| `src/engine.rs`, `src/project.rs` | construct a package-aware component graph before flattening resource fallbacks | Current engine intentionally yields resources only. |
| `src/capability.rs`, `src/bundle.rs` | possibly distinguish external envelope/component metadata from a capability kind | Do not make Plugin a capability. |
| `src/integration.rs`, `src/router.rs`, `src/report.rs` | additive native-plugin planning, consumed-component accounting, explicit capability report | The existing router is correct for resource-only fallbacks. |
| `src/integrations/claude.rs`, `codex.rs` | documented package/marketplace probes and native exposure plans | Needs a separately approved, empirical tracer bullet. |
| `src/importers/*` | recognize standard plus vendor envelopes without conflating their semantics | Must be driven by real import examples and security review. |
| `src/main.rs`, `src/state.rs` | eventual `list`, `remove`, package-level `inspect`, setup facts | No UX expansion is authorized. |

## Third harness recommendation

**Recommend OpenCode as the next adversarial harness; keep Gemini CLI as the
second candidate.** OpenCode is a relevant, documented, open-source agent
with individual Skills/MCP/Agents/Commands and a powerful in-process
JavaScript/TypeScript plugin API, but no native Agent Plugins package or
marketplace model. Its current official Agent Plugins support request is
still open. This forces a genuine distinction between a portable package and
capability-level adaptation without pretending formats are identical.

Gemini CLI is a strong alternate: it has a documented proprietary extension
bundle, local/Git installation, symlink development link, Skills, MCP,
Hooks, Commands, and preview subagents. It is better for a future
“translate to another package envelope” study, but is less adversarial to
the package-first hypothesis because it already offers a broad bundle
primitive. Cursor is not adversarial for the portable core: it directly
loads Agent Plugins without changes.

## Risks and required future proofs

- Agent Plugins 1.0 is new and deliberately narrow; client support and
  version behavior need pinned empirical conformance.
- Vendor extension namespaces have no portable semantics. Similar directory
  names do not establish equivalent hook, command, or agent behavior.
- Marketplace catalogs are vendor-specific and installation can copy/cache
  the source, which affects update and symlink assumptions.
- Executable hooks and MCP servers carry code execution, secret, trust,
  approval, and supply-chain risks. Native installation must not silently
  bypass harness review or policy.
- Marketplace/config behavior is partially undocumented or evolving; a
  future implementation must label `OFFICIAL`, `SOURCE_CONFIRMED`, and
  `EMPIRICAL` evidence separately.

## Deferred decision

No ADR is created in this change. After review, an ADR may accept,
constrain, or reject Plugin First based on a small, explicit native-plugin
attachment experiment. Until then the accepted architecture remains ADR-006
and ADR-007.
