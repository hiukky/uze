# Antigravity CLI ↔ Gemini CLI Compatibility Map

**Phase 1 read-only audit** — produced before production code changed, from
current official Antigravity docs (`antigravity.google`, CLI v1.1.17 docs
snapshot) and empirical evidence collected against real `agy` **1.1.19** in
an isolated `$HOME` (never the developer's real config). Every "PROVEN"
row is a behavior observed directly; "DOCUMENTED" is official-docs
evidence with no headless probe possible; "UNKNOWN" is a genuine gap.

> **Historical record.** This document is the evidence base for ADR-027
> (Antigravity is the Google-family v0 harness). The migration's final
> outcome went further than this audit anticipated: the Gemini CLI
> integration was **removed** from the codebase after parity was proven —
> no legacy code path remains. The Gemini-side facts below are retained
> only as the audit's record.

Key baseline facts:

- `agy --version` → `1.1.19` (bare token). Docs site shows v1.1.17; the
  binary self-updates and reports its own version.
- Official install: `curl -fsSL https://antigravity.google/cli/install.sh |
  bash` → `~/.local/bin/agy` (Linux/macOS); PowerShell `irm ... install.ps1
  | iex` → `%LOCALAPPDATA%\agy\bin` (Windows). The docs mention
  `--skip-aliases`/`--skip-path` flags, but the **current live script
  rejects them** ("Unknown parameter") and accepts only `-d/--dir` — the
  plain installer therefore *does* append PATH exports to the user's shell
  profiles (observed), and no supported flag suppresses that in this
  version. `agy update` is the update verb; the CLI self-updates in the
  background during normal runs.
- `agy` honors `$HOME`: with `HOME=/tmp/...` every artifact landed under
  `$HOME/.gemini/`.

## Area-by-area classification

| Area | Gemini CLI 0.56 (UZE today) | Antigravity CLI 1.1.19 | Classification |
|---|---|---|---|
| Executable | `gemini` (`npm` package) | `agy` (native Go binary, installer script) | **DIFFERENT** |
| Provisioning | `npm install -g @google/gemini-cli@latest` | `curl …/install.sh \| bash`; update `agy update` | **REUSABLE_WITH_PATH_CHANGE** (shared `provision_cli` shape; install/update specs and verify probe differ) |
| Version detection | `gemini --version` → bare token | `agy --version` → bare token | **IDENTICAL** (same probe shape, new name) |
| Config root | `~/.gemini/` (settings, extensions, commands, skills, agents) | `~/.gemini/` shared, but reorganized: settings `~/.gemini/antigravity-cli/settings.json`; customizations `~/.gemini/config/*`; staged plugins `~/.gemini/config/plugins/<name>/` | **REUSABLE_WITH_PATH_CHANGE** (same HOME-derived root, different subpaths) |
| Install mechanism | `gemini extensions link <dir> --consent` (link, no copy, no catalogue) | `agy plugin install <dir>` — stages a **byte copy** at `~/.gemini/config/plugins/<name>/` and registers it in `~/.gemini/config/import_manifest.json`; **no link verb exists** (symlinks are dereferenced: PROVEN) | **DIFFERENT** (is the single biggest architectural divergence) |
| Uninstall | `gemini extensions uninstall <name>` | `agy plugin uninstall <name>` (removes staged copy + registration; PROVEN) | **REUSABLE_WITH_PATH_CHANGE** |
| List / inspect | `gemini extensions list --output-format=json` (JSON on **stderr** in 0.56) | `agy plugin list` (JSON on **stdout**, `{"imports":[{name,source,importedAt,components}]}`; PROVEN) | **REUSABLE_WITH_FORMAT_CHANGE** |
| Enable/disable | `gemini extensions disable/enable`; `isActive` unreliable (stays true after disable — PROVEN 0.56) | `agy plugin disable/enable <name>`; no enablement field in the list output; enablement lives in user config (PROVEN absence) | **REUSABLE_WITH_PATH_CHANGE** (neither is part of ownership proof) |
| Re-install semantics | link is idempotent (no copy) | install **merges**: stale files from a previous version of the same-named plugin survive (PROVEN — `format-tests.md` lingered after reinstalling a smaller `flow`) | **DIFFERENT** — UZE must treat the staged tree as rebuildable and wipe/reinstall as its derived-artifact discipline |
| Package marker | `gemini-extension.json` (author-provided, optional) | `plugin.json` at plugin root — **the canonical UZE manifest itself satisfies it** (`name` pattern `^[a-zA-Z0-9-_]+$`, `description` optional, extra fields tolerated: PROVEN) | **IDENTICAL_AND_SIMPLER** — canonical package is a valid Antigravity plugin with zero vendor files |
| Package resources | `skills/`, `commands/*.toml`, `mcpServers` inline | `skills/` (dir), `commands/` (**`.md` files, converted to Skills at load** — PROVEN `commands: N processed (converted to skills)`), `mcp_config.json`, `agents/`, `hooks.json`, `rules/` | **REUSABLE_WITH_FORMAT_CHANGE** (skills/ identical; commands natively converted — no `.toml` generation; MCP file renamed + schema changed) |
| Skills (workspace) | `.gemini/skills/<name>/SKILL.md` | `.agents/skills/<name>/SKILL.md` (also `_agents/`) | **REUSABLE_WITH_PATH_CHANGE** (docs; workspace-scoped — not UZE's machine-scope concern) |
| Skills (global) | `~/.gemini/skills/` | `~/.gemini/antigravity-cli/skills/` (CLI docs; binary's builtin skills sit beside it under `antigravity-cli/builtin/skills/`) | **REUSABLE_WITH_PATH_CHANGE** (DOCUMENTED — no headless listing verb exists; TUI `/skills` only) |
| Skills model | model-discoverable + slash-invocable | model-discoverable (progressive disclosure) + **auto slash-command**; no explicit-only control documented | **REUSABLE_WITH_PATH_CHANGE** for skills; **DIFFERENT** for the Command-vs-Skill distinction (see Commands) |
| Commands | native custom commands: user-scope `~/.gemini/commands/*.toml` + extension commands; explicit-only by construction | **no custom-command primitive**: legacy commands convert to Skills (PROVEN via `agy plugin import gemini`: `review.toml` → `skills/review/SKILL.md`); Skills are model-discoverable | **DIFFERENT** — commands are now Skills; explicit-only property degrades (no observable policy: UNKNOWN whether any hidden field exists; none documented) |
| MCP config | inline in `~/.gemini/settings.json` `mcpServers` | dedicated `~/.gemini/config/mcp_config.json` (global) and `.agents/mcp_config.json` (workspace); **stdio `command`/`args`/`env`/`cwd`; remote `serverUrl`** (`url`/`httpUrl` NOT supported — docs + verified add/list) | **REUSABLE_WITH_FORMAT_CHANGE** |
| MCP CLI | `gemini mcp add --scope user --transport stdio`; `gemini mcp list` human-readable | `agy mcp add <name> <command> [args…]` (add-or-update; flags before name — PROVEN error); `agy mcp remove`, `disable`, `enable`; `agy mcp list` human-readable only | **REUSABLE_WITH_FORMAT_CHANGE** (same shape, different invocation args; config file read for inspection) |
| MCP schema keys | `command`, `args`, `env` | `command`, `args`, `env`, `cwd` (`"cwd": ""`/`"env": null` written by the import verb), `disabled` (bool, written on add/disable — PROVEN), remote `serverUrl`, `headers`, `authProviderType`, `oauth`, `disabledTools` | **REUSABLE_WITH_FORMAT_CHANGE** — the `disabled` key is new and must be accepted as a user preference in inspection (non-bool = Blocked) |
| Native package model | Extension | **Plugin** (namespaced bundles: skills, subagents, rules, MCP, hooks) | **DIFFERENT term/format — preserve vendor terminology: Plugin, never Extension** |
| Agents/Subagents | `agents/` `.md` files (static) | plugin `agents/agent.json` (dynamic on-the-fly subagents; format differs — legacy `.md` migration advised) | **DIFFERENT / NOT_SUPPORTED by UZE today** (documented as future surface) |
| Hooks | `hooks` via settings.json | plugin `hooks.json` / settings | **NOT_SUPPORTED by UZE today** (documented as future surface) |
| Context (workspace) | `GEMINI.md` (+ `AGENTS.md` support in later versions) | **`AGENTS.md` and `GEMINI.md` both parsed** — official migration: "Both CLI platforms utilize identical workspace context rules. No modifications are needed" | **IDENTICAL** — context route is **Native**; UZE generates **no** `@AGENTS.md` bridge for Antigravity (simplification confirmed) |
| Context (global) | `~/.gemini/GEMINI.md` | `~/.gemini/GEMINI.md` (global rules) | **IDENTICAL** |
| Runtime shim | none (passthrough) | none needed (passthrough; no documented UZE-relevant runtime projection) | **IDENTICAL** (no shim) |
| Marketplace/catalogue | none (link-direct) | none required for local install (`plugin install <dir>`; `plugin@marketplace` and `plugin link <mp> <target>` exist for remote distribution) | **IDENTICAL for UZE's needs** — `republish_packages` stays default no-op; publication remains optional |
| Workspace plugin discovery | n/a (extensions only) | `.agents/plugins/` and `_agents/plugins/` scanned; `~/.gemini/config/plugins/` global — BUT `agy plugin list` shows **only registered imports** (a plugin placed manually is not listed — PROVEN) | **DIFFERENT** — registration is via the vendor import manifest; UZE uses the vendor verbs, never hand-writes it |
| Path containment | link points wherever it points | `%s isn't relative to %s` and `path is already tracked` strings exist in the binary (marketplace/link paths) — local direct `install <abs path>` works (PROVEN) | **REUSABLE_WITH_PATH_CHANGE** (no containment issue for direct install) |
| Workspace MCP headless visibility | `gemini mcp list` user-scope only | `agy mcp list` showed **global only**; workspace `.agents/mcp_config.json` not visible headlessly (PROVEN — docs say it is loaded in agent sessions) | **UNKNOWN for workspace load semantics** |

## Statements that were checked and found false / misleading

- Docs page "Plugins & Skills" states installs are staged at
  `~/.gemini/antigravity-cli/plugins/<plugin_name>/`. **Actual v1.1.19
  behavior: `~/.gemini/config/plugins/<name>/`** (PROVEN by `find`; the
  import manifest also lives under `~/.gemini/config/`). UZE trusts the
  binary.
- Official inventory of CLI commands lists no `plugin list` JSON output —
  actual output is machine-readable JSON on stdout (PROVEN), which makes
  UZE's inspection cheap and reliable.
- Antigravity 2.0 docs say global skills live at
  `~/.gemini/config/skills/<folder>/`; the CLI docs and the CLI's own
  builtin layout say `~/.gemini/antigravity-cli/skills/`. UZE uses the
  **CLI-specific** path; the discrepancy is documented, not resolved by
  guessing (headless skill listing is impossible without a model).

## Gemini → Antigravity module map (classification of each reused piece)

| Gemini module | Antigravity equivalent | Disposition |
|---|---|---|
| `gemini/provision.rs` | `antigravity/provision.rs` | **REUSABLE_WITH_PATH_CHANGE** — same `--version` probe; install/update specs pass through shared `provision_cli` |
| `gemini/extension.rs` (link verb, listing, coverage, name-from-manifest) | `antigravity/plugin.rs` | **MOVE_TO_GENERIC_HELPER** where identical (run_quiet/capture, CLI-safe token, component-wise path checks); **KEEP_SEPARATE** for everything vendor (`plugin install` copies, JSON shape, fingerprint ownership, name pattern) |
| `gemini/generate.rs` (generated extension) | `antigravity/generate.rs` | **SHARE_HELPER** (derived-dir layout, symlink-following materialization, wipe-and-rebuild, remove-by-id); **REUSABLE_WITH_FORMAT_CHANGE** (manifest shape + `mcp_config.json` translation) |
| `gemini/skills.rs` | `antigravity/skills.rs` | **MOVE_TO_GENERIC_INTEGRATION_HELPER** candidate — the module is ~50 lines of identical shape; left duplicated for now per the crate's stated scope discipline (proven-by-comparison only) — a follow-up can hoist it |
| `gemini/commands.rs` (TOML generation) | `antigravity/commands.rs` (SKILL.md generation) | **KEEP_SEPARATE** — different physical primitive (vendor conversion vs generated TOML), different route classification (Native vs Adapted), different naming (nested `.toml` path vs verbatim label) |
| `gemini/mcp.rs` | `antigravity/mcp.rs` | **KEEP_SEPARATE** — different verb args (`--scope user --transport stdio` vs positional), different config file and schema (`settings.json` vs `mcp_config.json`, `disabled` key, `serverUrl`); the inspection value-comparison logic is structurally identical and could be hoisted later |

## STOP-condition verdicts (from the migration brief)

1. Plugin docs/behavior validated — **YES** (official docs + real 1.1.19).
2. Native Plugin lifecycle inspectable — **YES** (`agy plugin list` JSON +
   staged-dir fingerprint).
3. Exact capability coverage determinable — **YES** (structural surfaces +
   declared `mcp_config.json`; pure functions, tested).
4. Plugin install/link requires authoritative duplication — **PARTIAL**:
   the vendor verb copies bytes (no link); UZE keeps the Store authoritative
   and treats the staged tree as a rebuildable Derived Artifact (fingerprint
   ownership, wholesale rebuild via uninstall+install, never reads from the
   copy). Reported honestly as a vendor-imposed copy, mitigated — not
   eliminated.
5. AGENTS.md behavior contradicts migration docs — **NO** (docs confirm
   native `AGENTS.md`; context route Native).
6. Non-default invocation policies preserve canonical semantics — **NO,
   deliberately**: Antigravity has no explicit-only mechanism and no
   user-catalog suppression (skills are model-discoverable and
   slash-invocable), so a user-only or model-only canonical Skill is
   **Adapted**, not Native — the one semantic loss, documented and
   declared in the plan evidence (ADR-030).
7. MCP lifecycle manageable — **YES** (verified verbs + JSON config).
8. Store vendor-specific changes — **NO**.
9. Core learns Antigravity concepts — **NO** (zero `uze-core` changes).
10. IntegrationPort major redesign — **NO**.
11. UZE depends on Gemini compatibility tooling — **NO** (`agy plugin import
    gemini` is evidence/reference only; UZE projects directly).
12. Real `agy` contradicts docs materially — **PARTIAL**: the staged-plugin
    path differs from the docs page (config/ vs antigravity-cli/); resolved
    by trusting the binary and documenting the discrepancy.

## Evidence log (isolated HOME, `agy` 1.1.19)

```
agy --version                                           → 1.1.19
agy plugin --help                                       → list/import/install/uninstall/enable/disable/validate/link
agy plugin install /tmp/…/flow                          → staged ~/.gemini/config/plugins/flow/* (byte copy; diff -r IDENTICAL)
agy plugin install (symlinked source)                   → dereferenced (real copy in config/plugins)
agy plugin install (same name, smaller content)         → stale files survive (merge, no cleanup)
agy plugin list                                         → {"imports":[{name,source,importedAt,components}]} on stdout
agy plugin validate <dir>                               → skills/agents/commands(converted)/mcpServers processed
agy plugin uninstall flow                               → staged dir + registration removed
agy plugin import gemini (fake ~/.gemini)               → extension → plugin (plugin.json, skills, converted SKILL.md, mcp_config.json, import manifest source "gemini-cli")
agy mcp add flow-a node /x.js                          → ~/.gemini/config/mcp_config.json {"mcpServers":{…,"disabled":false}}
agy mcp disable/enable                                 → disabled:true / key removed (command/args preserved)
agy mcp add --type http <name> <url>                   → {"serverUrl": …} (flags must precede name)
agy mcp list                                           → human-readable table; plugin MCPs NOT listed; workspace .agents/mcp_config.json NOT listed
commands/*.md in a plugin                              → "commands : 1 processed (converted to skills)"
skill name with colon (flow:commit)                    → validate OK, install OK
Bad plugin name ("Bad Name!")                          → validate OK, install ERROR invalid plugin name
```
