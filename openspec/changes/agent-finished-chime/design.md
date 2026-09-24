## Context

The workspace client already decides when an agent's turn ends: repaint
rhythm per pane, expired after a quiet window, in `expire_agent_activity`.
That moment moves a background tab to *Completed* and drops the spinner on
the focused one. Nothing listens to it for anything but the sidebar and a
readiness re-read. The Manage modal's Appearance screen (now Settings) already chose two
machine-scoped things (theme, glyph set) as groups of cards, each persisted
as a small record behind a `uze-application` service.

## Goals / Non-Goals

**Goals:**
- One listener on the existing turn-end moment; no second detector.
- A choice that changes the running workspace without a relaunch.

**Non-Goals:**
- Desktop notifications (OSC 9/777, `notify-send`): terminal support is
  uneven, and a bell already reaches the OS through the terminal.
- Reading the harness's own "stop" hooks or the pane's BEL: the repaint
  detector is what the sidebar trusts, and two sources would disagree.
- Knowing whether the terminal window has OS focus (focus reporting would
  also have to be forwarded to panes that asked for it). *Always* is the
  answer for an operator who looks away from the terminal.

## Decisions

- **The bell, not audio.** Writing BEL through the terminal session defers
  sound, volume and "visual bell" to the operator's terminal, adds no
  dependency, works over SSH and inside tmux. Playing a sample would need
  an audio stack per platform and ignore the operator's own bell setting.
- **Three choices, not a toggle.** On/off forces either a ring every time
  the operator watches an agent finish (annoying) or no ring for the one
  tab they are watching from another window (useless). *Out of sight* is
  the discreet middle; *Always* covers looking away from the terminal.
- **One settings file, `~/.uze/config.toml`**, rather than a one-line JSON
  record per choice under `state/`. Operator choices sit at the root beside
  `keys.json` and `theme-overrides.json` — things someone chooses and may
  write by hand — and TOML takes comments. Not `layout.json`: that is
  rewritten whole on every drag by the workspace's recorder thread, from its
  own copy, so a choice written by the modal would be overwritten. Not the
  ledgers either: they have their own writers, locks and ladders.
- **`toml_edit`'s document model, one section per concern.** `config` knows
  files and sections; `appearance` owns `[appearance]` and
  `notifications` owns `[notifications]`. A write parses, sets one key
  and writes atomically, so comments, ordering and unknown sections survive.
  A file that does not parse is an error on read and on write — reading it
  as empty would undo every choice, writing over it would destroy the text
  being fixed. No shape ladder: this is authored text evolving by additive
  keys, like `keys.json`, and an unknown value reads as the default.
- **No path from `state/theme.json`.** UZE is in alpha: a machine's earlier
  theme choice is made again rather than carried by a step someone has to
  remember to delete.
- **Concurrent writers.** Two clients writing different keys at the same
  instant can lose one (read-modify-write, no lock) — the same exposure
  `theme.json` had between its two halves; settings are written by a
  person, one choice at a time.
- **Live value in a process-wide cell**, set at attach and by the worker
  when the modal chooses — the same way the active theme is held — so the
  workspace reads the current choice at ring time with no plumbing between
  the modal's model and the workspace's.
- **A settle window before ringing.** The turn detector ends a turn after
  five seconds without a repaint, which a long tool call or a pause to think
  also reaches. The sidebar tolerates that; a bell must not. A turn rings
  only after staying ended for ten more seconds, judged then: resumed work
  drops it, and under *Out of sight* a tab looked at meanwhile (its check
  acknowledged) drops it. A pause that persists — waiting on the operator —
  rings, which is what the bell is for. Alternative considered: the
  harness's own Stop hook, precise but per-vendor and delivered through
  plugins, a far larger change for the same signal.
- **Coalesced in the workspace**: the expiry pass raises one "due" flag; the
  loop rings after drawing when the flag is set and the last ring is older
  than the cooldown (a few seconds).
- **A Settings screen, mirroring `config.toml`.** The Appearance route
  becomes Settings, one card group per section (Theme, Glyphs,
  Notifications), so the file and the screen read the same way and a later
  setting has an obvious place. Keys stays its own screen: another file and
  another kind of choosing. The key scope `appearance` becomes `settings`.

## Risks / Trade-offs

- [A terminal with the bell disabled makes *Out of sight* look broken] →
  the drawer says the terminal decides what a ring sounds like, and the
  preview ring on selection shows it at once.
- [False turn ends from a long pause mid-turn] → inherited from the
  sidebar's detector, which already accepts this; the cooldown keeps a
  flapping pane from ringing repeatedly.
