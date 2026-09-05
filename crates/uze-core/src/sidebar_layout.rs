//! What the workspace client's sidebar keeps between runs.
//!
//! Folding the commit timeline shut is a preference, not a transient. The
//! section holds the foot of a column whose reason for existing is the
//! spaces above it, so a timeline that opened itself again on every launch
//! made the choice something the user had to keep making — and made it in
//! the one place where the answer is obviously personal rather than
//! per-repository, which is why this is machine-scoped like
//! [`profile_state`](crate::profile_state) rather than keyed by workspace.
//!
//! Best-effort by construction: an unreadable or malformed file answers
//! with the defaults. Nothing derives from this, so a TUI that refused to
//! start over its own sidebar state would be trading the product for a
//! preference.

use std::fs;

use serde::{Deserialize, Serialize};

use crate::{Result, home::UzeHome, persistence::write_atomic};

/// The sidebar's remembered shape. Every field is what the user last left
/// it at, never what the client computed for itself — which is also why
/// the width is one value rather than one per mode: the workspace and the
/// management sidebar are the same column to the person dragging it.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct SidebarLayout {
    /// The columns the sidebar was dragged to; `None` leaves the width to
    /// the client's responsive default.
    pub width: Option<u16>,
    /// Whether the commit timeline shows only its header.
    pub timeline_collapsed: bool,
    /// The commit rows the timeline was dragged to; `None` leaves the
    /// height to the client's own default.
    pub timeline_rows: Option<u16>,
}

impl Default for SidebarLayout {
    /// Folded, at whatever width the terminal argues for. The sidebar is
    /// for the spaces, and an unasked-for history taking half of it is the
    /// client deciding for the user; the header row stays either way, so
    /// opening it is one click away.
    fn default() -> Self {
        Self {
            width: None,
            timeline_collapsed: true,
            timeline_rows: None,
        }
    }
}

pub fn load(home: &UzeHome) -> SidebarLayout {
    fs::read(home.sidebar_layout_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save(home: &UzeHome, layout: &SidebarLayout) -> Result<()> {
    write_atomic(
        &home.sidebar_layout_path(),
        serde_json::to_vec_pretty(layout)
            .expect("a sidebar layout serializes")
            .as_slice(),
    )
}

#[cfg(test)]
mod tests {
    use super::{SidebarLayout, load, save};
    use crate::home::UzeHome;

    fn temp_home(label: &str) -> UzeHome {
        UzeHome::at(uze_testkit::temp::scratch(label))
    }

    #[test]
    fn an_unwritten_layout_reads_as_the_default() {
        let home = temp_home("unwritten");

        assert_eq!(load(&home), SidebarLayout::default());
        assert!(
            load(&home).timeline_collapsed,
            "the column opens on the spaces"
        );
    }

    #[test]
    fn what_was_saved_is_what_the_next_run_reads() {
        let home = temp_home("round-trip");
        let layout = SidebarLayout {
            width: Some(34),
            timeline_collapsed: false,
            timeline_rows: Some(6),
        };

        save(&home, &layout).unwrap();

        assert_eq!(load(&home), layout);
    }

    #[test]
    fn an_unreadable_layout_is_the_default_rather_than_a_failed_attach() {
        let home = temp_home("unreadable");
        save(&home, &SidebarLayout::default()).unwrap();
        std::fs::write(home.sidebar_layout_path(), b"{ not json").unwrap();

        assert_eq!(load(&home), SidebarLayout::default());
    }
}
