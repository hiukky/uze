# Adopt Native Projection Principle

Status: Accepted

Status note: §2's condition for Native Package ("the package ships the
harness's own envelope") is refined, not overturned, by
[ADR-020](020-generated-native-package-projection.md): a source package's
absence of a vendor envelope no longer by itself forces capability-level
decomposition. The hierarchy becomes `Explicit Native Package > Generated
Native Package > Native Capability > Safe Adaptation > Unsupported`. Every
other decision in this ADR — Derived Artifact, `Native ≠ zero-copy`,
physical location ≠ ownership, exact coverage via `discovered ∩ declared` —
remains in effect unchanged and is reused, not replaced, by ADR-020.

## Context

UZE's Store already owns canonical plugin bytes and the Engine already discovers portable capabilities, but delivery had no explicit hierarchy. A plugin could reach Claude Code as decomposed `~/.claude/skills` shims + `~/.claude.json` MCP entries, even when the same plugin ships a valid `.claude-plugin/plugin.json` that Claude Code can consume as a single native bundle. That duality needed a principle: when to prefer a bundle, when to fall back to capabilities, and what may be derived without becoming authoritative. The same tension exists for Codex (catalogue), Gemini (linked extension), and OpenCode (capability-native projection) — each with a different native surface and a different native-vs-copy trade-off.

The Claude tracer bullet proved the hardest case empirically: Claude requires `source` relative and contained in the marketplace root, so a Store-referencing marketplace must be co-located (`$UZE_HOME/store/.claude-plugin/marketplace.json` with `source: "./packages/<id>"`) and always copies to `~/.claude/plugins/cache/<market>/<plugin>/<ver>/`. Copying looks like it breaks `install-once`, and an over-eager bundle could claim capabilities it doesn't actually deliver.

## Decision

Adopt **Native Projection**:

```
Native Package  >  Native Capability  >  Safe Adaptation  >  Unsupported
```

1. Preserve the canonical source verbatim in the UZE Store. The Store never mutates on behalf of a harness, never knows a harness name, and never writes harness artifacts.

2. Prefer **Native Package** when the package ships the harness's own envelope (`.claude-plugin/plugin.json`, `.codex-plugin/plugin.json`, `gemini-extension.json`). The owning Integration decides package-level delivery via `package_exposure_plan` and declares exactly which discovered resources the envelope actually delivers (`provided_resource_identities = discovered ∩ declared`). Undeclared resources are not marked provided and continue through normal `exposure_plan` fallback — no silent disappearance.

3. Fall back to **Native Capability** (`ManagedUserScopeReference` with `route:Native`, direct Standard consumption) and then **Safe Adaptation** (shim/marketplace generation, vendor CLI) only when native package is unavailable. `Unsupported` carries a rationale, never a silent translation.

4. Introduce **Derived Artifact** as a documented concept, not a new Rust ledger type:

   > An artifact created from the Store's source of truth for delivery to a harness.

   Properties: not authoritative, rebuildable from Store alone, inspectable via official harness surface, safely removable (ADR-009), may be a copy, generated catalogue/manifest, symlink/reference, or harness-owned cache. `AttachmentReceipt::IntegrationOwned` (with `kind` string, `selector`, `detail` map) already represents the lifecycle; no new ledger or `PackageKind` enum is added to the Core.

5. Explicitly: **Native ≠ zero-copy**. For Claude, `Store → derived marketplace.json → claude plugin install → Claude-owned cache copy` does not violate `install-once` because the cache is not a source of truth — `rm cache && reinstall` rebuilds byte-identically from the Store. This matches the official marketplace's own cache behavior.

6. Formalize **physical location ≠ ownership**: `$UZE_HOME/store/.claude-plugin/marketplace.json` lives physically under `store/` so `source: "./packages/<id>"` is valid, but it is owned and rebuilt solely by `ClaudeIntegration::republish_packages` (derived view, never Store-owned), mirroring Codex's `store/.agents/plugins/marketplace.json`.

Vendor-specific manifest parsing (`skills`, `mcpServers`, path normalization, `..` rejection, duplicate deduplication, malformed handling) belongs to the owning Integration. Core/Engine remain vendor-neutral and never generate harness artifacts.

## Consequences

Claude delivery becomes package-aware: plugins with a valid envelope yield one `IntegrationOwned{kind:"claude-plugin"}` receipt covering exactly declared skills/MCP; plugins without an envelope keep the previous per-capability shim/MCP behavior unchanged. Uncovered capabilities fall back individually — proven with a partial-coverage fixture (1 native skill, 1 uncovered skill, 1 uncovered MCP).

`claude_exact_coverage` enforces intersection, `..`/absolute/duplicate/malformed/empty cases are ignored, and an envelope with empty coverage still enables package delivery with empty `provided_resource_identities` (no invented equivalence, no universal schema, no Store mutation).

`republish_packages`/`publication` now exist for Claude (deterministic catalogue generation, `doctor` distinguishes `Published`/`Unpublished`). No new Core enums, no hashes, no universal projection abstraction, no TUI changes, no migration automation, no frontmatter transformation — all deferred. Runtime projection (`--add-dir CLAUDE.md`) remains orthogonal.

Tests: 12 new L0 coverage/lifecycle tests for Claude native package (subset, ghost, extra, malformed, empty, duplicate, escape, MCP exact, marketplace determinism, fallback, receipt coverage), existing `cargo test` suite remains green. The principle applies unchanged to Codex (catalogue reference, no copy), Gemini (link, no copy), and OpenCode (capability-native, no fake bundle).
