# Breaking changes

Observable changes on `feat/space-kinds` (pre-1.0; no compatibility path).

| Surface | Change |
|---|---|
| `~/.uze/state/terminal/workspace.json` | A tab stores its `launch`; `kind`/`env`/`agent` are required. An older file is read as nothing persisted: restored spaces are lost once. |
| `~/.uze/state/integrations.json` | Records drop `harness` (the key) and `installed` (presence means installed). |
| `~/.uze/state/marketplaces.json` | Records drop `name` (the key). |
| `~/.uze/state/plugin_marketplaces.json` | No longer written or read. |
| `~/.uze/state/attachments.json` | Receipts drop `strategy`. |
| Terminal runtime | Protocol reshaped at version 13 (unreleased): `Attach`/`CreateSpace` carry a `SpaceSeat`, `CloseSpace` a replacement and size, `Snapshot` replaces `Attached`. The pid file holds only a pid. A server of another build is replaced on attach. |
| `uze doctor`, `uze market/plugin inspect` JSON | No `verification`, `representation` or `direct_standard`; mechanisms are `Managed(…)`/`Unsupported`; hook compatibility loses `artifacts`. |
| Harness before `uze setup` | Skills report Unsupported ("run `uze setup`") instead of a runtime-bridge fallback. |
| Harness detection and probes | Run with a timeout and closed stdin; a binary counts as present only if `--version` succeeds. |
| `uze <plugin>@<market>` | A marketplace name with `.` or a leading `-` is refused up front. |
| `uze status`, `uze context inspect` | A harness the runtime shim delivers to reports `Projected`, not a bridge gap. |
| Workspace sidebar | The "+ new" control no longer shows its key; the last space can be closed (a home space replaces it). |
| Code surface diff | Lines appear in Git's order, not interleaved. |
| Help | Rendered from the command tree; `theme` and `terminal` now listed. |
