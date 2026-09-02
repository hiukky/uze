## Why

A plugin's scripts — hook handlers above all — depend on tools the plugin cannot declare and no harness checks: Claude Code's own hook examples assume `jq` ("install `jq` and make sure it is on your `PATH`"), and a hook whose dependency is missing exits 127, which every command-hook harness treats as a non-blocking error — the tool runs, the guard silently never did. None of the four harnesses lets a plugin declare a system dependency (Claude installs only plugin-to-plugin `dependencies` and a plugin's own `package.json`; OpenCode resolves npm packages; Codex and Antigravity declare nothing). With `native-first-hooks` the delivered wrapper itself needs `jq`, so UZE now ships a dependency of its own into the user's machine. UZE already provisions the harnesses through their official installers with the user's confirmation; the same discipline is missing one level down.

## What Changes

- **`requirements` in the canonical plugin manifest.** A package may declare the executables it needs on `PATH` (name, optional version constraint, optional purpose), and runtimes as executables (`python3`, `node`). Declarations are validated at install; unknown or malformed entries are rejected before any attachment, never ignored.
- **Requirements the packager introduces are declared by the packager.** A generated artifact that brings its own dependency (the `sh` hook wrapper's `jq`) contributes that requirement to the package's effective set, so the author declares only what the author's scripts use.
- **Detect → propose → confirm → install → receipt.** At install (and on `uze doctor`), UZE checks each effective requirement against the machine. Missing ones are presented as a plan naming the executable, the purpose and the installer UZE would use — chosen from what the machine offers (system package manager, `brew`, `winget`, a user-level manager such as `mise` when present). Nothing is installed without an explicit confirmation, in the CLI (`y/N`, `--yes` for automation) and in the TUI (the same plan, one confirmation). An installation performed by UZE is receipt-owned; a tool the user already had is recorded as observed, never owned.
- **Declining is a supported outcome.** A package whose requirements are unmet still installs; the affected capabilities are delivered with the requirement reported unmet (a hook wrapper keeps its fail-closed/fail-open rule), and `uze plugin list`/`uze doctor` show the gap with the command that would close it.
- **`uze doctor` re-verifies** every effective requirement of every installed package and reports drift (a tool removed or downgraded since install).
- **Uninstall never removes a tool.** Removing a package drops its requirement records and its receipts; an executable UZE installed stays, because other packages or the user may depend on it. `uze doctor` may list UZE-installed tools no package needs anymore.

Out of scope: language-package dependencies inside a plugin (npm, pip — the harness or the author's own tooling handles them), version pinning beyond a minimum constraint, and any installation without confirmation.

## Capabilities

### New Capabilities

- `plugin-requirements`: declaring executables a package needs, deriving the effective set including packager-introduced ones, detecting them on the machine, proposing and confirming installation through the machine's own installers, recording results, and reporting unmet or drifted requirements.

### Modified Capabilities

- `plugin`: install validates and resolves requirements before attachment; a package with unmet requirements installs with the gap reported; list shows requirement status.
- `doctor`: reports unmet and drifted requirements per package and the command that closes each gap.

## Impact

- `crates/uze-core`: manifest schema (`requirements` in `plugin.json`), requirement model and effective-set derivation, receipt kind for a UZE-installed executable, detection through the existing subprocess/PATH facilities; no vendor names.
- `crates/uze-integrations`: integrations contribute the requirements of what they generate (the `sh` wrapper contributes `jq`).
- `crates/uze-application`: install/update/doctor lifecycle gains the requirement plan and the confirmation step as a read model and an action; installer selection lives beside the existing harness provisioning.
- CLI (`src/main.rs`): confirmation prompt and `--yes`; `plugin list`/`doctor` output. TUI (`src/ui`): the plan and confirmation as a view; `command_performance.rs` classification for any new leaf command.
- Fixtures, `agent-plugins.org` schema extension documented, `docs/capabilities/portable-hooks.md` (the `jq` note points here), conformance Lab: a fixture declaring a requirement, one vertical proving the unmet-requirement report and the fail-closed rule with the tool absent.
