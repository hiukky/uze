## Why

The workspace client captures the mouse for its own chrome, which takes the
host terminal's native text selection away from every pane it draws. Copying
an agent's output — a path, an error, a command it suggested — meant leaving
UZE or holding a terminal-specific modifier that each harness answers
differently. Selecting text has to be one gesture with one result in every
pane, whatever runs in it.

## What Changes

- Pressing, dragging and releasing the left button over a pane selects its
  text, drawn as it is dragged; the release copies it to the system clipboard
  and a toast says how much was copied. No key is needed to copy.
- The selection stays drawn after the release and is put away by the next
  press or key.
- UZE's selection takes precedence over the pane's program: a drag is always
  a selection, even when the program asked for mouse reports. The program
  keeps its clicks — a press is held until it cannot be the start of a drag
  and is delivered, press and release together, when the button comes up
  without having moved. The wheel is unchanged.
- Shift hands the drag back to a program that asked for the mouse, for the
  program that has a use for a drag of its own.
- The clipboard is reached through the host terminal (OSC 52), so a copy
  lands on the machine the operator sits at — from WSL into Windows, over
  SSH — with no platform tool and no new dependency.

## Capabilities

### New Capabilities
- `pane-text-selection`: selecting a pane's text with the pointer, copying it
  on release, and who gets a pointer gesture over a pane — UZE or the pane's
  program.

### Modified Capabilities
<!-- none: `terminal-runtime`'s pane rendering and input forwarding are
     unchanged; what this adds is the client deciding, before forwarding, which
     gestures are its own. -->

## Impact

- `src/ui/orchestrator/selection.rs` (new): the selection's geometry, its
  text, and the OSC 52 sequence.
- `src/ui/orchestrator/session.rs`: press, drag and release over a pane;
  holding a click back and delivering it on release.
- `src/ui/orchestrator/render.rs`: the selection drawn in reverse video over
  the pane's own colours.
- `src/ui.rs`: `TerminalSession::emit`, bytes for the host terminal written
  between frames.
- No protocol, persistence, dependency or theme-token change.
- Follow-up, out of scope: a program in a pane that copies with its own
  OSC 52 is dropped by `uze-terminal`'s `ReplySink` (`Event::ClipboardStore`
  falls through), so a harness's own copy command does not reach the
  clipboard. Carrying it to the client needs a protocol event.
