use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::launch::Launch;

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
    pub spaces: Vec<Space>,
    pub selected_space: SpaceId,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Space {
    pub id: SpaceId,
    pub label: String,
    /// Where this space's work lives, chosen when it was created: an agent
    /// created here starts from it, and a shell opens in it.
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

/// Where a space sits — what a request names for a space the server may
/// have to open on the client's behalf. A root and nothing else: a space
/// has no kind to be told apart by, so one directory names one space.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SpaceSeat {
    pub root: PathBuf,
}

/// What [`Session::remove_space`] leaves the caller to do: stop `panes`,
/// and spawn `replacement` when the removed space was the last one.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct RemovedSpace {
    pub(crate) panes: Vec<PaneId>,
    pub(crate) replacement: Option<NewSpace>,
}

/// A space just created, whose first pane is not running yet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NewSpace {
    pub space: SpaceId,
    pub pane: PaneId,
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
    /// What the tab's launch put into its first process's environment
    /// beyond the pane's own — the server's record of the launch it made,
    /// which a client reads back to know which agent a tab is for. Empty
    /// for a shell, and emptied again when a finished agent's pane is
    /// respawned as one.
    pub env: crate::launch::Environment,
    pub pane: Pane,
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

/// What [`Session::open_space`] found or did — the caller only has a pane
/// to spawn when a space was actually created.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OpenedSpace {
    /// A space for that seat was already open, and is the answer.
    Existing(SpaceId),
    Created(NewSpace),
}

/// The size a pane is spawned at before any client has said how large it
/// is drawn — a restored pane, or a space opened on a client's behalf with
/// no size to go by. The next resize from a client that shows it corrects
/// it.
pub(crate) const PLACEHOLDER_PANE_SIZE: (u16, u16) = (80, 24);

/// A space as it outlives the server: the shape a previous instance had,
/// and what each of its tabs was launched as — written on every structural
/// change, so a crash or a reboot leaves no more than one change unsaved.
#[derive(Deserialize, Serialize)]
pub(crate) struct SpaceSeed {
    pub(crate) label: String,
    pub(crate) root: PathBuf,
    pub(crate) tabs: Vec<TabSeed>,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct TabSeed {
    pub(crate) label: String,
    pub(crate) cwd: PathBuf,
    /// Which tab of this same space this one belongs with, by index into
    /// `SpaceSeed::tabs` — a seed cannot name a [`TabId`], since restoring
    /// mints fresh ones. Out of range, or pointing at itself, restores as
    /// `None`.
    pub(crate) agent: Option<usize>,
    pub(crate) launch: Launch,
}

impl Session {
    pub fn new(seat: SpaceSeat, columns: u16, rows: u16) -> Self {
        let mut session = Self::empty();
        session.add_space(space_label(&seat.root), seat, columns, rows);
        session
    }

    fn empty() -> Self {
        Self {
            workspace: Workspace {
                spaces: Vec::new(),
                selected_space: SpaceId(1),
            },
            next_space_id: 1,
            next_tab_id: 1,
            next_pane_id: 1,
        }
    }

    /// Rebuilds the shape `seeds` describe, minting ids the way ordinary
    /// use does, and pairs every pane with the launch it is to be spawned
    /// with. A seed space with no tabs is dropped — a space always has
    /// somewhere to focus — and `None` is a workspace with nothing left
    /// standing.
    pub(crate) fn restore(seeds: Vec<SpaceSeed>) -> Option<(Self, Vec<(PaneId, Launch)>)> {
        let (columns, rows) = PLACEHOLDER_PANE_SIZE;
        let mut session = Self::empty();
        let mut launches = Vec::new();
        for seed in seeds.into_iter().filter(|seed| !seed.tabs.is_empty()) {
            // Tab ids are minted in order, so the tab a seed names by
            // position is known before it exists.
            let first_tab = session.next_tab_id;
            let seeded = seed.tabs.len();
            let mut tabs = Vec::with_capacity(seeded);
            for (index, tab) in seed.tabs.into_iter().enumerate() {
                let agent = tab
                    .agent
                    .filter(|agent| *agent != index && *agent < seeded)
                    .map(|agent| TabId(first_tab + agent as u64));
                let minted = session.mint_tab(tab.label, agent, tab.cwd, columns, rows);
                launches.push((minted.pane.id, tab.launch));
                tabs.push(minted);
            }
            let seat = SpaceSeat { root: seed.root };
            session.push_space(seed.label, seat, tabs);
        }
        session.workspace.selected_space = session.workspace.spaces.first()?.id;
        Some((session, launches))
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

    /// The space open at `seat`, its root compared as the filesystem sees
    /// it. One root names one space: a directory reached by two spellings
    /// is the same place, and there is nothing else for a space to be.
    pub fn space_for(&self, seat: &SpaceSeat) -> Option<SpaceId> {
        let canonical = |root: &Path| root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let wanted = canonical(&seat.root);
        self.workspace
            .spaces
            .iter()
            .find(|space| canonical(&space.root) == wanted)
            .map(|space| space.id)
    }

    pub fn space(&self, space: SpaceId) -> Option<&Space> {
        self.workspace.spaces.iter().find(|s| s.id == space)
    }

    /// Goes to the space at `seat`: the one already open there when there
    /// is one, a new one named after its directory otherwise.
    ///
    /// This is "take me to this directory" — what `uze` run inside a pane
    /// asks — and landing on the space that already has it is the whole
    /// answer. Asking for a *new* space over a directory is a different
    /// question, and [`Session::create_space`] answers it by creating one
    /// whatever is already open there.
    pub(crate) fn open_space(&mut self, seat: SpaceSeat, columns: u16, rows: u16) -> OpenedSpace {
        if let Some(space) = self.space_for(&seat) {
            return OpenedSpace::Existing(space);
        }
        let label = space_label(&seat.root);
        OpenedSpace::Created(self.add_space(label, seat, columns, rows))
    }

    /// Opens a new space at `seat`, whatever is already open there.
    ///
    /// Two spaces over one directory is a normal way to work — one per
    /// branch, one per thing being tried — and the "+ new" prompt is an
    /// explicit request for one, not a lookup. What the duplicate needs
    /// is to be tellable apart, so a label already on screen is numbered
    /// rather than repeated.
    pub fn create_space(
        &mut self,
        label: Option<String>,
        seat: SpaceSeat,
        columns: u16,
        rows: u16,
    ) -> NewSpace {
        let label = label.unwrap_or_else(|| self.unrepeated_label(&seat.root));
        self.add_space(label, seat, columns, rows)
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

    /// A selected space at `seat` holding one shell tab.
    fn add_space(&mut self, label: String, seat: SpaceSeat, columns: u16, rows: u16) -> NewSpace {
        let tab = self.mint_tab("shell".to_owned(), None, seat.root.clone(), columns, rows);
        let pane = tab.pane.id;
        let space = self.push_space(label, seat, vec![tab]);
        NewSpace { space, pane }
    }

    /// Adds a space holding `tabs`, the first of them selected, and selects
    /// it.
    fn push_space(&mut self, label: String, seat: SpaceSeat, tabs: Vec<Tab>) -> SpaceId {
        let id = SpaceId(self.next_space_id);
        self.next_space_id += 1;
        let SpaceSeat { root } = seat;
        self.workspace.spaces.push(Space {
            id,
            label,
            root,
            selected_tab: tabs[0].id,
            tabs,
        });
        self.workspace.selected_space = id;
        id
    }

    /// A tab with a fresh id, holding a pane with a fresh id that has not
    /// been spawned yet.
    fn mint_tab(
        &mut self,
        label: String,
        agent: Option<TabId>,
        cwd: PathBuf,
        columns: u16,
        rows: u16,
    ) -> Tab {
        let tab = TabId(self.next_tab_id);
        let pane = PaneId(self.next_pane_id);
        self.next_tab_id += 1;
        self.next_pane_id += 1;
        Tab {
            id: tab,
            label,
            agent,
            env: Vec::new(),
            pane: Pane {
                id: pane,
                cwd,
                columns,
                rows,
                process: "shell".to_owned(),
            },
        }
    }

    /// Removes `space` and returns every pane across every tab it owned, so
    /// the caller can stop them all. Removing the workspace's last space
    /// opens `replacement` in its place — a workspace always has somewhere
    /// to focus, the same invariant [`Session::remove_tab`] holds for tabs —
    /// and reports that space's first pane for the caller to spawn.
    pub(crate) fn remove_space(
        &mut self,
        space: SpaceId,
        replacement: SpaceSeat,
        columns: u16,
        rows: u16,
    ) -> Option<RemovedSpace> {
        let index = self.workspace.spaces.iter().position(|s| s.id == space)?;
        let removed = self.workspace.spaces.remove(index);
        let panes = removed.tabs.iter().map(|tab| tab.pane.id).collect();
        if self.workspace.spaces.is_empty() {
            return Some(RemovedSpace {
                panes,
                replacement: Some(self.create_space(None, replacement, columns, rows)),
            });
        }
        if self.workspace.selected_space == space {
            let next = index.min(self.workspace.spaces.len() - 1);
            self.workspace.selected_space = self.workspace.spaces[next].id;
        }
        Some(RemovedSpace {
            panes,
            replacement: None,
        })
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

    /// Moves `space` to sit immediately before `before` in the sidebar's
    /// order (`before: None` moves it to the end), reporting whether the
    /// order changed — the same contract as [`Session::reorder_tab`].
    pub fn reorder_space(&mut self, space: SpaceId, before: Option<SpaceId>) -> bool {
        let spaces = &mut self.workspace.spaces;
        let Some(from) = spaces.iter().position(|candidate| candidate.id == space) else {
            return false;
        };
        let insert_at = match before {
            Some(before) if before == space => return false,
            Some(before) => match spaces.iter().position(|candidate| candidate.id == before) {
                Some(index) => index,
                None => return false,
            },
            None => spaces.len(),
        };
        // An index into the order before `space` leaves it, as in
        // `reorder_tab`.
        let landing = if insert_at > from {
            insert_at - 1
        } else {
            insert_at
        };
        if landing == from {
            return false;
        }
        let moved = spaces.remove(from);
        spaces.insert(landing, moved);
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
        let default = self.workspace.selected_space;
        let index = self
            .workspace
            .spaces
            .iter()
            .position(|s| s.id == space)
            .or_else(|| self.workspace.spaces.iter().position(|s| s.id == default))
            .expect("session selected space is always present");
        // A tab can only belong with an agent of its own space — a client
        // naming one from elsewhere (or one that has since been closed)
        // gets a tab of the space itself rather than a dangling reference.
        let agent = agent.filter(|agent| {
            self.workspace.spaces[index]
                .tabs
                .iter()
                .any(|tab| tab.id == *agent)
        });
        let tab = self.mint_tab(label, agent, cwd, columns, rows);
        let pane = tab.pane.id;
        let space = &mut self.workspace.spaces[index];
        space.selected_tab = tab.id;
        space.tabs.push(tab);
        pane
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
        Some(vec![removed.pane.id])
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

    /// Records what `pane`'s tab was launched with: the environment the
    /// spawn applied, or none when the pane came up as a plain shell. The
    /// server is the only writer, at the moment it spawns, so what a tab
    /// reports is the server's own record of the launch it made and never
    /// something a pane's processes can rewrite.
    pub fn record_launch(&mut self, pane: PaneId, env: crate::launch::Environment) -> bool {
        let Some(tab) = self.tab_of_pane_mut(pane) else {
            return false;
        };
        if tab.env == env {
            return false;
        }
        tab.env = env;
        true
    }

    /// Applies a fresh live-probe reading for `pane`'s cwd/process, and
    /// reports whether anything actually changed — so the caller only
    /// broadcasts a `SessionUpdated` when the sidebar tree would show
    /// something new, not on every probe tick.
    pub fn update_pane_status(&mut self, pane: PaneId, cwd: PathBuf, process: String) -> bool {
        let Some(tab) = self.tab_of_pane_mut(pane) else {
            return false;
        };
        if tab.pane.cwd == cwd && tab.pane.process == process {
            return false;
        }
        tab.pane.cwd = cwd;
        tab.pane.process = process;
        true
    }

    /// The pane `pane`, in whichever space's tab it lives.
    pub fn pane(&self, pane: PaneId) -> Option<&Pane> {
        self.workspace
            .spaces
            .iter()
            .flat_map(|space| &space.tabs)
            .map(|tab| &tab.pane)
            .find(|candidate| candidate.id == pane)
    }

    fn tab_of_pane_mut(&mut self, pane: PaneId) -> Option<&mut Tab> {
        self.workspace
            .spaces
            .iter_mut()
            .flat_map(|space| &mut space.tabs)
            .find(|tab| tab.pane.id == pane)
    }
}

fn space_containing_tab_mut(spaces: &mut [Space], tab: TabId) -> Option<&mut Space> {
    spaces
        .iter_mut()
        .find(|space| space.tabs.iter().any(|t| t.id == tab))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_round_trips_and_allocates_stable_identifiers() {
        let mut session = Session::new(seat("/tmp/a"), 80, 24);
        assert_eq!(session.selected_tab().pane.id, PaneId(1));
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
        let pane = &session.selected_tab().pane;
        assert_eq!(pane.cwd, PathBuf::from("/tmp/agent"));
        let encoded = serde_json::to_string(&session).unwrap();
        assert_eq!(serde_json::from_str::<Session>(&encoded).unwrap(), session);
    }

    #[test]
    fn remove_tab_refuses_the_last_tab_but_allows_the_rest() {
        let mut session = Session::new(seat("/tmp/a"), 80, 24);
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
        let mut session = Session::new(seat("/tmp/a"), 80, 24);
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

    /// One directory names one space. Opening a root that already has
    /// one selects it — the first round let a root carry a space of each
    /// kind, which was the workaround for a choice made too early and is
    /// the thing this round removed.
    #[test]
    fn a_root_names_one_space() {
        let root = PathBuf::from("/tmp/shared");
        let mut session = Session::new(seat("/tmp/elsewhere"), 80, 24);
        let first = session.open_space(SpaceSeat { root: root.clone() }, 80, 24);
        let OpenedSpace::Created(NewSpace { space, .. }) = first else {
            panic!("the first open creates");
        };
        assert_eq!(
            session.open_space(SpaceSeat { root: root.clone() }, 80, 24),
            OpenedSpace::Existing(space),
            "the same root is the same space"
        );
        assert_eq!(
            session.space_for(&SpaceSeat { root: root.clone() }),
            Some(space)
        );
    }

    #[test]
    fn a_launch_is_recorded_on_the_tab_owning_the_pane_and_cleared_by_a_shell() {
        let mut session = Session::new(seat("/tmp/a"), 80, 24);
        let space = session.workspace.selected_space;
        let pane = session.add_tab(
            space,
            "agent 1".into(),
            None,
            80,
            24,
            PathBuf::from("/tmp/a"),
        );
        let stamp = vec![("UZE_AGENT".to_owned(), "abc".to_owned())];
        assert!(session.record_launch(pane, stamp.clone()));
        assert_eq!(session.selected_tab().env, stamp);
        assert!(
            !session.record_launch(pane, stamp),
            "recording the same launch changes nothing"
        );
        assert!(session.record_launch(pane, Vec::new()));
        assert!(session.selected_tab().env.is_empty());
        assert!(
            !session.record_launch(PaneId(99), Vec::new()),
            "an unknown pane records nothing"
        );
    }

    #[test]
    fn update_pane_status_reports_change_only_when_something_actually_moved() {
        let mut session = Session::new(seat("/tmp/a"), 80, 24);
        assert!(!session.update_pane_status(PaneId(1), PathBuf::from("/tmp/a"), "shell".into()));
        assert!(session.update_pane_status(PaneId(1), PathBuf::from("/tmp/b"), "vim".into()));
        assert_eq!(session.selected_tab().pane.id, PaneId(1));
        let pane = &session.selected_tab().pane;
        assert_eq!(pane.cwd, PathBuf::from("/tmp/b"));
        assert_eq!(pane.process, "vim");
        assert!(!session.update_pane_status(PaneId(99), PathBuf::from("/tmp/c"), "x".into()));
    }

    #[test]
    fn rename_tab_trims_refuses_blank_and_reports_real_changes_only() {
        let mut session = Session::new(seat("/tmp/a"), 80, 24);
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
        let mut session = Session::new(seat("/tmp/a"), 80, 24);

        session.create_space(None, seat("/tmp/frontend"), 80, 24);
        assert_eq!(session.selected_space().label, "frontend");

        let NewSpace { pane, .. } = session.create_space(None, seat("/tmp/frontend"), 80, 24);
        assert_eq!(session.workspace.spaces.len(), 3);
        assert_eq!(session.selected_space().label, "frontend 2");
        assert_eq!(
            session.selected_space().root,
            PathBuf::from("/tmp/frontend")
        );
        assert_eq!(session.selected_tab().pane.id, pane);

        session.create_space(None, seat("/tmp/frontend"), 80, 24);
        assert_eq!(session.selected_space().label, "frontend 3");

        // A name the caller gives is its own business, repeated or not.
        session.create_space(Some("frontend".into()), seat("/tmp/frontend"), 80, 24);
        assert_eq!(session.selected_space().label, "frontend");
    }

    #[test]
    fn add_space_creates_a_selected_space_with_its_own_bootstrap_tab() {
        let mut session = Session::new(seat("/tmp/a"), 80, 24);
        let NewSpace { pane, .. } =
            session.add_space("frontend".into(), seat("/tmp/frontend"), 80, 24);
        assert_eq!(session.workspace.spaces.len(), 2);
        assert_eq!(session.selected_space().label, "frontend");
        assert_eq!(session.selected_space().tabs.len(), 1);
        assert_eq!(session.selected_tab().pane.id, pane);
    }

    /// "Take me to this directory" is answered by the space that has it,
    /// not by a new one — this is the path `uze` run inside a pane takes,
    /// where a second row over the same directory is never what was meant.
    /// Asking for a space outright is `create_space`, below.
    #[test]
    fn going_to_a_root_twice_lands_on_the_same_space() {
        let mut session = Session::new(seat("/tmp/a"), 80, 24);

        let first = session.open_space(seat("/tmp/frontend"), 80, 24);
        let OpenedSpace::Created(NewSpace { space, pane }) = first else {
            panic!("the first open creates: {first:?}");
        };
        assert_eq!(session.workspace.spaces.len(), 2);
        assert_eq!(session.selected_space().label, "frontend");
        assert_eq!(session.selected_tab().pane.id, pane);

        let again = session.open_space(seat("/tmp/frontend"), 80, 24);
        assert_eq!(again, OpenedSpace::Existing(space));
        assert_eq!(session.workspace.spaces.len(), 2, "no second row");
        assert_eq!(
            session.space(space).expect("still open").label,
            "frontend",
            "and the open space keeps the name it has"
        );
    }

    #[test]
    fn remove_space_returns_every_pane_and_keeps_the_selection_on_a_survivor() {
        let mut session = Session::new(seat("/tmp/a"), 80, 24);
        let first_space = session.workspace.selected_space;
        session.add_space("frontend".into(), seat("/tmp/frontend"), 80, 24);
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
            .remove_space(second_space, home_seat(), 80, 24)
            .expect("second space removed");
        assert_eq!(removed.panes.len(), 2);
        assert_eq!(removed.replacement, None, "a space still stands");
        assert_eq!(session.workspace.spaces.len(), 1);
        assert_eq!(session.workspace.selected_space, first_space);
    }

    /// The last space can be closed like any other: the workspace is never
    /// left with nowhere to focus, so the seat the client named takes its
    /// place — even when it is the very directory just closed.
    #[test]
    fn removing_the_last_space_opens_the_replacement_in_its_place() {
        let mut session = Session::new(seat("/home/someone"), 80, 24);
        let only = session.workspace.selected_space;

        let removed = session
            .remove_space(only, home_seat(), 80, 24)
            .expect("the last space closes");

        assert_eq!(removed.panes, vec![PaneId(1)]);
        let replacement = removed.replacement.expect("a replacement pane to spawn");
        assert_eq!(session.workspace.spaces.len(), 1);
        let space = session.selected_space();
        assert_ne!(space.id, only, "a new space, not the closed one kept");
        assert_eq!(space.id, replacement.space);
        assert_eq!(space.root, PathBuf::from("/home/someone"));
        assert_eq!(session.selected_tab().pane.id, replacement.pane);
    }

    fn seat(root: &str) -> SpaceSeat {
        SpaceSeat {
            root: PathBuf::from(root),
        }
    }

    fn home_seat() -> SpaceSeat {
        SpaceSeat {
            root: PathBuf::from("/home/someone"),
        }
    }

    #[test]
    fn rename_space_trims_refuses_blank_and_reports_real_changes_only() {
        let mut session = Session::new(seat("/tmp/a"), 80, 24);
        let space = session.workspace.selected_space;
        assert!(session.rename_space(space, "  frontend  ".into()));
        assert_eq!(session.selected_space().label, "frontend");
        assert!(!session.rename_space(space, "frontend".into()));
        assert!(!session.rename_space(space, "   ".into()));
        assert!(!session.rename_space(SpaceId(99), "ghost".into()));
    }

    #[test]
    fn select_space_moves_selection_only_when_the_target_exists_and_differs() {
        let mut session = Session::new(seat("/tmp/a"), 80, 24);
        let first_space = session.workspace.selected_space;
        session.add_space("frontend".into(), seat("/tmp/frontend"), 80, 24);
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
        let mut session = Session::new(seat("/tmp/a"), 80, 24);
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

        session.add_space("frontend".into(), seat("/tmp/frontend"), 80, 24);
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
        let mut session = Session::new(seat("/tmp/a"), 80, 24);
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
        let mut session = Session::new(seat("/tmp/a"), 80, 24);
        let original_tab = session.selected_tab().id;
        let original_pane = session.selected_tab().pane.id;
        session.add_space("frontend".into(), seat("/tmp/frontend"), 80, 24);
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
        let pane = &session.selected_tab().pane;
        assert_eq!(pane.cwd, PathBuf::from("/tmp/moved"));
    }

    /// A seed names its agent by position because restoring mints fresh
    /// ids; what must survive a server restart is which tab belongs with
    /// which, not the numbers they happened to carry.
    #[test]
    fn restoring_rebuilds_which_tab_belongs_with_which() {
        let (session, _) = Session::restore(vec![SpaceSeed {
            label: "frontend".into(),
            root: PathBuf::from("/tmp/seed"),
            tabs: vec![
                TabSeed {
                    label: "claude".into(),
                    cwd: PathBuf::from("/tmp/a/web"),
                    agent: None,
                    launch: Launch::Shell,
                },
                TabSeed {
                    label: "shell".into(),
                    cwd: PathBuf::from("/tmp/a/web"),
                    agent: Some(0),
                    launch: Launch::Shell,
                },
                TabSeed {
                    label: "loose".into(),
                    cwd: PathBuf::from("/tmp/a"),
                    agent: Some(7),
                    launch: Launch::Shell,
                },
            ],
        }])
        .expect("a space stands");

        let tabs = &session.workspace.spaces[0].tabs;
        assert_eq!(tabs[1].agent, Some(tabs[0].id));
        assert_eq!(tabs[0].agent, None, "an agent belongs with nothing");
        assert_eq!(tabs[2].agent, None, "an index off the end is nobody");
    }

    #[test]
    fn restore_rebuilds_the_seeded_shape_with_sequential_ids() {
        let (session, _) = Session::restore(vec![
            SpaceSeed {
                label: "frontend".into(),
                root: PathBuf::from("/tmp/seed"),
                tabs: vec![
                    TabSeed {
                        label: "claude".into(),
                        cwd: PathBuf::from("/tmp/a/web"),
                        agent: None,
                        launch: Launch::Shell,
                    },
                    TabSeed {
                        label: "shell".into(),
                        cwd: PathBuf::from("/tmp/a"),
                        agent: None,
                        launch: Launch::Shell,
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
                    launch: Launch::Shell,
                }],
            },
        ])
        .expect("a space stands");

        assert_eq!(session.workspace.spaces.len(), 2);
        assert_eq!(session.workspace.selected_space, SpaceId(1));

        let frontend = &session.workspace.spaces[0];
        assert_eq!(frontend.id, SpaceId(1));
        assert_eq!(frontend.label, "frontend");
        assert_eq!(frontend.tabs.len(), 2);
        assert_eq!(frontend.selected_tab, frontend.tabs[0].id);
        assert_eq!(frontend.tabs[0].id, TabId(1));
        assert_eq!(frontend.tabs[0].label, "claude");
        let pane = &frontend.tabs[0].pane;
        assert_eq!(pane.id, PaneId(1));
        assert_eq!(pane.cwd, PathBuf::from("/tmp/a/web"));
        assert_eq!(frontend.tabs[1].id, TabId(2));

        let backend = &session.workspace.spaces[1];
        assert_eq!(backend.id, SpaceId(2));
        assert_eq!(backend.tabs[0].id, TabId(3));
        let pane = &backend.tabs[0].pane;
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
        let mut session = Session::new(seat("/tmp/a"), 80, 24);
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
        let mut session = Session::new(seat("/tmp/a"), 80, 24);
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
        let mut session = Session::new(seat("/tmp/a"), 80, 24);
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
        let mut session = Session::new(seat("/tmp/a"), 80, 24);
        let first_space_tab = session.selected_tab().id;
        session.add_space("frontend".into(), seat("/tmp/frontend"), 80, 24);
        let other_space_tab = session.selected_tab().id;
        assert_ne!(first_space_tab, other_space_tab);

        assert!(!session.reorder_tab(first_space_tab, Some(other_space_tab)));
        // Neither space's order changed.
        assert_eq!(session.workspace.spaces[0].tabs[0].id, first_space_tab);
        assert_eq!(session.workspace.spaces[1].tabs[0].id, other_space_tab);
    }

    #[test]
    fn reorder_tab_rejects_missing_tabs_and_self_targeting() {
        let mut session = Session::new(seat("/tmp/a"), 80, 24);
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
    fn reorder_space_moves_before_a_target_or_to_the_end() {
        let mut session = Session::new(seat("/tmp/a"), 80, 24);
        session.add_space("b".into(), seat("/tmp/b"), 80, 24);
        session.add_space("c".into(), seat("/tmp/c"), 80, 24);
        let order = |session: &Session| -> Vec<u64> {
            session
                .workspace
                .spaces
                .iter()
                .map(|space| space.id.0)
                .collect()
        };
        let [a, b, c] = [SpaceId(1), SpaceId(2), SpaceId(3)];
        assert_eq!(order(&session), vec![1, 2, 3]);

        assert!(session.reorder_space(c, Some(a)));
        assert_eq!(order(&session), vec![3, 1, 2]);

        assert!(session.reorder_space(c, None));
        assert_eq!(order(&session), vec![1, 2, 3]);

        assert!(!session.reorder_space(a, Some(b)), "already before it");
        assert!(!session.reorder_space(c, None), "already last");
        assert!(!session.reorder_space(a, Some(a)), "before itself");
        assert!(!session.reorder_space(SpaceId(9), None), "no such space");
        assert!(
            !session.reorder_space(a, Some(SpaceId(9))),
            "no such target"
        );
        assert_eq!(order(&session), vec![1, 2, 3]);
    }

    #[test]
    fn restore_with_no_usable_seeds_restores_nothing() {
        let restored = Session::restore(vec![SpaceSeed {
            label: "empty".into(),
            root: PathBuf::from("/tmp/seed"),
            tabs: vec![],
        }]);
        assert!(restored.is_none());
    }

    /// A space with nothing in it is dropped on the way back, and every
    /// pane after it still comes back with its own launch rather than the
    /// one of whichever tab happened to sit at its position.
    #[test]
    fn an_empty_space_shifts_no_launch_onto_another_tab() {
        let agent = Launch::Program {
            argv: vec!["claude".to_owned()],
            env: vec![("UZE_AGENT".to_owned(), "abc".to_owned())],
        };
        let seed = |label: &str, launch: Launch| SpaceSeed {
            label: label.into(),
            root: PathBuf::from("/tmp/seed"),
            tabs: vec![TabSeed {
                label: label.into(),
                cwd: PathBuf::from("/tmp/seed"),
                agent: None,
                launch,
            }],
        };
        let empty = SpaceSeed {
            tabs: Vec::new(),
            ..seed("empty", Launch::Shell)
        };

        let (session, launches) = Session::restore(vec![
            empty,
            seed("agent", agent.clone()),
            seed("shell", Launch::Shell),
        ])
        .expect("two spaces stand");

        let pane_of = |label: &str| {
            session
                .workspace
                .spaces
                .iter()
                .find(|space| space.label == label)
                .expect("restored")
                .tabs[0]
                .pane
                .id
        };
        assert_eq!(
            launches,
            vec![(pane_of("agent"), agent), (pane_of("shell"), Launch::Shell)]
        );
    }
}
