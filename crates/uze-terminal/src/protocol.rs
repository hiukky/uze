use serde::{Deserialize, Serialize};

use crate::{PaneId, Session, SpaceId, TabId, WorkspaceId};

/// Bumped for the `Space` grouping layer: the `Session`/`Workspace` shape
/// changed (`Workspace::tabs` → `Workspace::spaces` of `Space`, each with
/// its own `tabs`) and four requests were added. `Session` is in-memory
/// only on the server (never persisted — see `runtime::serve`), so there is
/// nothing to migrate; a server still running the previous shape simply
/// fails this version check instead of desyncing.
pub const PROTOCOL_VERSION: u16 = 2;

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
    Resize {
        pane: PaneId,
        columns: u16,
        rows: u16,
    },
    CreateTab {
        label: String,
        columns: u16,
        rows: u16,
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
    pub changed: Vec<(u16, u16, RenderCell)>,
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
