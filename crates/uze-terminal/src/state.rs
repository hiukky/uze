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
    /// The agent tab this one was born from — a shell opened while an agent
    /// was in front of the person, which therefore starts in that agent's
    /// own directory and is shown with it. `None` is a tab that belongs to
    /// the space itself: its bootstrap shell, a shell opened with no agent
    /// selected, and every agent tab (an agent *is* a context; it does not
    /// sit inside one).
    pub agent: Option<TabId>,
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

/// What [`Session::open_space`] found or did — the caller only has a pane
/// to spawn when a space was actually created.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpenedSpace {
    /// A space for that root was already open, and is the answer.
    Existing(SpaceId),
    /// A new space, whose bootstrap pane is not running yet.
    Created { space: SpaceId, pane: PaneId },
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
    /// Which tab of this same space this one belongs with, by index into
    /// `SpaceSeed::tabs` — a seed cannot name a [`TabId`], since restoring
    /// mints fresh ones. Out of range, or pointing at itself, restores as
    /// `None`.
    pub agent: Option<usize>,
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
            agent: None,
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
            // Every tab of this space gets its id before any `agent` is
            // resolved: a seed names its agent by position, and the tab at
            // that position may not have been minted yet.
            let first_tab_id = next_tab_id;
            let seeded = seed.tabs.len();
            let mut tabs = Vec::new();
            for (index, tab_seed) in seed.tabs.into_iter().enumerate() {
                let tab_id = TabId(next_tab_id);
                let pane_id = PaneId(next_pane_id);
                next_tab_id += 1;
                next_pane_id += 1;
                tabs.push(Tab {
                    id: tab_id,
                    label: tab_seed.label,
                    agent: tab_seed
                        .agent
                        .filter(|agent| *agent != index && *agent < seeded)
                        .map(|agent| TabId(first_tab_id + agent as u64)),
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

    /// Goes to the space rooted at `root`: the one already open for it
    /// when there is one, a new one otherwise (labelled `label`, or after
    /// the directory itself).
    ///
    /// This is "take me to this directory" — what `uze` run inside a pane
    /// asks — and landing on the space that already has it is the whole
    /// answer. Asking for a *new* space over a directory is a different
    /// question, and [`Session::create_space`] answers it by creating one
    /// whatever is already open there.
    pub fn open_space(
        &mut self,
        label: Option<String>,
        root: PathBuf,
        columns: u16,
        rows: u16,
    ) -> OpenedSpace {
        if let Some(space) = self.space_for_root(&root) {
            return OpenedSpace::Existing(space);
        }
        let label = label.unwrap_or_else(|| space_label(&root));
        let pane = self.add_space(label, root, columns, rows);
        OpenedSpace::Created {
            space: self.workspace.selected_space,
            pane,
        }
    }

    /// Opens a new space at `root`, whatever is already open there.
    ///
    /// Two spaces over one directory is a normal way to work — one per
    /// branch, one per thing being tried — and the "+ new" prompt is an
    /// explicit request for one, not a lookup. What the duplicate needs
    /// is to be tellable apart, so a label already on screen is numbered
    /// rather than repeated.
    pub fn create_space(
        &mut self,
        label: Option<String>,
        root: PathBuf,
        columns: u16,
        rows: u16,
    ) -> PaneId {
        let label = label.unwrap_or_else(|| self.unrepeated_label(&root));
        self.add_space(label, root, columns, rows)
    }

    /// `space_label`'s answer for `root`, numbered from 2 while a space on
    /// screen already carries it.
    fn unrepeated_label(&self, root: &Path) -> String {
        let base = space_label(root);
        let taken = |name: &str| {
            self.workspace
                .spaces
                .iter()
                .any(|space| space.label == name)
        };
        if !taken(&base) {
            return base;
        }
        (2..)
            .map(|ordinal| format!("{base} {ordinal}"))
            .find(|candidate| !taken(candidate))
            .expect("an unused ordinal always exists")
    }

    /// Creates a space rooted at `root` with one default tab (mirroring
    /// [`Session::new`]'s own bootstrap tab), selects it, and returns the
    /// new tab's pane so the caller can spawn it exactly like
    /// [`Session::add_tab`]'s result. [`Session::create_space`] is the
    /// entry point that names it; [`Session::open_space`] the one that
    /// first looks for a space already there.
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
                agent: None,
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
        agent: Option<TabId>,
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
        // A tab can only belong with an agent of its own space — a client
        // naming one from elsewhere (or one that has since been closed)
        // gets a tab of the space itself rather than a dangling reference.
        let agent = agent.filter(|agent| space.tabs.iter().any(|tab| tab.id == *agent));
        space.tabs.push(Tab {
            id: tab_id,
            label,
            agent,
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
        // Closing an agent never closes the shells opened alongside it —
        // they carry a person's work and outlive the agent. They become
        // the space's own instead of pointing at a tab that is gone.
        for orphan in space.tabs.iter_mut().filter(|t| t.agent == Some(tab)) {
            orphan.agent = None;
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

    /// Moves `tab` (found by searching every space, same as `rename_tab`)
    /// to sit immediately before `before` within its own space's `tabs` —
    /// `before: None` moves it to the end. Reports whether the order
    /// actually changed, same contract as every other mutation here.
    ///
    /// `before` is looked up *within the space `tab` was found in*, not
    /// searched for globally — naming a tab of a different space, or one
    /// that no longer exists, is indistinguishable from "not found" and
    /// simply refused, the same way `add_tab`'s `agent` is refused rather
    /// than silently substituted. This is also what confines a reorder to
    /// the dragged tab's own group: nothing here restricts `before` to an
    /// agent tab or a shell tab specifically, but the client never offers
    /// one from outside the group being dragged within in the first place
    /// (see the workspace TUI's drag hit-testing).
    pub fn reorder_tab(&mut self, tab: TabId, before: Option<TabId>) -> bool {
        let Some(space) = space_containing_tab_mut(&mut self.workspace.spaces, tab) else {
            return false;
        };
        let Some(from) = space.tabs.iter().position(|t| t.id == tab) else {
            return false;
        };
        let insert_at = match before {
            Some(before) if before == tab => return false,
            Some(before) => match space.tabs.iter().position(|t| t.id == before) {
                Some(index) => index,
                None => return false,
            },
            None => space.tabs.len(),
        };
        // `insert_at` is an index into the vec as it stands *before* `tab`
        // is removed; removing `tab` shifts everything after it down by
        // one, so a target past `tab`'s own position needs the same
        // adjustment before it says where `tab` actually lands.
        let landing = if insert_at > from {
            insert_at - 1
        } else {
            insert_at
        };
        if landing == from {
            return false;
        }
        let moved = space.tabs.remove(from);
        space.tabs.insert(landing, moved);
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
                None,
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
            None,
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
            None,
            80,
            24,
            PathBuf::from("/tmp/a"),
        );
        session.add_tab(
            session.workspace.selected_space,
            "three".into(),
            None,
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

    /// A directory is not owned by the first space that opened it: asking
    /// for a space over one already open creates one, and the repeated
    /// name is numbered so the two rows are tellable apart.
    #[test]
    fn a_second_space_over_one_directory_opens_under_a_numbered_name() {
        let mut session = Session::new(
            WorkspaceId("workspace-a".into()),
            PathBuf::from("/tmp/a"),
            80,
            24,
        );

        session.create_space(None, PathBuf::from("/tmp/frontend"), 80, 24);
        assert_eq!(session.selected_space().label, "frontend");

        let pane = session.create_space(None, PathBuf::from("/tmp/frontend"), 80, 24);
        assert_eq!(session.workspace.spaces.len(), 3);
        assert_eq!(session.selected_space().label, "frontend 2");
        assert_eq!(
            session.selected_space().root,
            PathBuf::from("/tmp/frontend")
        );
        assert_eq!(session.selected_tab().focus.pane, pane);

        session.create_space(None, PathBuf::from("/tmp/frontend"), 80, 24);
        assert_eq!(session.selected_space().label, "frontend 3");

        // A name the caller gives is its own business, repeated or not.
        session.create_space(
            Some("frontend".into()),
            PathBuf::from("/tmp/frontend"),
            80,
            24,
        );
        assert_eq!(session.selected_space().label, "frontend");
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

    /// "Take me to this directory" is answered by the space that has it,
    /// not by a new one — this is the path `uze` run inside a pane takes,
    /// where a second row over the same directory is never what was meant.
    /// Asking for a space outright is `create_space`, below.
    #[test]
    fn going_to_a_root_twice_lands_on_the_same_space() {
        let mut session = Session::new(
            WorkspaceId("workspace-a".into()),
            PathBuf::from("/tmp/a"),
            80,
            24,
        );

        let first = session.open_space(None, PathBuf::from("/tmp/frontend"), 80, 24);
        let OpenedSpace::Created { space, pane } = first else {
            panic!("the first open creates: {first:?}");
        };
        assert_eq!(session.workspace.spaces.len(), 2);
        assert_eq!(session.selected_space().label, "frontend");
        assert_eq!(session.selected_tab().focus.pane, pane);

        let again = session.open_space(
            Some("a second name".into()),
            PathBuf::from("/tmp/frontend"),
            80,
            24,
        );
        assert_eq!(again, OpenedSpace::Existing(space));
        assert_eq!(session.workspace.spaces.len(), 2, "no second row");
        assert_eq!(
            session.space(space).expect("still open").label,
            "frontend",
            "and the open space keeps the name it has"
        );
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
            None,
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

    /// A shell opened alongside an agent belongs with it, and only an
    /// agent of its own space can be named — a tab from elsewhere leaves
    /// the new one belonging to the space itself rather than dangling.
    #[test]
    fn a_tab_belongs_only_with_an_agent_of_its_own_space() {
        let mut session = Session::new(
            WorkspaceId("workspace-a".into()),
            PathBuf::from("/tmp/a"),
            80,
            24,
        );
        let first_space = session.workspace.selected_space;
        let agent = session.selected_tab().id;
        session.add_tab(
            first_space,
            "shell".into(),
            Some(agent),
            80,
            24,
            PathBuf::from("/tmp/a"),
        );
        assert_eq!(session.selected_tab().agent, Some(agent));

        session.add_space("frontend".into(), PathBuf::from("/tmp/frontend"), 80, 24);
        let elsewhere = session.workspace.selected_space;
        session.add_tab(
            elsewhere,
            "shell".into(),
            Some(agent),
            80,
            24,
            PathBuf::from("/tmp/frontend"),
        );
        assert_eq!(
            session.selected_tab().agent,
            None,
            "an agent of another space is no context of this one"
        );
    }

    /// A shell holds a person's own work and outlives the agent it was
    /// opened next to — closing the agent hands it to the space rather
    /// than leaving it pointing at a tab that is gone.
    #[test]
    fn closing_an_agent_hands_its_shells_back_to_the_space() {
        let mut session = Session::new(
            WorkspaceId("workspace-a".into()),
            PathBuf::from("/tmp/a"),
            80,
            24,
        );
        let space = session.workspace.selected_space;
        session.add_tab(space, "agent".into(), None, 80, 24, PathBuf::from("/tmp/a"));
        let agent = session.selected_tab().id;
        session.add_tab(
            space,
            "shell".into(),
            Some(agent),
            80,
            24,
            PathBuf::from("/tmp/a"),
        );
        let shell = session.selected_tab().id;

        session.remove_tab(agent).expect("the agent is removable");

        let space = session.selected_space();
        assert!(space.tabs.iter().any(|tab| tab.id == shell), "it survives");
        assert!(
            space.tabs.iter().all(|tab| tab.agent.is_none()),
            "and belongs to the space now"
        );
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

    /// A seed names its agent by position because restoring mints fresh
    /// ids; what must survive a server restart is which tab belongs with
    /// which, not the numbers they happened to carry.
    #[test]
    fn restoring_rebuilds_which_tab_belongs_with_which() {
        let session = Session::restore(
            WorkspaceId("workspace-a".into()),
            PathBuf::from("/tmp/a"),
            80,
            24,
            vec![SpaceSeed {
                label: "frontend".into(),
                root: PathBuf::from("/tmp/seed"),
                tabs: vec![
                    TabSeed {
                        label: "claude".into(),
                        cwd: PathBuf::from("/tmp/a/web"),
                        agent: None,
                    },
                    TabSeed {
                        label: "shell".into(),
                        cwd: PathBuf::from("/tmp/a/web"),
                        agent: Some(0),
                    },
                    TabSeed {
                        label: "loose".into(),
                        cwd: PathBuf::from("/tmp/a"),
                        agent: Some(7),
                    },
                ],
            }],
        );

        let tabs = &session.workspace.spaces[0].tabs;
        assert_eq!(tabs[1].agent, Some(tabs[0].id));
        assert_eq!(tabs[0].agent, None, "an agent belongs with nothing");
        assert_eq!(tabs[2].agent, None, "an index off the end is nobody");
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
                            agent: None,
                        },
                        TabSeed {
                            label: "shell".into(),
                            cwd: PathBuf::from("/tmp/a"),
                            agent: None,
                        },
                    ],
                },
                SpaceSeed {
                    label: "backend".into(),
                    root: PathBuf::from("/tmp/seed"),
                    tabs: vec![TabSeed {
                        label: "codex".into(),
                        cwd: PathBuf::from("/tmp/a/api"),
                        agent: None,
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
    fn reorder_tab_moves_among_agent_tabs_and_ignores_a_no_op_move() {
        let mut session = Session::new(
            WorkspaceId("workspace-a".into()),
            PathBuf::from("/tmp/a"),
            80,
            24,
        );
        let space = session.workspace.selected_space;
        // The bootstrap tab (id 1) plus two agent tabs: [1, 2, 3].
        session.add_tab(space, "b".into(), None, 80, 24, PathBuf::from("/tmp/a"));
        session.add_tab(space, "c".into(), None, 80, 24, PathBuf::from("/tmp/a"));
        let ids = |session: &Session| -> Vec<u64> {
            session
                .selected_space()
                .tabs
                .iter()
                .map(|t| t.id.0)
                .collect()
        };
        assert_eq!(ids(&session), vec![1, 2, 3]);

        // Moving tab 1 before tab 2 (its immediate successor) is already
        // the current order — a no-op.
        assert!(!session.reorder_tab(TabId(1), Some(TabId(2))));
        assert_eq!(ids(&session), vec![1, 2, 3]);

        // Moving tab 1 before tab 3 puts it between 2 and 3.
        assert!(session.reorder_tab(TabId(1), Some(TabId(3))));
        assert_eq!(ids(&session), vec![2, 1, 3]);
    }

    #[test]
    fn reorder_tab_moves_among_one_agents_shell_tabs() {
        let mut session = Session::new(
            WorkspaceId("workspace-a".into()),
            PathBuf::from("/tmp/a"),
            80,
            24,
        );
        let space = session.workspace.selected_space;
        let agent = session.selected_tab().id;
        session.add_tab(
            space,
            "shell-a".into(),
            Some(agent),
            80,
            24,
            PathBuf::from("/tmp/a"),
        );
        session.add_tab(
            space,
            "shell-b".into(),
            Some(agent),
            80,
            24,
            PathBuf::from("/tmp/a"),
        );
        // [agent(1), shell-a(2), shell-b(3)].
        let shell_a = TabId(2);
        let shell_b = TabId(3);

        assert!(session.reorder_tab(shell_b, Some(shell_a)));
        let ids: Vec<u64> = session
            .selected_space()
            .tabs
            .iter()
            .map(|t| t.id.0)
            .collect();
        assert_eq!(ids, vec![1, 3, 2], "shell-b now sits before shell-a");
        // Reordering never touches which agent a shell belongs with.
        assert_eq!(session.selected_space().tabs[1].agent, Some(agent));
        assert_eq!(session.selected_space().tabs[2].agent, Some(agent));
    }

    #[test]
    fn reorder_tab_moves_to_the_end_when_before_is_none() {
        let mut session = Session::new(
            WorkspaceId("workspace-a".into()),
            PathBuf::from("/tmp/a"),
            80,
            24,
        );
        let space = session.workspace.selected_space;
        session.add_tab(space, "b".into(), None, 80, 24, PathBuf::from("/tmp/a"));
        session.add_tab(space, "c".into(), None, 80, 24, PathBuf::from("/tmp/a"));

        // Tab 3 is already last — moving it to the end is a no-op.
        assert!(!session.reorder_tab(TabId(3), None));

        assert!(session.reorder_tab(TabId(1), None));
        let ids: Vec<u64> = session
            .selected_space()
            .tabs
            .iter()
            .map(|t| t.id.0)
            .collect();
        assert_eq!(ids, vec![2, 3, 1]);
    }

    #[test]
    fn reorder_tab_rejects_a_target_from_a_different_space() {
        let mut session = Session::new(
            WorkspaceId("workspace-a".into()),
            PathBuf::from("/tmp/a"),
            80,
            24,
        );
        let first_space_tab = session.selected_tab().id;
        session.add_space("frontend".into(), PathBuf::from("/tmp/frontend"), 80, 24);
        let other_space_tab = session.selected_tab().id;
        assert_ne!(first_space_tab, other_space_tab);

        assert!(!session.reorder_tab(first_space_tab, Some(other_space_tab)));
        // Neither space's order changed.
        assert_eq!(session.workspace.spaces[0].tabs[0].id, first_space_tab);
        assert_eq!(session.workspace.spaces[1].tabs[0].id, other_space_tab);
    }

    #[test]
    fn reorder_tab_rejects_missing_tabs_and_self_targeting() {
        let mut session = Session::new(
            WorkspaceId("workspace-a".into()),
            PathBuf::from("/tmp/a"),
            80,
            24,
        );
        let space = session.workspace.selected_space;
        session.add_tab(space, "b".into(), None, 80, 24, PathBuf::from("/tmp/a"));

        assert!(
            !session.reorder_tab(TabId(99), Some(TabId(1))),
            "no such tab"
        );
        assert!(
            !session.reorder_tab(TabId(1), Some(TabId(99))),
            "no such target"
        );
        assert!(
            !session.reorder_tab(TabId(1), Some(TabId(1))),
            "before itself"
        );
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
