## Context

See `proposal.md` for motivation and `research-notes.md` for the historical
cross-harness evidence. Claude Code and Codex are the first real integrations
for this increment. Cursor, OpenCode, Windsurf/Devin Desktop, and future
harnesses are peers in the architecture, but are not implementation targets
in this increment.

The preceding design correctly adopted AGENTS.md, Agent Skills, and MCP, but
still centered UZE on importing a vendor bundle into a single capability graph
and projecting generated artifacts to every harness. That boundary obscures a
more useful distinction: a project can already own portable instructions,
skills, and tool/context configuration, while a client and an agent can use a
separate protocol to communicate at runtime.

ACP is that separate Client ↔ Agent protocol. It negotiates protocol version
and capabilities during initialization and covers runtime concerns such as
session lifecycle, prompts, streaming updates, tool activity, permission
requests, diffs, and bidirectional notifications. UZE must reuse that
negotiation where ACP applies; it must not model a second capability-discovery
surface for the same facts. The official Rust SDK's `Proxy` and `Conductor`
are relevant implementation options for an explicit ACP proxy chain, but
their existence does not make ACP a universal UZE substrate.

## Goals / Non-Goals

**Goals:**

- Resolve an effective agent environment from a project-owned portable core
  and a selected runtime's optional enhancements.
- Preserve clear boundaries: project instructions/capabilities, MCP tool and
  context interoperability, and ACP Client ↔ Agent interoperability.
- Prefer native ACP, then a reliable ACP adapter, then a minimal explicit
  adapter, without requiring ACP from all targets.
- Report which standard, native mechanism, fallback, or lack of support led
  to every outcome.
- Record the remaining composition and harness-specific gaps without
  inventing a solution for them.

**Non-Goals:**

- No universal UZE wire protocol, client-agent protocol bridge, or wrapper
  for every coding-agent harness.
- No proprietary replacement for AGENTS.md, Agent Skills, MCP, or ACP; no
  duplicate ACP capability negotiation.
- No automatic synchronization of `.claude/`, `.cursor/`, `.codex/`, or
  `.opencode/` as a normal operation.
- No A2A integration, agent-to-agent orchestration, marketplace, daemon,
  cloud sync, lockfile resolution, or portable memory implementation.
- The core is implemented in Rust (ADR-004). The Rust ACP SDK remains an
  optional runtime-integration dependency, not an architectural dependency of
  the portable project core.

## Decisions

### 1. Compose the effective environment; do not translate a universal harness format

UZE resolves a project as a portable core plus optional harness enhancements:

```text
Agent Project
├── portable core
│   ├── AGENTS.md                 instructions
│   ├── Agent Skills              reusable capabilities
│   └── MCP                       tools, resources, external context
└── optional enhancements
    ├── Claude Code-specific
    ├── Codex-specific
    ├── Cursor-specific
    └── OpenCode-specific
```

The composition layer discovers the available inputs and produces an
explainable *effective agent environment* for the selected runtime. It is a
resolver and compatibility assessor, not a proprietary project format and not
a transcoder for every vendor directory. This records **adopt before invent**:

```text
open standard → native capability → explicit adapter → unsupported
```

The prior alternatives—one UZE-owned graph for all primitives and a
filesystem synchronizer as the primary product—were rejected because both
would make vendor configuration, rather than portable project composition,
the product's core abstraction. See ADR-001 and ADR-003.

The effective environment is composed from multiple resource sources: project
resources remain at the project, and Agent Plugin packages installed by UZE
remain in the UZE Store. The Engine merges those two sources before routing;
runtime resources are intentionally empty in this increment. The model reserves
global, user, and other package layers without implementing profiles, cloud
state, or a package graph.

### 1a. Keep the core harness-agnostic; make integrations peers

The UZE core SHALL operate on an effective environment, portable capabilities,
and a capability description supplied by an integration. It SHALL not contain
a named-harness support matrix, vendor-directory rules, or a source/target
relationship between harnesses.

```text
                 UZE Core
        Effective Environment + Router
                     │
             Integration Contract
              ┌──────┴──────┐
              ▼             ▼
     Claude Integration   Codex Integration
              │             │
          Claude Code      Codex
```

An integration supplies its identifier and `HarnessCapabilities`; the router
uses those inputs rather than branches over named harnesses. Removing a
Claude integration therefore leaves the core meaningful. Adding Cursor is a
new integration plus tests unless Cursor introduces a genuinely new capability
kind.

An **importer** is a separate boundary: it consumes a foreign representation
and produces core capabilities. `ClaudePluginImporter` may know
`.claude-plugin/plugin.json`; `ClaudeIntegration` uses an effective
environment to work with Claude Code. Neither is a source or destination in a
conversion pipeline. See ADR-005.

### 1b. Compose package resources through UZE_HOME before exposure

`UZE_HOME` is the sole source of UZE-owned paths. It uses the `UZE_HOME`
environment variable when set and otherwise resolves to `~/.uze`. The CLI
composition root resolves it once for `uze add` and `uze inspect`; deterministic
tests use `UzeHome::at`. The root
contains operational state only; it is not a UZE replacement for Agent Plugin
manifests:

```text
$UZE_HOME/
  store/packages/<agent-plugin-name>/
    plugin.json                 # preserved external Agent Plugins manifest
    skills/<name>/SKILL.md      # preserved standard Agent Skill
  state/packages.json           # UZE installation registry
  cache/
  runtime/<integration>/<session>/
```

The minimal composition flow is:

```text
Resource → Agent Plugin Package → UZE Store → UZE Engine
        → EffectiveEnvironment → Capability Router → Integration
```

Representation and exposure are independent. A resource can remain
`STANDARD` while an integration selects an explicit `RUNTIME_BRIDGE` or
`FILESYSTEM_PROJECTION`; a standard representation is never evidence that a
harness can discover a UZE store path directly. Integrations choose one of
`DIRECT_NATIVE`, `RUNTIME_BRIDGE`, `FILESYSTEM_PROJECTION`, or `UNSUPPORTED`.
Filesystem projection is permitted only as an explicit compatibility fallback.
It preserves the real caller project as the harness CWD and may create only the
minimal UZE-managed artifact required by a documented discovery mechanism. Its
runtime ownership metadata lives under `$UZE_HOME/runtime/<integration>/<session>`
and it cleans up only the artifact it created; it never copies or virtualizes
the project into a shadow workspace.

### 2. Keep standards and runtime protocols at distinct boundaries

The conceptual architecture is:

```text
                    PROJECT
                       │
              Agent Project Layer
                       │
        ┌──────────────┼──────────────┐
        │              │              │
   instructions      skills         tools
        │              │              │
   AGENTS.md      Agent Skills       MCP
        │              │              │
        └──────────────┼──────────────┘
                       │
               composition layer
                       │
                Agent Runtime
                       │
                      ACP
                       │
                    Client
```

AGENTS.md, Agent Skills, and MCP remain direct standards-facing inputs. MCP
does not become a substitute for ACP: it addresses tools/resources/context,
whereas ACP addresses Client ↔ Agent runtime interaction. A2A is reserved for
a future Agent ↔ Agent need, if one is concrete. UZE's distinct responsibility
is the project composition layer: resolving what the project means on a
selected runtime and stating what remains non-portable.

### 3. Use ACP only where its boundary applies

When UZE needs to integrate a client with an agent runtime, it selects the
first available path in this order:

1. native ACP implementation;
2. official or demonstrably reliable ACP adapter;
3. minimal, explicit integration adapter;
4. no runtime integration, with the limitation reported.

ACP protocol capabilities are taken from the ACP initialization handshake and
remain protocol facts, not UZE classifications. An ACP `Proxy` may mediate one
explicit concern; the official Rust SDK's `Conductor` may own an explicit chain
of such proxies. Neither is inserted silently, used to transform portable
project resources, nor required for a runtime that lacks ACP. This is a
hard-to-reverse boundary; see ADR-003.

### 4. Separate representation, route, and exposure

The classifier distinguishes two domains:

| Domain | Source of truth | Treatment by UZE |
| --- | --- | --- |
| Protocol capabilities | ACP negotiation, when ACP is selected | Preserve and report advertised support; do not re-discover or re-label it. |
| Capability representation | External standard, native source, or importer provenance | Record `STANDARD`, `NATIVE`, `UZE`, or `FOREIGN`; this does not claim availability in a harness. |
| Compatibility route | Capability plus integration-supplied `HarnessCapabilities` | Report `NATIVE`, `ADAPTABLE`, `DEGRADED`, or `UNSUPPORTED`, with evidence. |
| Exposure strategy | Integration-specific `ExposurePlan` | Report `DIRECT_NATIVE`, `RUNTIME_BRIDGE`, `FILESYSTEM_PROJECTION`, or `UNSUPPORTED`; strategy is not verification. |
| Verification | Real conformance result | Report `VERIFIED`, `NOT_EXPOSED`, `UNVERIFIED`, `FAILED`, or `BLOCKED_BY_ENVIRONMENT`. Authentication, quota, executable, service, and timeout blocks are never compatibility failures. |

The initial implementation keeps only the fields necessary for the Agent Skill
PoC, but preserves the separation. In particular, a standard representation
does not by itself prove that a harness has discovered or exposed it.

Hooks, custom commands, subagents, proprietary permission models, memory,
and experimental lifecycle extensions remain potential harness/project gaps.
They are not normalized for feature parity.

### 5. Make progressive enhancement and DX observable

The developer flow is:

```text
define project → standards discovered → capabilities discovered
→ runtime selected → agent works
```

At each step UZE reports the source and result: standard-native, runtime-native,
ACP-negotiated, explicit fallback, or unsupported. It never treats a generated
file or proxy as evidence that semantics are equivalent. A compatibility import
of a declarative plugin bundle remains available, but its contents are treated
as evidence and optional enhancements—not as canonical UZE configuration.

### 6. Keep importers and filesystem fallback outside normal composition

Project discovery starts with standard project resources. Foreign formats are
read only by explicit importers. Filesystem projection is a managed fallback
after standard, native integration, and explicit adaptation have been
considered; it is not normal harness integration or project synchronization.

## Standards Coverage / Remaining Gap

| Concern | Standard | Coverage | Remaining gap |
| --- | --- | --- | --- |
| Project instructions | AGENTS.md | Portable project instruction file where a runtime supports it | Adoption and discovery behavior vary; proprietary rules may remain optional enhancements. |
| Reusable capabilities | Agent Skills | Portable `SKILL.md` capabilities and assets | Skill locations, invocation UX, and vendor extensions can vary. |
| Tools/resources | MCP | Standard tool, resource, and prompt/context interoperability | Client configuration, authorization, and runtime-specific exposure vary. |
| Client ↔ Agent | ACP | Session lifecycle, prompts, streaming, tool activity, permissions, diffs, and negotiated capabilities when both sides support ACP | Not every target supports ACP; adapters and transports remain a compatibility decision. |
| Agent ↔ Agent | A2A | Candidate standard for independent agents | No concrete MVP need; orchestration, identity, and coordination are out of scope. |
| Project composition | None | No standard defines how to resolve a whole effective agent environment across standards and optional native enhancements | UZE must discover, resolve, and explain composition without defining a competing project format. |
| Harness-specific capabilities | None | Native vendor mechanisms may be used as optional enhancements | Hooks, custom commands, subagents, permission models, memory, and experimental features have no safe universal equivalence. |

## Risks / Trade-offs

- **ACP is an emerging standard and unevenly adopted.** → Select it only at
  the Client ↔ Agent boundary and keep native/explicit fallback paths.
- **ACP Proxy/Conductor chains can make adaptation look universally solved.**
  → Permit them only as explicit runtime integration components; do not use
  them to convert project standards or conceal a semantic mismatch.
- **Standards do not standardize project composition.** → Make the unresolved
  gap visible in the report and keep UZE's resolver thin and evidence-based.
- **A standard representation can still need harness-specific discovery.**
  → Report conventional-location and configuration differences; never infer
  that standards support means zero setup.
- **Harness-specific enhancements may tempt feature-parity translation.** →
  Require `ADAPTABLE` evidence and mark the rest `UNSUPPORTED` rather than
  generating a lossy approximation.

## Migration Plan

The inspector's static `Harness` enum, evidence matrix, and vendor-directory
discovery move behind integration and importer boundaries. The CLI adds the
minimal local `uze add` store operation and makes `uze inspect` resolve the
actual composed environment; it is not a harness launcher. Existing bundle
import remains available as explicit foreign-format import; its Claude-specific
discovery moves to `ClaudePluginImporter`.

The LikeC4 model is updated in the same increment to show the harness-agnostic
core and peer Claude/Codex integration containers.

## Open Questions

- Which of the four active targets offer native ACP or a reliable maintained
  ACP adapter at implementation time? This is a runtime-selection fact to
  verify per release, not a prerequisite for the model.
- Which documented project conventions are safe enough to be recognized as
  optional enhancements without assigning them portable semantics?
- If Rust is selected for implementation, which Proxy/Conductor concerns are
  concrete enough to justify a chain rather than a direct ACP connection?
