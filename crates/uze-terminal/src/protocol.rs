use serde::{Deserialize, Serialize};

use crate::{PaneId, Session, SpaceId, TabId, WorkspaceId};

/// Bumped for the `Space` grouping layer: the `Session`/`Workspace` shape
/// changed (`Workspace::tabs` → `Workspace::spaces` of `Space`, each with
/// its own `tabs`) and four requests were added. `Session` is in-memory
/// only on the server (never persisted — see `runtime::serve`), so there is
/// nothing to migrate; a server still running the previous shape simply
/// fails this version check instead of desyncing.
///
/// Bumped again for `MouseMode` on `PaneSnapshot`/`PaneDamage`: unlike a
/// request-shape change (rejected cleanly by the check above, on the one
/// message a client sends once), a *pushed* shape a still-running old
/// server keeps sending forever has no such gate — a client built against
/// the new shape fails to deserialize every `Snapshot`/`Damage` event from
/// an unbumped old server, which silently kills its read thread and never
/// surfaces as more than a pane stuck on "starting shell…". Any field
/// added to either struct needs this bumped too, for the same reason.
///
/// Bumped again for `bracketed_paste` on the same two structs, for the
/// same reason.
///
/// Bumped again for the wire framing itself switching from newline-
/// delimited JSON to length-prefixed bincode (see `runtime::write_message`)
/// — an old client/server speaking the previous framing would otherwise
/// misread a length prefix as JSON bytes or vice versa, corrupting the
/// stream instead of failing this version check cleanly.
///
/// Bumped again for terminal-owned scrollback requests.
pub const PROTOCOL_VERSION: u16 = 7;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ClientRequest {
    Attach {
        version: u16,
        workspace: WorkspaceId,
        columns: u16,
        rows: u16,
    },
    Detach,
    Input {
        pane: PaneId,
        bytes: Vec<u8>,
    },
    /// Move the pane's terminal-owned scrollback viewport. Positive values
    /// move toward older output; negative values return toward the live end.
    Scroll {
        pane: PaneId,
        lines: i32,
    },
    Resize {
        pane: PaneId,
        columns: u16,
        rows: u16,
    },
    CreateTab {
        label: String,
        columns: u16,
        rows: u16,
        /// Optional directory for the pane's first process. A missing value
        /// keeps the workspace-root behavior used by ordinary shell tabs.
        cwd: Option<std::path::PathBuf>,
        /// Command to run in the new pane's PTY, as `argv` — `None` (or
        /// `Some(&[])`) keeps the default `$SHELL`. Lets a client open a
        /// tab running a specific program directly instead of a shell the
        /// user would otherwise have to type the program into themselves.
        command: Option<Vec<String>>,
    },
    SelectTab {
        tab: TabId,
    },
    CloseTab {
        tab: TabId,
    },
    RenameTab {
        tab: TabId,
        label: String,
    },
    CreateSpace {
        label: String,
        columns: u16,
        rows: u16,
    },
    SelectSpace {
        space: SpaceId,
    },
    CloseSpace {
        space: SpaceId,
    },
    RenameSpace {
        space: SpaceId,
        label: String,
    },
    Stop,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ClientEvent {
    Attached {
        session: Session,
    },
    Snapshot {
        session: Session,
        panes: Vec<PaneSnapshot>,
    },
    /// Tab/selection structure changed with no pane content affected —
    /// every open pane already stays current through [`ClientEvent::Damage`]
    /// on its own, so this carries no cell data.
    SessionUpdated {
        session: Session,
    },
    Damage(PaneDamage),
    Detached,
    Stopped,
    Error {
        message: String,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PaneSnapshot {
    pub pane: PaneId,
    pub columns: u16,
    pub rows: u16,
    pub cursor: Cursor,
    pub alternate_screen: bool,
    pub mouse: MouseMode,
    /// Whether the pane's own program has asked the terminal for bracketed
    /// paste (mode 2004), read straight off the PTY's VT state alongside
    /// `mouse`. The client uses this to decide how to frame a physical
    /// paste before forwarding it into the pane — see `bracketed_paste`
    /// on `PaneDamage`.
    pub bracketed_paste: bool,
    pub cells: Vec<RenderCell>,
}

/// A pane update pushed by PTY output: only the cells that actually
/// changed since the last event this pane sent, addressed by
/// `(row, column)`. Sending the whole grid (as [`PaneSnapshot`] does) on
/// every keystroke's echo made typing feel like it hung — a single changed
/// character was serializing thousands of unchanged ones alongside it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PaneDamage {
    pub pane: PaneId,
    pub columns: u16,
    pub rows: u16,
    pub cursor: Cursor,
    pub alternate_screen: bool,
    pub mouse: MouseMode,
    /// See [`PaneSnapshot::bracketed_paste`].
    pub bracketed_paste: bool,
    pub changed: Vec<(u16, u16, RenderCell)>,
}

/// What mouse tracking the pane's own program has asked the terminal for
/// (xterm mouse-tracking modes 1000/1002/1003/1006), read straight off the
/// PTY's VT state. The client uses this to decide whether a click/drag/
/// scroll that misses uze's own chrome should be encoded and forwarded into
/// the pane at all — forwarding into a pane that never asked for mouse
/// reports would inject raw escape bytes into a plain shell prompt.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct MouseMode {
    /// Button press/release/wheel reporting is on (mode 1000, or the two
    /// motion modes below, which imply it).
    pub reports_clicks: bool,
    /// Motion while a button is held is also reported (mode 1002/1003) —
    /// xterm doesn't send drag events under plain click-reporting alone.
    pub reports_drag: bool,
    /// SGR extended coordinate encoding (mode 1006) is on; otherwise the
    /// legacy X10 encoding applies, which caps coordinates at 223.
    pub sgr: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Cursor {
    pub column: u16,
    pub row: u16,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RenderCell {
    pub character: char,
    pub foreground: TerminalColor,
    pub background: TerminalColor,
    pub attributes: CellAttributes,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum TerminalColor {
    DefaultForeground,
    DefaultBackground,
    Indexed(u8),
    Rgb { red: u8, green: u8, blue: u8 },
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct CellAttributes {
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
    pub hidden: bool,
    pub strikeout: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_is_versioned_and_serializable() {
        let request = ClientRequest::Attach {
            version: PROTOCOL_VERSION,
            workspace: WorkspaceId("w".into()),
            columns: 80,
            rows: 24,
        };
        assert_eq!(
            serde_json::from_str::<ClientRequest>(&serde_json::to_string(&request).unwrap())
                .unwrap(),
            request
        );
    }

    #[test]
    fn scroll_request_round_trips() {
        let request = ClientRequest::Scroll {
            pane: PaneId(3),
            lines: -3,
        };
        assert_eq!(
            serde_json::from_str::<ClientRequest>(&serde_json::to_string(&request).unwrap())
                .unwrap(),
            request
        );
    }

    #[test]
    fn space_requests_round_trip() {
        let requests = [
            ClientRequest::CreateSpace {
                label: "frontend".into(),
                columns: 80,
                rows: 24,
            },
            ClientRequest::SelectSpace { space: SpaceId(1) },
            ClientRequest::CloseSpace { space: SpaceId(1) },
            ClientRequest::RenameSpace {
                space: SpaceId(1),
                label: "backend".into(),
            },
        ];
        for request in requests {
            assert_eq!(
                serde_json::from_str::<ClientRequest>(&serde_json::to_string(&request).unwrap())
                    .unwrap(),
                request
            );
        }
    }
}
