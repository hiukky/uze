# UZE Integrations

Peer harness integrations: Claude Code, Codex, OpenCode, Gemini CLI. Each
implements `uze-core::integration::IntegrationPort` — the only contract Core
knows. No integration imports another; no harness name appears in
`uze-core`, `uze-application`'s Store/Engine/Router layer, or any other
integration. See `docs/adr/005-establish-peer-harness-integrations.md`.
Enforced structurally, not just by convention: `tests/integration_conformance.rs::core_never_names_a_vendor_harness`.

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
- [Gemini CLI](src/gemini/README.md)

This document is the cross-harness view only.

## Integration Matrix

Status: **PROVEN** (real-CLI behavioral evidence) · **SUPPORTED** (implemented, config/logic-level evidence only) · **PARTIAL** (implemented, a real gap or unclosed verification tier) · **EXPERIMENTAL** (implemented, no behavioral evidence, self-described as conformance-only) · **NOT_IMPLEMENTED**.

| Surface | Claude | Codex | OpenCode | Gemini |
|---|---|---|---|---|
| Package | **PROVEN** (config) — Native Plugin, `claude plugin install` | **SUPPORTED** — Native Plugin, `codex plugin add`; exact coverage, see below | **N/A** — no native envelope exists to consume (deliberate) | **SUPPORTED** — Native Extension, `gemini extensions link`; exact coverage, see below |
| Skills | **PROVEN** — via package or managed symlink, real behavioral proof-token run | **PROVEN** — managed symlink, real behavioral proof-token run | **PROVEN** — managed symlink, real behavioral proof-token run (v1.18.18) | **SUPPORTED** — managed symlink, DOCUMENTED native root, no behavioral run |
| MCP | **PARTIAL** — config/discovery PROVEN live, behavioral tool-call gap open | **PARTIAL** — config PROVEN, discovery inconclusive (vendor JSON has no connectivity field), behavioral blocked by an approval gate | **SUPPORTED** — TESTED only, zero recorded conformance run of any tier | **SUPPORTED** — TESTED only, zero recorded conformance run of any tier |
| Context (AGENTS.md) | **PARTIAL** — `--add-dir` runtime projection, extensive real-CLI evidence, one open gap (`/compact` retention) | Native (reads `AGENTS.md` directly) — DOCUMENTED, outside this crate | Native (reads `AGENTS.md` directly) — DOCUMENTED, outside this crate | Reads its own `GEMINI.md`; the `AGENTS.md` bridge lives in `uze-application`, outside this crate |
| Agents | NOT_IMPLEMENTED | NOT_IMPLEMENTED (also a real, open Codex *vendor* gap — plugins can't declare subagents today) | NOT_IMPLEMENTED | NOT_IMPLEMENTED |
| Hooks | NOT_IMPLEMENTED | NOT_IMPLEMENTED | NOT_IMPLEMENTED | NOT_IMPLEMENTED |
| Commands | NOT_IMPLEMENTED (Claude itself merged Commands into Skills upstream) | NOT_IMPLEMENTED | NOT_IMPLEMENTED | NOT_IMPLEMENTED |
| Runtime Integration | Yes — the only harness with a projection mechanism (`--add-dir`) | None (passthrough default) | None (passthrough default; see note below) | None (passthrough default) |

Agents/Hooks/Commands are `NOT_IMPLEMENTED` project-wide, not per-harness gaps:
`CapabilityKind::{Agent, Hook, Action}` are recognized only by
`uze-core::importers` and routed to zero integrations (`grep` confirms —
these three variants and `Policy`, which is entirely unused anywhere, never
appear in any `IntegrationPort::capabilities()`/`exposure_plan()`).
`docs/capabilities/{agents,hooks,commands}.md` document this as a deliberate
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

Gemini:  Store plugin → `gemini extensions link --consent` (no copy, no catalogue)
           → Gemini's own registry → 1 receipt → Skill/MCP: VIA_PACKAGE (exact ∩)

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
- **Gemini** (`gemini_exact_coverage`): `gemini-extension.json` declares no
  `skills` field at all (confirmed by
  `e2e/fixtures/gemini-native-conformance/gemini-extension.json`) — Skill
  coverage is convention-based, a skill is covered iff it lives under the
  extension root's fixed `skills/` subdirectory. `mcpServers` is declared
  inline as a name-keyed object (unlike Codex's external-file reference). 8
  dedicated tests.

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
| Gemini | Real intersection (`gemini_exact_coverage`) | Yes — undeclared resources fall through | Yes | 8 tests |
| OpenCode | N/A — no native package tier exists | N/A | N/A | N/A |

None of this was exercised against a real `codex`/`gemini` CLI for this fix:
each coverage function is pure and was validated against the exact real
fixture manifest shapes already used elsewhere in this repository's
conformance suite, so a live install/link run would add side-effect risk
without adding coverage-computation evidence.

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
(`/compact` retention). Codex, OpenCode, and Gemini inherit
`IntegrationPort`'s passthrough defaults and do nothing with the shim/dispatch
machinery ADR-014 also gives them for free.

**Note on an in-progress, external change observed during this audit:** all
four parallel audits independently noticed that `crates/uze-integrations/
src/opencode.rs`, `crates/uze-core/src/integration.rs`, `src/shim.rs`, and
`crates/uze-application/src/application.rs` changed on disk mid-session —
not made by this audit or any of its forks. The change adds a new
`IntegrationPort::runtime_executable_aliases()` method and flips
`OpenCodeIntegration::supports_runtime_integration()` to `true`,
generalizing the Claude-only PATH-shim mechanism to resolve OpenCode's
`opencode2`-named v2 binary without a physical symlink. ADR-014 explicitly
does not anticipate this ("only if and when [Codex, OpenCode, or Gemini] has
a real runtime-projection need of its own — nothing here requires or
anticipates that"). The workspace briefly failed to compile mid-edit
(missing `use std::fs`/`PathBuf` in `opencode/provision.rs`) and now builds
again; `cargo test -p uze-integrations --lib` currently reports **38**
passing (was 39 at the start of this audit — the old
`symlink_alias_is_created_repaired_and_leaves_foreign_files_alone` test
appears to have been removed as part of the same change). This audit did
not touch, revert, or evaluate that change — it was out of scope, made no
attempt to finish or fix it, and the OpenCode README reflects the
mechanism as it stood *before* this edit began. **Flagging for your
review, not folding into any verdict above.**

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
| Gemini CLI | 0.56.0 | Module doc comment (`gemini.rs`) — no dated ADR entry, no behavioral run recorded anywhere |

No codex/opencode/gemini binary was installed in the environment this audit
ran in, so none of the above was independently re-verified this pass beyond
Claude's version string.
