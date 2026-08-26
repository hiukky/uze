# UZE Integrations

Peer harness integrations: Claude Code, Codex, OpenCode, Antigravity CLI. Each
implements `uze-core::integration::IntegrationPort` — the only contract Core
knows. No integration imports another; no harness name appears in
`uze-core`, `uze-application`'s Store/Engine/Router layer, or any other
integration. See `docs/adr/005-establish-peer-harness-integrations.md`.
Enforced structurally, not just by convention: `tests/integrations/identity.rs`
(`core_never_names_a_vendor_harness`, `application_never_names_a_vendor_harness`,
`cli_and_tui_never_name_a_vendor_harness`).

## Registry / composition root

`registry::IntegrationRegistry` is the **single production composition
root**: the only place that constructs the concrete integration types.
`builtin(&home)` composes the environment-based set; `isolated(root,
&home)` composes the same set against a throwaway root for tooling and
tests. Everything else — `UzeApplication::from_env`, the PATH shim
dispatch, the README harness-matrix generator — consumes the registry or
the `IntegrationPort` contract, never a concrete type.

```rust
let registry = IntegrationRegistry::builtin(&home)?; // names the harnesses
for integration in registry.iter() { /* generic */ }
registry.resolve("claude")?;        // id or declared alias
registry.by_shim_name("claude");    // runtime-shim opt-ins only
```

A new harness therefore means: one vertical under `src/<harness>/`, one
entry in `builtin`/`isolated`, conformance, and docs — nothing in core,
application, CLI, or TUI changes.

This is about *delivery*: canonical Store content projected out to each
harness. Acquisition can, in principle, run the other direction — a
foreign, vendor-authored artifact imported *into* canonical form — and
that is a deliberately separate concern (`uze-core::importers`), not this
crate's job even in principle. It is currently unimplemented in
production: the one foreign importer this codebase ever had
(`ClaudePluginImporter`) was confirmed dead and removed (ADR-022). Only
the canonical `plugin.json` importer (`AgentPluginImporter`) is live.

Per-harness detail lives in each integration's own README:

- [Claude Code](src/claude/README.md)
- [Codex](src/codex/README.md)
- [OpenCode](src/opencode/README.md)
- [Antigravity CLI](src/antigravity/README.md)

This document is the cross-harness view only.

## Integration Matrix

Status: **PROVEN** (real-CLI behavioral evidence) · **SUPPORTED** (implemented, config/logic-level evidence only) · **PARTIAL** (implemented, a real gap or unclosed verification tier) · **EXPERIMENTAL** (implemented, no behavioral evidence, self-described as conformance-only) · **NOT_IMPLEMENTED**.

| Surface | Claude | Codex | OpenCode | Antigravity |
|---|---|---|---|---|
| Package | **PROVEN** (config) — Native Plugin, `claude plugin install` | **SUPPORTED** — Native Plugin, `codex plugin add`; exact coverage, see below | **N/A** — no native envelope exists to consume (deliberate) | **SUPPORTED** — Native Plugin, `agy plugin install`; the canonical `plugin.json` IS the vendor manifest; exact coverage, real-binary dogfood (1.1.19) |
| Skills | **PROVEN** — via package or managed symlink, real behavioral proof-token run | **PROVEN** — managed symlink, real behavioral proof-token run | **PROVEN** — managed symlink, real behavioral proof-token run (v1.18.18) | **SUPPORTED** — via plugin (native) or managed global-skills symlink; DOCUMENTED root, no behavioral run |
| MCP | **PARTIAL** — config/discovery PROVEN live, behavioral tool-call gap open | **PARTIAL** — config PROVEN, discovery inconclusive (vendor JSON has no connectivity field), behavioral blocked by an approval gate | **SUPPORTED** — TESTED only, zero recorded conformance run of any tier | **SUPPORTED** — native plugin `mcp_config.json` + `agy mcp add` fallback; real-binary dogfood |
| Context (AGENTS.md) | **PARTIAL** — `--add-dir` runtime projection, extensive real-CLI evidence, one open gap (`/compact` retention) | Native (reads `AGENTS.md` directly) — DOCUMENTED, outside this crate | Native (reads `AGENTS.md` directly) — DOCUMENTED, outside this crate | **Native** (reads `AGENTS.md` and `GEMINI.md`; official docs: identical context rules) — no bridge generated |
| Agents | NOT_IMPLEMENTED | NOT_IMPLEMENTED (also a real, open Codex *vendor* gap — plugins can't declare subagents today) | NOT_IMPLEMENTED | NOT_IMPLEMENTED (vendor supports `agents/` — future surface) |
| Hooks | NOT_IMPLEMENTED | NOT_IMPLEMENTED | NOT_IMPLEMENTED | NOT_IMPLEMENTED (vendor supports `hooks.json` — future surface) |
| Commands | NOT_IMPLEMENTED (Claude itself merged Commands into Skills upstream) | NOT_IMPLEMENTED | NOT_IMPLEMENTED | **ADAPTED** — routes through the vendor's official commands→Skills conversion; explicit-only property degrades (declared, never hidden) |
| Runtime Integration | Yes — the only harness with a projection mechanism (`--add-dir`) | None (passthrough default) | None (passthrough default; see note below) | None (passthrough default; no shim) |

Agents/Hooks/Commands are `NOT_IMPLEMENTED` project-wide, not per-harness gaps:
`CapabilityKind::{Agent, Hook, Action}` are recognized only by
`uze-core::importers` and routed to zero integrations (`grep` confirms —
these three variants and `Policy`, which is entirely unused anywhere, never
appear in any `IntegrationPort::capabilities()`/`exposure_plan()`).
`docs/capabilities/overview.md` documents this as a deliberate
research-only posture, not an oversight.

## Native-first routing

```
Native Package/Extension  >  Native Capability  >  Safe Adaptation  >  Unsupported
```

(ADR-013.) All four integrations check native-package delivery first when
one exists. The **order** is respected everywhere. The **coverage
computation** that decides which resources a native package actually
covers is a real intersection for all three harnesses with a native-package
tier — see below.

## Plugin Distribution

```
Claude:  Store plugin → derived marketplace.json → `claude plugin install`
           → Claude-owned cache → 1 receipt → Skill/MCP: VIA_PACKAGE (exact ∩)

Codex:   Store plugin → derived marketplace.json → `codex plugin add`
           → Codex-owned cache → 1 receipt → Skill/MCP: VIA_PACKAGE (exact ∩)

Antigravity: Store plugin (canonical plugin.json IS the vendor manifest)
           → `agy plugin install` → staged copy at ~/.gemini/config/plugins/
           (+ import_manifest.json registration) → 1 receipt → Skill/MCP:
           VIA_PACKAGE (exact ∩); MCP-bearing packages get a generated
           envelope translating canonical mcp.json → mcp_config.json
           (registry-free — no catalogue)

OpenCode: Store plugin → no native step → decompose →
           Skill: symlink into ~/.agents/skills
           MCP:   direct write into opencode.json
```

Full per-harness diagrams are in each integration's README.

## Package coverage

`provided_resource_identities` on `PackageExposurePlan` is `discovered ∩
declared` (ADR-013 §2: "Undeclared resources are not marked provided and
continue through normal `exposure_plan` fallback — no silent
disappearance"). All three harnesses with a native-package tier now compute
this as a real intersection rather than a presence check:

- **Claude** (`claude_exact_coverage`): parses the manifest's `skills`/
  `mcpServers` arrays, rejects `..`/absolute/malformed/duplicate entries, 12
  dedicated tests.
- **Codex** (`codex_exact_coverage`): `.codex-plugin/plugin.json`'s `skills`
  field names one shared directory (subtree membership, component-wise);
  `mcpServers` names one external file (typically `.mcp.json`) holding the
  standard `{"mcpServers": {...}}` shape — a server is covered iff its name
  appears there. Either field independently degrades to "no coverage" on
  absence, malformed content, or an escaping/absolute path, rather than
  erroring. 11 dedicated tests.
- **Antigravity** (`antigravity/plugin.rs`): the canonical `plugin.json` is
  the vendor manifest, so coverage is structural — a skill is covered iff it
  lives under the package's fixed `skills/`, a canonical command iff under
  `commands/` (the CLI converts it to a Skill at load), and an MCP server
  iff declared in `mcp_config.json` (or, for the generated route, present
  in canonical `mcp.json`). 17 dedicated tests.

Each was traced through `UzeApplication::attach_package_to`
(`crates/uze-application/src/application/lifecycle/attach.rs`), which skips
individual attachment for any resource identity present in
`provided_resource_identities`: a dedicated test per harness now proves a
package with one natively-covered resource plus an undeclared skill and an
undeclared MCP server produces exactly one package receipt for the covered
resource, and that both undeclared resources still route through the normal
capability-level fallback (`ExposureMechanism` other than `Unsupported`) —
the silent-capability-loss failure mode ADR-013 §2 exists to prevent is now
guarded for all three, not just Claude.

| Harness | Coverage computed how | Partial coverage handled? | Fallback-safe? | Tested |
|---|---|---|---|---|
| Claude | Real intersection (`claude_exact_coverage`) | Yes — undeclared resources fall through | Yes | 12 tests |
| Codex | Real intersection (`codex_exact_coverage`) | Yes — undeclared resources fall through | Yes | 11 tests |
| Antigravity | Real intersection (structural `skills/`/`commands/` + declared/translated MCP) | Yes — undeclared resources fall through | Yes | 17 tests (coverage/plan/generated) |
| OpenCode | N/A — no native package tier exists | N/A | N/A | N/A |

Each coverage function is pure and was validated against the exact real
fixture manifest shapes already used elsewhere in this repository's
conformance suite.

## Lifecycle safety (ADR-009)

All four correctly implement inspect-before-detach
(MATCHED/MISSING/DRIFTED/CONFLICT/BLOCKED, only MATCHED permits detach).
Mutation goes through each harness's own CLI except OpenCode, which writes
`opencode.json` directly (collision-safe by construction — refuses to
overwrite a differently-configured existing entry) because no ADR in this
repository confirms OpenCode lacks an equivalent CLI verb; the "no
structured API exists" premise behind that choice is DOCUMENTED as
deliberate (ADR-008) but not itself sourced. No cross-integration coupling
exists anywhere (`grep` for cross-module imports between the four found
none beyond each integration's own submodules referencing its own root
struct).

## Runtime

Only Claude has a runtime-projection mechanism (`--add-dir` delivery of
`AGENTS.md`, ADR-014) — extensively EMPIRICALLY verified, one open gap
(`/compact` retention). Codex, OpenCode, and Antigravity inherit
`IntegrationPort`'s passthrough defaults and do nothing with the shim/dispatch
machinery ADR-014 also gives them for free.

## Evidence legend

- **CODE_FACT** — confirmed by reading the source.
- **TESTED** — exercised by a `cargo test` that passes.
- **EMPIRICAL** — exercised against a real, installed harness binary, with a real observed result (a proof token, a `✔ Connected` status, etc.).
- **DOCUMENTED** — stated in the harness's own vendor documentation.
- **SOURCE_CONFIRMED** — stated in this repository's own source comments as a first-hand empirical finding, without a dated, reproducible ADR record backing it.
- **UNKNOWN** — not established either way.

## Last validated versions

Real-CLI evidence recorded in this repository's own ADRs (not memory,
not vendor changelogs):

| Harness | Version | Where |
|---|---|---|
| Claude Code | 2.1.239 | ADR-006/007/013/014; re-confirmed live during this audit (`claude --version`, the only harness binary installed in this environment) |
| Codex CLI | 0.148.0 | ADR-005/006/007/008 |
| OpenCode | 1.18.18 | ADR-005/006 — predates the `opencode`/`opencode2` v2 binary split this crate's provisioning code now handles; no re-validation against a v2 install is recorded |
| Antigravity CLI | 1.1.19 | ADR-027 — real-binary dogfood in an isolated `$HOME` (attach → MATCHED → detach → Missing → reinstall) |

The real-binary dogfood for Antigravity was run against an isolated
`$HOME`/`UZE_HOME`; no developer harness configuration was touched.
