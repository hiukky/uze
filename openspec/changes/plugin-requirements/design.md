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
- UZE never runs an installer: it names what is missing, why, and the command; the person runs it with their own privileges, in their own shell.

**Non-Goals:**
- Language-package dependencies (npm, pip): the harness or the author's tooling.
- Exact version pinning or lockfiles for tools; a minimum constraint is enough.
- Running installers, elevating privileges, or owning tools (records of what UZE installed do not exist because UZE installs nothing).
- Sandboxing or vendoring the tools themselves.

## Decisions

### D1 — `requirements` is a manifest field, executables only
`plugin.json` gains `requirements: [{ executable, version?, purpose? }]`. Runtimes are executables (`python3`, `node`). Alternatives: a separate `requirements.json` (one more file for one list); free-text install instructions (unverifiable). Executable + minimum version is what `command -v` and `--version` can check on every platform.

### D2 — Effective set = declared + packager-introduced
Each integration contributes the requirements of the artifacts it generates (the `sh` wrapper contributes `jq`), attributed to the artifact so the report says who needs it. The author never declares a packager dependency. Alternative: bake `jq` as a global UZE requirement — wrong scope (only packages with hooks on `sh` need it) and invisible in the report.

### D3 — Detect in core, suggest the command, never run it
Detection is `PATH` lookup plus a `--version` probe with a small per-executable table for the version flag/format. The suggestion picks a package manager from what the machine has (a user-level manager already in use such as `mise`; then `brew`; then the system manager `apt`/`dnf`/`pacman`/`apk`; `winget` on Windows) and renders the exact command from an executable → package-name table owned by `uze-core::machine`, with `sudo` where the manager needs it. UZE does not execute it. Alternatives: running it with confirmation (puts UZE in the credential and sandbox path — privileges, `sudo` prompts, CI semantics, receipts for tools — for a command the person can paste); downloading binaries ourselves (a second, unauditable distribution channel).

### D4 — The gap is a read model, surfaced in three places
Install/update/doctor produce a requirement report (met / too old / missing, purpose, suggested command). CLI prints it after install and in `plugin list`/`doctor`; the TUI shows it as an issue on the package in the manage view. No new action runs processes.

### D5 — The TUI hands the command to a shell tab
Acting on the issue in the TUI opens a terminal tab (the terminal runtime already exists) with the command pre-filled, not executed. The person runs it, closes the tab, and the requirement is re-checked. This keeps the person's shell, PATH and privileges in charge, and gives the TUI the same one-step fix the CLI gives by printing the command.

### D6 — Unmet is a state, not an error
A package with unmet requirements installs; the gap is carried on the read models and delivered artifacts keep their own rules (the hook wrapper's fail-closed/fail-open). Alternative: refuse the install — punishes the person for a tool they may install a minute later and hides the rest of the package.

## Risks / Trade-offs

- [Suggestion tables drift across distros] → small table, one row per executable per package manager; suggestions are text, so a wrong row costs a failed paste, not a broken machine; manual fallback message when no row matches.
- [The person may never run the command] → the gap stays visible in three places and the hook wrapper's fail-closed rule protects the deny case; that is the accepted trade-off of not running installers.
- [Version probing is per-tool folklore] → constraint optional; unknown format reports "present, version unknown" rather than failing.
- [Report noise] → met requirements are silent; only gaps are reported, grouped per package.

## Migration Plan

1. Manifest field + effective-set derivation + detection: `plugin list`/`doctor`/install report gaps with the command.
2. TUI issue on the package and the shell-tab handoff.
3. `native-first-hooks` wrapper contributes `jq`.
Rollback: the field is optional and everything is read-only; the report can be hidden without touching the machine.
