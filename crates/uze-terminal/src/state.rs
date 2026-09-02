use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct WorkspaceId(pub String);

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SpaceId(pub u64);

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct TabId(pub u64);

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct PaneId(pub u64);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Session {
    pub workspace: Workspace,
    pub next_space_id: u64,
    pub next_tab_id: u64,
    pub next_pane_id: u64,
}

/// The one server per user — an infrastructure detail, not something a
/// person organizes their work by. `spaces` is where that organizing
/// happens: a person creates as many as they like, freely renamed, each
/// born with a root directory that says what its agents work on.
///
/// `selected_space`, like every `Space::selected_tab`, is the server's
/// default; what each attached client actually looks at is the client's
/// own, and the `Session` a client receives carries *its* selection here.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub spaces: Vec<Space>,
    pub selected_space: SpaceId,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Space {
    pub id: SpaceId,
    pub label: String,
    /// Where this space's work lives, chosen when it was created: an agent
    /// created here starts from it, a shell opens in it, and whether it is
    /// a Git repository decides whether agents get slots.
    pub root: PathBuf,
    pub tabs: Vec<Tab>,
    pub selected_tab: TabId,
}

/// The label a space gets from its root when nobody names it: the root's
/// last component, or `home` for the home directory itself.
pub fn space_label(root: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if home.as_deref().is_some_and(|home| home == root) {
        return "home".to_owned();
    }
    root.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "root".to_owned())
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

/// A space to recreate on startup, restoring the shape (not the running
/// state — see [`Session::restore`]) a previous server instance had before
/// it stopped, whether cleanly or not (a crash, a reboot — anything that
/// left no chance to save more than this). Deliberately minimal: just
/// enough to reopen the same tabs in the same places, with no notion here
/// of what command a tab's pane should relaunch with — [`Session::restore`]
/// only rebuilds structure; the caller spawns each pane afterward however
/// it sees fit.
pub struct SpaceSeed {
    pub label: String,
    pub root: PathBuf,
    pub tabs: Vec<TabSeed>,
}

pub struct TabSeed {
    pub label: String,
    pub cwd: PathBuf,
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
        let space = Space {
            id: SpaceId(1),
            label: space_label(&root),
            root,
            tabs: vec![tab],
            selected_tab: TabId(1),
        };
        Self {
            workspace: Workspace {
                id,
                spaces: vec![space],
                selected_space: SpaceId(1),
            },
            next_space_id: 2,
            next_tab_id: 2,
            next_pane_id: 2,
        }
    }

    /// Rebuilds the space/tab shape `seeds` describes, allocating ids the
    /// same sequential way [`Session::new`]/[`Session::add_space`] do so
    /// the result is indistinguishable from one built up through ordinary
    /// use. A seed space with no tabs is dropped (a space always has
    /// somewhere to focus); if nothing is left standing, falls back to
    /// [`Session::new`]'s ordinary single-space bootstrap rather than
    /// producing a workspace with zero spaces.
    pub fn restore(
        id: WorkspaceId,
        root: PathBuf,
        columns: u16,
        rows: u16,
        seeds: Vec<SpaceSeed>,
    ) -> Self {
        let mut next_space_id = 1;
        let mut next_tab_id = 1;
        let mut next_pane_id = 1;
        let mut spaces = Vec::new();
        for seed in seeds {
            if seed.tabs.is_empty() {
                continue;
            }
            let space_id = SpaceId(next_space_id);
            next_space_id += 1;
            let mut tabs = Vec::new();
            for tab_seed in seed.tabs {
                let tab_id = TabId(next_tab_id);
                let pane_id = PaneId(next_pane_id);
                next_tab_id += 1;
                next_pane_id += 1;
                tabs.push(Tab {
                    id: tab_id,
                    label: tab_seed.label,
                    layout: Layout::Pane(Pane {
                        id: pane_id,
                        cwd: tab_seed.cwd,
                        columns,
                        rows,
                        process: "shell".to_owned(),
                    }),
                    focus: Focus { pane: pane_id },
                });
            }
            let selected_tab = tabs[0].id;
            spaces.push(Space {
                id: space_id,
                label: seed.label,
                root: seed.root,
                tabs,
                selected_tab,
            });
        }
        let Some(selected_space) = spaces.first().map(|space| space.id) else {
            return Self::new(id, root, columns, rows);
        };
        Self {
            workspace: Workspace {
                id,
                spaces,
                selected_space,
            },
            next_space_id,
            next_tab_id,
            next_pane_id,
        }
    }

    pub fn selected_space(&self) -> &Space {
        self.workspace
            .spaces
            .iter()
            .find(|space| space.id == self.workspace.selected_space)
            .expect("session selected space is always present")
    }

    pub fn selected_tab(&self) -> &Tab {
        let space = self.selected_space();
        space
            .tabs
            .iter()
            .find(|tab| tab.id == space.selected_tab)
            .expect("space selected tab is always present")
    }

    /// The space whose root is `root`, compared as the filesystem sees it.
    pub fn space_for_root(&self, root: &Path) -> Option<SpaceId> {
        let wanted = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        self.workspace
            .spaces
            .iter()
            .find(|space| {
                space
                    .root
                    .canonicalize()
                    .unwrap_or_else(|_| space.root.clone())
                    == wanted
            })
            .map(|space| space.id)
    }

    pub fn space(&self, space: SpaceId) -> Option<&Space> {
        self.workspace.spaces.iter().find(|s| s.id == space)
    }

    /// Creates a space rooted at `root` with one default tab (mirroring
    /// [`Session::new`]'s own bootstrap tab), selects it, and returns the
    /// new tab's pane so the caller can spawn it exactly like
    /// [`Session::add_tab`]'s result.
    pub fn add_space(&mut self, label: String, root: PathBuf, columns: u16, rows: u16) -> PaneId {
        let space_id = SpaceId(self.next_space_id);
        let tab_id = TabId(self.next_tab_id);
        let pane_id = PaneId(self.next_pane_id);
        self.next_space_id += 1;
        self.next_tab_id += 1;
        self.next_pane_id += 1;
        self.workspace.spaces.push(Space {
            id: space_id,
            label,
            tabs: vec![Tab {
                id: tab_id,
                label: "shell".to_owned(),
                layout: Layout::Pane(Pane {
                    id: pane_id,
                    cwd: root.clone(),
                    columns,
                    rows,
                    process: "shell".to_owned(),
                }),
                focus: Focus { pane: pane_id },
            }],
            root,
            selected_tab: tab_id,
        });
        self.workspace.selected_space = space_id;
        pane_id
    }

    /// Removes `space` and returns every pane across every tab it owned, so
    /// the caller can stop them all. Refuses to remove the workspace's only
    /// remaining space — same "always somewhere to focus" invariant
    /// [`Session::remove_tab`] holds for tabs.
    pub fn remove_space(&mut self, space: SpaceId) -> Option<Vec<PaneId>> {
        if self.workspace.spaces.len() <= 1 {
            return None;
        }
        let index = self.workspace.spaces.iter().position(|s| s.id == space)?;
        let removed = self.workspace.spaces.remove(index);
        if self.workspace.selected_space == space {
            let next = index.min(self.workspace.spaces.len() - 1);
            self.workspace.selected_space = self.workspace.spaces[next].id;
        }
        Some(
            removed
                .tabs
                .iter()
                .flat_map(|tab| panes_in_layout(&tab.layout))
                .collect(),
        )
    }

    /// Renames `space`, trimming the given label. Refuses a blank label and
    /// reports whether anything changed — same contract as
    /// [`Session::rename_tab`].
    pub fn rename_space(&mut self, space: SpaceId, label: String) -> bool {
        let trimmed = label.trim();
        if trimmed.is_empty() {
            return false;
        }
        let Some(found) = self.workspace.spaces.iter_mut().find(|s| s.id == space) else {
            return false;
        };
        if found.label == trimmed {
            return false;
        }
        found.label = trimmed.to_owned();
        true
    }

    /// Selects `space` if it exists, reporting whether the selection
    /// actually moved.
    pub fn select_space(&mut self, space: SpaceId) -> bool {
        if self.workspace.selected_space == space {
            return false;
        }
        if !self.workspace.spaces.iter().any(|s| s.id == space) {
            return false;
        }
        self.workspace.selected_space = space;
        true
    }

    /// Adds a tab to `space` — the one the creating client is looking at,
    /// which is not necessarily the server's default — and selects it
    /// there. Falls back to the default space when `space` is gone.
    pub fn add_tab(
        &mut self,
        space: SpaceId,
        label: String,
        columns: u16,
        rows: u16,
        cwd: PathBuf,
    ) -> PaneId {
        let tab_id = TabId(self.next_tab_id);
        let pane_id = PaneId(self.next_pane_id);
        self.next_tab_id += 1;
        self.next_pane_id += 1;
        let default = self.workspace.selected_space;
        let index = self
            .workspace
            .spaces
            .iter()
            .position(|s| s.id == space)
            .or_else(|| self.workspace.spaces.iter().position(|s| s.id == default))
            .expect("session selected space is always present");
        let space = &mut self.workspace.spaces[index];
        space.tabs.push(Tab {
            id: tab_id,
            label,
            layout: Layout::Pane(Pane {
                id: pane_id,
                cwd,
                columns,
                rows,
                process: "shell".to_owned(),
            }),
            focus: Focus { pane: pane_id },
        });
        space.selected_tab = tab_id;
        pane_id
    }

    /// Selects `tab` (found by searching every space, not just the
    /// currently selected one) as both its own space's `selected_tab` and,
    /// if that space isn't already the selected one, the workspace's
    /// `selected_space` too — clicking any tab shown anywhere in the
    /// sidebar switches to it, space and all. Reports whether either moved.
    pub fn select_tab(&mut self, tab: TabId) -> bool {
        let Some(space) = self
            .workspace
            .spaces
            .iter_mut()
            .find(|space| space.tabs.iter().any(|t| t.id == tab))
        else {
            return false;
        };
        let tab_moved = space.selected_tab != tab;
        space.selected_tab = tab;
        let space_id = space.id;
        let space_moved = self.workspace.selected_space != space_id;
        self.workspace.selected_space = space_id;
        tab_moved || space_moved
    }

    /// Removes `tab` (found by searching every space, not just the selected
    /// one) and returns the panes it owned, so the caller can stop their
    /// processes. Refuses to remove a space's only remaining tab — a space
    /// always has somewhere to focus.
    pub fn remove_tab(&mut self, tab: TabId) -> Option<Vec<PaneId>> {
        let space = space_containing_tab_mut(&mut self.workspace.spaces, tab)?;
        if space.tabs.len() <= 1 {
            return None;
        }
        let index = space.tabs.iter().position(|t| t.id == tab)?;
        let removed = space.tabs.remove(index);
        if space.selected_tab == tab {
            let next = index.min(space.tabs.len() - 1);
            space.selected_tab = space.tabs[next].id;
        }
        Some(panes_in_layout(&removed.layout))
    }

    /// Renames `tab` (found by searching every space), trimming the given
    /// label. Refuses a blank label (a tab always has a name) and reports
    /// whether anything changed, same contract as
    /// [`Session::update_pane_status`].
    pub fn rename_tab(&mut self, tab: TabId, label: String) -> bool {
        let trimmed = label.trim();
        if trimmed.is_empty() {
            return false;
        }
        let Some(space) = space_containing_tab_mut(&mut self.workspace.spaces, tab) else {
            return false;
        };
        let Some(found) = space.tabs.iter_mut().find(|t| t.id == tab) else {
            return false;
        };
        if found.label == trimmed {
            return false;
        }
        found.label = trimmed.to_owned();
        true
    }

    /// Applies a fresh live-probe reading for `pane`'s cwd/process (found by
    /// searching every space's tabs), and reports whether anything actually
    /// changed — so the caller only broadcasts a `SessionUpdated` when the
    /// sidebar tree would show something new, not on every probe tick.
    pub fn update_pane_status(&mut self, pane: PaneId, cwd: PathBuf, process: String) -> bool {
        for space in &mut self.workspace.spaces {
            for tab in &mut space.tabs {
                if let Some(found) = find_pane_mut(&mut tab.layout, pane) {
                    if found.cwd == cwd && found.process == process {
                        return false;
                    }
                    found.cwd = cwd;
                    found.process = process;
                    return true;
                }
            }
        }
        false
    }
}

fn space_containing_tab_mut(spaces: &mut [Space], tab: TabId) -> Option<&mut Space> {
    spaces
        .iter_mut()
        .find(|space| space.tabs.iter().any(|t| t.id == tab))
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
        assert_eq!(
            session.add_tab(
                session.workspace.selected_space,
                "agent".into(),
                100,
                30,
                PathBuf::from("/tmp/agent")
            ),
            PaneId(2)
        );
        assert_eq!(session.selected_space().selected_tab, TabId(2));
        let Layout::Pane(pane) = &session.selected_tab().layout else {
            panic!("expected a single pane layout");
        };
        assert_eq!(pane.cwd, PathBuf::from("/tmp/agent"));
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

        let second_pane = session.add_tab(
            session.workspace.selected_space,
            "agent".into(),
            80,
            24,
            PathBuf::from("/tmp/a"),
        );
        assert_eq!(session.selected_space().selected_tab, TabId(2));

        let removed = session.remove_tab(TabId(2)).expect("second tab removed");
        assert_eq!(removed, vec![second_pane]);
        assert_eq!(session.selected_space().tabs.len(), 1);
        assert_eq!(session.selected_space().selected_tab, TabId(1));
    }

    #[test]
    fn remove_tab_reselects_a_neighbor_when_the_active_tab_closes() {
        let mut session = Session::new(
            WorkspaceId("workspace-a".into()),
            PathBuf::from("/tmp/a"),
            80,
            24,
        );
        session.add_tab(
            session.workspace.selected_space,
            "two".into(),
            80,
            24,
            PathBuf::from("/tmp/a"),
        );
        session.add_tab(
            session.workspace.selected_space,
            "three".into(),
            80,
            24,
            PathBuf::from("/tmp/a"),
        );
        assert_eq!(session.selected_space().selected_tab, TabId(3));

        session.remove_tab(TabId(3)).expect("third tab removed");
        assert_eq!(session.selected_space().selected_tab, TabId(2));
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

    #[test]
    fn add_space_creates_a_selected_space_with_its_own_bootstrap_tab() {
        let mut session = Session::new(
            WorkspaceId("workspace-a".into()),
            PathBuf::from("/tmp/a"),
            80,
            24,
        );
        let pane = session.add_space("frontend".into(), PathBuf::from("/tmp/frontend"), 80, 24);
        assert_eq!(session.workspace.spaces.len(), 2);
        assert_eq!(session.selected_space().label, "frontend");
        assert_eq!(session.selected_space().tabs.len(), 1);
        assert_eq!(session.selected_tab().focus.pane, pane);
    }

    #[test]
    fn remove_space_refuses_the_last_space_but_allows_the_rest_and_returns_every_pane() {
        let mut session = Session::new(
            WorkspaceId("workspace-a".into()),
            PathBuf::from("/tmp/a"),
            80,
            24,
        );
        let first_space = session.workspace.selected_space;
        assert_eq!(session.remove_space(first_space), None);

        session.add_space("frontend".into(), PathBuf::from("/tmp/frontend"), 80, 24);
        let second_space = session.workspace.selected_space;
        session.add_tab(
            session.workspace.selected_space,
            "extra".into(),
            80,
            24,
            PathBuf::from("/tmp/a"),
        );
        assert_eq!(session.selected_space().tabs.len(), 2);

        let removed = session
            .remove_space(second_space)
            .expect("second space removed");
        assert_eq!(removed.len(), 2);
        assert_eq!(session.workspace.spaces.len(), 1);
        assert_eq!(session.workspace.selected_space, first_space);
    }

    #[test]
    fn rename_space_trims_refuses_blank_and_reports_real_changes_only() {
        let mut session = Session::new(
            WorkspaceId("workspace-a".into()),
            PathBuf::from("/tmp/a"),
            80,
            24,
        );
        let space = session.workspace.selected_space;
        assert!(session.rename_space(space, "  frontend  ".into()));
        assert_eq!(session.selected_space().label, "frontend");
        assert!(!session.rename_space(space, "frontend".into()));
        assert!(!session.rename_space(space, "   ".into()));
        assert!(!session.rename_space(SpaceId(99), "ghost".into()));
    }

    #[test]
    fn select_space_moves_selection_only_when_the_target_exists_and_differs() {
        let mut session = Session::new(
            WorkspaceId("workspace-a".into()),
            PathBuf::from("/tmp/a"),
            80,
            24,
        );
        let first_space = session.workspace.selected_space;
        session.add_space("frontend".into(), PathBuf::from("/tmp/frontend"), 80, 24);
        let second_space = session.workspace.selected_space;

        assert!(!session.select_space(second_space), "already selected");
        assert!(session.select_space(first_space));
        assert_eq!(session.workspace.selected_space, first_space);
        assert!(!session.select_space(SpaceId(99)), "unknown space");
        assert_eq!(session.workspace.selected_space, first_space);
    }

    /// The one genuinely new invariant this layer introduces: tab lookups
    /// (`rename_tab`/`update_pane_status`) must find a tab that lives in a
    /// space other than the currently selected one, not just search the
    /// selected space's own list.
    #[test]
    fn tab_operations_reach_a_tab_in_a_non_selected_space() {
        let mut session = Session::new(
            WorkspaceId("workspace-a".into()),
            PathBuf::from("/tmp/a"),
            80,
            24,
        );
        let original_tab = session.selected_tab().id;
        let original_pane = session.selected_tab().focus.pane;
        session.add_space("frontend".into(), PathBuf::from("/tmp/frontend"), 80, 24);
        // The newly added space is now selected; `original_tab` lives in
        // the *other*, non-selected space.
        assert_ne!(session.selected_tab().id, original_tab);

        assert!(session.rename_tab(original_tab, "renamed".into()));
        assert!(session.update_pane_status(
            original_pane,
            PathBuf::from("/tmp/moved"),
            "vim".into()
        ));

        // The change must have landed on the non-selected space's tab, not
        // the currently selected one — switch back and check.
        let original_space = session.workspace.spaces[0].id;
        session.select_space(original_space);
        assert_eq!(session.selected_tab().label, "renamed");
        let Layout::Pane(pane) = &session.selected_tab().layout else {
            panic!("expected a single pane layout");
        };
        assert_eq!(pane.cwd, PathBuf::from("/tmp/moved"));
    }

    #[test]
    fn restore_rebuilds_the_seeded_shape_with_sequential_ids() {
        let session = Session::restore(
            WorkspaceId("workspace-a".into()),
            PathBuf::from("/tmp/a"),
            80,
            24,
            vec![
                SpaceSeed {
                    label: "frontend".into(),
                    root: PathBuf::from("/tmp/seed"),
                    tabs: vec![
                        TabSeed {
                            label: "claude".into(),
                            cwd: PathBuf::from("/tmp/a/web"),
                        },
                        TabSeed {
                            label: "shell".into(),
                            cwd: PathBuf::from("/tmp/a"),
                        },
                    ],
                },
                SpaceSeed {
                    label: "backend".into(),
                    root: PathBuf::from("/tmp/seed"),
                    tabs: vec![TabSeed {
                        label: "codex".into(),
                        cwd: PathBuf::from("/tmp/a/api"),
                    }],
                },
            ],
        );

        assert_eq!(session.workspace.spaces.len(), 2);
        assert_eq!(session.workspace.selected_space, SpaceId(1));

        let frontend = &session.workspace.spaces[0];
        assert_eq!(frontend.id, SpaceId(1));
        assert_eq!(frontend.label, "frontend");
        assert_eq!(frontend.tabs.len(), 2);
        assert_eq!(frontend.selected_tab, frontend.tabs[0].id);
        assert_eq!(frontend.tabs[0].id, TabId(1));
        assert_eq!(frontend.tabs[0].label, "claude");
        let Layout::Pane(pane) = &frontend.tabs[0].layout else {
            panic!("expected a single pane layout");
        };
        assert_eq!(pane.id, PaneId(1));
        assert_eq!(pane.cwd, PathBuf::from("/tmp/a/web"));
        assert_eq!(frontend.tabs[1].id, TabId(2));

        let backend = &session.workspace.spaces[1];
        assert_eq!(backend.id, SpaceId(2));
        assert_eq!(backend.tabs[0].id, TabId(3));
        let Layout::Pane(pane) = &backend.tabs[0].layout else {
            panic!("expected a single pane layout");
        };
        assert_eq!(pane.id, PaneId(3));

        // Ids allocated after a restore must not collide with any restored
        // one — proves the counters were advanced past the highest id
        // handed out, not reset to the defaults `Session::new` starts at.
        assert_eq!(session.next_space_id, 3);
        assert_eq!(session.next_tab_id, 4);
        assert_eq!(session.next_pane_id, 4);
    }

    #[test]
    fn restore_with_no_usable_seeds_falls_back_to_the_ordinary_bootstrap() {
        let restored = Session::restore(
            WorkspaceId("workspace-a".into()),
            PathBuf::from("/tmp/a"),
            80,
            24,
            vec![SpaceSeed {
                label: "empty".into(),
                root: PathBuf::from("/tmp/seed"),
                tabs: vec![],
            }],
        );
        let fresh = Session::new(
            WorkspaceId("workspace-a".into()),
            PathBuf::from("/tmp/a"),
            80,
            24,
        );
        assert_eq!(restored, fresh);
    }
}
