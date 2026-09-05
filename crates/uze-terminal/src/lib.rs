//! Local terminal runtime used by UZE's workspace client.
//!
//! The server owns pseudoterminals and emulation state.  UI clients only
//! attach, render snapshots, and forward input; this keeps a pane alive when
//! a client leaves the workspace.

mod protocol;
mod runtime;
mod state;

pub use protocol::{
    CellAttributes, ClientEvent, ClientRequest, Cursor, MouseMode, PROTOCOL_VERSION, Palette,
    PaneDamage, PaneSnapshot, RenderCell, TerminalColor,
};
pub use runtime::{
    RuntimeError, attach, open_space, read_event, send_request, serve, socket_path, stop,
};
pub use state::{
    Focus, Layout, OpenedSpace, Pane, PaneId, Session, Space, SpaceId, SpaceSeed, Tab, TabId,
    TabSeed, Workspace, WorkspaceId,
};
