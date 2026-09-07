# Instructions — the design record

**Historical record.** This is the design that was reviewed and implemented; the
shipped result is the [Context Manager](context-manager.md), which is the current
truth. Kept because ADR-014, `application.rs` and source comments cite it, and
because the rejected alternatives below are the reason the current shape looks
the way it does.

Researched 2026-08-21, when Gemini CLI was still a target harness. It was removed
by ADR-027 and its role taken by Antigravity, which follows the same workspace
context rules; the reasoning that named Gemini reads unchanged for Antigravity.

## What each harness does with instruction files

| | Claude Code | Codex | OpenCode | Gemini CLI |
|---|---|---|---|---|
| **Files** | `CLAUDE.md`, `CLAUDE.local.md` | `AGENTS.md`, `AGENTS.override.md`, configurable fallbacks | `AGENTS.md` preferred, `CLAUDE.md` fallback | `GEMINI.md`; `context.fileName` accepts a string or array |
| **Project scope** | `./CLAUDE.md` or `./.claude/CLAUDE.md` | per-directory, git root → cwd | project root | project root and ancestors |
| **Precedence** | concatenation, broad → narrow; conflicts are the model's problem | concatenation, **position-significant**: files closer to cwd override | first matching file wins **per category** — `AGENTS.md` wins over `CLAUDE.md` | hierarchical, general → specific |
| **Imports** | **`@path` syntax**, recursive, max depth 4; the official example targets `@AGENTS.md` by name | none | none inline; the `instructions` config field takes paths, globs and URLs | **`@path` syntax** ("Memory Import Processor") |
| **Interop with `AGENTS.md`** | the vendor's own documented path is the `@AGENTS.md` import or a symlink | native | **built-in** — falls back to `CLAUDE.md` only when `AGENTS.md` is absent | none beyond `context.fileName` |
| **Inspect command** | `/context`, `/memory` | none documented | none documented | `/memory show` |

Three findings were load-bearing:

- Claude Code's own documentation prescribes `@AGENTS.md` as *the* interop
  mechanism. The bridge is the vendor's stated usage, not uze inventing one.
- OpenCode prefers `AGENTS.md` over `CLAUDE.md` with zero configuration.
- **`AGENTS.override.md` replaces, it does not merge.** Writing one where a user
  override might later be expected is exactly the silent destruction ADR-009
  rules out, so this design never uses it.

## Markdown-equal is not semantics-equal

Content, and project scope, are lossless: plain Markdown reaches all four
identically once it is in the right file. Everything else is not.

**Precedence is not a property of content.** No harness lets a file declare its
own rank — every precedence rule is about *where* the file sits (directory
depth, scope tier, file-type tie-break). So a package's `AGENTS.md` carries no
precedence metadata, and ordering *within* the shared file is uze's own
insertion policy: **lexicographic by package id**, chosen for reproducibility
across machines rather than install order.

**Vendor limits are lossy and disclosed.** Codex enforces a 32 KiB hard cap
(`project_doc_max_bytes`) and skips empty files; Claude's ~200-line guidance is
not enforced. A package's `AGENTS.md` over Codex's cap is truncated by Codex, not
by uze — a real cross-harness inconsistency that must be surfaced, never hidden.

**Nesting was left out of scope.** Claude's lazy subdirectory load and Codex's
eager root→cwd walk are different execution models, not different depths. Flagged
rather than assumed equivalent.

## Package-side representation: a plain `AGENTS.md` at the package root

Chosen over `instructions/AGENTS.md` and over a uze-specific format because it is
already the exact file Codex and OpenCode read natively, and the same content the
others need behind a one-line bridge. It is usable without uze, adds no
directory convention no external tool expects, and decomposes cleanly into a
region.

## Ownership: uze owns a region, never a file

The project's `AGENTS.md` is **one shared artifact** with a delimited region per
contributing package. Three harnesses read it close to natively; the fourth needs
one much smaller managed artifact — a one-line bridge in its own file.

| Harness | Mechanism | Why |
|---|---|---|
| Codex | native `AGENTS.md`, **zero uze artifact** | native format, no import needed |
| OpenCode | native `AGENTS.md`, **zero uze artifact** | automatic preference over `CLAUDE.md` |
| Claude Code | a one-line region in `CLAUDE.md` carrying `@AGENTS.md` | never reads `AGENTS.md`; the vendor's own docs prescribe this |
| Gemini CLI *(now Antigravity)* | the same one-line bridge | same import syntax family |

Alternatives evaluated and rejected on concrete grounds:

- **A managed symlink** (`CLAUDE.md -> AGENTS.md`) only works when `CLAUDE.md`
  does not already exist or is not meant to hold other content, and the design
  cannot assume that. The one-line import works either way.
- **OpenCode's `instructions` config field** and **Gemini's `context.fileName`
  array** are genuine second mechanisms, but each would need a new
  `ManagedArtifact` variant for a JSON-array config edit, with undocumented
  order and conflict semantics, and each risks silently changing a value the
  user customized — to serve one harness that already has a zero-artifact
  native path.

So uze writes a whole file only when it finds one absent, and every byte it ever
writes is delimited and removable without touching anything else.

**The Core knows none of these filenames.** `AGENTS.md`, `CLAUDE.md`,
`GEMINI.md` and the bridge-vs-content distinction are integration knowledge,
expressed as different region parameters passed to one generic primitive.

## Why a generic Managed Text Region

Two independent contexts need it — content regions in `AGENTS.md`, bridge regions
in a vendor file — which clears the bar for a shared primitive rather than an
integration-local hack. Marker parsing is structural, never `string.find`: a
malformed or nested marker pair is reported as malformed and refused, never
partially rewritten.

## Why project scope

Global scope was considered and rejected. It is a *single file per user*, not a
directory of discrete entries the way `~/.claude/skills/<name>` is, so multiple
packages collide on the same region-merge problem project scope already solves —
it buys no safety, only a different path. `AGENTS.md`'s own convention is
project-level, version-controlled and team-shared. And a global write touches
machine-wide configuration for every harness on every install, a much larger
blast radius than one repository the user can already `git diff` and revert.

## Plugin-first still wins

Unchanged from ADR-013: where a package ships a native envelope a harness
consumes whole, native delivery wins and uze does not additionally attach a
resource that envelope already covers.
`PackageExposurePlan.provided_resource_identities` is what prevents the double
delivery, and Instructions uses it exactly as Skills and MCP do.

## Capability model: nothing new was needed

`CapabilityKind::Instruction` already existed. A
`Capability { kind: Instruction, representation: Standard, path, payload }`
carries the content; everything else routing needs is *destination* knowledge,
which lives in the integration. `IntegrationPort` gained no method across this
work.
