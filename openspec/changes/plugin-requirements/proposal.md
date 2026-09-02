## Why

A plugin's scripts — hook handlers above all — depend on tools the plugin cannot declare and no harness checks: Claude Code's own hook examples assume `jq` ("install `jq` and make sure it is on your `PATH`"), and a hook whose dependency is missing exits 127, which every command-hook harness treats as a non-blocking error — the tool runs, the guard silently never did. None of the four harnesses lets a plugin declare a system dependency (Claude installs only plugin-to-plugin `dependencies` and a plugin's own `package.json`; OpenCode resolves npm packages; Codex and Antigravity declare nothing). With `native-first-hooks` the delivered wrapper itself needs `jq`, so UZE now ships a dependency of its own into the user's machine. UZE's job here is to make the gap visible and the fix one command away — never to run installers on the person's machine.

## What Changes

- **`requirements` in the canonical plugin manifest.** A package may declare the executables it needs on `PATH` (name, optional version constraint, optional purpose), and runtimes as executables (`python3`, `node`). Declarations are validated at install; unknown or malformed entries are rejected before any attachment, never ignored.
- **Requirements the packager introduces are declared by the packager.** A generated artifact that brings its own dependency (the `sh` hook wrapper's `jq`) contributes that requirement to the package's effective set, so the author declares only what the author's scripts use.
- **Detect → explain → hand the command to the person.** At install (and on `uze doctor`), UZE checks each effective requirement against the machine. For each missing or too-old executable it shows what is needed, why, and the exact command to install it, suggested from what the machine offers (system package manager, `brew`, `winget`, a user-level manager such as `mise` when present). UZE never runs that command: the person does, in their own shell, with their own privileges. In the TUI the same gap appears as an issue on the package (manage view and doctor); acting on it opens a shell tab with the command pre-filled, still for the person to run.
- **Unmet is a supported state.** A package whose requirements are unmet still installs; the affected capabilities are delivered with the requirement reported unmet (a hook wrapper keeps its fail-closed/fail-open rule), and `uze plugin list`, the TUI manage view and `uze doctor` show the gap with the command that closes it until the person does.
- **`uze doctor` re-verifies** every effective requirement of every installed package and reports drift (a tool removed or downgraded since install), with the same command.
- **UZE owns no tool.** It installs nothing, records nothing about tools it did not install, and removing a package removes only the package's requirement records.

Out of scope: running any installer (by design — the person does, in their own shell), language-package dependencies inside a plugin (npm, pip — the harness or the author's own tooling handles them), and version pinning beyond a minimum constraint.

## Capabilities

### New Capabilities

- `plugin-requirements`: declaring executables a package needs, deriving the effective set including packager-introduced ones, detecting them on the machine, showing the person the exact install command for what is missing, and reporting unmet or drifted requirements in the CLI, the TUI and doctor.

### Modified Capabilities

- `plugin`: install validates requirements and reports the missing ones with their install command; a package with unmet requirements installs with the gap reported; list shows requirement status.
- `doctor`: reports unmet and drifted requirements per package and the command that closes each gap.

## Impact

- `crates/uze-core`: manifest schema (`requirements` in `plugin.json`), requirement model and effective-set derivation, detection through the existing PATH/subprocess facilities, install-command suggestion table per package manager; no vendor names, no installer execution.
- `crates/uze-integrations`: integrations contribute the requirements of what they generate (the `sh` wrapper contributes `jq`).
- `crates/uze-application`: install/update/doctor lifecycle gains the requirement report as a read model (missing, too old, suggested command); no new action that runs processes.
- CLI (`src/main.rs`): report after install, `plugin list`/`doctor` output. TUI (`src/ui`): the gap as an issue on the package in the manage view; "open in shell" hands the command to a terminal tab through the terminal runtime; `command_performance.rs` classification for any new leaf command.
- Fixtures, `agent-plugins.org` schema extension documented, `docs/capabilities/portable-hooks.md` (the `jq` note points here), conformance Lab: a fixture declaring a requirement, one vertical proving the unmet-requirement report and the fail-closed rule with the tool absent.
