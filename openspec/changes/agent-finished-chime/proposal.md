## Why

An agent working in a tab the operator is not looking at finishes silently:
the only signal is a check mark in the sidebar, which is found only by
looking at it. With several agents running, the operator either polls the
sidebar or finds out late. A discreet, opt-in sound closes that gap without
becoming one more thing that interrupts.

## What Changes

- The workspace client rings the terminal's own bell when an agent's turn
  ends, according to a machine-scoped choice: **Silent** (default), **Out of
  sight** (only a turn that ended in a tab not on screen), or **Always**.
- Several turns ending close together ring once, not once each.
- The Manage modal's Appearance screen becomes **Settings**, mirroring
  `config.toml` one group per section; the choice is made there, as a third
  group of cards beside Theme and Glyphs; choosing a ringing option rings
  once as a preview.
- The operator's settings move into one hand-editable file,
  `~/.uze/config.toml`, one section per concern: `[appearance]` (theme,
  glyphs) and `[notifications]`. `state/theme.json` is no longer read:
  UZE is in alpha, so a machine's earlier theme choice is simply made again. A write touches only its own key and keeps the rest of
  the file, comments included.

## Capabilities

### New Capabilities
- `agent-finished-chime`: when and how the workspace client makes a sound
  for a finished agent turn, and where the operator chooses it.

### Modified Capabilities
<!-- none: turn-end detection is unchanged, and where the theme selection is
     stored is not a requirement of `ui-theme` -->

## Impact

- `uze-core`: `config` (the settings file, via `toml_edit`, already a
  workspace dependency), `notifications`, and `appearance` reading and
  writing its section there; `UzeHome::config_path()`.
- `uze-application`: a `notifications()` service and the `Chime` re-export.
- Docs: `appearance.mdx` (where the selection lives) and `workspace/index.mdx`.
- `src/ui`: the Appearance route renamed Settings (key scope `appearance` →
  `settings`), its rows and drawer, a worker intent, the
  workspace loop ringing through the terminal session.
- No new dependency: the bell is a control character the host terminal
  already knows how to sound (or flash, or ignore, per its own settings).
