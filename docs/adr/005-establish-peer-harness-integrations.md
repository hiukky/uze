# Establish peer harness integrations around a harness-agnostic core

Status: Accepted
Consolidates: ADR-022 (remove the dead foreign Claude plugin importer) —
see the "Consolidated records" section of `README.md`.

## Context

The first cut of UZE encoded harness support as named rules inside the
domain: the core knew which harness was a source and which was a
destination, and adding a harness meant editing core routing. That makes
every new vendor a change to the domain, and it bakes an asymmetry
("Claude is where config comes from, Codex is where it goes") into a
product whose whole premise is that no harness is privileged.

A related question was where foreign-format import belongs. A
`ClaudePluginImporter` recognized `.claude-plugin/plugin.json` and produced
core capabilities with foreign provenance — structurally separate from
`ClaudeIntegration`, which projects canonical content *out* to Claude.

## Decision

**The core operates on generic inputs only.** UZE Core operates on portable
capabilities, an effective environment, and integration-supplied
`HarnessCapabilities`. It contains no named harness support rules and no
source/destination semantics. A capability router returns compatibility and
exposure results from those generic inputs alone.

Adding a harness that uses existing capability kinds requires a new
integration implementation and its tests — not a change to Store, Engine,
Router, application, CLI, or TUI. This is enforced by test, not convention:
`core_never_names_a_vendor_harness`, `application_never_names_a_vendor_harness`,
and `cli_and_tui_never_name_a_vendor_harness`.
`uze-integrations::registry::IntegrationRegistry` is the single composition
root that names concrete integration types.

**Three facts are kept separate.** Representation/provenance, compatibility
route, and exposure/verification are independent. A standard Agent Skill
representation is not evidence that it has been exposed in a particular
harness. Real probes report `VERIFIED`, `NOT_EXPOSED`, `UNVERIFIED`,
`FAILED`, or `BLOCKED_BY_ENVIRONMENT`; quota, authentication, missing
executables, service failures, and timeouts are never capability
incompatibility.

**The Store is authoritative only for packages UZE installed.** Project
resources remain project-owned. `UzeEngine` composes both sources into one
effective environment before routing it to integrations.

**Normal harness invocation is the target DX.** The user runs `claude`,
`codex`, `agy`, or `opencode` in the real project directory. A UZE launcher,
a per-session flag, or replacing the harness executable on `PATH` is not the
product architecture.

**No foreign importer is retained speculatively.** Foreign-format import, if
it returns, still belongs structurally separate from harness delivery — an
importer converting a vendor artifact into canonical form is a different
concern from an `IntegrationPort` projecting canonical content out to a
vendor, and conflating them was never the fix under consideration. But the
`ClaudePluginImporter` was never reachable from any production path, so it
was removed along with `import_bundle()`:

> Foreign-format importing remains a separate concern from harness delivery,
> but no foreign importer is retained in production until a real
> acquisition or reverse-discovery flow requires it.

`AgentPluginImporter` — the live importer `Store::ingest` depends on — and
the `ForeignImporter` trait it implements are unchanged.

Alternatives rejected: retaining the named-harness core matrix; treating
Claude import as a canonical source pipeline; and requiring filesystem
projection for every integration.

## Consequences

Easier: core unit and router tests use fake integrations without installed
harnesses; integrations evolve independently; removing an integration leaves
UZE Core intact; adding one is additive.

Harder: each integration must publish evidence-backed capability
descriptions and conformance tests before claiming verified exposure (see
ADR-035). The vendor-neutrality tests are string-level and will reject an
innocent mention of a harness name in the wrong crate — that friction is the
point.
