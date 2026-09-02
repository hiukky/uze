## Context

See proposal.md — Why. Constraints the design works within:

- No harness offers a channel for system dependencies (Claude: plugin-to-plugin `dependencies` and `package.json` only; OpenCode: npm; Codex, Antigravity: none), and a missing dependency at hook time is a non-blocking error on all three command-hook harnesses — the tool runs.
- UZE already provisions the harnesses through their official installers with the person's confirmation (`uze setup`, `provision-harnesses-through-official-sources`); the process-runner, PATH and detection facilities live in `uze-core::machine`.
- `uze-core` names no vendor; installer knowledge is machine knowledge (package managers), not harness knowledge.
- Managed filesystem effects are receipt-owned; drift blocks destructive action; removal inspects before detaching.
- `native-first-hooks` makes the delivered `sh` wrapper depend on `jq`, so the first packager-introduced requirement exists the moment that change lands.

## Goals / Non-Goals

**Goals:**
- A plugin author states what their scripts need once; the machine either has it, gets it with the person's consent, or the gap is visible everywhere it matters.
- The packager is held to the same rule for what it generates.
- Nothing runs on the machine without a confirmation that names what and how.

**Non-Goals:**
- Language-package dependencies (npm, pip): the harness or the author's tooling.
- Exact version pinning or lockfiles for tools; a minimum constraint is enough.
- Removing tools on uninstall.
- Sandboxing or vendoring the tools themselves.

## Decisions

### D1 — `requirements` is a manifest field, executables only
`plugin.json` gains `requirements: [{ executable, version?, purpose? }]`. Runtimes are executables (`python3`, `node`). Alternatives: a separate `requirements.json` (one more file for one list); free-text install instructions (unverifiable). Executable + minimum version is what `command -v` and `--version` can check on every platform.

### D2 — Effective set = declared + packager-introduced
Each integration contributes the requirements of the artifacts it generates (the `sh` wrapper contributes `jq`), attributed to the artifact so the report says who needs it. The author never declares a packager dependency. Alternative: bake `jq` as a global UZE requirement — wrong scope (only packages with hooks on `sh` need it) and invisible in the report.

### D3 — Detect in core, install through the machine's own installers
Detection is `PATH` lookup plus a `--version` probe with a small per-executable table for the version flag/format. Installation picks an installer from what the machine has, in a fixed preference (a user-level manager already in use such as `mise`; then `brew`; then the system manager `apt`/`dnf`/`pacman`/`apk`; `winget` on Windows), and maps executable → package name per installer in a table owned by `uze-core::machine`. Alternative: download binaries ourselves — a second, unauditable distribution channel.

### D4 — Confirmation is a lifecycle step with a read model
Install/update produce a `RequirementPlan` read model (missing, too old, installer, command line). CLI renders it and asks `y/N` (`--yes` skips; non-interactive without `--yes` declines); TUI renders the same plan as a view and confirms once. The action that runs installers is separate from the one that attaches capabilities, so declining never blocks delivery.

### D5 — Receipts distinguish installed from observed
A tool UZE installed gets a receipt (executable, installer, package that required it); a tool found already present is recorded as observed. Removal drops records, never tools. `doctor` reads both to report drift and orphans.

### D6 — Unmet is a state, not an error
A package with unmet requirements installs; the gap is carried on the read models (`plugin list`, `doctor`) and delivered artifacts keep their own rules (the hook wrapper's fail-closed/fail-open). Alternative: refuse the install — punishes the person for a tool they may install a minute later and hides the rest of the package.

## Risks / Trade-offs

- [Installer tables drift across distros] → small table, one row per executable per installer, covered by tests that run the detection (not the install) on CI images; manual fallback message when no row matches.
- [Running package managers needs privileges (`sudo`)] → the plan shows the exact command; UZE runs it as the person would and surfaces the failure; never escalates on its own.
- [Version probing is per-tool folklore] → constraint optional; unknown format reports "present, version unknown" rather than failing.
- [Confirmation fatigue] → one plan per install, grouped; `--yes` for automation; already-met requirements never prompt.

## Migration Plan

1. Manifest field + effective-set derivation + detection (no installs yet): `plugin list`/`doctor` start reporting.
2. Plan + confirmation + installers behind CLI/TUI.
3. `native-first-hooks` wrapper contributes `jq`.
Rollback: the field is optional and detection is read-only; disabling the install step leaves reporting intact.
