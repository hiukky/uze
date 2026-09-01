//! Translating this client's own input events into the bytes a PTY expects.
//!
//! The other half of what `orchestrator.rs` used to carry inline: pure
//! encoding, with no session state and no drawing. Everything here answers
//! one question — given a key, a click, or a paste, what does the terminal
//! on the far side of the pane need to receive?

use super::*;

/// Encodes and forwards a click/drag/scroll that missed every uze chrome
/// hit into the focused pane's PTY — the counterpart to `encode_key` for
/// mouse input. A no-op unless the pane's own program has actually turned
/// mouse reporting on (see `uze_terminal::MouseMode`): sending raw mouse
/// escape sequences into a plain shell prompt would just inject garbage
/// text at the cursor.
pub(super) fn forward_mouse<W: io::Write>(
    stream: &mut W,
    model: &WorkspaceModel,
    pane: Rect,
    mouse: MouseEvent,
) {
    let Some(snapshot) = model.panes.get(&model.focused_pane()) else {
        return;
    };
    if !snapshot.mouse.reports_clicks {
        return;
    }
    if matches!(mouse.kind, MouseEventKind::Drag(_)) && !snapshot.mouse.reports_drag {
        return;
    }
    let Some((column, row)) = pane_relative(mouse, pane) else {
        return;
    };
    let Some(bytes) = encode_mouse(mouse.kind, column, row, snapshot.mouse.sgr) else {
        return;
    };
    let _ = send_request(
        stream,
        &ClientRequest::Input {
            pane: model.focused_pane(),
            bytes,
        },
    );
}

/// Forwards the wheel into a pane. Programs that requested xterm mouse
/// reports receive a real mouse sequence. Normal-screen programs receive a
/// terminal scrollback request, matching a physical terminal; alternate
/// screens retain the conventional arrow-key fallback because they have no
/// normal-screen history to display.
pub(super) fn forward_scroll<W: io::Write>(
    stream: &mut W,
    model: &WorkspaceModel,
    pane: Rect,
    mouse: MouseEvent,
) {
    let Some(snapshot) = model.panes.get(&model.focused_pane()) else {
        return;
    };
    if snapshot.mouse.reports_clicks {
        forward_mouse(stream, model, pane, mouse);
        return;
    }
    if snapshot.alternate_screen {
        let bytes = match mouse.kind {
            MouseEventKind::ScrollUp => b"\x1b[A".to_vec(),
            MouseEventKind::ScrollDown => b"\x1b[B".to_vec(),
            _ => return,
        };
        let _ = send_request(
            stream,
            &ClientRequest::Input {
                pane: model.focused_pane(),
                bytes,
            },
        );
        return;
    }
    let lines = match mouse.kind {
        MouseEventKind::ScrollUp => 3,
        MouseEventKind::ScrollDown => -3,
        _ => return,
    };
    let _ = send_request(
        stream,
        &ClientRequest::Scroll {
            pane: model.focused_pane(),
            lines,
        },
    );
}

/// Forwards a physical paste into the focused pane's PTY — the counterpart
/// to `encode_key` for bulk pasted text. Framed with the same
/// `ESC[200~ … ESC[201~` bracket the pane's own program would have seen
/// pasting directly into a real terminal only when it actually turned
/// bracketed-paste mode on (`uze_terminal::PaneSnapshot::bracketed_paste`);
/// a plain shell that never asked for it would otherwise echo the bracket
/// markers themselves as literal garbage instead of treating them as
/// framing. This is also what lets a terminal's own clipboard-image-to-text
/// conversion (an image copied, then pasted) reach the pane at all — with
/// no bracketed-paste request mirrored onto the physical terminal in the
/// first place, most terminal emulators never attempt that conversion, and
/// pasting an image into an agent's input silently does nothing.
pub(super) fn forward_paste<W: io::Write>(stream: &mut W, model: &WorkspaceModel, text: &str) {
    let Some(snapshot) = model.panes.get(&model.focused_pane()) else {
        return;
    };
    let bytes = if snapshot.bracketed_paste {
        let mut framed = Vec::with_capacity(text.len() + 12);
        framed.extend_from_slice(b"\x1b[200~");
        framed.extend_from_slice(text.as_bytes());
        framed.extend_from_slice(b"\x1b[201~");
        framed
    } else {
        text.as_bytes().to_vec()
    };
    let _ = send_request(
        stream,
        &ClientRequest::Input {
            pane: model.focused_pane(),
            bytes,
        },
    );
}

/// `mouse`'s position translated into 1-indexed coordinates relative to
/// `pane`'s own top-left — the coordinate space every mouse-tracking
/// protocol reports in — or `None` when it falls outside `pane` entirely
/// (over the sidebar, tab strip, or an overlay uze's own hit-testing
/// already would have claimed first).
pub(super) fn pane_relative(mouse: MouseEvent, pane: Rect) -> Option<(u16, u16)> {
    if mouse.column < pane.x || mouse.row < pane.y {
        return None;
    }
    let column = mouse.column - pane.x;
    let row = mouse.row - pane.y;
    if column >= pane.width || row >= pane.height {
        return None;
    }
    Some((column + 1, row + 1))
}

/// The xterm mouse-tracking byte sequence for one click/drag/scroll event,
/// at pane-relative `column`/`row` (see `pane_relative`) — SGR (mode 1006)
/// when the pane asked for it, else the legacy X10 encoding every terminal
/// still understands as a fallback, whose single-byte coordinates saturate
/// at 223 rather than wrapping for anything larger.
pub(super) fn encode_mouse(
    kind: MouseEventKind,
    column: u16,
    row: u16,
    sgr: bool,
) -> Option<Vec<u8>> {
    let (code, release) = match kind {
        MouseEventKind::Down(MouseButton::Left) => (0u8, false),
        MouseEventKind::Up(MouseButton::Left) => (0u8, true),
        MouseEventKind::Drag(MouseButton::Left) => (32u8, false),
        MouseEventKind::ScrollUp => (64u8, false),
        MouseEventKind::ScrollDown => (65u8, false),
        _ => return None,
    };
    if sgr {
        Some(
            format!(
                "\x1b[<{code};{column};{row}{}",
                if release { 'm' } else { 'M' }
            )
            .into_bytes(),
        )
    } else {
        // Legacy X10: release never carries a button, always code 3; both
        // axes are single bytes offset by 32, so anything past 223 would
        // overflow into control-character range instead of wrapping.
        let legacy_code = if release { 3 } else { code };
        let cb = 32u16 + u16::from(legacy_code);
        let cx = 32u16 + column.min(223);
        let cy = 32u16 + row.min(223);
        Some(vec![0x1b, b'[', b'M', cb as u8, cx as u8, cy as u8])
    }
}

pub(super) fn encode_key(key: KeyEvent) -> Option<Vec<u8>> {
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Char(character) if control && character.is_ascii_alphabetic() => {
            Some(vec![(character.to_ascii_lowercase() as u8) - b'a' + 1])
        }
        KeyCode::Char(character) => Some(character.to_string().into_bytes()),
        KeyCode::Enter => Some(vec![b'\r']),
        KeyCode::Backspace => Some(vec![0x7f]),
        KeyCode::Tab => Some(vec![b'\t']),
        KeyCode::Esc => Some(vec![0x1b]),
        KeyCode::Up => Some(b"\x1b[A".to_vec()),
        KeyCode::Down => Some(b"\x1b[B".to_vec()),
        KeyCode::Right => Some(b"\x1b[C".to_vec()),
        KeyCode::Left => Some(b"\x1b[D".to_vec()),
        KeyCode::Home => Some(b"\x1b[H".to_vec()),
        KeyCode::End => Some(b"\x1b[F".to_vec()),
        KeyCode::Delete => Some(b"\x1b[3~".to_vec()),
        _ => None,
    }
}
