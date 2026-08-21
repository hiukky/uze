## Why

The user accepted the direction **Plugin First, Capability Aware**. UZE must
now prove its first plugin-level vertical slice before it grows the core or
implements Hooks/Agents: one external multi-capability plugin, installed once
in the UZE Store, exposed through native plugin systems wherever they truly
support its envelope, and safely decomposed only for an adversarial harness.

The two completed tracer bullets proved Agent Skill and MCP attachments in
isolation. They did not prove that UZE preserves and reasons about one plugin
containing both components, nor that marketplace provenance is distinct from
compatibility. OpenCode is the first adversarial harness because current
documentation proves its runtime primitives but not the Agent Plugins package
envelope.

## What Changes

- Research, design, and implement the Codex-native, Claude-fallback, and
  OpenCode-adversarial paths
  for one external multi-capability plugin.
- Establish the minimal source/provenance model required to keep an imported
  plugin's original/open representation intact.
- Add the combined Skill + MCP conformance fixture and deterministic shared-
  store planning evidence.
- Specify the minimum package-aware planning layer required before current
  resource-level routing, without changing `CapabilityRouter` yet.
- Formalize the local-marketplace concept as an installed plugin library,
  compatibility metadata, and harness attachments—not a cloud service.
- Propose a thin Rust TUI architecture that presents existing engine/report
  data without duplicating domain logic.

## Non-Goals

- No OpenCode behavioral probe beyond deterministic configuration/attachment.
- No TUI implementation.
- No Hooks, Agents, Commands/`UzeAction`, remote registry, cloud marketplace,
  login, runtime proxy, or new UZE plugin manifest.
- No Router rewrite, TUI, remote source, proprietary plugin format, or
  implementation beyond this approved vertical slice.

## Implemented success criterion

```text
one external plugin
  -> one UZE installation preserving its original representation/provenance
  -> package-aware plan
  -> Claude native plugin attachment when source envelope supports it
  -> Codex native Agent Plugin attachment
  -> OpenCode native primitive/fallback attachment per component
  -> an explicit report of what was native, adaptable, degraded, or unsupported
```

No outcome is considered success merely because every component is forced to
load. Graceful, evidence-backed unsupported status is correct behavior.
