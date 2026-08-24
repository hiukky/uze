# Claude Code Integration

Peer integration for Claude Code. Delivers a UZE package as one native
plugin — either the package's own explicit `.claude-plugin/plugin.json`
(Explicit Native Package), or, absent one, a UZE-synthesized envelope
covering the package's conventional `skills/`/`mcp.json` surface (Generated
Native Package, ADR-020) — or, when neither surface is safely
representable, decomposed into a managed Skill symlink plus a registered
MCP server. The only integration in this crate with a runtime-projection
mechanism (`--add-dir` delivery of `AGENTS.md`, independent of package
delivery).

## Support

| Surface | Status | Delivery | Evidence |
|---|---|---|---|
| Plugin (native, explicit) | SUPPORTED | Derived marketplace catalogue → `claude plugin install` | EMPIRICAL (marketplace/install config confirmed live 2026-08-20 per ADR-013); CLI-shelling functions have no unit test |
| Plugin (native, generated) | SUPPORTED | Second, UZE-owned `uze-store` catalogue → `claude plugin install` (ADR-020) | TESTED (`claude::generate::generated_native_tests`) + CODE_FACT |
| Skills | SUPPORTED | Native envelope (VIA_PACKAGE) or managed skills-dir symlink (NATIVE_CAPABILITY) | EMPIRICAL — real `claude -p` run returned the exact proof token end-to-end (ADR-006) |
| MCP | SUPPORTED (config), PARTIAL (behavioral) | Native envelope (VIA_PACKAGE) or `claude mcp add --scope user --transport stdio` (SAFE_ADAPTATION) | EMPIRICAL for config/discovery (`claude mcp get`/`list` confirmed `✔ Connected` live, ADR-007); a real tool call needed a non-default `--allowedTools=mcp__...` flag and a secondary headless-discovery quirk was never fully closed |
| Context (runtime) | EXPERIMENTAL | `--add-dir` + `CLAUDE_CODE_ADDITIONAL_DIRECTORIES_CLAUDE_MD=1` (RUNTIME_PROJECTION) | EMPIRICAL — extensive real-CLI evidence (ADR-014); `/compact` retention across a session is the one open gap |
| Agents | NOT_IMPLEMENTED | — `CapabilityKind::Agent` is recognized only by `uze-core::importers`, never routed here | CODE_FACT |
| Hooks | NOT_IMPLEMENTED | — `CapabilityKind::Hook` same as above | CODE_FACT |
| Skill invocation policy | SUPPORTED | Canonical `invoke: {model,user}` is translated into Claude's own SKILL.md frontmatter: `disable-model-invocation: true` (model=false) and `user-invocable: false` (user=false). Generated envelopes materialize those markers; an explicit envelope is only claimed as covered when the author's own bytes already carry them (never rewritten) — ADR-030 | EMPIRICAL — real `claude -p` run, `UZE_BYPASS=1` against the actual `materialize_generated_package` output, proved both explicit `/name` invocation and model-auto-invocation-blocked (marker technique carried over from ADR-028) |

Claude is the only harness whose package coverage computation
(`claude_exact_coverage`) actually intersects the manifest's declared
`skills`/`mcpServers` against what UZE separately discovered, and gates
each Skill on its canonical invocation policy being preserved — see
[Package coverage](#package-coverage).

## Delivery

```
Store plugin (.claude-plugin/plugin.json present)          [Explicit Native Package]
        │
        ▼
store/.claude-plugin/marketplace.json   (derived, republish_packages)
        │
        ▼
claude plugin marketplace add  (once)  →  claude plugin install <id>@uze-local
        │
        ▼
Claude-owned cache (~/.claude/plugins/cache/...)
        │
        ▼
one IntegrationOwned{kind:"claude-plugin"} receipt
        │
        ▼
Skill/MCP resources declared in the manifest: VIA_PACKAGE (no second receipt)
Skill/MCP resources NOT declared: fall through to the paths below, unchanged
```

```
Store plugin (no explicit envelope, but skills/ dir and/or mcp.json present)  [Generated Native Package, ADR-020]
        │
        ▼
$UZE_HOME/state/attachments/claude/generated/<id>/.claude-plugin/plugin.json
   (UZE-synthesized: name/version/description from canonical plugin.json,
    skills symlinked from the Store, mcp.json's mcpServers copied verbatim)
        │
        ▼
$UZE_HOME/.../generated/.claude-plugin/marketplace.json   ("uze-store")
        │
        ▼
claude plugin marketplace add (once) → claude plugin install <id>@uze-store
        │
        ▼
one IntegrationOwned{kind:"claude-plugin-generated", detail.origin:"generated"} receipt
        │
        ▼
provided = discovered ∩ (conventional skills/ ∪ mcp.json's mcpServers) — same exact-coverage discipline as explicit
```

```
Store plugin (no envelope of either kind, or resource undeclared by one)
        │
        ├── Skill → managed shim (.claude-plugin/plugin.json + SKILL.md
        │            symlink) at $UZE_HOME state dir → symlinked once into
        │            <claude_home>/skills/<name>/         [NATIVE_CAPABILITY]
        │            (pre-setup fallback: --plugin-dir conformance probe)
        │
        └── MCP   → claude mcp add --scope user --transport stdio
                     (writes ~/.claude.json's mcpServers)   [SAFE_ADAPTATION]
                     (pre-setup: Unsupported, no fallback — ADR-007)
```

## Native package

`package_exposure_plan` requires `.claude-plugin/plugin.json` to exist, then
calls `claude_exact_coverage` (`plugin.rs`) to compute the actual
intersection between the manifest's declared `skills: [...]`/`mcpServers: {}`
and the resources UZE's engine separately discovered in the same package —
not "all resources," an honest set intersection. Undeclared resources are not
marked `provided`, so they continue through normal `exposure_plan` fallback
individually (Skill shim or MCP config entry) rather than silently
disappearing. This is `ADR-013 §2`'s requirement implemented as written, and
is the one integration in this crate that verifiably does so (other
integrations' coverage may also mark "all discovered resources" as provided
the moment their respective manifest file exists, without reading its
contents — see their READMEs).

Path handling in `claude_exact_coverage` rejects `..`/absolute/empty
declarations, deduplicates repeats, and tolerates a malformed manifest by
returning empty coverage (the plugin still installs, just with
`provided_resource_identities` empty — nothing silently claimed). A Skill
additionally requires (ADR-030) that its canonical `invoke:` policy is
actually preserved by the bytes the author shipped before it counts as
covered — `model: false` needs the author's own
`disable-model-invocation: true`, `user: false` needs `user-invocable:
false`, and the invalid combination is never covered. A path match alone
is not proof the harness won't auto-invoke it, and UZE never rewrites
explicit-envelope content. All of this is unit-tested
(`claude::plugin::claude_native_coverage_tests`).

The marketplace/install/list/uninstall CLI-shelling functions themselves
(`claude_marketplace_exists`, `run_claude_marketplace_add`,
`claude_plugin_installed`, `attach_package`, `inspect_claude_plugin`,
`remove_claude_plugin`) are **not** unit-tested — they shell out to the
resolved `claude` executable directly (via `provisioning_executable()`,
never a bare `Command::new("claude")` — see Runtime shim boundary below)
rather than through the crate's injectable `ProcessRunner` trait, so only a
real `claude` binary (or an opt-in conformance suite outside this crate)
exercises them today.

**Generated Native Package** (`claude/generate.rs`, ADR-020): when no
explicit envelope exists, `generatable()` checks for a conventional
`skills/` directory and/or root `mcp.json`; `generated_exact_coverage()`
computes the same discovered-∩-declared intersection structurally, against
those conventions rather than a re-parsed manifest — generation and
coverage agree by construction, since the same module writes both.
Eligibility is capability-based, not resource-count-based: a single Skill
or a single MCP server alone already qualifies (ADR-020). Generation is
read-only inside `package_exposure_plan`; `materialize_generated_package`
(called only from `attach_package`) rebuilds the derived envelope
wholesale on every call — deterministic, idempotent, never touching the
Store package. An explicit envelope, even malformed, always wins; presence
alone (not validity) decides the branch.

Default-policy `skills/` entries are still whole-directory symlinks to the
Store (byte-preserving). A Skill with a non-default `invoke:` policy
(ADR-030) is materialized as one real, UZE-owned SKILL.md per Skill
instead: it carries the canonical `name`/`description` (description
re-quoted as a safely escaped YAML double-quoted scalar — never
raw-interpolated, so no description content can forge or duplicate a
frontmatter key) plus the injected markers for whichever half of the
policy Claude needs (`disable-model-invocation: true`, `user-invocable:
false`), then the canonical body verbatim — with every other file in the
canonical skill directory still referenced. The invalid policy
(`model: false, user: false`) is never materialized nor claimed.

## Fallbacks

- **Skill, setup incomplete:** `ExposureMechanism::RuntimeBridge` — the
  original ADR-005 `--plugin-dir` conformance probe. Still live code
  (`skill_exposure_plan`'s `else` branch), not dead: it's the correct
  behavior before `uze setup claude` has run.
- **MCP, setup incomplete:** no fallback exists by design — reports
  `Unsupported` (ADR-007: MCP has no per-session probe the way Skills do).

## Runtime

`claude/runtime.rs` is Claude's unique mechanism: `claude_runtime_projection`
builds `$UZE_HOME/runtime/claude-code/projects/<id>/CLAUDE.md` (a single
`@<AGENTS.md path>` import line, content-compared before writing —
idempotent), and `runtime_contribution` turns that into `--add-dir <dir>` +
`CLAUDE_CODE_ADDITIONAL_DIRECTORIES_CLAUDE_MD=1`. Reached through
`src/shim.rs`'s `argv[0]` dispatch, entirely outside this crate.
Fail-open by construction (`HarnessRuntimeContribution` has no `Err` variant
— a blocked runtime dir degrades to passthrough with a stderr note, verified
by `unwritable_runtime_dir_falls_open_to_passthrough_with_a_note`). No other
integration in this crate overrides `runtime_contribution` or
`supports_runtime_integration` (both default to passthrough/`false`).

This is independent of Native Projection (ADR-013): it delivers
project-context, not package/capability content, and there is no higher-tier
option to fall back from for an externally-scoped `AGENTS.md` — see ADR-014's
"Relationship to Native Plugin Projection". A separate, older mechanism (the
persistent `CLAUDE.md` bridge via `text_region`) also exists, owned by
`crates/uze-application`, not this crate — which of the two is authoritative
long-term is explicitly undecided (ADR-014 Consequences).

## Lifecycle

| Receipt | Inspect | Detach | Drift-safe |
|---|---|---|---|
| `IntegrationOwned{kind:"claude-plugin"}` (explicit) | `inspect_claude_plugin` — `claude plugin marketplace list --json` + `plugin list --json`, checks marketplace root + installed + enabled | `claude plugin uninstall <selector>` | Yes — MATCHED only when marketplace root, installed, and enabled all agree |
| `IntegrationOwned{kind:"claude-plugin-generated"}` (generated) | Same `inspect_claude_plugin` (marketplace-root-agnostic) | Same `remove_claude_plugin`, plus `remove_generated_package_by_id` (Derived Artifact, safe to delete unconditionally) | Yes — identical inspection path to explicit |
| `SymlinkReference` (Skill shim) | standard receipt inspection (`inspect_standard_receipt`) | standard detach + `cleanup_unused_shim` GC if the shim is now unreferenced | Yes |
| `VendorConfigEntry` (MCP) | `inspect_claude_mcp` — read-only `~/.claude.json` parse, exact command+args match | `claude mcp remove <name>` | Yes — Blocked (not silently accepted) if the receipt requests cwd/env/enabled state this integration can't verify |

One thing worth a second look, not necessarily a bug: `inspect_claude_plugin`
computes `package_root` from the receipt but only uses it to select which
plugin.json fields to read — it never compares `package_root` against
anything in Claude's own JSON response. The code comment states existence +
enabled + marketplace-root match is "sufficient for Matched" since the cache
is a Derived Artifact, not a second source of truth. Plausible, but it means
a plugin re-pointed at a *different* package under the *same* marketplace
selector would not be independently caught by a `package_root` mismatch —
only by the marketplace-root or install-state checks. Worth confirming this
reasoning holds, not confirmed by a dedicated test.

## Limitations

- Native-plugin CLI-shelling functions (install/list/uninstall/inspect) have
  no unit test coverage — only reachable via a real `claude` binary.
- MCP behavioral verification (an actual tool call) is not closed; a headless
  discovery quirk was characterized, not fixed (ADR-007).
- `/compact` retention of runtime-projected context is unverified (ADR-014).
- The persistent-bridge-vs-runtime-projection question for context delivery
  is explicitly undecided.

## Evidence

- Tests: 50/50 passing in `claude::{lifecycle_tests, plugin::claude_native_coverage_tests, runtime::runtime_projection_tests, generate::generated_native_tests}`.
- Real harness version last validated: Claude Code **2.1.241** — the exact
  binary present in this environment (`claude --version` reconfirmed live
  during the ADR-028 audit). The `disable-model-invocation` marker
  technique was proven against a real model turn on this same binary, both
  for a hand-built probe plugin and for the actual output of
  `materialize_generated_package` against a real UZE package; ADR-030 now
  applies the same technique to canonical user-only Skills.
- Source: `docs/adr/{006,007,009,013,014,020,028,030}-*.md`.

## Next

1. Add unit coverage for the native-plugin CLI functions (`inspect_claude_plugin`, `attach_package`) via a fake/injectable process boundary, matching how `provision_cli` already uses `ProcessRunner`. (An `executable_override` escape hatch was added and then removed this milestone for the generated-native pass — it ended up with zero call sites once PATH-based test isolation solved the actual failing tests more directly; worth reconsidering if this item is picked up for real.)
2. Close the MCP headless-discovery/behavioral gap from ADR-007's last entry.
3. Verify or refute the `package_root`-not-compared observation above with a dedicated drift test.
4. Resolve persistent-bridge vs. runtime-projection precedence for Claude context delivery (ADR-014 Future Work).
