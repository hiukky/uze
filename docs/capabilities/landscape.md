# M3 Capability Landscape

**Date:** 2026-08-21. Research for the M3 phase (Capability Landscape & Semantic
Portability). This is documentation of external ecosystem behavior and an
architectural recommendation, not an implementation and not an authorization to
implement.

North star, repeated because every section below is judged against it:

> **Semantics before syntax.** Two harnesses sharing a format is not
> convergence. Two harnesses sharing a format *and* a behavior contract is
> convergence. UZE preserves a real difference rather than papering over it.

Scope: Claude Code, Codex (`openai/codex`), OpenCode (`sst/opencode`), Gemini
CLI (`google-gemini/gemini-cli`) — researched in depth. Cursor, GitHub Copilot
CLI and Cline are covered only where the existing
[local real-harness conformance research](../../openspec/changes/establish-local-real-harness-conformance/research-notes.md)
already added value; this pass did not re-research them.

Evidence labels follow the existing convention: **OFFICIAL** (vendor docs/spec),
**SOURCE_CONFIRMED** (current first-party source), **UNKNOWN** (not found /
not verified — never inferred from a similar name elsewhere). Per-capability
detail lives in sibling documents; this file is the index and the synthesis.

- [hooks.md](hooks.md) — the deep dive: format landscape, semantic matrix,
  portability classification, generated-adapter assessment, trust implications.
- [commands.md](commands.md) — the implemented Command capability: canonical model, per-harness support matrix (current official behavior), routing/exact-coverage rules, argument and security posture.
- [agents.md](agents.md)
- [instructions.md](instructions.md) — the tracer bullet research and design.
- [context-manager.md](context-manager.md) — **the implemented, tested
  result**: the Context Manager boundary, `inspect`/`plan`/`reconcile`, and
  the current portability evidence per harness.

## Implementation status (2026-08-21)

The research below stays as originally written — it is what justified each
decision. This table is the one place status is kept current as
capabilities actually ship, so it doesn't require re-reading the whole
document to answer "is this real yet."

| Capability | Status | Evidence |
|---|---|---|
| Skills | **IMPLEMENTED** | Real delivery to Claude Code, Codex, OpenCode, Gemini CLI. |
| MCP | **IMPLEMENTED** | Real delivery to all four. |
| Instructions | **IMPLEMENTED / Context Manager** | Codex: native, **empirically confirmed** (`codex debug prompt-input`, no credential). OpenCode: native, documented + L1-tested, not re-confirmed live this session — declared limitation, not inflated. Claude Code / Gemini CLI: bridge (`@AGENTS.md`) implemented and tested at the file level; model-level resolution **unverified** (would need credentials). See [context-manager.md](context-manager.md). |
| Hooks | RESEARCHED | Partial portable subset justified for Claude↔Gemini only; not implemented. See [hooks.md](hooks.md). |
| Commands | **IMPLEMENTED** (v0) | First-class capability, delivered natively to Claude Code (plugin `commands/`), OpenCode V2 (user-global `.md`), Gemini CLI (generated `.toml`), and Codex (official explicit-invocation-only Skill per ADR-025's semantics definition of Native). See [commands.md](commands.md) — supersedes the 2026-08-21 "converging to skills, do not model" recommendation (ADR-025). Exposed under stable namespaced invocation labels `<plugin>:<capability>` (ADR-026). |
| Agents | RESEARCH | Real cross-vendor semantic gaps (isolation, nesting, package format); native pass-through only. See [agents.md](agents.md). |
| Memory | FUTURE / Context Manager | Not researched or implemented; the Context Manager boundary is where it would land if pursued — see [context-manager.md](context-manager.md)'s future-`/uze` section. |

None of the four harness legs for Instructions is claimed `PROVEN` in the
sense of end-to-end, credentialed, real-model verification across all four
— that would overstate what this session's evidence supports. Two legs
(Codex confirmed, OpenCode documented) are load-bearing; two (Claude,
Gemini) are implemented and unit/L1-tested but not model-verified.

---

## Part 1 — General capability landscape

| Capability | Claude Code | Codex | OpenCode | Gemini CLI | Cross-harness pattern |
|---|---|---|---|---|---|
| Agent Skills | Native, package-native | Native, package-native | Native, package-native (discovery dir) | Native, package-native | **Open standard** — already UZE's portable core. |
| MCP | Native, package-native | Native, package-native | Native, package-native | Native, package-native | **Open standard** — already UZE's portable core. |
| Hooks | Native, package-native, declarative JSON, subprocess | Native, package-native (structurally confirmed), TOML/JSON, subprocess — **behavioral detail largely unverified** | Native, but **executable JS/TS**, in-process, no package-declared trust gate found | Native, package-native, declarative JSON, subprocess | Convergent shape between Claude/Gemini; Codex plausibly same family but unverified; OpenCode categorically different (code, not config). See [hooks.md](hooks.md). |
| Commands / Actions | **Merged into Skills** (official) | Distinct from Skills; ~29 built-ins; third-party package extensibility unconfirmed | Distinct from Skills; markdown + shell-injection (`` !`cmd` ``) | Distinct from Skills; TOML, package-native, lowest-precedence namespacing | Not converging on one contract; Claude's own docs say the concept collapsed into Skills. See [commands.md](commands.md). |
| Agents / Subagents | Rich: fork vs. fresh-context isolation, worktree isolation, per-agent memory, nesting depth 3, plugin-native minus `hooks`/`mcpServers`/`permissionMode` | Real gap: **cannot be declared inside `plugin.json`** today (open issue), `.toml` files only | Three tiers (primary/subagent/system), rich per-agent tool permission grammar, no package-shipping mechanism documented | Shipped (no longer preview), YAML frontmatter, **nesting explicitly disallowed**, own MCP servers per agent | Every harness models delegation differently at the isolation/nesting/package-native axis. See [agents.md](agents.md). |
| Instructions / Rules | `CLAUDE.md`; concatenated hierarchical scopes; reads `@AGENTS.md` via import | `AGENTS.md` (origin harness); strict concatenation, **later-directory-wins by position** | `AGENTS.md` native; explicit `CLAUDE.md` fallback logic built in | `GEMINI.md`, filename **configurable** (can add/substitute `AGENTS.md`) | **The strongest real convergence found outside Skills/MCP.** Already named in ADR-003 as part of UZE's "portable project core." See [instructions.md](instructions.md). |
| Permissions | Managed/org policy tier above user settings; per-agent allow/deny/ask | `requirements.toml`, `allow_managed_hooks_only` — real managed tier | Rich per-agent `allow/ask/deny` grammar incl. glob bash rules | Extension policy engine (`.toml`, tiered rules), separate from hooks | Every harness has *a* permission system; none share a schema. Out of M3 scope — noted, not modeled. |
| LSP / language tooling | Plugin-native `.lsp.json` (found incidentally) | UNKNOWN | `lsp` permission scope exists; discovery mechanism unresearched | UNKNOWN | Incidental finding only; not researched to the depth this table implies for other rows. Flagged for a future pass, not scored here. |

Initial per-cell classification (research/documentation labels — **not** a
Core enum, per the M3 brief):

`NATIVE` (exists, first-class, real semantics) · `ADAPTABLE` (real cross-vendor
intersection UZE could safely bridge) · `PARTIAL` (intersection exists but with
provable loss) · `VENDOR_SPECIFIC` (exists, no safe cross-vendor bridge)
· `UNAVAILABLE` (harness has nothing analogous) · `UNKNOWN` (not verified).

| Capability | Claude Code | Codex | OpenCode | Gemini CLI |
|---|---|---|---|---|
| Skills | NATIVE | NATIVE | NATIVE | NATIVE |
| MCP | NATIVE | NATIVE | NATIVE | NATIVE |
| Hooks | NATIVE | UNKNOWN (structure confirmed, behavior not) | NATIVE but VENDOR_SPECIFIC in mechanism | NATIVE |
| Hooks (cross-vendor) | ADAPTABLE with Gemini CLI, PARTIAL/UNKNOWN with Codex, VENDOR_SPECIFIC vs. OpenCode | — | — | — |
| Commands | VENDOR_SPECIFIC (absorbed into Skills) | VENDOR_SPECIFIC | VENDOR_SPECIFIC | VENDOR_SPECIFIC |
| Agents/Subagents | NATIVE, VENDOR_SPECIFIC semantics | NATIVE but PARTIAL (no package format) | NATIVE, VENDOR_SPECIFIC semantics | NATIVE, VENDOR_SPECIFIC semantics |
| Instructions | NATIVE | NATIVE | NATIVE | NATIVE |
| Instructions (cross-vendor) | ADAPTABLE — see [instructions.md](instructions.md) | ADAPTABLE | ADAPTABLE | ADAPTABLE |

---

## Part 12 — Current Core fit

Assessed against the abstractions in `crates/uze-core/src`: `Package`,
`Resource`/`Capability`, `CapabilityKind`, `EffectiveEnvironment`,
`CapabilityRouter` (`router.rs::route`), `IntegrationPort`,
`PackageExposurePlan`, `AttachmentReceipt`/`ManagedArtifact`.

| Capability | Fit | Why |
|---|---|---|
| Instructions | **FITS**, one `MINOR_EXTENSION` | `Resource`/`Capability`/`CapabilityRouter` need nothing new: an instructions file is a `Capability { kind: Instruction, representation: Standard }` like any other. The one real gap is delivery: Skills/MCP attach as a *discrete, additive* reference (`ManagedUserScopeReference`, `ManagedVendorConfig`) — one symlink or one config entry per resource, safe to detach independently. Instructions **merge textually** into one shared vendor file per harness (`CLAUDE.md`, `AGENTS.md`, `GEMINI.md`) under harness-specific precedence rules UZE does not control. `ManagedArtifact` has no variant for "a delimited, UZE-owned block inside a document the user also edits." That is genuinely new: not a new `CapabilityKind`, but a new `ManagedArtifact::ManagedTextBlock`-shaped variant with its own drift semantics (what does DRIFTED mean when a user edited *around* the block vs. *inside* it?). |
| Hooks | **CORE_MODEL_INSUFFICIENT** for anything beyond native pass-through | `CapabilityKind::Hook` is a bare enum tag. Routing a hook safely needs to know *which* semantic requirement a package is asking for (observe vs. block vs. mutate-input vs. mutate-output) before `CapabilityRouter::route` can honestly answer NATIVE/ADAPTABLE/DEGRADED — today `route()` only looks at `(kind, representation)`, and a Hook's representation alone cannot carry "needs mutate_input." A generic `Capability` also cannot carry an event name, matcher or timeout — those live inside `payload`, opaque to Core. Native pass-through (Claude plugin hooks stay Claude plugin hooks) fits today with zero change; anything resembling cross-vendor adaptation does not, until a typed hook-intent shape exists (see [hooks.md](hooks.md) Part 6). |
| Commands | **FITS, implemented** | `Resource`/`Capability`/`CapabilityRouter` needed nothing new beyond the existing capability kind (renamed `Action` → `Command`, the same concept the importer already attached to `commands/` directories): one new generic `ManagedArtifact::ManagedFile`/`ExposureMechanism::ManagedFile` for whole-file generated vendor representation (Gemini TOML), plus per-integration plans. The 2026-08-21 "N/A — do not model" verdict was withdrawn by ADR-025 on current official evidence (OpenCode V2 and Gemini have stable native command surfaces; Claude's plugin format still ships `commands/`). |
| Agents/Subagents | **CORE_MODEL_INSUFFICIENT** | Beyond the same payload-opacity problem as Hooks, isolation/nesting/memory semantics differ so much (Claude's fork-vs-fresh-context distinction, Gemini's flat no-nesting rule, Codex's total absence of a package format) that a single `CapabilityKind::Agent` cannot honestly represent "this package wants an isolated, non-nesting delegate with tool allowlist X" in a way three of four harnesses could route identically. Native pass-through is the only fit today. |

No finding in this pass calls for changing `Package`, `Store` ownership,
`EffectiveEnvironment`, or the Application façade — consistent with the prior
ecosystem research's verdict ("UZE architecture generalizes with minor
extensions").

## Part 13 — `CapabilityKind` audit

Current enum: `Instruction, AgentSkill, Mcp, Agent, Action, Hook, Policy`
(`crates/uze-core/src/capability.rs`). Only `AgentSkill` and `Mcp` are
implemented. Historical note worth surfacing: ADR-002 (superseded by ADR-003)
originally scoped the *residual* model to `Action, Subagent, Hook, Policy` and
explicitly *excluded* Instruction ("already fully covered by AGENTS.md...
would duplicate, not complement, the standards layer"). ADR-003 then named
`AGENTS.md` part of the portable core directly. The current enum keeps
`Instruction` anyway and renamed `Subagent` to `Agent`. That drift from the
ADR record should be reconciled in the ADR itself when Instructions is
implemented — not silently left implicit.

| Variant | Recommendation | Reasoning |
|---|---|---|
| `Instruction` | **KEEP** | Research confirms it independently of ADR-002/003: real, homogeneous-enough concept across all four harnesses (see [instructions.md](instructions.md)). This is the strongest-evidenced variant in the enum today. |
| `AgentSkill` | **KEEP** | Implemented, proven. |
| `Mcp` | **KEEP** | Implemented, proven. |
| `Agent` | **RENAME_LATER, SPLIT_LATER candidate** | "Agent" is generic enough to mean either a top-level mode (Claude's Build/Plan-style primary agent) or a delegated subagent — these are different capabilities with different portability profiles. ADR-002's original name, `Subagent`, was more precise for the delegation case this research actually found evidence for; a top-level "primary agent" was not independently evidenced as a package-shippable capability. Do not rename now — flag for the day Agents research turns into implementation work, since the right split (one kind? two kinds?) depends on which harnesses ship a package-native agent format by then. |
| `Action` | **REPLACED by `Command`** (ADR-025) | The variant existed but was unimplemented; its only use was a validation-only importer mapping of `commands/` directories. It has been renamed to `Command` — the evidenced name — and is now a delivered capability. |
| `Hook` | **KEEP** | Real, evidenced, non-trivial concept — but see Part 12: the variant alone cannot carry what routing a Hook honestly requires. Keeping the tag is correct; do not extend its payload shape until the [hooks.md](hooks.md) portability subset is validated by real conformance evidence, not just documentation research. |
| `Policy` | **KEEP, flag for its own research pass** | Gemini CLI's extension policy engine (tiered `.toml` rules) is the first concrete evidence a "Policy" capability might mean something specific — but it looks like *admin/trust infrastructure* (who is allowed to run what), not a *package-shipped content capability* the way Skills/MCP/Instructions/Hooks are. Conflating the two would be a mistake. Needs a dedicated research pass before any implementation; not a M3 finding on its own. |

## Part 14 — Portability levels (public taxonomy)

For the README and future `uze inspect`/TUI output — the question a user
actually needs answered: *"If I install this package, what happens in each
harness?"*

| Level | Meaning |
|---|---|
| **Native** | The harness consumes this exactly as the package author shipped it — no UZE transformation. |
| **Portable** | UZE proved a real, lossless-or-documented-loss adaptation across harnesses through its own capability delivery (today: Skills, MCP). |
| **Partial** | Delivered, but with a known, disclosed loss (e.g. observe-only where the package asked for block). |
| **Vendor-specific** | Exists in this harness; UZE does not attempt to bridge it elsewhere because no safe semantic intersection was found. |
| **Research** | UZE has not shipped delivery for this yet; documented here so the gap is honest, not silent. |
| **Unsupported** | Package asks for something no examined harness offers, or the trust/security bar cannot currently be met. |

This is the taxonomy the README summary table (Part 2, below) uses.

## Part 16 — Composability / honest reporting

Worked example, per the M3 brief: a package with 3 Skills, 1 MCP, 2 Hooks.

```
Claude Code   : Skills=Native   MCP=Native            Hooks=Native (package-native, both)
OpenCode      : Skills=Native   MCP=Adapted           Hooks=1 Partial (executable-code loss), 1 Unsupported
Gemini CLI    : Skills=Native   MCP=Adapted           Hooks=Research (not yet delivered by UZE)
```

The read model this implies is **per-resource, not per-package**: a
`PackageExposurePlan` already separates "resources this native delivery
covers" from the rest; `IntegrationAssessment` (`integration.rs`) already
carries one `RouteDecision` per resource per integration. Nothing new is
needed structurally to report this honestly — `uze inspect` already has the
right shape to walk `(resource, integration) → RouteDecision`. What's missing
is only that most resource kinds beyond Skill/MCP have no `IntegrationPort`
implementation yet, so today every Hook/Agent/Command resource correctly
routes `UNSUPPORTED` by the router's own default arm. **One capability
routing Unsupported must never suppress delivery of the others** — this
already holds today, since `exposure_plan`/`route` operate per-resource; no
change is needed to preserve it going forward.

## Part 17 — Fail closed

Restated as a rule, not just a preference, because Hooks/Agents specifically
invite the opposite temptation: a package that requests
`observe + block + mutate_input` on a harness that only proves `observe`
must route **PARTIAL**, and a request no examined harness can satisfy at all
must route **UNSUPPORTED** — never a best-effort translation that silently
drops `block`. This is exactly what `CompatibilityRoute::Degraded` already
exists to express (`router.rs`) — the machinery is present, only the
capability-specific requirement vocabulary to feed it is not (Part 12).
Concretely, this pass surfaced two real "looked-portable-but-isn't" traps to
guard against: OpenCode's `permission.ask` hook is defined in the SDK but
[documented as not currently firing](https://github.com/anomalyco/opencode/issues/7006),
and its `tool.execute.before` veto
[does not cover subagent-issued tool calls](https://github.com/sst/opencode/issues/5894).
A naive capability-name match would call OpenCode's veto hook equivalent to
Claude's `PreToolUse` block; the evidence says it currently is not.

## Part 18 — Security / trust

M2 drew the trust boundary at *process execution introduced by an installed
package* (`crates/uze-core/src/trust.rs`), scoped for M2 to MCP servers with a
`command`. Hooks and (to a lesser extent) native package-shipped Agents are
squarely inside that same question — they are UZE's second and third
capability kinds that cause a process (or, for OpenCode, in-process code) to
run once a harness picks them up.

- **A remote package with an executable Hook requires trust** — yes, no
  weaker than an MCP server; arguably stronger, since a `PreToolUse`/`BeforeTool`
  hook in Claude Code and Gemini CLI can run *before the user sees the tool
  call it is vetting*, and Gemini CLI's own docs say so explicitly ("Hooks
  execute arbitrary code with your user privileges").
- **Native vendor plugin with an executable hook still requires trust** — the
  fact that Claude Code, Codex and Gemini CLI all grant trust at
  plugin/extension-install granularity (not per-hook) means UZE inherits that
  same coarse boundary when it delivers a package's plugin natively; UZE
  should not claim finer-grained consent than the harness itself offers.
- **Generated adapter code requires *additional* trust**, not the same trust
  already granted to the package — see [hooks.md](hooks.md) Part 8. This is
  the sharpest new finding: OpenCode's own trust model has *no* separate
  install-vs-execute state (loading a plugin file *is* granting it shell
  access via Bun's `$`), so any UZE-generated JS/TS bridge for OpenCode would
  be introducing code that runs with full host trust, authored by UZE, not
  the package author or the user — a strictly worse position than M2's
  existing MCP boundary, which at least always names the package as the
  author of what runs.
- **Changes should re-trigger trust** — `introduces_new_execution` in
  `trust.rs` already treats a changed command/args as new execution requiring
  fresh consent; the same principle extends naturally to a hook whose command,
  matcher, or event changed on update. Gemini CLI already does exactly this
  today at the harness level (content-fingerprint re-trust on project hook
  change) — independent validation that the M2 pattern generalizes correctly.
- **Can a hook execute before the user sees a prompt?** Yes, confirmed for
  Claude Code (`UserPromptSubmit`) and Gemini CLI (`BeforeAgent`,
  `BeforeModel`) — both can rewrite or veto a prompt before it reaches the
  model. This is a materially higher-stakes trust surface than MCP server
  registration and should not be treated as equivalent risk.
- **Does package-native delivery bypass UZE inspection?** For hooks shipped
  inside a harness-native plugin/extension that UZE delivers as a whole via
  `attach_package`, yes by design — UZE preserves the envelope and does not
  parse vendor-internal hook declarations. That is consistent with the
  existing Plugin First posture (ADR-008): UZE is not obligated to re-review
  what the harness's own install path already reviews, but the M2 trust
  prompt (`uze add <url>`) already fires before that delivery happens at all,
  so no execution occurs UZE never asked about — it just does not ask a
  *second*, hook-specific question today. Recommendation: extend
  `executable_capabilities` (`trust.rs`) to also surface hook commands once
  Hook delivery is implemented, so the single M2 trust prompt lists every
  process a package can cause to run, not only MCP servers.

No new permission system is proposed. The recommendation is entirely additive
to the existing `TrustAuthority`/`ExecutableCapability` shape: broaden what
counts as an `ExecutableCapability` when Hook delivery ships, and treat
OpenCode-style generated-code adaptation as *out of scope for automatic
generation* rather than as a trust problem to solve later (Part 7 in
[hooks.md](hooks.md)).

## Part 20 — Tracer bullet selection

| Criterion | Hooks | Instructions |
|---|---|---|
| User value | High when it works | High — every existing package already wants its context read |
| Real portability (this research) | Partial: solid Claude↔Gemini pair, Codex unverified, OpenCode categorically different | Strong: all four, already named in ADR-003 as portable core |
| Falsifies UZE's model | Yes, hard — forces the typed-requirement question in Part 12 | Yes, differently — forces the "merge into shared document" `ManagedArtifact` gap in Part 12 |
| Security | Highest-stakes capability researched (pre-prompt execution, generated-code risk) | None — text only, no execution |
| Lifecycle manageability | Unclear until the merge/veto/mutate semantics are pinned down | Clear: still additive-ish, one new `ManagedArtifact` variant |
| Observability | Good (Claude `/hooks`, Gemini debug fields) | Good (it's a file) |
| ≥2-harness intersection | Yes (2 solid, 1 unverified, 1 excluded) | Yes (4 of 4) |
| No mandatory UZE runtime | Yes, for the Claude↔Gemini subset only | Yes |

**Score favors Instructions.** This matches the steer already embedded in
ADR-003 (`AGENTS.md` named as portable core alongside Skills/MCP) and in the
`CapabilityKind::Instruction` variant that already exists unimplemented. Hooks
remain the more architecturally interesting long-term problem — they are the
capability most likely to force a real typed-requirement model into Core —
but that is a reason to research them further (a real conformance spike on
the Claude↔Gemini subset, and direct source verification of Codex's
`schema.rs`/`output_parser.rs`, both flagged as open items in
[hooks.md](hooks.md)), not a reason to ship them first.

**Recommended M3 tracer bullet: Instructions/Rules delivery**
(`CapabilityKind::Instruction`, package `AGENTS.md`/equivalent → managed,
safely-detachable block inside each harness's native instructions file).
Full rationale, scope sketch and the `ManagedArtifact` gap it needs to close
are in [instructions.md](instructions.md).

## Stop conditions triggered

Evaluated against the eight stop conditions in the M3 brief, for a *full,
all-four-harness* portable Hook model:

1. **Not triggered wholesale** — Claude↔Gemini share more than names (see
   [hooks.md](hooks.md) Part 4). But **triggered for the Codex leg**: current
   evidence is name-and-structure only; behavioral semantics are unverified,
   so a claim of shared semantics with Codex today would be exactly the
   mistake this condition warns against.
2. **Triggered for OpenCode** — adapting into OpenCode's hook surface would
   require emulating a declarative/subprocess contract OpenCode does not
   offer (it is in-process JS with no stdin/stdout protocol).
3. **Not clearly triggered** for the Claude↔Gemini pair specifically — both
   payloads reduce to a comparable shape for observe/block/add-context; the
   `mutate_input` merge-vs-replace semantics need a real conformance spike
   before calling this LOSSLESS, but nothing found so far looks IMPOSSIBLE.
4. **Not triggered** for Claude↔Gemini (both are ordinary subprocess hooks,
   no daemon needed). **Triggered** for any OpenCode adaptation attempt that
   isn't native pass-through.
5. **Not triggered** — a native-pass-through or thin-glue strategy never asks
   UZE to run the package's own hook logic.
6/7. **Ambiguous for OpenCode specifically** (Part 18) — its own trust model
   has no install-vs-execute distinction, which is close to condition 7
   ("trust boundary ambiguous").
8. **Not triggered** — at minimum two harnesses (Claude Code, Gemini CLI)
   converge on a real, evidenced shape.

**Verdict for this phase: PARTIAL PORTABLE HOOK SUBSET JUSTIFIED** — scoped to
Claude Code ↔ Gemini CLI, pending a real conformance spike; Codex stays
`UNKNOWN` until source-verified; OpenCode stays native-pass-through-only.
**Hooks are not the M3 tracer bullet. Instructions/Rules is.**
See [hooks.md](hooks.md) for the full matrix this verdict is built on.
