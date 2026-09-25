## Context

The workspace client enables mouse capture on the host terminal so its chrome
(sidebar, tabs, toasts, extensions) can be clicked. A pointer event that hits
no chrome is forwarded into the focused pane's PTY, but only when the pane's
program turned mouse reporting on (`MouseMode` on the pane snapshot);
otherwise it is dropped. Nothing selected text. The client holds each pane's
full cell grid (`PaneSnapshot`), so the text under a selection is already on
the client's side of the protocol.

## Goals / Non-Goals

**Goals:**
- One selection gesture over every pane, owned by the client.
- The copy reaches the operator's own clipboard across WSL and SSH.
- No change to the terminal runtime, the protocol, or persisted state.

**Non-Goals:**
- Scrollback selection: only what the pane shows can be selected; the
  selection does not follow content that scrolls under it.
- Word/line selection by double/triple click, and keyboard-driven selection.
- Carrying a pane program's own OSC 52 to the host (see Risks).

## Decisions

**The client decides, before forwarding.** Selection lives in
`src/ui/orchestrator/selection.rs` (geometry, text, OSC 52) and is driven from
the press/drag/release handlers in `session.rs`, ahead of `forward_mouse`.
The runtime is not involved: it would need to be told about a gesture only to
hand back text the client already has. *Alternative:* select in the runtime
over its own grid, which would include scrollback — rejected for now as a
protocol change bought for a non-goal.

**A drag is always the client's; a click is held, then delivered.** A press
over a pane cannot be known to be a click until the button comes up, so it is
held and, if it never moved, the press and release are forwarded together on
release. A program that owns the mouse loses only drags, and no gesture ever
reaches both the client and the program. *Alternatives:* forward the press
immediately and start selecting on the first motion — rejected, the program
would see a press with no release, or a press and a drag it then never gets
to finish; defer to the program whenever it owns the mouse (the usual terminal
default, Shift to override) — rejected, it makes the gesture differ per
harness, which is the problem being solved.

**Shift is the inverse escape.** Terminals use Shift to take selection back
from a program; here selection is the default, so Shift gives the drag back
to the program, forwarded as it happens.

**The clipboard is the host terminal's, via OSC 52.** The client writes
`ESC ] 52 ; c ; <base64> BEL` through `TerminalSession::emit`, between frames,
the way the bell is rung, so it cannot interleave with a frame. The only thing
that can reach the clipboard of the machine the operator sits at is the
terminal they sit at; a platform tool (`clip.exe`, `pbcopy`, `wl-copy`) would
copy on the machine UZE runs on. Base64 is a dozen lines of std, not a crate.

**Copy on release, not on a key.** The request is copy-on-select; a key would
compete with what the pane's program binds (Ctrl+C is SIGINT).

**Drawn by reversing the cell's own colours**, not with a theme token: a
pane's content can be any colour, and inversion is the one mark legible over
all of them. It names no colour value, so the theme rule holds.

## Risks / Trade-offs

- [The host terminal ignores OSC 52, or caps its size] → the toast still says
  "copied". The client cannot observe the terminal's answer. Windows Terminal,
  iTerm2, kitty, WezTerm, Alacritty and foot honour it; tmux needs
  `set-clipboard on`.
- [A harness's own drag gestures (drag-select, dragging its scrollbar) stop
  working without Shift] → accepted as the price of one gesture everywhere;
  Shift restores them.
- [A click reaches the program on release instead of on press] → imperceptible
  for a click; a program that acts on press-and-hold loses the hold.
- [A selection over output that keeps repainting copies what is on screen at
  release, not at press] → acceptable for a gesture measured in seconds.
- [A pane program's own OSC 52 is dropped by the runtime] (`ReplySink` ignores
  `Event::ClipboardStore`) → a harness's own copy command does not reach the
  clipboard. Follow-up: a protocol event carrying the text to the client,
  which then emits it through the same path. Reading the clipboard
  (`ClipboardLoad`) stays refused.

## Candidate ADRs

- Selection over a pane is the workspace client's, ahead of the pane's
  program — decides who owns a pointer gesture over every pane, and later
  mouse features inherit it.
