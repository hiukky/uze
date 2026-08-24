# Instructions / Rules — research

Companion to [landscape.md](landscape.md). Part 11 of the M3 brief, and the
document backing the M3 tracer-bullet recommendation.

## Why this capability gets special attention

Two things converge on this capability independently of this research pass:

1. **ADR-003** (accepted, current) already names the "portable project core"
   as `AGENTS.md`, Agent Skills, and MCP — i.e. Instructions was already
   architected as a first-class peer of the two capabilities UZE has
   actually implemented. It has simply never been built.
2. **`CapabilityKind::Instruction`** already exists in
   `crates/uze-core/src/capability.rs`, unimplemented, sitting next to
   `AgentSkill`/`Mcp` in an enum where only those two have real delivery
   logic.

Neither of these facts required new research to notice — they were sitting
in the codebase. What this pass adds is the cross-harness evidence to confirm
the architectural bet was right.

## Mapping

| | Claude Code | Codex | OpenCode | Gemini CLI |
|---|---|---|---|---|
| File | `CLAUDE.md` | `AGENTS.md` (origin harness) | `AGENTS.md` | `GEMINI.md` (filename **configurable** via `context.fileName`, can add/substitute `AGENTS.md`) |
| Scope | Managed/org (fixed path, non-excludable) → user (`~/.claude/CLAUDE.md`) → project (`./CLAUDE.md` or `./.claude/CLAUDE.md`) → local (`./CLAUDE.local.md`, gitignored) | User + per-repo, walking Git root → cwd; global `~/.codex/AGENTS.md` tier | Project root (upward traversal) + global `~/.config/opencode/AGENTS.md` | User + project + extension-scoped, simultaneously in play |
| Nesting/precedence | **Concatenation, not override** — all applicable files load into context together; nested subdirectory `CLAUDE.md` files load on-demand (lazy, not eager); conflicts are explicitly documented as unresolved ("Claude may pick one arbitrarily") | **Concatenation, position-significant**: "files closer to your current directory override earlier guidance **because they appear later** in the combined prompt" — a real, different mechanism from Claude's (later position, not later-file-wins as a rule, though the practical effect is similar) | Explicit **Claude Code migration fallback**: reads `CLAUDE.md`/`~/.claude/CLAUDE.md` if the `AGENTS.md` equivalent is absent (first match wins); disable via `OPENCODE_DISABLE_CLAUDE_CODE=1` | No fetched doc gave an explicit precedence order across user/project/extension tiers when multiple coexist — flagged **UNKNOWN**, needs a dedicated follow-up before implementation |
| Cross-vendor interop already built by a vendor | `@AGENTS.md` import supported, or symlink | N/A (origin) | **Yes — direct, load-bearing fallback to Claude Code's own filename** | Filename is configurable at the settings level, which is itself a (manual) interop mechanism |
| Size limits | Unresearched | 32 KiB default cap (`project_doc_max_bytes`), empty files skipped | Unresearched | Unresearched |
| Package-native | Plugin-shippable | Plugin-shippable | Extension-shippable | Extension-shippable, own `contextFileName` per extension |

**This is the strongest convergence found anywhere in this research pass
outside Skills/MCP themselves.** OpenCode's own fallback logic is direct
evidence the ecosystem already treats this as shared ground, not merely
similarly-named: a harness vendor wrote code specifically to interoperate
with a *different* vendor's instructions filename.

## What is still genuinely divergent (do not paper over)

- **Precedence mechanics differ in a way that matters for merge safety.**
  Claude's model is closer to "everything concatenates, conflicts are the
  model's problem." Codex's is "position in the concatenated string
  determines effective precedence." A UZE-managed block inserted at the
  wrong position could silently change effective precedence in Codex in a
  way it would not in Claude — this is a real implementation hazard, not a
  paperwork detail.
- **Gemini CLI's precedence across simultaneous user/project/extension files
  is unconfirmed.** Do not assume it matches either Claude's or Codex's
  model until verified.
- **Managed/org-tier instructions exist in Claude Code and sit above user
  control entirely.** A UZE-managed block must never be placed where it
  could be confused with, or interfere with, that tier.

## Core-fit gap this capability actually needs (see landscape.md Part 12)

Unlike Skills/MCP, which attach as one discrete reference per resource,
Instructions **merge into a single shared document per harness** that the
user also edits by hand. `ManagedArtifact` (`crates/uze-core/src/integration.rs`)
has no variant for this today — `SymlinkReference` and `VendorConfigEntry`
both assume the managed thing is either an independent filesystem entry or a
generated config entry UZE fully owns. A UZE-managed instructions block needs
its own drift semantics: DRIFTED must mean something specific (the block's
own delimited content changed) that is distinguishable from ordinary user
edits elsewhere in the same file, which the current `AttachmentState`
vocabulary (`Matched/Missing/Drifted/Conflict/Blocked`) can express *if* the
artifact is scoped correctly — but the scoping (a delimited block inside a
larger file, not the whole file) is new and should be designed, not
retrofitted from `SymlinkReference`.

This is real, scoped work — not a reason to prefer Hooks instead. It is
smaller and lower-risk than what Hooks would require (Part 12 of
[landscape.md](landscape.md) calls Hooks `CORE_MODEL_INSUFFICIENT` for
routing at all; Instructions is `FITS` with one `MINOR_EXTENSION`).

## Answer to the central question

> Existe uma capability portátil de instructions que o UZE deveria suportar
> antes de Hooks?

**Yes.** Evidence: four-of-four convergence (vs. Hooks' two-solid/one-
unverified/one-excluded), zero execution/trust risk (vs. Hooks' highest-
stakes-in-this-research security profile), a `CapabilityKind` that already
exists unimplemented and is already named in an accepted ADR as portable
core, and a Core gap (`ManagedArtifact` block-merge variant) that is scoped
and boring compared to Hooks' unresolved typed-requirement problem. This is
the answer that determines [landscape.md](landscape.md) Part 20's tracer
bullet recommendation.

## Tracer bullet results (implemented, 2026-08-21)

The design above (Fases 1–14 of the follow-on implementation brief) was
implemented and tested. Full report in the session record; summary here.

**What shipped:**
- `ManagedTextRegion` (`crates/uze-core/src/text_region.rs`) — the generic
  primitive, proven harness-agnostic by grep (zero vendor/capability names
  in its logic). 22 L0 tests: attach/inspect/detach, CRLF preservation,
  drift-blocks-detach (ADR-009), duplicate/malformed-marker rejection,
  multi-region independence, orphan cleanup with its documented weaker
  (structural-only) safety guarantee.
- `crates/uze-core/src/context.rs` — composes every installed package's
  `AGENTS.md` into one project's shared file. 6 L0 tests.
- `UzeApplication::context_reconcile(project_root)` — a new, **explicitly
  separate** operation from `add_plugin`/`remove_plugin`. `uze add` was
  **not** changed; it stays 100% global and project-independent, per an
  explicit correction during design review that rejected introducing any
  persistent "Project" concept. 12 L1 end-to-end tests against the real
  `UzeApplication`. (An earlier `context_inspect` was removed during review:
  its own doc comment claimed read-only behavior it didn't actually have on
  a not-yet-reconciled project — `context_reconcile` alone is honest about
  writing when it writes.)
- Zero `CapabilityKind` changes, zero new `IntegrationPort` methods, zero
  `exposure_plan` signature changes. The one place vendor knowledge
  legitimately exists is a single explicit constant in
  `uze-application/src/application.rs`:
  `BRIDGE_INTEGRATIONS = [("claude-code", "CLAUDE.md")]`
  — Codex, OpenCode and Antigravity are deliberately absent from it, because they need
  nothing beyond the shared file.

**Empirical (L2a) evidence, no credentials required:** `codex debug
prompt-input` (Codex 0.148.0, real binary, isolated `$HOME`) rendered the
exact model-visible prompt for a project holding a UZE-composed `AGENTS.md`,
containing both the package's literal content and the line `"AGENTS.md
instructions for <project path>"` — direct proof Codex loads the file UZE
wrote, natively, with no additional artifact. OpenCode 1.18.19's non-model
debug surfaces (`debug config`, `debug agent <name>`) do not expose
instructions content pre-session, so its native `AGENTS.md` preference is
confirmed by design-time documentation research and by this session's L1
tests, not re-confirmed live — reported as such, not inflated to
`VERIFIED`. Claude Code's bridge mechanism (`@AGENTS.md`
import) is implemented and tested at the file level only; confirming a real
model actually resolves the import requires credentials this session did
not use, so that leg stays `UNVERIFIED` at the conversational tier.

**Verdict: PARTIALLY PROVEN.** See the session record for the full
evidence, the discovered limitations (orphan-region cleanup's weaker safety
guarantee; the bridge file is never deleted, only emptied, once created),
and the adversarial test results.
