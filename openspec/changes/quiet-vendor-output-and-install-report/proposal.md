## Why

`uze plugin install` / `uze <plugin>@<market>` used to run every mutating vendor command (Codex/Claude/Gemini `plugin add`, `extensions link`, `mcp add`, ...) with inherited stdio, so each vendor CLI's own progress output — banners, spinners, consent narratives, warnings — was written straight into UZE's terminal, interleaved with UZE's own report and, worst, across the TUI's alternate screen where it corrupted the layout. The user sees a wall of mixed-provenance text: they cannot tell UZE's log from the harness CLIs', and the vendor lines are written in the vendor's vocabulary, not UZE's.

The fix is ownership: the install log is UZE's, designed by UZE; vendor CLI output is captured internally and used only to *control* UZE's own status (exit code → success/failure, vendor's last words on failure). The report itself is also restructured: one compact line per harness instead of three verbose sections (delivery route + long evidence paragraph + attachment line).

## What Changes

- Every mutating vendor command now runs with captured stdio and null stdin: discard output on success; on failure the error carries the vendor's own last words (capped tail).
- `--verbose` (global flag; also accepted by the `uze <plugin>@<market>` shorthand) shows each harness's delivery evidence and attachment details.
- Default text report for `plugin install`/`add` is compact: `Installed plugin`, Store path, one line per harness (`route` + attachment location when recorded).
- JSON output and all other text output unchanged.

## Capabilities

### New Capabilities

- (none)

### Modified Capabilities

- `plugin` — install/add reporting is UZE-owned and compact; vendor CLI output never interleaves with UZE's output; `--verbose` opts into evidence detail.

## Impact

- CLI: `src/main.rs` (global `--verbose`, `ShorthandArgs.verbose`, `render_add_report` for `plugin install` and the project shorthand)
- Integrations: `crates/uze-integrations/src/shared/process.rs` (new capture helper + unit tests); every mutating `codex`/`claude`/`gemini` plugin/extension/MCP call site switched from inherited-stdio `.status()` to captured output
- Tests: `tests/cli.rs` assertions updated to the compact lines
- Docs: none beyond this change (no ADR — presentation choice, reversible, not architecturally significant)
