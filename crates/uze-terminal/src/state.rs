use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct WorkspaceId(pub String);

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct TabId(pub u64);

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct PaneId(pub u64);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Session {
    pub workspace: Workspace,
    pub next_tab_id: u64,
    pub next_pane_id: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub root: PathBuf,
    pub tabs: Vec<Tab>,
    pub selected_tab: TabId,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Tab {
    pub id: TabId,
    pub label: String,
    pub layout: Layout,
    pub focus: Focus,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Layout {
    Pane(Pane),
    Split {
        axis: SplitAxis,
        first: Box<Layout>,
        second: Box<Layout>,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SplitAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Pane {
    pub id: PaneId,
    /// Best-known current working directory — set at spawn time, then kept
    /// live by the server's periodic foreground-process probe (a shell
    /// `cd` is not otherwise observable from outside the PTY).
    pub cwd: PathBuf,
    pub columns: u16,
    pub rows: u16,
    /// Best-known foreground process name (e.g. the shell, or whatever it
    /// last exec'd into) — same live-probe caveat as `cwd`.
    pub process: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Focus {
    pub pane: PaneId,
}

impl Session {
    pub fn new(id: WorkspaceId, root: PathBuf, columns: u16, rows: u16) -> Self {
        let pane = Pane {
            id: PaneId(1),
            cwd: root.clone(),
            columns,
            rows,
            process: "shell".to_owned(),
        };
        let tab = Tab {
            id: TabId(1),
            label: "shell".to_owned(),
            layout: Layout::Pane(pane),
            focus: Focus { pane: PaneId(1) },
        };
        Self {
            workspace: Workspace {
                id,
                root,
                tabs: vec![tab],
                selected_tab: TabId(1),
            },
            next_tab_id: 2,
            next_pane_id: 2,
        }
    }

    pub fn selected_tab(&self) -> &Tab {
        self.workspace
            .tabs
            .iter()
            .find(|tab| tab.id == self.workspace.selected_tab)
            .expect("session selected tab is always present")
    }

    pub fn add_tab(&mut self, label: String, columns: u16, rows: u16) -> PaneId {
        let tab_id = TabId(self.next_tab_id);
        let pane_id = PaneId(self.next_pane_id);
        self.next_tab_id += 1;
        self.next_pane_id += 1;
        self.workspace.tabs.push(Tab {
            id: tab_id,
            label,
            layout: Layout::Pane(Pane {
                id: pane_id,
                cwd: self.workspace.root.clone(),
                columns,
                rows,
                process: "shell".to_owned(),
            }),
            focus: Focus { pane: pane_id },
        });
        self.workspace.selected_tab = tab_id;
        pane_id
    }

    /// Removes `tab` and returns the panes it owned, so the caller can stop
    /// their processes. Refuses to remove the workspace's only remaining
    /// tab — a workspace always has somewhere to focus.
    pub fn remove_tab(&mut self, tab: TabId) -> Option<Vec<PaneId>> {
        if self.workspace.tabs.len() <= 1 {
            return None;
        }
        let index = self.workspace.tabs.iter().position(|t| t.id == tab)?;
        let removed = self.workspace.tabs.remove(index);
        if self.workspace.selected_tab == tab {
            let next = index.min(self.workspace.tabs.len() - 1);
            self.workspace.selected_tab = self.workspace.tabs[next].id;
        }
        Some(panes_in_layout(&removed.layout))
    }

    /// Renames `tab`, trimming the given label. Refuses a blank label (a
    /// tab always has a name) and reports whether anything changed, same
    /// contract as [`Session::update_pane_status`].
    pub fn rename_tab(&mut self, tab: TabId, label: String) -> bool {
        let trimmed = label.trim();
        if trimmed.is_empty() {
            return false;
        }
        let Some(found) = self.workspace.tabs.iter_mut().find(|t| t.id == tab) else {
            return false;
        };
        if found.label == trimmed {
            return false;
        }
        found.label = trimmed.to_owned();
        true
    }

    /// Applies a fresh live-probe reading for `pane`'s cwd/process, and
    /// reports whether anything actually changed — so the caller only
    /// broadcasts a `SessionUpdated` when the sidebar tree would show
    /// something new, not on every probe tick.
    pub fn update_pane_status(&mut self, pane: PaneId, cwd: PathBuf, process: String) -> bool {
        for tab in &mut self.workspace.tabs {
            if let Some(found) = find_pane_mut(&mut tab.layout, pane) {
                if found.cwd == cwd && found.process == process {
                    return false;
                }
                found.cwd = cwd;
                found.process = process;
                return true;
            }
        }
        false
    }
}

fn panes_in_layout(layout: &Layout) -> Vec<PaneId> {
    match layout {
        Layout::Pane(pane) => vec![pane.id],
        Layout::Split { first, second, .. } => {
            let mut panes = panes_in_layout(first);
            panes.extend(panes_in_layout(second));
            panes
        }
    }
}

fn find_pane_mut(layout: &mut Layout, wanted: PaneId) -> Option<&mut Pane> {
    match layout {
        Layout::Pane(pane) if pane.id == wanted => Some(pane),
        Layout::Pane(_) => None,
        Layout::Split { first, second, .. } => {
            find_pane_mut(first, wanted).or_else(|| find_pane_mut(second, wanted))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_round_trips_and_allocates_stable_identifiers() {
        let mut session = Session::new(
            WorkspaceId("workspace-a".into()),
            PathBuf::from("/tmp/a"),
            80,
            24,
        );
        assert_eq!(session.selected_tab().focus.pane, PaneId(1));
        assert_eq!(session.add_tab("agent".into(), 100, 30), PaneId(2));
        assert_eq!(session.workspace.selected_tab, TabId(2));
        let encoded = serde_json::to_string(&session).unwrap();
        assert_eq!(serde_json::from_str::<Session>(&encoded).unwrap(), session);
    }

    #[test]
    fn remove_tab_refuses_the_last_tab_but_allows_the_rest() {
        let mut session = Session::new(
            WorkspaceId("workspace-a".into()),
            PathBuf::from("/tmp/a"),
            80,
            24,
        );
        assert_eq!(session.remove_tab(TabId(1)), None);

        let second_pane = session.add_tab("agent".into(), 80, 24);
        assert_eq!(session.workspace.selected_tab, TabId(2));

        let removed = session.remove_tab(TabId(2)).expect("second tab removed");
        assert_eq!(removed, vec![second_pane]);
        assert_eq!(session.workspace.tabs.len(), 1);
        assert_eq!(session.workspace.selected_tab, TabId(1));
    }

    #[test]
    fn remove_tab_reselects_a_neighbor_when_the_active_tab_closes() {
        let mut session = Session::new(
            WorkspaceId("workspace-a".into()),
            PathBuf::from("/tmp/a"),
            80,
            24,
        );
        session.add_tab("two".into(), 80, 24);
        session.add_tab("three".into(), 80, 24);
        assert_eq!(session.workspace.selected_tab, TabId(3));

        session.remove_tab(TabId(3)).expect("third tab removed");
        assert_eq!(session.workspace.selected_tab, TabId(2));
    }

    #[test]
    fn update_pane_status_reports_change_only_when_something_actually_moved() {
        let mut session = Session::new(
            WorkspaceId("workspace-a".into()),
            PathBuf::from("/tmp/a"),
            80,
            24,
        );
        assert!(!session.update_pane_status(PaneId(1), PathBuf::from("/tmp/a"), "shell".into()));
        assert!(session.update_pane_status(PaneId(1), PathBuf::from("/tmp/b"), "vim".into()));
        assert_eq!(session.selected_tab().focus.pane, PaneId(1));
        let Layout::Pane(pane) = &session.selected_tab().layout else {
            panic!("expected a single pane layout");
        };
        assert_eq!(pane.cwd, PathBuf::from("/tmp/b"));
        assert_eq!(pane.process, "vim");
        assert!(!session.update_pane_status(PaneId(99), PathBuf::from("/tmp/c"), "x".into()));
    }

    #[test]
    fn rename_tab_trims_refuses_blank_and_reports_real_changes_only() {
        let mut session = Session::new(
            WorkspaceId("workspace-a".into()),
            PathBuf::from("/tmp/a"),
            80,
            24,
        );
        assert!(session.rename_tab(TabId(1), "  agent  ".into()));
        assert_eq!(session.selected_tab().label, "agent");
        assert!(!session.rename_tab(TabId(1), "agent".into()));
        assert!(!session.rename_tab(TabId(1), "   ".into()));
        assert_eq!(session.selected_tab().label, "agent");
        assert!(!session.rename_tab(TabId(99), "ghost".into()));
    }
}
