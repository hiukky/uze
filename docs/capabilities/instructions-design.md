# Instructions/Rules — M3 Phase A design

Status: **historical design record.** This Phase A design (Fases 1–14) was
reviewed and implemented — the shipped result is the Context Manager
described in [context-manager.md](context-manager.md), which is the current
truth. This file is retained because ADR-014 and source doc comments cite it.

Builds on the 2026-08-21 M3 capability research; the research corpus itself
is not retained as permanent documentation (see
[overview.md](overview.md)). Only points that were ambiguous and blocking a
real implementation decision were re-verified (Fase 1).

---

## Fase 1 — Revalidated matrix (Instructions only)

Re-verified 2026-08-21 against official docs, narrowly, for exactly the
points the prior Instructions research had flagged UNKNOWN or that block a
design decision below. Everything else is carried over from the prior
research unchanged.

| | Claude Code | Codex | OpenCode | Gemini CLI |
|---|---|---|---|---|
| **1. File(s) supported** | `CLAUDE.md`, `CLAUDE.local.md` | `AGENTS.md`, `AGENTS.override.md`, configurable fallback filenames (`project_doc_fallback_filenames`) | `AGENTS.md` (preferred), `CLAUDE.md` (fallback) | `GEMINI.md` by default; `context.fileName` accepts a string **or array** of filenames |
| **2. User/global scope** | `~/.claude/CLAUDE.md` | `~/.codex/AGENTS.md` (+ `AGENTS.override.md`) | `~/.config/opencode/AGENTS.md` | `~/.gemini/<configured filename>` |
| **3. Project scope** | `./CLAUDE.md` or `./.claude/CLAUDE.md` | Per-directory, git root → cwd | Project root | Project root and ancestors |
| **4. Precedence** | Concatenation; broader-to-narrower load order (managed → user → project → local); conflicts are the model's problem ("Claude may pick one arbitrarily") | Concatenation, **position-significant**: "files closer to your current directory override earlier guidance because they appear later" — confirmed verbatim | "First matching file wins **per category**": if both `AGENTS.md` and `CLAUDE.md` exist, only `AGENTS.md` is used — confirmed verbatim, this is a same-directory tie-break, not a merge | Hierarchical concatenation, general → specific, **confirmed**: "Content from files lower in this list (more specific) typically overrides or supplements content from files higher up" — same general-to-specific shape as the other three |
| **5. Inheritance/nesting** | Directory-hierarchy loaded eagerly above cwd; **subdirectory files load lazily**, only when Claude reads a file there | Eager, git-root → cwd, one file per directory level (override-or-plain, never both) | Documented as project-root discovery; deep nesting behavior not separately researched — **out of scope for the tracer bullet** (Fase 10) | Sub-directory context files also load, capped at 200 dirs by default — out of scope for the tracer bullet |
| **6. Imports/references** | **Yes — `@path/to/file` syntax, confirmed.** Recursive, max depth 4. Both relative and absolute paths. **Official example targets `@AGENTS.md` by name.** External imports (resolving outside the working directory) trigger a one-time approval dialog for project-scope files; user-scope files load without it. | **No import syntax found.** AGENTS.md is plain markdown, concatenated as-is. Confirmed by two independent doc fetches. | **No automatic import parsing** — confirmed verbatim: "opencode doesn't automatically parse file references in AGENTS.md." Separately, the `instructions` config field (`opencode.json`) accepts file paths, globs, and remote URLs, and "all instruction files are combined with your AGENTS.md files" — a config-level reference mechanism, not an inline-markdown one. | **Yes — `@path/to/file.md` syntax, confirmed** ("Memory Import Processor"). Same shape as Claude's. |
| **7. Fallback compatibility** | Reads only `CLAUDE.md`; the *documented, official* interop path for `AGENTS.md` is the `@AGENTS.md` import or a symlink | None found (fallback filenames are alternate *names* Codex will look for, not a compatibility read of another harness's convention) | **Explicit, built-in**: falls back to `CLAUDE.md`/`~/.claude/CLAUDE.md` if `AGENTS.md` is absent | None found beyond the configurable `context.fileName` |
| **8. Multiple applicable files** | Concatenated, ordered root→cwd, `CLAUDE.local.md` appended after `CLAUDE.md` at each level | Concatenated, position-significant as above; **`AGENTS.override.md` replaces `AGENTS.md` at that one directory level — confirmed, not a merge** | "First matching file wins per category" at a given location; cross-location (global vs. project) behavior not separately confirmed this pass | Concatenated with path-labeled separators, per `/memory show` |
| **9. User-owned/shared expectation** | Explicit — hand-edited, version-controlled team file | Explicit — same | Explicit — same | Explicit — same |
| **10. Inspect CLI/API** | **Yes, confirmed: `/context`** lists loaded "Memory files"; `/memory` browses/edits file locations | **Not confirmed.** No dedicated inspect command found in official docs; a natural-language workaround (asking the model to summarize its instructions) is not the same thing and is not treated as evidence here | **Not documented.** No CLI/session command found. | **Yes, confirmed: `/memory show`** displays combined instructional context and origin; CLI footer shows a loaded-file count |

**New, load-bearing findings not in the original research:**

- Claude Code's own documentation prescribes `@AGENTS.md` (or a symlink) as
  *the* interop mechanism when a repo already has `AGENTS.md` — this is not
  UZE inventing a bridge, it is the vendor's stated intended usage.
- OpenCode prefers `AGENTS.md` over `CLAUDE.md` automatically, with **zero
  configuration** — confirmed, not assumed.
- `AGENTS.override.md` is a real destructive footgun if UZE ever created one
  where a user override might later be expected: it *replaces*, it does not
  merge. This design does not use it (see Fase 4).
- Gemini CLI supports the identical `@path` import syntax as Claude Code
  ("Memory Import Processor") — a second harness converging on the same
  bridge mechanism, independent of Claude's.
- OpenCode's `instructions` config field is a genuine second delivery
  mechanism (config-referenced files, not inline-parsed) — evaluated in Fase
  4 and set aside in favor of uniformity (see rationale there).

---

## Fase 2 — Portable semantic core

| Property | Claude Code | Codex | OpenCode | Gemini CLI | Classification |
|---|---|---|---|---|---|
| **Content** (the markdown text itself) | Rendered as-is into context | Rendered as-is | Rendered as-is | Rendered as-is | **LOSSLESS** — plain markdown text is the one thing all four consume identically once it reaches their respective file. |
| **Scope** (project-level applicability) | Supported | Supported | Supported | Supported | **LOSSLESS** at project scope (Fase 10 restricts the tracer bullet to this scope). |
| **Precedence** (this content's rank relative to other instruction sources) | Determined by *where* the file sits in Claude's own hierarchy | Determined by *directory position* in Codex's own walk | Determined by *file-type* tie-break, then presumably location | Determined by *file-list position* in Gemini's own hierarchy | **NOT_APPLICABLE to package content.** Confirms the Fase 9 hypothesis below: no harness lets a *file's own content* declare its precedence — it is entirely a property of where the destination integration places the file. UZE's package model should carry no precedence field. |
| **Inheritance/nesting** (subdirectory-level instructions) | Real, lazy | Real, eager-at-session-start | Unresearched depth | Real, capped at 200 dirs | **NOT_APPLICABLE for the M3 tracer bullet** — genuinely divergent (Claude's lazy load vs. Codex's eager walk are different execution models, not just different depths), but out of scope since Fase 10 scopes the tracer bullet to one project-root file per harness. Flagged for future work, not silently assumed equivalent. |
| **Matching** (which resource applies to what) | n/a | n/a | n/a | n/a | **NOT_APPLICABLE** — Instructions have no matcher concept, unlike Hooks. There is exactly one canonical destination file per harness per scope; "matching" does not exist as a property here. |
| **Vendor metadata / limits** | ~200-line *guidance* (not enforced) | **32 KiB hard cap** (`project_doc_max_bytes`), empty files skipped | Unresearched | Unresearched | **LOSSY, disclosed.** A package's `AGENTS.md` that exceeds Codex's cap is silently truncated by Codex itself — not a UZE failure, but a real cross-harness inconsistency UZE must surface (Fase 18/19), not hide. |

**Markdown-equal is not semantics-equal**, exactly as the brief warned: Codex's
"later position overrides" and Claude's "concatenate, conflicts are the
model's problem" are two different behaviors that happen to look similar in
the common case (no actual conflict) and diverge the moment two regions
genuinely disagree. UZE's design does not try to normalize this — it treats
insertion order as a UZE-controlled, disclosed policy (Fase 6), not a claim
that ordering means the same thing everywhere.

---

## Fase 3 — Package-side representation

**Decision: plain `AGENTS.md` at the package root. Option A.**

Evaluated against the stated criteria:

| Criterion | `AGENTS.md` (A) | `instructions/AGENTS.md` (B) | UZE-specific format (D) |
|---|---|---|---|
| Usable directly without UZE | **Yes** — it's already the exact file Codex and OpenCode read natively, and the same content Claude/Gemini need behind their one-line bridge | Only after UZE moves it | No |
| Compatible with existing standards | **Yes** — this *is* the agents.md convention | Adds a UZE-chosen subdirectory convention no external tool expects | No |
| No proprietary metadata | **Yes** | Yes | No, by definition |
| Package-native delivery | **Yes** — a package's own root `AGENTS.md` requires no UZE transformation to be independently useful outside UZE | Yes, with an extra path segment | N/A |
| UZE decomposition capability | **Yes** — Fase 4/5 show a package-root `AGENTS.md` decomposes cleanly into a region merged into the project's canonical file | Yes, equivalently | N/A |

Rejected B because it adds a directory convention with no external meaning
and no advantage over A given every consuming harness already expects the
bare filename. Rejected D outright per the brief's explicit steer and because
nothing in Fase 1/2 shows A is insufficient.

---

## Fase 4 — Ownership model (per harness)

**Central design decision:** the project's canonical `AGENTS.md` is not one
UZE-owned file with per-harness copies — it is **one shared artifact**, and
UZE manages a **delimited region per contributing package** inside it. Three
of four harnesses read that one file close to natively; the fourth
(vendor-format) case is Claude Code, which needs a second, much smaller
managed artifact: a one-line bridge inside its own `CLAUDE.md`.

| Harness | Chosen mechanism | Why this is the least invasive option |
|---|---|---|
| **Codex** | Read `AGENTS.md` at project root **natively — zero UZE artifact beyond the shared file itself** | Native format. No import needed. `AGENTS.override.md` is explicitly **not used** by this design — Fase 1 confirmed it *replaces* rather than merges, which is exactly the kind of silent-destruction risk ADR-009 rules out. |
| **OpenCode** | Read `AGENTS.md` at project root **natively — zero UZE artifact beyond the shared file itself** | Confirmed automatic preference over `CLAUDE.md`. The `instructions` config-field alternative (option B-shaped: dedicated file + config reference) was evaluated and set aside: it would require a **new** `ManagedArtifact` variant for a JSON-array config edit, while OpenCode's native `AGENTS.md` preference already gives a zero-artifact native path. Building the config-array mechanism for one harness that doesn't need it is exactly the premature generalization Fase 5 warns against. |
| **Claude Code** | **Delimited one-line region inside `CLAUDE.md`**, content = the vendor-documented `@AGENTS.md` import syntax (option A: delimited text region, applied to the *smallest possible* content — one line, not the instructions themselves) | Claude never reads `AGENTS.md` on its own; the vendor's own docs prescribe exactly this bridge. A managed *symlink* (`CLAUDE.md -> AGENTS.md`, option C) was considered and rejected as the default: it only works when `CLAUDE.md` doesn't already exist or isn't meant to hold other content, and this design cannot assume that. The one-line import is strictly less invasive and works whether or not the user already has a `CLAUDE.md`. |
| **Gemini CLI** | **Delimited one-line region inside `GEMINI.md`**, content = the equivalent `@AGENTS.md` import (Gemini's own Memory Import Processor, confirmed same syntax family) | Same reasoning as Claude. The `context.fileName` array alternative (config-level reference, closer to option B) was evaluated and set aside for the same reason as OpenCode's `instructions` field: it needs a new `ManagedArtifact` config-array variant, order/conflict semantics for that array are undocumented (Fase 1), and it risks silently changing a value the user may have customized. A one-line, uniformly-implemented bridge (same primitive as Claude's) is simpler and lower-risk than a second new config-editing code path for a single harness. |

This directly answers the brief's ownership-model question: **UZE owns a
region of a shared artifact, never the whole artifact**, in exactly two
shapes — a per-package content region inside `AGENTS.md`, and a fixed,
package-independent bridge region inside `CLAUDE.md`/`GEMINI.md`. Neither
requires UZE to ever write a whole file it did not already find empty, and
both are options A from the brief's list (delimited text region) — options B
(dedicated file + native reference) and C (symlink) were seriously evaluated
per-harness above and rejected on concrete grounds, not skipped.

**The Core does not know any of the filenames above.** `CLAUDE.md`,
`AGENTS.md`, `GEMINI.md`, and the bridge-vs-content distinction are
`uze-integrations` knowledge, expressed as different `resource_path`/region
parameters passed to one generic mechanism (Fase 5, Fase 13).

---

## Fase 5 — Managed Text Region (generic primitive)

Two independent contexts need it (content regions in `AGENTS.md`; bridge
regions in `CLAUDE.md`/`GEMINI.md`) — clears the "at least two contexts" bar
in the brief, so a generic primitive is justified rather than an
integration-local hack.

Proposed shape (illustrative field list, not final Rust — that is Phase B):

| Field | Purpose | Notes |
|---|---|---|
| `target_file` | Absolute path to the shared file | Integration-supplied; Core never inspects the filename |
| `region_identity` | Stable string identifying this region, independent of content | E.g. `package_id + resource_identity` for content regions, a fixed constant for a bridge region (Fase 4) |
| `expected_content` | The exact text this region should contain | Compared verbatim on inspect, not by hash-only, so a human diff is always available; a content hash may additionally be stored for fast comparison |
| `begin_marker` / `end_marker` | Delimiters derived from `region_identity`, not free text | See parsing safety below |
| `insertion_policy` | Where a new region goes when the file doesn't have one yet | Append at end of file, preceded by exactly one blank line if the file is non-empty; deterministic — never "wherever felt convenient" |
| `newline_policy` | Preserve the file's existing line-ending style (LF/CRLF) and ensure exactly one trailing newline after write | Never reformat content outside the region |
| `encoding` | UTF-8, reject (BLOCK, per Fase 6) on invalid encoding rather than guess | |
| `atomic_write` | Write to a temp file in the same directory, then rename over the target | Same durability pattern as any other Store write; avoids partial-write corruption |

Explicitly **not** in the primitive: `claude`, `gemini`, `agents`,
`instruction`, or any capability-kind knowledge. It is named and shaped
identically to how `SymlinkReference`/`VendorConfigEntry` already stay
generic — this is a third `ManagedArtifact` shape for "a delimited slice of a
file the integration otherwise does not own," not a fourth capability kind.

### Marker parsing safety (answers Fase 18's "do not accept `string.find`")

- Markers are generated as
  `<!-- uze:begin {region_identity} -->` /
  `<!-- uze:end {region_identity} -->`, where `region_identity` is restricted
  to a fixed safe character set (already true of `PackageId`/resource
  identities in the existing model) so a marker cannot itself be forged by
  package content containing similar text.
- Parsing must find **exactly one** begin marker and **exactly one** matching
  end marker for a given `region_identity`, both as **whole lines** (a marker
  string appearing mid-line, e.g. quoted inside a code fence, does not
  count), and the end marker must not precede the begin marker.
- Any other shape — zero begin markers with an end marker present, two begin
  markers for the same identity, an end marker for a different identity
  nested inside, an unterminated begin marker — is **not** "best effort
  guess," it is `BLOCKED` (Fase 7/8). This is the direct implementation of
  "não aceite parser baseado em `string.find` sem provar unicidade e
  boundaries."

---

## Fase 6 — Safe attach

| Case | Behavior |
|---|---|
| 1. File doesn't exist | Create it, insertion policy applies to an empty file (region only, no leading blank line). |
| 2. File exists, user-owned, no UZE region yet | Append per `insertion_policy`; existing content is never reflowed, reformatted, or reordered. |
| 3. File exists, already has *this* region | Idempotent — re-attach compares `expected_content`; if it already matches, no write occurs (no gratuitous diff/timestamp churn). |
| 4. Region MATCHED | No-op on re-attach (case 3). |
| 5. Region DRIFTED | **Do not overwrite.** Attach of a *different* package/region may proceed; attach/update of *this* region reports the drift and does not silently replace user edits — this is a write path, so it inherits the same "never destroy what you cannot prove you own" rule as detach (Fase 8). |
| 6. Markers broken (per Fase 5 parsing rules) | `BLOCK`. Never guess a repair. |
| 7. Two packages provide instructions | Two independent regions, two independent `region_identity` values, in insertion order — see Fase 9 for the ordering policy. Neither package's attach touches the other's region. |
| 8. Package reinstalled | Same as case 3 — idempotent, content compared, not re-appended. |
| 9. Content identical to what's already there but arrived via a different code path (e.g. hand-typed by the user, byte-for-byte matching a package's content, no markers) | Content equality alone is never ownership. Without the markers, this is user content coincidentally matching, and attach still inserts a proper delimited region rather than assuming the marker-less text is "close enough" — matches Fase 7/8's rule that ownership is proven structurally, not textually. |
| 10. File doesn't end in a newline | Attach normalizes to exactly one trailing newline as part of `newline_policy`, and preserves everything before that unchanged. |

**"If it cannot prove a safe operation, BLOCK"** is the default, not an
exception path — this mirrors `ManagedEntryDrift`/`ManagedEntryConflict`
already thrown by `ExposureMechanism::attach` for symlinks (`exposure.rs`),
extended to a region rather than a whole path.

---

## Fase 7 — Safe inspect

Reuses the existing `AttachmentState` vocabulary
(`Matched | Missing | Drifted | Conflict | Blocked`) from `integration.rs` —
no new states, per the brief's explicit preference.

| Scenario | State |
|---|---|
| Region present, content byte-identical to `expected_content` | `MATCHED` |
| User edits text **outside** the region | `MATCHED` — inspection is scoped to the declared region only, never the whole file, exactly as the brief requires |
| User edits text **inside** the region | `DRIFTED` |
| Region removed entirely (markers gone, file otherwise intact) | `MISSING` |
| Markers duplicated, malformed, or nested for this `region_identity` | `BLOCKED` (parsing-safety failure, not a content comparison) |
| Two different packages' regions both present and each individually well-formed | Each inspected **independently** — one package's `DRIFTED` region must never affect another package's `MATCHED` region in the same file |
| Target file itself missing | `MISSING` — same semantics as a missing symlink target |
| Target file unreadable (permissions) | `BLOCKED`, matching the existing `inspect_standard_receipt` pattern for an unreadable symlink parent |

---

## Fase 8 — Safe detach

**The core proof of this milestone.** Given:

```
user text A
<!-- uze:begin pkg/resource -->
managed content
<!-- uze:end pkg/resource -->
user text B
```

`uze remove` on a `MATCHED` region must produce exactly:

```
user text A
user text B
```

with **no reformatting** of A or B — same line endings, same trailing
whitespace, same everything, minus the region and exactly the blank line(s)
the insertion policy added (so that repeated attach/detach cycles are
byte-idempotent, not just semantically similar).

Test matrix to prove this in Phase C (not run yet — planning only):

| Case | Expected outcome |
|---|---|
| CRLF file | Detach preserves CRLF throughout; no LF creep into surrounding content |
| Trailing newline present/absent before attach | Detach restores the file to what it would have been had attach never run, modulo the one normalization documented in Fase 6 case 10 |
| Unicode content, in-region and out-of-region | Byte-preserved, no re-encoding |
| Arbitrary Markdown around the region (headers, code fences, tables) | Untouched |
| Multiple regions from different packages | Detaching one leaves all others, including their exact surrounding blank-line spacing, untouched |
| Region is `DRIFTED` | **Detach refuses** — `BLOCK`, per ADR-009, no destructive path exists for a state UZE cannot prove it still owns |
| Markers invalid/broken | **Detach refuses** — same `BLOCKED` state as inspect, never "best effort cleanup" |
| Detach leaves the file empty (only the region existed) | Empty-file cleanup: delete the file **only if** UZE also created it (tracked in the receipt) — never delete a file UZE found pre-existing, even if detach leaves it content-free, since the user may intentionally keep an empty `AGENTS.md` under version control |

---

## Fase 9 — Precedence

**Hypothesis from the brief: precedence belongs to the Integration/destination,
not to package content.**

Attempted falsification: is there any harness where a package's own content
must declare something like "load me first" or "I am high-priority"? None
found in Fase 1/2 — every harness's precedence rule is a property of *where*
a file sits (directory depth, scope tier, file-type tie-break), never
something the file's content asserts about itself. The one place ordering
*within* a single shared file matters — multiple packages' regions inside one
`AGENTS.md`, where Codex's own "later position overrides" rule means
insertion order has a real effect — is not the package declaring precedence
either; it is a property of **UZE's own insertion policy** (Fase 5:
deterministic append order), which this design fixes as **lexicographic by
`package_id`**, chosen for reproducibility across machines/reinstalls rather
than arbitrary install order. A package's `AGENTS.md` carries no precedence
metadata.

**Hypothesis holds. Precedence stays out of the package/content model
entirely**, consistent with `CapabilityRouter` remaining capability-only.

---

## Fase 10 — Scope

**Decision: project scope, for the tracer bullet.**

User/global scope was seriously considered and rejected, not skipped:

- Global scope (`~/.claude/CLAUDE.md`, `~/.codex/AGENTS.md`, etc.) is a
  **single file per user**, not a directory of many discrete entries the way
  Skills' `~/.claude/skills/<name>` already is. Multiple packages would
  collide on the exact same global-scope region-merge problem project scope
  already has to solve — global scope buys no safety, only a different file
  path.
- `AGENTS.md`'s own ecosystem convention *is* project-level: a
  version-controlled, team-shared file describing one project. Placing it at
  global scope would be modeling the capability against its own designed
  purpose.
- Global scope would also touch the user's machine-wide configuration for
  every harness on every package install — a larger, less local blast radius
  than touching one project's own repository, which the user already
  controls via version control and can review/revert with tools they already
  know (`git diff`, `git checkout`).

Project scope does mean UZE writes into the user's own repository files for
the first time (see the note in Fase 4/13 about this being new territory)
but the region-based ownership model (Fase 4–8) is exactly what makes that
safe: every byte UZE ever writes there is provably UZE's own, delimited, and
removable without touching anything else. Nesting/subdirectory scope stays
explicitly out of scope for this tracer bullet (Fase 2).

---

## Fase 11 — Plugin-first

Unchanged from the existing rule (ADR-008): a package with a native envelope
a harness consumes as a whole plugin is delivered natively, and UZE never
separately attaches a resource that native delivery already covers.

For Instructions specifically: if a package ships as a Claude Code plugin
whose own manifest already causes Claude to load a bundled instructions file
(plugin-scoped context, if such a mechanism exists in that harness's native
package format), the native path wins and the region-based `AGENTS.md`
delivery described above must not also run for that resource on that
harness — `PackageExposurePlan.provided_resource_identities` already exists
precisely to prevent this double-delivery (`exposure.rs`), and Instructions
uses it exactly as Skills/MCP do today. Phase C's test plan (Fase 16, item J)
explicitly proves this for a package containing Skill + MCP + Instruction: on
whichever harness consumes it as one native plugin, the Instruction resource
must not additionally receive a project-`AGENTS.md` region.

---

## Fase 12 — Capability model

`CapabilityKind::Instruction` already exists (`capability.rs`). Testing
whether the current `Resource`/`Capability` shape can represent it as-is,
before proposing any change:

- `Capability { kind: Instruction, representation: Standard, path, payload }`
  — `path` is the package's root `AGENTS.md`, `payload` is its raw bytes.
  This is sufficient to *carry* the content.
- What routing an Instruction resource needs beyond that is not new semantic
  information *on the capability* — it's **destination knowledge**, which
  already correctly lives in each `IntegrationPort`'s `exposure_plan`, not in
  `Capability` (this is the same reason Skills/MCP need no extra fields
  today: `IntegrationPort` decides *how*, `Capability` only says *what*).

**Conclusion: `CapabilityKind::Instruction` and the existing `Resource`
shape already suffice. No field addition, no new variant.** This is a
meaningfully different conclusion than Hooks reached in the M3 landscape
research (`CORE_MODEL_INSUFFICIENT`) — Instructions needed no new Core
knowledge here because its one real hard problem (region ownership) lives
entirely in the *delivery* layer (`ExposureMechanism`/`ManagedArtifact`), not
in the capability description itself.

---

## Fase 13 — IntegrationPort

**Goal met: no new vendor-specific Core knowledge, and no method resembling
`attach_instruction_to_claude_file(...)`.**

The existing `IntegrationPort::exposure_plan(&self, resource: &Resource) ->
ExposurePlan` is sufficient: each integration, given an `Instruction`
resource, returns an `ExposurePlan` whose `mechanism` is the new
region-based variant (Fase 5) parameterized with *that integration's own*
`target_file` and `region_identity` — Claude's integration returns a bridge
region in `CLAUDE.md`, Codex's and OpenCode's integrations return... nothing
extra, because their `exposure_plan` for `Instruction` should resolve to a
mechanism pointing directly at the shared `AGENTS.md` with no bridge layer,
and Gemini's returns a bridge region in `GEMINI.md`. All four express this
through the **same** generic mechanism type and the **same** trait method
already in place; only the parameters differ, which is exactly what
`IntegrationPort` already exists to encapsulate.

The one open question genuinely worth surfacing before Phase B, per the
brief's own instruction to stop and explain rather than add a vendor-shaped
method: **is a "bridge region" (Claude, Gemini) the same generic mechanism
as a "content region" (the shared `AGENTS.md` itself), or two different
things?** This design's position: they are the *same* mechanism
(`ManagedTextRegion`, Fase 5) applied to two different files with two
different `region_identity` conventions (per-package vs. fixed) — not two
mechanisms. Evidence: both need identical marker-safety, drift-detection,
and detach guarantees; the only difference is *what decides the region's
expected content* (package payload vs. a fixed bridge string), which is a
parameter, not a new mechanism shape. If Phase B implementation finds this
doesn't hold, that is a real stop condition (per the brief's #5/#8) and
should come back for review rather than be quietly special-cased.

---

## Fase 14 — Receipt / ledger

`ManagedTextRegion` becomes a fourth `ManagedArtifact` variant, alongside
`SymlinkReference` / `VendorConfigEntry` / `IntegrationOwned`. Proposed
shape, mirroring the existing two concrete variants' level of detail:

```
ManagedTextRegion {
    target_file: PathBuf,
    region_identity: String,
    begin_marker: String,
    end_marker: String,
    expected_content: String,
}
```

- **Serialization compatibility**: this is a new enum variant, not a change
  to any existing one — exactly the same additive shape
  `IntegrationOwned` used when it was introduced (`integration.rs`'s own
  `#[serde(alias = "MARKETPLACE_PLUGIN")]` precedent). No migration
  framework needed; old receipts keep deserializing unchanged, and this
  variant simply becomes selectable for new writes.
- **Attach/inspect/detach** all route through this variant using the exact
  logic in Fase 6/7/8 above — parallel to, not replacing,
  `inspect_standard_receipt`/`detach_standard_receipt`'s existing symlink
  logic. Whether this lives as a third arm in those same shared functions or
  as sibling functions is a Phase B implementation detail, not a design
  question — either preserves the "safety pattern," which is what
  `inspect_standard_receipt`/`detach_standard_receipt` exist to centralize.
- **The bridge-region reference-counting question** (raised in this
  document's internal reasoning, surfaced honestly here rather than
  solved): when a bridge region (Fase 4, Claude/Gemini) is shared
  infrastructure for *however many* packages currently contribute content
  regions to `AGENTS.md`, its own removal should not be tied to any single
  package's receipt. Proposed answer for the tracer bullet: **do not
  reference-count via the ledger.** At detach time, re-inspect the live
  `AGENTS.md` for any *remaining* well-formed content region before removing
  the bridge — consistent with ADR-009's "ask the owning integration to
  inspect real state" rather than trusting stored receipt bookkeeping. This
  is fully exercised only once a second package is involved, which the
  tracer bullet (Fase 16, single package) does not reach — flagged as a
  real but deferred question, not a blind spot.

---

## Fase 15 — Trust

Instructions are declarative text, never executed by UZE. Per the brief's own
framing: "esta instruction pode causar execução diretamente pelo UZE?" — no.
A package's `AGENTS.md` telling a model "run `make test` before committing"
is the model choosing to run a shell command through its own existing tool
permissions, identical in kind to a hand-written `CLAUDE.md` doing the same
thing today with zero UZE involvement. **No executable-capability trust
prompt is added.** `trust.rs`'s `executable_capabilities` stays scoped to MCP
`command` declarations exactly as M2 left it; this milestone does not extend
it. Hooks remain the separate, harder trust conversation the M3 landscape
research already flagged.

---

## Matrix / representation / ownership summary (as requested)

- **Matriz confirmada**: Fase 1 table above.
- **Representação package-side**: plain `AGENTS.md` at package root (Fase 3).
- **Ownership strategy por harness**: Codex/OpenCode read the shared
  `AGENTS.md` natively, zero extra artifact; Claude/Gemini each get one
  fixed, package-independent bridge region in their own native file (Fase
  4).
- **Semantic core**: content is lossless; precedence and matching are
  not-applicable to package content; nesting is out of scope for this
  tracer bullet (Fase 2).
- **Core diff previsto**: none in `Capability`/`CapabilityKind` (Fase 12);
  one new `ManagedArtifact::ManagedTextRegion` variant (Fase 14); no new
  `IntegrationPort` methods (Fase 13).
- **Lifecycle**: attach/inspect/detach fully specified in Fase 6/7/8,
  reusing the existing `AttachmentState` vocabulary.
- **Receipt strategy**: additive enum variant, existing
  `inspect_standard_receipt`/`detach_standard_receipt` pattern extended, no
  migration framework (Fase 14).

## Stop conditions reviewed

| # | Condition | Triggered? |
|---|---|---|
| 1 | Must assume ownership of the user's entire file | **No** — region-scoped throughout |
| 2 | Safe detach cannot be proven | **No**, per Fase 8's test plan — but not yet *empirically* proven; that is exactly what Phase C exists to do |
| 3 | Precedence requires vendor-specific Core semantics | **No** — Fase 9 falsification held |
| 4 | Instruction needs a proprietary UZE format | **No** — Fase 3 chose `AGENTS.md` |
| 5 | `IntegrationPort` needs vendor-oriented methods | **No** — Fase 13, one open question flagged, not a violation |
| 6 | A portable representation loses critical semantics | **No** — Fase 2, only nesting is deferred, and disclosed rather than assumed |
| 7 | Native package delivery causes inevitable duplication | **No** — `PackageExposurePlan` already prevents this, reused as-is (Fase 11) |
| 8 | `ManagedTextRegion` starts carrying harness semantics | **No** — Fase 5, filename/region-identity conventions live in the Integration, not the primitive |
| 9 | Attach requires a permanent UZE runtime | **No** — plain file writes |
| 10 | A destructive operation would violate ADR-009 | **No** — Fase 6/8 fail closed by design |

No stop condition is triggered by this design. **This is not the same as a
verdict** — the milestone's final verdict is earned in Phase C, empirically,
not asserted here.

---

## What Phase B / Phase C would cover (not started)

Per the brief's own phasing, listed here only so the review has the full
shape of what approval would unlock — none of this is implemented:

- **Phase B**: `ManagedTextRegion` in `uze-core`, marker-safety parser,
  attach/inspect/detach logic, L0 (pure parsing/lifecycle) and L1
  (filesystem) tests. No new harness integration yet.
- **Phase C**: the tracer bullet itself — one package, one `AGENTS.md`,
  against Claude Code, Codex, OpenCode, Gemini CLI, proving items A–J from
  the brief's Fase 16 (baseline, decomposition, attach, harness discovery,
  MATCHED, out-of-region edit stays MATCHED, in-region edit becomes DRIFTED,
  clean detach, blocked detach on drift, no native-package duplication),
  L2a conformance framing (Fase 17: file presence is not the same claim as
  verified harness discovery), and the adversarial test list from Fase 18.

**Stopping here for review, per the milestone's explicit instruction.**
