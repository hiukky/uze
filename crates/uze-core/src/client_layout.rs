//! What the TUI keeps of its own shape between runs, in both of its modes.
//!
//! The column the sidebar was dragged to, whether the commit timeline is
//! folded, which management screen was open, how each drawer there was
//! left: all preferences, not transients. Every one of them used to be
//! answered by the client on every launch, so each was a choice the user
//! kept making — and made in the one place where the answer is obviously
//! personal rather than per-repository, which is why this is machine-scoped
//! like [`profile_state`](crate::profile_state) rather than keyed by
//! workspace.
//!
//! One file, one struct, sectioned by who owns each part: the two modes are
//! one product to the person switching between them with Ctrl+O, and a
//! preference is remembered by the product, not by whichever screen
//! happened to write it. A mode adds a field to its own section; nothing
//! here decides what a section means.
//!
//! Best-effort by construction: an unreadable or malformed file answers
//! with the defaults. Nothing derives from this, so a TUI that refused to
//! start over its own layout would be trading the product for a preference.

use std::collections::BTreeSet;
use std::fs;

use serde::{Deserialize, Serialize};

use crate::{Result, home::UzeHome, persistence::write_atomic};

/// The client's remembered shape. Every field is what the user last left
/// it at, never what the client computed for itself.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct ClientLayout {
    /// The column both modes draw — one value rather than one per mode,
    /// because the workspace and the management sidebar are the same
    /// column to the person dragging it.
    pub sidebar: SidebarLayout,
    pub workspace: WorkspaceLayout,
    pub management: ManagementLayout,
    pub first_steps: FirstStepsLayout,
}

/// What the operator has already done once, and whether they still want to
/// be shown what they have not.
///
/// Progress rather than shape, and here anyway: it is the same kind of
/// thing — machine-scoped, personal, best-effort, and worth nothing to
/// anyone but the client that wrote it. A section of its own because it is
/// one list drawn at the foot of both sidebars, so a step taken in one mode
/// is taken in the other.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct FirstStepsLayout {
    /// Folded to its header. Open on a first run, because a list of what
    /// to try is worth nothing to the person who has not seen it yet.
    pub collapsed: bool,
    /// The steps already taken, by the client's own name for each. A name
    /// the client no longer recognises is simply a step that is no longer
    /// listed, so nothing has to be cleaned up when the list changes.
    pub taken: BTreeSet<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct SidebarLayout {
    /// The columns the sidebar was dragged to; `None` leaves the width to
    /// the client's responsive default.
    pub width: Option<u16>,
}

/// What the workspace client — the terminal side, with its spaces and
/// agent tabs — keeps of its own arrangement.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct WorkspaceLayout {
    /// Whether the sidebar's commit timeline shows only its header.
    pub timeline_collapsed: bool,
    /// The commit rows the timeline was dragged to; `None` leaves the
    /// height to the client's own default.
    pub timeline_rows: Option<u16>,
}

impl Default for WorkspaceLayout {
    /// Folded. The sidebar is for the spaces, and an unasked-for history
    /// taking half of it is the client deciding for the user; the header
    /// row stays either way, so opening it is one click away.
    fn default() -> Self {
        Self {
            timeline_collapsed: true,
            timeline_rows: None,
        }
    }
}

/// What the management client — plugins, extensions, integrations,
/// profiles — keeps of its own arrangement.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct ManagementLayout {
    /// The screen that was open, by the client's own id for it; `None`,
    /// or an id the client no longer recognizes, opens its default screen.
    pub route: Option<String>,
    pub marketplace_drawer_open: bool,
    pub extension_drawer_open: bool,
    pub harnesses_drawer_open: bool,
    pub marketplace_drawer_width: Option<u16>,
    pub extension_drawer_width: Option<u16>,
    pub harness_drawer_width: Option<u16>,
    pub profile_columns_width: Option<u16>,
    /// The marketplaces folded shut in the catalog, by name.
    pub collapsed_marketplaces: BTreeSet<String>,
}

impl Default for ManagementLayout {
    /// Every drawer open: a screen's detail is the point of the screen,
    /// and a first visit should show it rather than ask for it.
    fn default() -> Self {
        Self {
            route: None,
            marketplace_drawer_open: true,
            extension_drawer_open: true,
            harnesses_drawer_open: true,
            marketplace_drawer_width: None,
            extension_drawer_width: None,
            harness_drawer_width: None,
            profile_columns_width: None,
            collapsed_marketplaces: BTreeSet::new(),
        }
    }
}

pub fn load(home: &UzeHome) -> ClientLayout {
    fs::read(home.client_layout_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save(home: &UzeHome, layout: &ClientLayout) -> Result<()> {
    write_atomic(
        &home.client_layout_path(),
        serde_json::to_vec_pretty(layout)
            .expect("a client layout serializes")
            .as_slice(),
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        ClientLayout, FirstStepsLayout, ManagementLayout, SidebarLayout, WorkspaceLayout, load,
        save,
    };
    use crate::home::UzeHome;

    fn temp_home(label: &str) -> UzeHome {
        UzeHome::at(uze_testkit::temp::scratch(label))
    }

    #[test]
    fn an_unwritten_layout_reads_as_the_default() {
        let home = temp_home("unwritten");

        let layout = load(&home);
        assert_eq!(layout, ClientLayout::default());
        assert!(
            layout.workspace.timeline_collapsed,
            "the column opens on the spaces"
        );
        assert!(
            layout.management.route.is_none(),
            "the management client opens on its own default screen"
        );
    }

    #[test]
    fn what_was_saved_is_what_the_next_run_reads() {
        let home = temp_home("round-trip");
        let layout = ClientLayout {
            sidebar: SidebarLayout { width: Some(34) },
            workspace: WorkspaceLayout {
                timeline_collapsed: false,
                timeline_rows: Some(6),
            },
            management: ManagementLayout {
                route: Some("plugins".to_owned()),
                marketplace_drawer_open: false,
                harness_drawer_width: Some(40),
                collapsed_marketplaces: BTreeSet::from(["uze-official".to_owned()]),
                ..ManagementLayout::default()
            },
            first_steps: FirstStepsLayout {
                collapsed: true,
                taken: BTreeSet::from(["open-action-index".to_owned()]),
            },
        };

        save(&home, &layout).unwrap();

        assert_eq!(load(&home), layout);
    }

    #[test]
    fn a_section_the_file_does_not_have_reads_as_its_default() {
        let home = temp_home("partial");
        std::fs::create_dir_all(home.state_dir()).unwrap();
        std::fs::write(
            home.client_layout_path(),
            br#"{ "sidebar": { "width": 30 }, "management": { "route": "profiles" } }"#,
        )
        .unwrap();

        let layout = load(&home);
        assert_eq!(layout.sidebar.width, Some(30));
        assert_eq!(layout.management.route.as_deref(), Some("profiles"));
        assert_eq!(layout.workspace, WorkspaceLayout::default());
        assert!(
            layout.management.marketplace_drawer_open,
            "a field the file does not name is the default, not false"
        );
    }

    #[test]
    fn an_unreadable_layout_is_the_default_rather_than_a_failed_attach() {
        let home = temp_home("unreadable");
        save(&home, &ClientLayout::default()).unwrap();
        std::fs::write(home.client_layout_path(), b"{ not json").unwrap();

        assert_eq!(load(&home), ClientLayout::default());
    }
}
