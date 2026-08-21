## Context

The prior research accepted Plugin First conceptually. The present code is
still resource-first operationally:

```text
UzeStore copies plugin.json + skills/ + optional mcp.json
  -> UzeEngine flattens each installed package into Resources
  -> IntegrationPort::exposure_plan(Resource)
  -> CapabilityRouter route per Resource
```

This was correct for isolated Skills/MCP tracer bullets, but it cannot choose
native whole-plugin attachment before flattening and it does not preserve an
external package's non-core vendor files or provenance revision. The design
below scopes the smallest necessary evolution without implementing it.

## Decided design boundary for the vertical slice

The implemented input is one **dual-envelope external package**, not a UZE-created
format:

```text
portable-review-tools/
├── plugin.json                       # Agent Plugins 1.0 core
├── skills/
│   └── uze-e2e/
│       └── SKILL.md
├── mcp.json                          # Agent Plugins 1.0 MCP config
├── .codex-plugin/plugin.json         # source-provided Codex-native envelope
└── .mcp.json                         # source-provided Codex MCP config
```

The root Agent Plugins core is the portable representation. Codex's native
metadata/config is an original, source-provided compatibility overlay—never a
UZE-generated wrapper. The fixture deliberately has no Claude envelope, so
Claude correctly falls back to existing Skill/MCP attachments rather than
being reported native-plugin compatible.

Current official Codex documentation requires the source-provided
`.codex-plugin/plugin.json` envelope. The slice preserves it and creates a
standard local marketplace catalog; actual installed-CLI behavior remains an
opt-in conformance check.

## Native attachment strategies to test next

### Claude Code

1. Check whether the stored package contains a valid original
   `.claude-plugin/plugin.json` plus every source-referenced artifact.
2. Expose that exact stored package through a UZE-managed local Claude
   marketplace (`.claude-plugin/marketplace.json` is catalog metadata, not a
   new plugin spec), then install `plugin@marketplace` at user scope.
3. Let Claude's plugin lifecycle own Skill and `.mcp.json` loading; do not
   separately register the same components.
4. If this exact native envelope is absent or rejected, make independent
   fallback plans: `ManagedUserScopeReference` for Skills and
   `ManagedVendorConfig` for MCP.

CLI and official documentation prove the necessary local marketplace/install
surface. Its cache-copy/update behavior must be measured in an isolated home.

### Codex

1. Detect a root Agent Plugins 1.0 manifest and validate it before planning.
2. Expose the untouched stored package through a UZE-managed local Codex
   marketplace (`.agents/plugins/marketplace.json`) and use
   `codex plugin add plugin@marketplace`.
3. Consider Skills + `mcp.json` consumed by that native package plan only
   after an empirical probe proves both discovery and behavior.
4. If native installation is unavailable in a version/environment, use the
   proven Resource fallbacks; do not generate a `.codex-plugin` wrapper.

Codex's current source recognizes root Agent Plugins while its docs establish
the local marketplace CLI. The implementation must version-pin and test that
combination.

### OpenCode

OpenCode has no Agent Plugins package/marketplace loader established. It is
therefore deliberately **not** asked to consume the package as a plugin.
Instead, the package-aware plan derives only AP 1.0 core components and uses
documented OpenCode primitives:

- **Skill — native capability attachment:** OpenCode globally discovers
  `~/.agents/skills/*/SKILL.md`, the same user-scope location already used
  for Codex. A UZE-managed reference there is native OpenCode discovery; it
  should be represented as consumed through a shared managed attachment,
  never projected into every workspace.
- **MCP — adaptable capability attachment:** map AP `mcp.json` transport
  semantics into OpenCode's global `~/.config/opencode/opencode.json` MCP
  shape or its supported management surface, preserving OpenCode's own
  configuration/authorization lifecycle. The config shapes are different
  (`type: stdio` + tokenized `command` versus `type: local` + command array),
  so this is `ADAPTABLE`, not native envelope support.
- **Hooks/Agents/Commands — out of scope:** do not fabricate an OpenCode
  plugin module. A future component-specific study can use its documented
  JS/TS extension API and separate native configuration, but must report
  semantics and trust differences explicitly.

## Minimal model evolution after approval (not in this change)

`PackageId` can remain the local installed identity and stay one-to-one with
the first `Plugin`. Do **not** introduce five persisted entities now. Add
only an in-memory package-aware plan before flattening:

```text
StoredPackage (= one Plugin in this slice)
  - id, root, original manifests/files, source provenance
  - derived components: Skill[], Mcp[]
  |
  +-- PluginAttachmentPlan per Integration
        - Native { consumed component identities }
        - NoNativeEnvelope
  |
  +-- remaining Resource exposure plans (existing router)
```

The critical new datum is `consumed component identities`, preventing native
plugin attachment and capability fallbacks from installing a Skill/MCP twice.
`Resource`, `Capability`, `Attachment`, and `Package` need not become five
new domain modules for this proof.

## Router meaning (unchanged until empirical proof)

The existing route labels are adequate if constrained precisely:

| Route | Meaning in the future slice |
|---|---|
| `NATIVE` | harness consumes the exact supported external plugin envelope, or the exact standard capability through a documented native discovery contract |
| `ADAPTABLE` | UZE makes an explicit, safe translation/registration into a different native primitive; source payload/envelope remains preserved |
| `DEGRADED` | a documented delivery path exists but knowingly omits or weakens material semantics, which are named in the report |
| `UNSUPPORTED` | no safe documented plan; never infer this from credentials, quota, or a failed probe |

No `CapabilityRouter` change is justified until the OpenCode MCP mapping and
native package consumption produce a real planning requirement.

## Source/import/provenance model (future)

Compatibility must be based on the preserved package and derived capabilities,
not the marketplace that supplied it. Minimum record, either alongside the
existing registry or derived from an immutable installation receipt:

```text
source.kind        local_path | git | claude_marketplace | codex_marketplace | agent_plugin
source.locator     original path, URL, or marketplace + package selector
source.revision    commit SHA / tag / version when observed; absent when unknown
source.fetched_at  install time
plugin.id          validated external manifest name
plugin.manifests   preserved relative paths + byte identity/digest
```

`source.kind` is provenance only. A Codex-marketplace package entering the
Store is never routed as `codex_plugin -> claude`; the integration examines
the stored envelopes and components. The Store must copy/preserve the full
validated source tree (including vendor compatibility overlays and referenced
assets), not only `plugin.json`, `skills/`, and `mcp.json`. It must reject
paths escaping the source root and retain original bytes; it must not
normalize manifests destructively.

Remote registry fetch is explicitly deferred. The first `uze add` may still
accept the existing local path while the data model makes Git/marketplace
provenance possible later.

## Multi-capability tracer bullet (future test design)

### Fixture

Add one immutable external source fixture separate from the existing
single-capability fixtures. It contains one AP Skill with the existing
deterministic proof token and one genuine stdio MCP server with a distinct
proof token; it also contains only the source-provided Claude compatibility
overlay necessary for native Claude plugin loading. It must not be created or
rewritten by UZE. Test setup may replace a clearly marked command placeholder
with Cargo's resolved fixture-binary path before Store installation.

### Deterministic proof

1. Install this one directory once in an isolated `$UZE_HOME`.
2. Assert complete source-tree preservation and one plugin/package identity.
3. Derive two components with their common plugin parent.
4. Assert Claude native-plugin plan, Codex native-plugin plan, and OpenCode
   component plans before executing any real harness.
5. Assert a native plan marks both components consumed and emits no duplicate
   fallback plan; OpenCode reports Skill `NATIVE` and MCP `ADAPTABLE`.

### Real-harness tiers

1. **Configuration:** each harness reports/retains its attachment.
2. **Discovery:** Skill visible and MCP connected/listed where a CLI supports
   that assertion.
3. **Behavioral:** a plain fresh harness session returns the Skill proof and
   invokes the MCP tool proof. OpenCode may be `BLOCKED_BY_ENVIRONMENT` when
   its model/provider is unavailable, not `UNSUPPORTED`.

Actual expected table, pending probes:

| Component | Claude | Codex | OpenCode |
|---|---|---|---|
| Plugin envelope | native only with source Claude overlay | native AP 1.0, source-confirmed pending empirical | unsupported envelope |
| Skill | consumed by native plugin | consumed by native plugin | native shared user-scope discovery |
| MCP | consumed by native plugin | consumed by native plugin | adaptable generated OpenCode config |
| Hook / Agent / Command | not part of fixture | not part of fixture | not part of fixture |

## UZE Local Marketplace model (future)

UZE Local Marketplace means local authoritative state, not a server:

```text
$UZE_HOME/
  store/packages/        immutable installed plugin roots
  state/packages.json    provenance + installed identity
  state/attachments.json harness attachment facts
  state/compatibility/   derived, reproducible evidence cache (if needed)
```

It can later support `list`, `add`, `remove`, `inspect`, and `doctor`; a
vendor marketplace catalog is merely one attachment/distribution adapter,
not UZE's canonical catalog schema. No remote registry or account is implied.

## Minimal Rust TUI architecture (future)

The TUI is an adapter over use-case functions, not another engine:

```text
clap commands / TUI events
        |
        v
application facade: list, add, remove, inspect, setup, doctor
        |
        v
UzeStore + UzeEngine + report builder + IntegrationPort
        |
        v
read-only ViewModel mapped from domain report/state
        |
        v
ratatui/crossterm renderer (future dependency only when implemented)
```

Start with a single-screen list/detail split: installed plugins, harness
health, selected plugin's explicit component-by-harness routes. Reuse the
same application facade for non-interactive CLI commands so TUI action
handlers contain no store/router policy. Add/remove/setup remain explicit
confirmation flows; no network/login is added.

## Risks

- Claude's AP root-envelope support is unproven; the slice must not mask this
  by generating vendor files. The native claim requires source-provided
  compatibility metadata.
- Codex AP support is current-source evidence, not yet UZE behavioral proof.
- Vendor marketplace installs commonly copy/cache packages, so Store updates
  do not automatically mean harness updates; attachment state needs version
  and refresh facts.
- OpenCode's plugin API is executable JS/TS and V2 is beta. It is not a safe
  target for a fabricated UZE wrapper.
- MCP config mapping may expose command, environment, authorization, and
  approval differences. It must retain source security boundaries and avoid
  secrets in UZE state.
- Shared `~/.agents/skills` discovery makes duplicate ownership/collision
  detection important; UZE must never overwrite unrelated entries.
- A full-tree Store copy broadens supply-chain/path-safety responsibility;
  install-time containment and symlink policy must be explicit.

## Classification

**PLUGIN-FIRST PORTABILITY READY FOR IMPLEMENTATION**, with the explicit
vertical-slice scope above. “Ready” does not mean every pure Agent Plugin is
native in Claude: the plan accurately falls back for an envelope Claude does
not understand. The first implementation must prove, rather than assume,
Claude/Codex native consumption and OpenCode's graceful per-capability path.
