# Adopt a canonical portable Hook capability

Status: Accepted

Its command ABI and dispatcher are superseded by
[040 — Compile portable hooks into the delivered artifact](040-compile-portable-hooks-into-the-delivered-artifact.md):
handlers now read the hook context from `HOOK_*` environment and answer with
an exit code, and the translation is compiled into a wrapper vendored in the
delivered artifact instead of running in the `uze` binary. Everything else
below — the canonical manifest, its events, matchers, effects and handler
shape, the capability profiles, the route vocabulary — still holds.

## Context

UZE's Store, Engine, IntegrationPort, and receipt ledger already separate
canonical package bytes from vendor delivery. `CapabilityKind::Hook` exists
only for importer compatibility, however: a package cannot currently publish
portable hooks and integrations deliberately leave that capability uncovered.

Claude Code, Codex, and Antigravity CLI each offer command hooks around tool
use and agent stop, but their event payloads, tool names, decisions, matcher
syntax, and execution ordering differ. OpenCode instead offers TypeScript
plugin callbacks for before/after tool execution and has no equivalent Stop
event. Choosing one vendor's JSON contract would make it the de-facto UZE
format; asking authors for an OpenCode plugin would make the common path
imperative and require a TypeScript toolchain.

The durable choice is how a package expresses portable automation and where
vendor normalization belongs. It refines ADR-013 (native projection,
explicit and generated), ADR-029 (projection conflicts), and ADR-031
(canonical Agent capability).

## Decision

UZE will adopt package-root `hooks.json` as a command-only canonical Hook
capability. Its initial semantic events are `PreToolUse`, `PostToolUse`, and
`Stop`; only the first two are claimed for OpenCode. A group has a stable
identity, a `|`-separated matcher of portable tool aliases or explicit
`native:<tool>` names, and ordered command handlers with a bounded timeout.

`uze-core::hook` owns the manifest parser, validation, normalized Hook IR,
portable aliases, and capability assessment inputs. It remains vendor-neutral.
Integrations own a declarative hook-capability profile and compute each route
from event, available data, decision/effect, transform safety, matcher,
handler type, and ordering. A route is explicitly `native`, `adapted`,
`degraded`, or `unsupported`; security-relevant loss cannot be implicit.

Handlers receive a normalized JSON object on stdin and may emit one bounded
JSON decision on stdout. UZE provides the small generated/owned dispatcher
needed to translate native payloads and decisions while keeping author scripts
portable, setting `PLUGIN_ROOT`, running handlers sequentially, and preserving
the first denial. This is intentionally a narrow command ABI, not a general
plugin runtime.

Claude Code, Codex, and Antigravity CLI receive their documented native hook
configuration through their integration. OpenCode receives an owned,
regenerable `.opencode/plugins/uze-hooks-<package>.ts` bridge and a narrowly
managed configuration entry; no author TypeScript compilation is required.
Every derived entry or file is receipt-owned and uses inspect-before-detach.

Raw vendor hook files were rejected because they leak vendor semantics into
the canonical package. A generic UZE TypeScript-plugin API was rejected
because it excludes shell/script authors and expands the portable surface far
beyond hooks. Directly invoking author commands with native payloads was
rejected because it abandons the promised cross-harness ABI.

## Consequences

Authors write `hooks.json` plus scripts once, and maintainers have an explicit
place to add a fifth harness: a profile and adapter rather than core vendor
conditionals. Status, doctor, plans, and the support matrix can explain an
exact semantic downgrade instead of presenting matching event names as proof.

The project owns a small dispatcher and one generated OpenCode bridge, both of
which must track vendor contracts and be covered by fixture and real-harness
conformance. Hook execution necessarily adds a child-process boundary, and
the cross-platform command/JSON contract must be tested on supported systems.
Some lifecycle hooks intentionally remain unavailable where a harness does
not offer their semantics; in particular, OpenCode Stop is never represented
as a tool callback.

## Implementation Plan

- **Affected paths:** `crates/uze-core/src/{capability,engine,project,router,
  integration,hook}.rs`; `crates/uze-application/src/application/doctor.rs`;
  `src/{main,command_performance,ui/view/harnesses}.rs`; each vertical in
  `crates/uze-integrations/src/`; tests under `crates/*/tests`, `tests/`, and
  `conformance/harnesses/{claude,codex,antigravity,opencode}/`; README and
  capability documentation.
- **Pattern:** model each group as a `CapabilityKind::Hook` resource, then
  follow existing AgentSkill/MCP exposure, attachment receipt, inspection, and
  detach paths. Generated outputs are deterministic, namespaced, and never
  replace an unowned user file or entry.
- **Avoid:** vendor identifiers in Core/Application/CLI; direct Store mutation;
  broad OpenCode SDK exposure; a native compatibility claim without a passing
  fixture/conformance scenario; destructive cleanup without receipt inspection.
- **Dependencies/configuration:** no new runtime dependency. `hooks.json` is
  the sole authored manifest; generated dispatcher/bridge locations are owned
  integration artifacts.

### Verification

- [x] Parser, schema validation, aliases, IR, compatibility and ABI decisions
      have deterministic unit coverage.
- [x] Each integration has native/bridge emission, existing-config merge,
      idempotence, inspection, drift, and safe-detach tests.
- [x] Bridge tests prove matching, normalized stdin, transformation, denial,
      reason propagation, sequence, handler error, timeout, and regeneration.
- [x] CLI status/doctor/TUI expose compatibility and generated artifacts.
- [ ] Four real-harness conformance verticals prove only documented, observed
      claims; unavailable semantics are diagnosed. (Phases added for all
      four verticals, grouped describe/test; real runs recorded — claude
      18/18, opencode/antigravity 27/28 + 2 ADAPTED each with one
      pre-existing base-phase MCP FAIL, codex deny/order proven and allow
      ADAPTED via the approval gate. The clean 3x gate remains the final
      step.)
- [ ] `cargo test --no-fail-fast`, formatting, clippy, strict OpenSpec
      validation, and the conformance matrix pass. (The deterministic half
      passes: 14/14 targets, clippy `-D warnings` clean, fmt clean,
      OpenSpec strict 15/15. The conformance matrix awaits its completed
      clean 3x run.)

Source change: openspec/changes/add-portable-hooks/
