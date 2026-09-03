//! Tests for the workspace client.
//!
//! Moved out of `orchestrator.rs` alongside the `render`/`input` split:
//! they were the last ~500 lines standing between a reader and the
//! session-driving code the file is actually about.

use super::*;

mod workspace_tests {
    use super::WorkspaceHit;
    use super::{
        AGENT_BUSY_REPAINTS, AGENT_ECHO_GRACE, AGENT_PASTE_GRACE, AgentIdentity, AgentTabStatus,
        DraggingTab, PendingDrop, PreservedOverlay, RootPicker, TabDragGroup, TaskResolution,
        TaskStateView, TaskView, UpstreamSync, WorkspaceModel, adopt_agent_labels,
        agent_identity_for_tab, blank_pane, can_close_tab_from_menu, encode_mouse, evaluation_key,
        forward_paste, forward_scroll, next_agent_label, next_shell_label, pane_relative,
        pending_tab_drop,
        render::{
            self, WorkspaceLayout, compute_layout, render_preserved, render_sidebar,
            render_status_catalog, render_tab_strip, task_mark,
        },
        selected_pane_cwd, space_own_tab, tab_drag_group, tab_drag_group_members,
        tab_needs_replacement_shell, workspace_has_active_agent_operation,
    };
    use crossterm::event::{MouseButton, MouseEventKind};
    use ratatui::layout::Rect;
    use ratatui::style::Color;
    use ratatui::{Terminal, backend::TestBackend};
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};
    use uze_terminal::{
        CellAttributes, ClientEvent, ClientRequest, Cursor, Focus, Layout, MouseMode, Pane,
        PaneDamage, PaneId, RenderCell, Session, SpaceId, Tab, TabId, TerminalColor, WorkspaceId,
    };

    const IDENTITIES: [AgentIdentity; 1] = [AgentIdentity {
        binary: "agent",
        integration: "agent",
        display_name: "Agent",
    }];

    /// A one-tab session whose only tab `agent_identity_for_tab` resolves
    /// to [`IDENTITIES`], matched on the tab label the way a tab created
    /// before generic agent labels is.
    fn agent_session() -> WorkspaceModel {
        let mut session = Session::new(WorkspaceId("workspace".into()), "/tmp".into(), 80, 24);
        session.workspace.spaces[0].tabs[0].label = "Agent".into();
        WorkspaceModel {
            session: Some(session),
            ..WorkspaceModel::default()
        }
    }

    /// Damage carrying one changed cell — the smallest thing that still
    /// counts as a pane having painted something.
    fn painted(pane: PaneId) -> ClientEvent {
        ClientEvent::Damage(PaneDamage {
            pane,
            columns: 80,
            rows: 24,
            cursor: Cursor { column: 0, row: 0 },
            alternate_screen: false,
            mouse: MouseMode {
                reports_clicks: false,
                reports_drag: false,
                sgr: false,
            },
            bracketed_paste: false,
            changed: vec![(
                0,
                0,
                RenderCell {
                    character: 'x',
                    foreground: TerminalColor::DefaultForeground,
                    background: TerminalColor::DefaultBackground,
                    attributes: CellAttributes::default(),
                },
            )],
        })
    }

    /// Damage redescribing every cell — what the server sends a client
    /// that has no comparable baseline to diff against (an attach) or
    /// after a resize.
    fn repainted_whole_grid(pane: PaneId) -> ClientEvent {
        let ClientEvent::Damage(mut damage) = painted(pane) else {
            unreachable!("painted builds damage")
        };
        let cell = damage.changed[0].2.clone();
        damage.changed = (0..damage.rows)
            .flat_map(|row| (0..damage.columns).map(move |column| (row, column)))
            .map(|(row, column)| (row, column, cell.clone()))
            .collect();
        ClientEvent::Damage(damage)
    }

    /// Paints `pane` the way a harness animating a running turn does:
    /// several frames, spread over enough time to be animation rather than
    /// one repaint whose bytes reached the client in pieces.
    fn animate(model: &mut WorkspaceModel, pane: PaneId, start: Instant) {
        for step in 0..=AGENT_BUSY_REPAINTS as u64 {
            model.note_agent_output(pane, &IDENTITIES, start + Duration::from_millis(120 * step));
        }
    }

    #[test]
    fn only_the_active_spaces_agent_carries_the_selected_dot() {
        // A background space keeps a `selected_tab` of its own — where it
        // would resume, not where the user is. Drawing the dot from that
        // alone gave the sidebar one "this is the agent you are talking to"
        // per open space.
        let mut session = Session::new(WorkspaceId("workspace".into()), "/tmp".into(), 80, 24);
        session.workspace.spaces[0].tabs[0].label = "Agent".into();
        session.add_space("second".into(), "/tmp/second".into(), 80, 24);
        session.workspace.spaces[1].tabs[0].label = "Agent".into();
        let model = WorkspaceModel {
            session: Some(session),
            ..WorkspaceModel::default()
        };

        let mut terminal = Terminal::new(TestBackend::new(40, 24)).unwrap();
        let mut hits = Vec::new();
        terminal
            .draw(|frame| render_sidebar(frame, frame.area(), &model, &IDENTITIES, &mut hits))
            .unwrap();
        let glyphs = |needle: &str| {
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .filter(|cell| cell.symbol() == needle)
                .count()
        };
        assert_eq!(glyphs("\u{25cf}"), 1, "one selected agent, sidebar-wide");
        assert_eq!(glyphs("\u{25cb}"), 1, "the other space's agent reads idle");
    }

    /// A task view as the application would answer it for the slot at
    /// `checkout`, in `state`.
    fn task_in(checkout: &str, label: &str, state: TaskStateView, ahead: usize) -> TaskView {
        TaskView {
            id: "t1".into(),
            label: label.into(),
            branch: "agent/t1".into(),
            target: "main".into(),
            checkout: Some(PathBuf::from(checkout)),
            state,
            ahead,
            published_as: None,
            created_at_unix: 1,
        }
    }

    /// A one-agent session in the slot `/repo/.worktrees/ai`, whose task is
    /// in `state`.
    fn agent_with_task(state: TaskStateView, ahead: usize) -> WorkspaceModel {
        let mut model = agent_session_in("/repo/.worktrees/ai");
        model.tasks.insert(
            PathBuf::from("/repo"),
            vec![task_in(
                "/repo/.worktrees/ai",
                "fix-auth-redirect",
                state,
                ahead,
            )],
        );
        model
    }

    /// The tab strip as text and hits.
    fn tab_strip(model: &WorkspaceModel) -> (Vec<String>, Vec<(Rect, WorkspaceHit)>) {
        let mut terminal = Terminal::new(TestBackend::new(80, 3)).unwrap();
        let mut hits = Vec::new();
        terminal
            .draw(|frame| render_tab_strip(frame, frame.area(), model, &IDENTITIES, &mut hits))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let rows = (0..buffer.area.height)
            .map(|row| {
                (0..buffer.area.width)
                    .map(|column| buffer[(column, row)].symbol())
                    .collect()
            })
            .collect();
        (rows, hits)
    }

    /// A space holding two agents, each with one shell of its own, plus
    /// the shell the space was born with. Returns the model and the two
    /// agent tabs, in creation order.
    fn two_agents_with_shells() -> (WorkspaceModel, TabId, TabId) {
        let mut session = Session::new(WorkspaceId("workspace".into()), "/repo".into(), 80, 24);
        let space = session.workspace.selected_space;
        let agent = |session: &mut Session, label: &str, cwd: &str| {
            let pane = session.add_tab(space, label.into(), None, 80, 24, cwd.into());
            let id = session.selected_space().selected_tab;
            // What makes a tab an agent is what is running in its pane —
            // the same live probe `agent_identity_for_tab` reads.
            session.update_pane_status(pane, cwd.into(), "agent".into());
            session.add_tab(
                space,
                format!("{label} shell"),
                Some(id),
                80,
                24,
                cwd.into(),
            );
            id
        };
        let first = agent(&mut session, "Agent one", "/repo/.worktrees/a");
        let second = agent(&mut session, "Agent two", "/repo/.worktrees/b");
        let model = WorkspaceModel {
            session: Some(session),
            ..WorkspaceModel::default()
        };
        (model, first, second)
    }

    /// The strip is about one agent at a time: the agent leads it, its own
    /// shells follow, and another agent's shells are simply elsewhere.
    #[test]
    fn the_strip_shows_the_selected_agent_and_only_its_own_shells() {
        let (mut model, first, second) = two_agents_with_shells();
        model.session.as_mut().expect("session").select_tab(first);

        let (rows, _) = tab_strip(&model);
        let strip = rows.join(" ");
        assert!(strip.contains("Agent one"), "the agent leads: {strip}");
        assert!(strip.contains("Agent one shell"), "its own shell: {strip}");
        assert!(
            !strip.contains("Agent two"),
            "and nothing of the other agent: {strip}"
        );
        assert!(
            !strip.contains("○ shell"),
            "not the space's own bootstrap shell either: {strip}"
        );

        model.session.as_mut().expect("session").select_tab(second);
        let (rows, _) = tab_strip(&model);
        let strip = rows.join(" ");
        assert!(strip.contains("Agent two shell"), "{strip}");
        assert!(!strip.contains("Agent one"), "{strip}");
    }

    /// Selecting one of an agent's shells keeps the strip on that agent —
    /// the context is the agent, not whichever tab is selected.
    #[test]
    fn a_shell_keeps_the_strip_on_the_agent_it_belongs_with() {
        let (mut model, first, _) = two_agents_with_shells();
        let shell = model
            .session
            .as_ref()
            .expect("session")
            .selected_space()
            .tabs
            .iter()
            .find(|tab| tab.agent == Some(first))
            .expect("the agent's own shell")
            .id;
        model.session.as_mut().expect("session").select_tab(shell);

        let (rows, _) = tab_strip(&model);
        let strip = rows.join(" ");
        assert!(strip.contains("Agent one"), "{strip}");
        assert!(strip.contains("Agent one shell"), "{strip}");
    }

    /// The space's own shells are its own context, reached from its row in
    /// the sidebar — no agent leads the strip there.
    #[test]
    fn the_spaces_own_shell_is_a_context_of_its_own() {
        let (mut model, _, _) = two_agents_with_shells();
        let own = model.session.as_ref().expect("session").workspace.spaces[0].tabs[0].id;
        model.session.as_mut().expect("session").select_tab(own);

        let (rows, _) = tab_strip(&model);
        let strip = rows.join(" ");
        assert!(strip.contains("shell"), "{strip}");
        assert!(!strip.contains("Agent"), "{strip}");
    }

    /// A shell is numbered within the group it joins, so an agent's first
    /// shell is "shell 1" however many tabs the space already holds.
    #[test]
    fn a_shells_number_counts_only_its_own_group() {
        let (mut model, first, _) = two_agents_with_shells();
        model.session.as_mut().expect("session").select_tab(first);

        assert_eq!(next_shell_label(&model, &IDENTITIES), "shell 2");

        let own = model.session.as_ref().expect("session").workspace.spaces[0].tabs[0].id;
        model.session.as_mut().expect("session").select_tab(own);
        assert_eq!(
            next_shell_label(&model, &IDENTITIES),
            "shell 2",
            "the space's own group counts neither agent"
        );
    }

    /// The space's row in the sidebar is the way back to the space's own
    /// shells: it lands on one, and stays put when you are already there.
    #[test]
    fn a_spaces_row_lands_on_a_shell_of_its_own() {
        let (mut model, first, _) = two_agents_with_shells();
        let session = model.session.as_mut().expect("session");
        let own = session.workspace.spaces[0].tabs[0].id;

        session.select_tab(first);
        assert_eq!(
            space_own_tab(&session.workspace.spaces[0], &IDENTITIES),
            Some(own),
            "from an agent, back to the space's own shell"
        );

        session.select_tab(own);
        assert_eq!(
            space_own_tab(&session.workspace.spaces[0], &IDENTITIES),
            Some(own),
            "and it stays where it already is"
        );
    }

    /// An agent leaves the strip only through the sidebar's confirmation,
    /// so its chip offers no × for a stray click to land on.
    #[test]
    fn the_agent_chip_carries_no_close_button() {
        let (mut model, first, _) = two_agents_with_shells();
        model.session.as_mut().expect("session").select_tab(first);

        let (_, hits) = tab_strip(&model);
        assert!(
            !hits
                .iter()
                .any(|(_, hit)| matches!(hit, WorkspaceHit::CloseTab(tab) if *tab == first)),
            "no close hit for the agent"
        );
        assert!(
            hits.iter()
                .any(|(_, hit)| matches!(hit, WorkspaceHit::CloseTab(_))),
            "its shell still closes"
        );
    }

    /// Renders the whole frame (sidebar + tab strip + pane) the way the
    /// real workspace loop does each frame, stores the resulting hits on
    /// `model` — mirroring `model.hits = hits;` in `attach_workspace`'s own
    /// loop — and returns the matching layout, the pair the drag-reorder
    /// helpers need to classify a hit's rect.
    fn full_frame(model: &mut WorkspaceModel) -> WorkspaceLayout {
        let area = Rect::new(0, 0, 80, 24);
        let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
        let mut hits = Vec::new();
        terminal
            .draw(|frame| render::render(frame, model, &IDENTITIES, &mut hits))
            .unwrap();
        model.hits = hits;
        compute_layout(area, model.sidebar_width)
    }

    /// A hit's rect alone says which drag group it belongs to: a sidebar
    /// row's own agent's space, or the tab strip's own (space, context)
    /// pair — the same tab can appear in both (its sidebar row and, when
    /// it's the context agent, its own strip chip too), so the rect is
    /// what tells them apart, not the tab id.
    #[test]
    fn tab_drag_group_classifies_by_the_region_a_rect_landed_in() {
        let (mut model, first, _second) = two_agents_with_shells();
        model.session.as_mut().expect("session").select_tab(first);
        let space = model.session.as_ref().unwrap().workspace.selected_space;
        let layout = full_frame(&mut model);

        let sidebar_rect = model
            .hits
            .iter()
            .find(|(rect, hit)| {
                matches!(hit, WorkspaceHit::SelectTab(tab) if *tab == first)
                    && rect.x < layout.sidebar.right()
            })
            .map(|(rect, _)| *rect)
            .expect("the agent's own sidebar row");
        assert_eq!(
            tab_drag_group(&model, &IDENTITIES, &layout, sidebar_rect, first),
            Some(TabDragGroup::Agents(space))
        );

        let strip_rect = model
            .hits
            .iter()
            .find(|(rect, hit)| {
                matches!(hit, WorkspaceHit::SelectTab(tab) if *tab == first)
                    && rect.x >= layout.sidebar.right()
            })
            .map(|(rect, _)| *rect)
            .expect("the agent's own strip chip");
        assert_eq!(
            tab_drag_group(&model, &IDENTITIES, &layout, strip_rect, first),
            Some(TabDragGroup::Strip(space, Some(first))),
            "the very same tab, but its strip chip's rect names the strip's group"
        );

        assert_eq!(
            tab_drag_group(&model, &IDENTITIES, &layout, layout.pane, first),
            None,
            "the pane itself belongs to no drag group"
        );
    }

    #[test]
    fn tab_drag_group_members_are_sorted_along_the_groups_axis_and_scoped_to_it() {
        let (mut model, first, second) = two_agents_with_shells();
        model.session.as_mut().expect("session").select_tab(first);
        let space = model.session.as_ref().unwrap().workspace.selected_space;
        let layout = full_frame(&mut model);

        let agents =
            tab_drag_group_members(&model, &IDENTITIES, &layout, TabDragGroup::Agents(space));
        assert_eq!(
            agents.iter().map(|(_, tab)| *tab).collect::<Vec<_>>(),
            vec![first, second],
            "sidebar rows top to bottom, one entry per agent despite each pushing two hits"
        );

        let strip = tab_drag_group_members(
            &model,
            &IDENTITIES,
            &layout,
            TabDragGroup::Strip(space, Some(first)),
        );
        let strip_ids: Vec<TabId> = strip.iter().map(|(_, tab)| *tab).collect();
        assert!(
            strip_ids.contains(&first),
            "the agent chip itself: {strip_ids:?}"
        );
        assert!(
            !strip_ids.contains(&second),
            "never the other agent's group: {strip_ids:?}"
        );
    }

    /// Reproduces the real sidebar geometry for four agent tabs (each two
    /// rows, a gap row between siblings) end to end, dragging the first
    /// one: releasing on the second agent's own label row — the bold row
    /// a click naturally lands on — has to move it, not silently do
    /// nothing because that label row's near half used to read as "put it
    /// back where it was".
    #[test]
    fn dragging_the_first_agent_onto_the_seconds_own_label_row_reorders_it() {
        let mut session = Session::new(WorkspaceId("workspace".into()), "/repo".into(), 80, 24);
        let space = session.workspace.selected_space;
        let mut agent_ids = Vec::new();
        for (label, cwd) in [
            ("Agent one", "/repo/.worktrees/a"),
            ("Agent two", "/repo/.worktrees/b"),
            ("Agent three", "/repo/.worktrees/c"),
            ("Agent four", "/repo/.worktrees/d"),
        ] {
            let pane = session.add_tab(space, label.into(), None, 80, 24, cwd.into());
            session.update_pane_status(pane, cwd.into(), "agent".into());
            agent_ids.push(session.selected_space().selected_tab);
        }
        let mut model = WorkspaceModel {
            session: Some(session),
            ..WorkspaceModel::default()
        };
        let layout = full_frame(&mut model);
        let dragged = agent_ids[0];

        let all = tab_drag_group_members(&model, &IDENTITIES, &layout, TabDragGroup::Agents(space));
        let origin = all
            .iter()
            .find(|(_, tab)| *tab == dragged)
            .expect("the dragged tab's own row")
            .0
            .y;
        let second_label_row = all
            .iter()
            .find(|(_, tab)| *tab == agent_ids[1])
            .expect("the second agent's own row")
            .0
            .y;
        let members: Vec<_> = all.into_iter().filter(|(_, tab)| *tab != dragged).collect();

        let pending = pending_tab_drop(
            &members,
            TabDragGroup::Agents(space),
            second_label_row,
            origin,
        );
        assert_eq!(
            pending,
            Some(PendingDrop::Before(agent_ids[2])),
            "landing right before the third agent puts the dragged one \
             straight after the second — releasing here must actually move it"
        );

        let PendingDrop::Before(before) = pending.expect("computed above") else {
            unreachable!("asserted Before above");
        };
        let server_session = model.session.as_mut().unwrap();
        assert!(
            server_session.reorder_tab(dragged, Some(before)),
            "a real move, not the no-op dropping on the immediate successor used to be"
        );
        let order: Vec<TabId> = server_session
            .selected_space()
            .tabs
            .iter()
            .map(|t| t.id)
            .filter(|id| agent_ids.contains(id))
            .collect();
        assert_eq!(
            order,
            vec![agent_ids[1], agent_ids[0], agent_ids[2], agent_ids[3]],
            "agent one now sits right after agent two: {order:?}"
        );
    }

    #[test]
    fn pending_tab_drop_resolves_the_nearest_half_and_end_past_the_last() {
        let members = vec![
            (Rect::new(0, 0, 10, 2), TabId(1)),
            (Rect::new(0, 2, 10, 2), TabId(2)),
            (Rect::new(0, 4, 10, 2), TabId(3)),
        ];
        let group = TabDragGroup::Agents(SpaceId(1));
        // Origin past every member here — as if dragging a tab that
        // started out below all three, so none of them is its "moot
        // successor" and every member's own midpoint splits plainly.
        let origin = 10;
        assert_eq!(
            pending_tab_drop(&members, group, 0, origin),
            Some(PendingDrop::Before(TabId(1))),
            "top half of the first row"
        );
        assert_eq!(
            pending_tab_drop(&members, group, 1, origin),
            Some(PendingDrop::Before(TabId(2))),
            "past the first row's own midpoint"
        );
        assert_eq!(
            pending_tab_drop(&members, group, 5, origin),
            Some(PendingDrop::End),
            "past every row's midpoint"
        );
    }

    #[test]
    fn pending_tab_drop_skips_straight_past_the_dragged_tabs_moot_successor() {
        // Dragging the first of four; its immediate successor (TabId(2))
        // can only ever land "before" it by reconstructing the exact slot
        // it just left, which `Session::reorder_tab` already refuses as a
        // no-op — so touching any part of it (not just its own back half)
        // has to resolve straight through to "after it", not sit there as
        // a dead, do-nothing target the way a plain per-member midpoint
        // split would leave it.
        let members = vec![
            (Rect::new(0, 2, 10, 2), TabId(2)),
            (Rect::new(0, 5, 10, 2), TabId(3)),
            (Rect::new(0, 8, 10, 2), TabId(4)),
        ];
        let group = TabDragGroup::Agents(SpaceId(1));
        let origin = 0; // TabId(1)'s own original row.
        assert_eq!(
            pending_tab_drop(&members, group, 1, origin),
            Some(PendingDrop::Before(TabId(2))),
            "still short of the moot successor: no target reached yet"
        );
        assert_eq!(
            pending_tab_drop(&members, group, 2, origin),
            Some(PendingDrop::Before(TabId(3))),
            "the moot successor's own label row already resolves past it"
        );
        assert_eq!(
            pending_tab_drop(&members, group, 3, origin),
            Some(PendingDrop::Before(TabId(3))),
            "and so does its detail row"
        );
        assert_eq!(
            pending_tab_drop(&members, group, 6, origin),
            Some(PendingDrop::Before(TabId(4))),
            "a real (non-moot) member still splits by its own midpoint"
        );
    }

    #[test]
    fn pending_tab_drop_is_none_outside_the_groups_own_area() {
        let members = vec![(Rect::new(0, 5, 10, 2), TabId(1))];
        let group = TabDragGroup::Agents(SpaceId(1));
        let origin = 10;
        assert_eq!(
            pending_tab_drop(&members, group, 0, origin),
            None,
            "well above the list, past its slack"
        );
        assert_eq!(
            pending_tab_drop(&members, group, 20, origin),
            None,
            "well below the list, past its slack"
        );
        assert_eq!(
            pending_tab_drop(&[], group, 0, origin),
            None,
            "nothing to drop onto at all"
        );
    }

    #[test]
    fn is_pending_drop_row_requires_armed_and_the_same_group() {
        let group = TabDragGroup::Agents(SpaceId(1));
        let dragging = DraggingTab {
            tab: TabId(9),
            group,
            origin: 0,
            armed: true,
            pending: Some(PendingDrop::Before(TabId(2))),
        };
        assert!(dragging.is_pending_drop_row(group, TabId(2), false));
        assert!(
            !dragging.is_pending_drop_row(group, TabId(3), false),
            "the wrong row"
        );
        assert!(
            !dragging.is_pending_drop_row(TabDragGroup::Agents(SpaceId(2)), TabId(2), false),
            "the wrong group"
        );
        let unarmed = DraggingTab {
            armed: false,
            ..dragging
        };
        assert!(
            !unarmed.is_pending_drop_row(group, TabId(2), false),
            "not armed yet — no indicator before the drag threshold"
        );

        let at_end = DraggingTab {
            pending: Some(PendingDrop::End),
            ..dragging
        };
        assert!(
            at_end.is_pending_drop_row(group, TabId(5), true),
            "dropping at the end lands on whichever row is last"
        );
        assert!(
            !at_end.is_pending_drop_row(group, TabId(5), false),
            "but not on a row that isn't"
        );
    }

    /// The sidebar draws its one insertion indicator on the pending drop's
    /// target row, and nowhere when the drag isn't armed yet — the plain
    /// click a press-without-movement still is (see `TAB_DRAG_THRESHOLD`).
    #[test]
    fn sidebar_indicator_marks_the_pending_drop_row_only_once_armed() {
        let (mut model, first, second) = two_agents_with_shells();
        model.session.as_mut().expect("session").select_tab(first);
        let space = model.session.as_ref().unwrap().workspace.selected_space;
        model.dragging_tab = Some(DraggingTab {
            tab: first,
            group: TabDragGroup::Agents(space),
            origin: 0,
            armed: true,
            pending: Some(PendingDrop::Before(second)),
        });

        let mut hits = Vec::new();
        let rows = sidebar_rows(&model, &mut hits);
        let second_row = hits
            .iter()
            .find(|(_, hit)| matches!(hit, WorkspaceHit::SelectTab(tab) if *tab == second))
            .map(|(rect, _)| rect.y)
            .expect("the drop target's own row");
        assert!(
            rows[second_row as usize].contains('▍'),
            "indicator on the target row: {:?}",
            rows[second_row as usize]
        );

        model.dragging_tab = model
            .dragging_tab
            .map(|d| DraggingTab { armed: false, ..d });
        let mut hits = Vec::new();
        let rows = sidebar_rows(&model, &mut hits);
        assert!(
            rows.iter().all(|row| !row.contains('▍')),
            "no indicator before the drag is armed"
        );
    }

    #[test]
    fn a_ready_task_names_its_row_marks_it_and_offers_delivery() {
        let model = agent_with_task(TaskStateView::Ready, 3);
        let rows = sidebar_rows(&model, &mut Vec::new());
        let name_row = rows
            .iter()
            .find(|row| row.contains("Agent"))
            .expect("the agent names its own row: {rows:?}");
        let (ready, _) = task_mark(&TaskStateView::Ready).expect("ready is marked");
        assert!(
            name_row.contains(ready),
            "ready carries its own mark: {name_row}"
        );
        assert!(
            rows.iter().any(|row| row.contains("agent/t1")),
            "and the task it is on reads underneath it: {rows:?}"
        );

        let (rows, hits) = tab_strip(&model);
        assert!(rows.iter().any(|row| row.contains("⇧3")), "{rows:?}");
        assert!(
            hits.iter()
                .any(|(_, hit)| matches!(hit, WorkspaceHit::Deliver(_))),
            "the button is a hit"
        );
    }

    /// A slot outlives the tasks that run in it, and a task that ended
    /// keeps naming the slot it ran in — so a reused directory is named by
    /// two tasks at once. The row belongs to whoever is in it now; reading
    /// the first match handed the new agent the previous one's branch and
    /// its delivered arrow.
    #[test]
    fn a_reused_slot_reads_the_task_in_it_now_not_the_one_before() {
        let mut model = agent_session_in("/repo/.worktrees/ai");
        let before = TaskView {
            id: "before".into(),
            branch: "agent/before".into(),
            created_at_unix: 1,
            ..task_in(
                "/repo/.worktrees/ai",
                "before",
                TaskStateView::Integrated,
                2,
            )
        };
        let now = TaskView {
            id: "now".into(),
            branch: "agent/now".into(),
            created_at_unix: 2,
            ..task_in("/repo/.worktrees/ai", "now", TaskStateView::Running, 0)
        };
        model
            .tasks
            .insert(PathBuf::from("/repo"), vec![before, now]);

        let rows = sidebar_rows(&model, &mut Vec::new());
        assert!(
            rows.iter().any(|row| row.contains("agent/now")),
            "the branch under the row is the one being written on: {rows:?}"
        );
        assert!(
            !rows.iter().any(|row| row.contains("agent/before")),
            "and never the branch of the task that ended here: {rows:?}"
        );
        let (delivered, _) = task_mark(&TaskStateView::Integrated).expect("integrated is marked");
        let name_row = rows
            .iter()
            .find(|row| row.contains("Agent"))
            .expect("the agent names its own row");
        assert!(
            !name_row.contains(delivered),
            "a new agent delivered nothing: {name_row}"
        );
    }

    /// The sidebar and the strip name the same agent the same way — the
    /// tab's own label, the one renaming edits — however its task is
    /// labelled. The strip used to prefer the task's label, so a renamed
    /// agent read as "Agent" in the sidebar and as its task slug up top.
    #[test]
    fn an_agent_carries_its_own_name_in_both_the_sidebar_and_the_strip() {
        let model = agent_with_task(TaskStateView::Ready, 1);
        assert!(
            sidebar_rows(&model, &mut Vec::new())
                .iter()
                .any(|row| row.contains("Agent")),
            "the sidebar names the tab"
        );

        let (rows, _) = tab_strip(&model);
        let strip = rows.join(" ");
        assert!(strip.contains("Agent"), "and so does the strip: {strip}");
        assert!(
            !strip.contains("fix-auth-redirect"),
            "not the task's label: {strip}"
        );
    }

    /// Every state a slot can be in, for the tests that have to cover the
    /// whole table rather than one interesting case.
    fn every_task_state() -> Vec<TaskStateView> {
        vec![
            TaskStateView::Running,
            TaskStateView::Uncommitted,
            TaskStateView::Ready,
            TaskStateView::Integrating,
            TaskStateView::Conflicted {
                files: vec![PathBuf::from("src/lib.rs")],
            },
            TaskStateView::GateFailed,
            TaskStateView::Integrated,
            TaskStateView::Parked,
        ]
    }

    /// The marks are symbols, not emoji. The line is drawn at U+2300: the
    /// blocks from there to U+27BF (Miscellaneous Technical, Miscellaneous
    /// Symbols, Dingbats) are where the pictographs live, and a terminal
    /// draws those from its emoji font — a different family, a width that
    /// varies by terminal, and a glyph that ignores the hue carrying the
    /// meaning. `⚠`, `⏸` and `✎` all came from there.
    #[test]
    fn no_status_mark_is_drawn_from_the_pictographic_blocks() {
        for state in every_task_state() {
            let Some((mark, _)) = task_mark(&state) else {
                continue;
            };
            let mut characters = mark.chars();
            let character = characters.next().expect("a mark is not empty");
            assert!(
                characters.next().is_none(),
                "{state:?}: one column, one character: {mark}"
            );
            assert!(
                (character as u32) < 0x2300,
                "{state:?}: {mark} (U+{:04X}) is a pictograph",
                character as u32
            );
        }
    }

    /// Color is what tells the marks apart at a glance — three states
    /// sharing `TEXT_DIM` meant the column read as one undifferentiated
    /// smudge. The glyphs are distinct for the same reason, and `Ready`
    /// specifically must not reuse the `✓` the agent column already spends
    /// on `Completed`.
    #[test]
    fn each_status_mark_carries_a_glyph_and_a_hue_of_its_own() {
        let marks: Vec<(&str, Color)> = every_task_state().iter().filter_map(task_mark).collect();
        for (index, (mark, hue)) in marks.iter().enumerate() {
            for (other_mark, other_hue) in &marks[index + 1..] {
                assert_ne!(mark, other_mark, "two states share a glyph");
                assert_ne!(
                    hue, other_hue,
                    "two states share a hue: {mark}/{other_mark}"
                );
            }
        }
        let agent_glyphs: Vec<String> = [
            AgentTabStatus::Completed,
            AgentTabStatus::Selected,
            AgentTabStatus::Idle,
        ]
        .into_iter()
        .map(|status| status.glyph(0).trim_end().to_owned())
        .collect();
        for (mark, _) in &marks {
            assert!(
                !agent_glyphs.iter().any(|glyph| glyph == mark),
                "{mark} means something else one column to the left"
            );
        }
    }

    /// Both status columns are click targets, and both open the catalog:
    /// the glyphs are the row's only wordless vocabulary, so the row has
    /// to carry the way to look them up.
    #[test]
    fn either_status_column_opens_the_catalog() {
        let model = agent_with_task(TaskStateView::Ready, 1);
        let mut hits = Vec::new();
        let rows = sidebar_rows(&model, &mut hits);
        let anchors: Vec<Rect> = hits
            .iter()
            .filter_map(|(_, hit)| match hit {
                WorkspaceHit::OpenStatusCatalog(anchor) => Some(*anchor),
                _ => None,
            })
            .collect();
        assert_eq!(anchors.len(), 2, "one per column: {rows:?}");
        // Each anchor is the glyph's own cell, not the row: a click on the
        // name still selects the tab. Read back out of the drawn rows, so
        // a hit that drifts off its glyph fails here rather than opening
        // the catalog from a blank column.
        let cell_at = |anchor: &Rect| {
            rows[anchor.y as usize]
                .chars()
                .nth(anchor.x as usize)
                .expect("the anchor is inside the row")
                .to_string()
        };
        let (ready, _) = task_mark(&TaskStateView::Ready).expect("ready is marked");
        let glyphs: Vec<String> = anchors.iter().map(cell_at).collect();
        for anchor in &anchors {
            assert_eq!((anchor.width, anchor.height), (1, 1));
        }
        assert!(
            glyphs.iter().any(|glyph| glyph == ready),
            "one anchor is the task mark: {glyphs:?}"
        );
        assert!(
            glyphs
                .iter()
                .any(|glyph| glyph == AgentTabStatus::Selected.glyph(0).trim_end()),
            "the other is the agent's own status: {glyphs:?}"
        );
        assert!(
            hits.iter()
                .any(|(rect, hit)| matches!(hit, WorkspaceHit::SelectTab(_)) && rect.width > 1),
            "the row itself still selects the tab"
        );
    }

    /// A scheduled evaluation is released under the key it reserved, not
    /// under one recomputed from the answer.
    ///
    /// The two ends resolve a repository differently — the scheduler
    /// lexically, off the path it already holds; the evaluation by asking
    /// Git — and for anything that is not a slot the two disagree. A
    /// removal under the second spelling never matched the insertion under
    /// the first, so the directory stayed reserved for the life of the
    /// session and `schedule_evaluation` returned early forever after:
    /// the row kept showing whatever state it last read, however much the
    /// checkout changed underneath it.
    #[test]
    fn an_evaluation_is_released_under_the_key_it_reserved() {
        let primary = PathBuf::from("/repo");

        // Every slot of a repository answers that repository, which is what
        // keeps a sidebar full of agents to one evaluation.
        let slot = primary.join(".worktrees").join("9k5vwm");
        assert_eq!(evaluation_key(&slot), primary);

        // Anything else answers itself — never the repository root Git
        // would name for it. This is the disagreement the key travels to
        // avoid.
        let nested = primary.join("crates").join("uze-core");
        assert_eq!(evaluation_key(&nested), nested);
        assert_ne!(evaluation_key(&nested), primary);

        // And an evaluation that found no working tree still answers, so
        // the reservation is released rather than left standing.
        let resolution = TaskResolution {
            key: evaluation_key(&nested),
            answered: None,
        };
        let mut pending = std::collections::BTreeSet::new();
        pending.insert(evaluation_key(&nested));
        pending.remove(&resolution.key);
        assert!(pending.is_empty());
    }

    /// The catalog explains every state that has a mark, in both columns —
    /// generated from the same tables the sidebar draws with, so it cannot
    /// drift from the row it explains. A state with no mark is *not*
    /// listed: `Running` draws nothing, and the agent column already says
    /// the process is alive.
    #[test]
    fn the_catalog_names_every_status_in_both_columns() {
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        terminal
            .draw(|frame| render_status_catalog(frame, frame.area(), Rect::new(4, 2, 1, 1), 0))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let text: String = (0..buffer.area.height)
            .map(|row| {
                (0..buffer.area.width)
                    .map(|column| buffer[(column, row)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        for name in [
            "working",
            "completed",
            "here",
            "idle",
            "uncommitted",
            "ready",
            "delivering",
            "conflict",
            "checks failed",
            "delivered",
            "parked",
        ] {
            assert!(text.contains(name), "{name} is missing from: {text}");
        }
        for state in every_task_state() {
            if let Some((mark, _)) = task_mark(&state) {
                assert!(text.contains(mark), "{mark} is missing from: {text}");
            }
        }
        assert!(
            task_mark(&TaskStateView::Running).is_none(),
            "the assertion below is only meaningful while Running is markless"
        );
        assert!(
            !text.contains("running"),
            "a state that draws no mark has no row in a legend of marks: {text}"
        );
    }

    #[test]
    fn a_running_task_offers_no_delivery_and_carries_no_mark() {
        let model = agent_with_task(TaskStateView::Running, 0);
        let rows = sidebar_rows(&model, &mut Vec::new());
        let name_row = rows.iter().find(|row| row.contains("Agent")).unwrap();
        assert!(
            !name_row.contains('\u{2713}') && !name_row.contains('\u{26a0}'),
            "{name_row}"
        );
        let (rows, hits) = tab_strip(&model);
        assert!(!rows.iter().any(|row| row.contains('⇧')), "{rows:?}");
        assert!(
            !hits
                .iter()
                .any(|(_, hit)| matches!(hit, WorkspaceHit::Deliver(_)))
        );
    }

    #[test]
    fn a_conflicted_task_is_marked_and_reported_but_not_a_button() {
        let model = agent_with_task(
            TaskStateView::Conflicted {
                files: vec![PathBuf::from("src/lib.rs")],
            },
            2,
        );
        let rows = sidebar_rows(&model, &mut Vec::new());
        let name_row = rows.iter().find(|row| row.contains("Agent")).unwrap();
        let (conflict, _) = task_mark(&TaskStateView::Conflicted { files: Vec::new() })
            .expect("a conflict is marked");
        assert!(name_row.contains(conflict), "{name_row}");
        let (rows, hits) = tab_strip(&model);
        assert!(rows.iter().any(|row| row.contains("conflict")), "{rows:?}");
        assert!(
            !hits
                .iter()
                .any(|(_, hit)| matches!(hit, WorkspaceHit::Deliver(_)))
        );
    }

    #[test]
    fn preserved_work_lists_tasks_without_a_live_tab_and_nothing_else() {
        let mut model = agent_with_task(TaskStateView::Ready, 1);
        let mut parked = task_in(
            "/repo/.worktrees/old",
            "yesterday",
            TaskStateView::Parked,
            0,
        );
        parked.id = "t2".into();
        let mut delivered = task_in(
            "/repo/.worktrees/gone",
            "shipped",
            TaskStateView::Integrated,
            0,
        );
        delivered.id = "t3".into();
        model
            .tasks
            .get_mut(Path::new("/repo"))
            .unwrap()
            .extend([parked, delivered]);

        let preserved = model.preserved_tasks();
        assert_eq!(preserved.len(), 1, "{preserved:?}");
        assert_eq!(preserved[0].1.label, "yesterday");

        model.preserved = Some(PreservedOverlay {
            selected: 0,
            confirm_discard: false,
        });
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
        terminal
            .draw(|frame| {
                render_preserved(
                    frame,
                    frame.area(),
                    &model,
                    model.preserved.as_ref().unwrap(),
                )
            })
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol().to_owned())
            .collect();
        assert!(
            text.contains("yesterday") && text.contains("uncommitted changes"),
            "{text}"
        );
        assert!(
            !text.contains("shipped"),
            "delivered work is not preserved work"
        );
        assert!(text.contains("[d] discard"));
    }

    /// A one-agent session whose only tab runs in `cwd`.
    fn agent_session_in(cwd: &str) -> WorkspaceModel {
        let mut session = Session::new(WorkspaceId("workspace".into()), "/repo".into(), 80, 24);
        let tab = &mut session.workspace.spaces[0].tabs[0];
        tab.label = "Agent".into();
        if let Layout::Pane(pane) = &mut tab.layout {
            pane.cwd = cwd.into();
        }
        WorkspaceModel {
            session: Some(session),
            ..WorkspaceModel::default()
        }
    }

    /// The sidebar as text, one string per row.
    fn sidebar_rows(model: &WorkspaceModel, hits: &mut Vec<(Rect, WorkspaceHit)>) -> Vec<String> {
        let buffer = sidebar_buffer(model, hits);
        (0..buffer.area.height)
            .map(|row| {
                (0..buffer.area.width)
                    .map(|column| buffer[(column, row)].symbol())
                    .collect()
            })
            .collect()
    }

    fn sidebar_buffer(
        model: &WorkspaceModel,
        hits: &mut Vec<(Rect, WorkspaceHit)>,
    ) -> ratatui::buffer::Buffer {
        let mut terminal = Terminal::new(TestBackend::new(40, 24)).unwrap();
        terminal
            .draw(|frame| render_sidebar(frame, frame.area(), model, &IDENTITIES, hits))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    /// The foreground the agent's caption — the row under its name — is
    /// drawn in, checked to be captioning `text`.
    fn caption_color_of(model: &WorkspaceModel, text: &str) -> Color {
        let buffer = sidebar_buffer(model, &mut Vec::new());
        let rows = sidebar_rows(model, &mut Vec::new());
        let row = rows
            .iter()
            .position(|row| row.contains("Agent"))
            .expect("the agent is named in the tree")
            + 1;
        let offset = rows[row]
            .find(text)
            .unwrap_or_else(|| panic!("{text} captions the agent: {rows:?}"));
        // A byte offset is not a column once the caption holds small caps
        // or subscript digits: one cell, several bytes.
        let column = rows[row][..offset].chars().count();
        buffer[(column as u16, row as u16)].fg
    }

    #[test]
    fn an_agent_in_a_slot_is_captioned_by_its_primary_and_left_unmarked() {
        // The whole point: `.worktrees/<id>` is two more segments in a
        // column this narrow, and every agent has a slot, so the caption
        // just stops spelling out the tail and the name row stays clean.
        let model = agent_session_in("/repo/.worktrees/ai");
        let rows = sidebar_rows(&model, &mut Vec::new());
        let name_row = rows
            .iter()
            .find(|row| row.contains("Agent"))
            .expect("the agent is named in the tree");
        assert_eq!(
            caption_color_of(&model, "/repo"),
            crate::ui::TEXT_DIM,
            "a slot is the rule and draws dim: {name_row}"
        );

        let caption = rows
            .iter()
            .find(|row| row.contains("/repo"))
            .expect("the caption row names where it is");
        assert!(!caption.contains(".worktrees"), "{caption}");
        assert!(
            !caption.contains('\u{22d4}'),
            "one mark, not two: {caption}"
        );
    }

    /// The column in front of an agent's name answers one question — how
    /// that agent is doing — and "outside any slot" is never said there,
    /// nor by a mark of its own: the caption's hue says it.
    #[test]
    fn an_agent_outside_any_slot_is_captioned_in_the_warning_hue() {
        let mut model = agent_session_in("/repo/src");
        let rows = sidebar_rows(&model, &mut Vec::new());
        let name_row = rows
            .iter()
            .find(|row| row.contains("Agent"))
            .expect("the agent is named in the tree");
        assert!(
            name_row.contains('\u{25cb}') || name_row.contains('\u{25cf}'),
            "the status glyph still leads: {name_row}"
        );
        assert_eq!(caption_color_of(&model, "/repo/src"), crate::ui::WARNING);

        model
            .branches
            .insert(PathBuf::from("/repo/src"), "main".into());
        assert_eq!(caption_color_of(&model, "main"), crate::ui::WARNING);
    }

    /// An agent outside any slot has no task to take a branch from, so
    /// its caption is the branch its own directory was evaluated on —
    /// and, until that evaluation answers, the directory itself.
    #[test]
    fn an_agent_outside_any_slot_is_captioned_by_its_branch() {
        let mut model = agent_session_in("/repo/src");
        let before = sidebar_rows(&model, &mut Vec::new());
        assert!(
            before.iter().any(|row| row.contains("/repo/src")),
            "the directory stands in until the branch is read: {before:?}"
        );

        model
            .branches
            .insert(PathBuf::from("/repo/src"), "feature/x".into());
        let rows = sidebar_rows(&model, &mut Vec::new());
        assert!(
            rows.iter().any(|row| row.contains("feature/x")),
            "the branch captions the agent: {rows:?}"
        );
        assert!(
            !rows.iter().any(|row| row.contains("/repo/src")),
            "the branch replaces the directory: {rows:?}"
        );
    }

    /// Inside a slot the branch is the task's to name: the primary's own
    /// branch, however recently read, is not what that agent delivers from.
    #[test]
    fn a_slot_never_borrows_the_primary_branch() {
        let mut model = agent_session_in("/repo/.worktrees/ai");
        model.branches.insert(PathBuf::from("/repo"), "main".into());
        let rows = sidebar_rows(&model, &mut Vec::new());
        assert!(
            !rows.iter().any(|row| row.contains("main")),
            "no task, no branch: {rows:?}"
        );
    }

    /// The operator's own tree is where a pull or a push is due, so an
    /// agent there carries what each would move at the right edge of its
    /// caption — an arrow for the direction and the count in subscript,
    /// red for what is to pull and green for what is to push — and only
    /// the halves that have a count.
    #[test]
    fn an_agent_outside_any_slot_is_captioned_with_what_a_pull_and_a_push_would_move() {
        let mut model = agent_session_in("/repo");
        model.branches.insert(PathBuf::from("/repo"), "main".into());
        model
            .upstream_syncs
            .insert(PathBuf::from("/repo"), UpstreamSync { pull: 1, push: 12 });
        let rows = sidebar_rows(&model, &mut Vec::new());
        let caption = rows
            .iter()
            .find(|row| row.contains("main"))
            .expect("the branch captions the agent");
        assert!(
            caption.ends_with("\u{21e3}\u{2081} \u{21e1}\u{2081}\u{2082} \u{2502}"),
            "⇣₁ ⇡₁₂ sit at the right edge, one pad off the divider: {caption:?}"
        );
        assert_eq!(caption_color_of(&model, "main"), crate::ui::WARNING);
        assert_eq!(caption_color_of(&model, "\u{21e3}"), crate::ui::DANGER);
        assert_eq!(caption_color_of(&model, "\u{21e1}"), crate::ui::SUCCESS);

        model
            .upstream_syncs
            .insert(PathBuf::from("/repo"), UpstreamSync { pull: 0, push: 3 });
        let rows = sidebar_rows(&model, &mut Vec::new());
        let caption = rows.iter().find(|row| row.contains("main")).unwrap();
        assert!(
            !caption.contains('\u{21e3}') && caption.ends_with("\u{21e1}\u{2083} \u{2502}"),
            "nothing to pull, three to push: {caption:?}"
        );

        model
            .upstream_syncs
            .insert(PathBuf::from("/repo"), UpstreamSync::default());
        let rows = sidebar_rows(&model, &mut Vec::new());
        let caption = rows.iter().find(|row| row.contains("main")).unwrap();
        assert!(
            !caption.contains('\u{21e1}') && !caption.contains('\u{21e3}'),
            "in sync says nothing: {caption:?}"
        );
    }

    /// Inside a slot the remote is the target's business, not the task's:
    /// whatever the primary's sync reads, a slot's caption never shows it.
    #[test]
    fn a_slot_never_shows_the_primary_sync() {
        let mut model = agent_session_in("/repo/.worktrees/ai");
        model
            .upstream_syncs
            .insert(PathBuf::from("/repo"), UpstreamSync { pull: 2, push: 2 });
        let rows = sidebar_rows(&model, &mut Vec::new());
        assert!(
            !rows
                .iter()
                .any(|row| row.contains('\u{21e1}') || row.contains('\u{21e3}')),
            "no arrow in a slot: {rows:?}"
        );
    }

    /// A shell opened beside an agent is part of that agent's context:
    /// typing into the shell must not unselect the agent in the tree.
    #[test]
    fn the_agent_stays_selected_while_one_of_its_shells_is() {
        let mut model = agent_session_in("/repo/.worktrees/ai");
        let session = model.session.as_mut().unwrap();
        let agent = session.workspace.spaces[0].tabs[0].id;
        session.add_tab(
            SpaceId(1),
            "shell 1".into(),
            Some(agent),
            80,
            24,
            "/repo/.worktrees/ai".into(),
        );
        let shell = session.workspace.spaces[0].tabs[1].id;
        session.select_tab(shell);
        assert_eq!(session.selected_space().selected_tab, shell);

        let rows = sidebar_rows(&model, &mut Vec::new());
        let name_row = rows
            .iter()
            .find(|row| row.contains("Agent"))
            .expect("the agent is named in the tree");
        assert!(
            name_row.contains('\u{25cf}'),
            "the agent still reads as selected: {name_row}"
        );
    }

    /// A shell the user typed an agent into keeps nothing of its generated
    /// label: it takes the `agent N` label it would have opened with. A
    /// label the user chose stays theirs.
    #[test]
    fn a_shell_that_starts_running_an_agent_takes_an_agent_label() {
        let mut model = agent_session_in("/repo");
        let session = model.session.as_mut().unwrap();
        session.workspace.spaces[0].tabs[0].label = "agent 1".into();
        for label in ["shell 2", "my shell", "shell"] {
            session.add_tab(SpaceId(1), label.into(), None, 80, 24, "/repo".into());
        }
        for tab in &mut session.workspace.spaces[0].tabs {
            if let Layout::Pane(pane) = &mut tab.layout {
                pane.process = "agent".into();
            }
        }

        let requests = adopt_agent_labels(&mut model, &IDENTITIES);
        assert_eq!(
            requests,
            vec![
                ClientRequest::RenameTab {
                    tab: TabId(2),
                    label: "agent 2".into(),
                },
                ClientRequest::RenameTab {
                    tab: TabId(4),
                    label: "agent 3".into(),
                },
            ]
        );
        assert!(
            adopt_agent_labels(&mut model, &IDENTITIES).is_empty(),
            "each tab is asked once"
        );

        let session = model.session.as_mut().unwrap();
        assert!(session.rename_tab(TabId(2), "agent 2".into()));
        assert!(session.rename_tab(TabId(4), "agent 3".into()));
        assert!(adopt_agent_labels(&mut model, &IDENTITIES).is_empty());
        assert!(
            model.label_adoptions.is_empty(),
            "a confirmed rename leaves the ledger"
        );
    }

    /// A plain shell stays a shell: nothing runs in it that could earn an
    /// agent label.
    #[test]
    fn a_shell_running_no_agent_keeps_its_label() {
        let mut model = agent_session_in("/repo");
        let session = model.session.as_mut().unwrap();
        session.add_tab(SpaceId(1), "shell 2".into(), None, 80, 24, "/repo".into());
        assert!(adopt_agent_labels(&mut model, &IDENTITIES).is_empty());
    }

    /// Every agent has a slot, so a slot is nothing to announce; the
    /// caption reads as the primary the slot hangs off.
    #[test]
    fn an_agent_in_a_slot_carries_no_marker() {
        let model = agent_session_in("/repo/.worktrees/ai");
        let rows = sidebar_rows(&model, &mut Vec::new());
        assert!(
            rows.iter()
                .any(|row| row.contains("/repo") && !row.contains(".worktrees")),
            "the caption reads as the primary: {rows:?}"
        );
    }

    /// The "+ new" prompt is a chooser, not a text field: the sidebar
    /// itself lists the directories the typed segment still matches, and
    /// clicking one is the same choice Enter makes.
    #[test]
    fn the_new_space_prompt_lists_the_directories_it_matches() {
        let root = uze_testkit::temp::TempDir::new("sidebar-root-picker");
        for directory in ["engine", "extensions", "docs"] {
            std::fs::create_dir_all(root.join(directory)).unwrap();
        }
        let mut model = agent_session_in("/repo");
        model.root_picker = Some(RootPicker::opened_in(&root.path().display().to_string()));

        let mut hits = Vec::new();
        let rows = sidebar_rows(&model, &mut hits);
        assert!(
            rows.iter().any(|row| row.contains("engine"))
                && rows.iter().any(|row| row.contains("docs")),
            "the listing is on screen: {rows:?}"
        );
        assert!(
            hits.iter()
                .any(|(_, hit)| matches!(hit, WorkspaceHit::PickSpaceRoot(_))),
            "every offered directory is clickable"
        );

        if let Some(picker) = model.root_picker.as_mut() {
            for character in "ex".chars() {
                picker.typed(character);
            }
        }
        let rows = sidebar_rows(&model, &mut Vec::new());
        assert!(
            rows.iter().any(|row| row.contains("extensions")),
            "what matches stays: {rows:?}"
        );
        assert!(
            !rows.iter().any(|row| row.contains("docs")),
            "what stopped matching is gone: {rows:?}"
        );
    }

    /// Two trees in one column — directories and spaces — would leave no
    /// telling which one is being chosen from, so the picker takes the
    /// column while it is open and gives it straight back when it closes.
    #[test]
    fn the_open_prompt_has_the_sidebar_to_itself() {
        let root = uze_testkit::temp::TempDir::new("sidebar-root-alone");
        std::fs::create_dir_all(root.join("engine")).unwrap();
        let mut model = agent_session_in("/repo");

        model.root_picker = Some(RootPicker::opened_in(&root.path().display().to_string()));
        let rows = sidebar_rows(&model, &mut Vec::new());
        assert!(
            rows.iter().any(|row| row.contains("engine")),
            "the directories are what is on offer: {rows:?}"
        );
        assert!(
            !rows.iter().any(|row| row.contains("Agent")),
            "the agents step aside: {rows:?}"
        );

        model.root_picker = None;
        let rows = sidebar_rows(&model, &mut Vec::new());
        assert!(
            rows.iter().any(|row| row.contains("Agent")),
            "and come back when it closes: {rows:?}"
        );
    }

    /// A root several levels deep is longer than the sidebar is wide, and
    /// the segment being typed is the half that must survive.
    #[test]
    fn a_long_root_gives_way_to_what_is_being_typed() {
        let root = uze_testkit::temp::TempDir::new("sidebar-root-elide");
        std::fs::create_dir_all(root.join("a-very-long-directory-name/inner")).unwrap();
        let mut model = agent_session_in("/repo");
        let mut picker = RootPicker::opened_in(&root.path().display().to_string());
        picker.descend();
        for character in "inn".chars() {
            picker.typed(character);
        }
        model.root_picker = Some(picker);

        let rows = sidebar_rows(&model, &mut Vec::new());
        let prompt = rows
            .iter()
            .find(|row| row.contains(" at "))
            .expect("the prompt row is drawn");
        assert!(prompt.contains("inn\u{258f}"), "{prompt}");
        assert!(prompt.contains('\u{2026}'), "the head gave way: {prompt}");
    }

    #[test]
    fn the_four_sidebar_states_are_decided_by_one_precedence() {
        // Selection is the only thing the glyph borrows from the cursor,
        // and it is the weakest claim: both states that describe the agent
        // itself outrank it, so the tab you are sitting on still shows you
        // a running turn or an unseen result rather than a plain dot.
        let mut model = agent_session();
        assert_eq!(
            model.agent_tab_status(PaneId(1), false),
            AgentTabStatus::Idle
        );
        assert_eq!(
            model.agent_tab_status(PaneId(1), true),
            AgentTabStatus::Selected
        );

        model.note_agent_prompt_submission(PaneId(1), &IDENTITIES, Some("hello"));
        assert_eq!(
            model.agent_tab_status(PaneId(1), true),
            AgentTabStatus::Working
        );

        model.agent_activity.remove(&PaneId(1));
        model.completed_agent_panes.insert(PaneId(1));
        assert_eq!(
            model.agent_tab_status(PaneId(1), true),
            AgentTabStatus::Completed
        );
    }

    #[test]
    fn each_sidebar_state_draws_its_own_glyph() {
        // Four states, four distinct indicators: the hollow dot, the green
        // dot, the spinner and the check must never collide, or the column
        // stops answering the question it exists for.
        let glyphs = [
            AgentTabStatus::Idle.glyph(0),
            AgentTabStatus::Selected.glyph(0),
            AgentTabStatus::Working.glyph(0),
            AgentTabStatus::Completed.glyph(0),
        ];
        for (index, glyph) in glyphs.iter().enumerate() {
            assert!(!glyphs[index + 1..].contains(glyph), "duplicate {glyph}");
        }
        assert_eq!(AgentTabStatus::Idle.color(), crate::ui::TEXT_FAINT);
        assert_eq!(AgentTabStatus::Selected.color(), crate::ui::ACCENT);
    }

    #[test]
    fn a_submitted_agent_prompt_works_until_its_pane_goes_quiet() {
        let mut model = agent_session();
        model.note_agent_prompt_submission(PaneId(1), &IDENTITIES, Some("hello"));
        assert_eq!(
            model.agent_tab_status(PaneId(1), false),
            AgentTabStatus::Working
        );
        assert!(workspace_has_active_agent_operation(&model, &IDENTITIES));

        assert!(!model.expire_agent_activity(Instant::now() + Duration::from_secs(1)));
        assert_eq!(
            model.agent_tab_status(PaneId(1), false),
            AgentTabStatus::Working
        );

        assert!(model.expire_agent_activity(Instant::now() + Duration::from_secs(4)));
        assert!(!workspace_has_active_agent_operation(&model, &IDENTITIES));
    }

    #[test]
    fn an_agent_that_starts_painting_on_its_own_reads_as_working() {
        // The regression that made the sidebar unreliable: activity used to
        // begin only at a literal Enter in the pane, so a turn the user did
        // not type — a hook, a queued follow-up, a subagent reporting back,
        // anything resumed after a reattach — ran to completion showing the
        // idle glyph.
        let mut model = agent_session();
        assert_eq!(
            model.agent_tab_status(PaneId(1), false),
            AgentTabStatus::Idle
        );

        model.apply(painted(PaneId(1)), &IDENTITIES);
        assert_eq!(
            model.agent_tab_status(PaneId(1), false),
            AgentTabStatus::Idle,
            "one repaint is a blink, not a turn"
        );

        animate(&mut model, PaneId(1), Instant::now());
        assert_eq!(
            model.agent_tab_status(PaneId(1), false),
            AgentTabStatus::Working
        );
    }

    #[test]
    fn one_repaint_arriving_in_pieces_is_not_an_animating_agent() {
        // A single harness redraw reaches the client as however many
        // damage events its bytes were chunked into, milliseconds apart.
        // Frame count alone would read that burst as a running turn.
        let mut model = agent_session();
        let start = Instant::now();
        for step in 0..4 * AGENT_BUSY_REPAINTS as u64 {
            model.note_agent_output(
                PaneId(1),
                &IDENTITIES,
                start + Duration::from_millis(10 * step),
            );
        }
        assert_eq!(
            model.agent_tab_status(PaneId(1), false),
            AgentTabStatus::Idle
        );
    }

    #[test]
    fn a_pane_that_only_blinks_is_never_working() {
        // The bug this rule exists for: an open agent sitting at its prompt
        // still repaints — a status line, a rotating hint — and treating
        // each one as work left idle agents spinning for as long as they
        // stayed open.
        let mut model = agent_session();
        let start = Instant::now();
        for step in 0..10 {
            model.note_agent_output(
                PaneId(1),
                &IDENTITIES,
                start + Duration::from_secs(2 * step),
            );
            assert_ne!(
                model.agent_tab_status(PaneId(1), false),
                AgentTabStatus::Working
            );
        }
    }

    #[test]
    fn reattaching_to_an_open_agent_does_not_read_as_a_running_turn() {
        // Every pane's first damage after an attach (and every damage after
        // a resize) redescribes the whole grid, because the server has no
        // comparable baseline to diff against. Counting those made every
        // open agent spin for a few seconds each time the workspace opened.
        let mut model = agent_session();
        for _ in 0..AGENT_BUSY_REPAINTS {
            model.apply(repainted_whole_grid(PaneId(1)), &IDENTITIES);
        }
        assert_eq!(
            model.agent_tab_status(PaneId(1), false),
            AgentTabStatus::Idle
        );
    }

    #[test]
    fn output_resuming_after_a_quiet_stretch_returns_the_pane_to_working() {
        // The other half of the same regression: a pane silent long enough
        // to expire could never get back to `Working`, because only Enter
        // could put it there. A long tool call therefore left the rest of
        // the turn showing as finished.
        let mut model = agent_session();
        model.note_agent_prompt_submission(PaneId(1), &IDENTITIES, Some("hello"));
        assert!(model.expire_agent_activity(Instant::now() + Duration::from_secs(4)));
        assert_ne!(
            model.agent_tab_status(PaneId(1), false),
            AgentTabStatus::Working
        );

        animate(&mut model, PaneId(1), Instant::now());
        assert_eq!(
            model.agent_tab_status(PaneId(1), false),
            AgentTabStatus::Working
        );
    }

    #[test]
    fn the_echo_of_a_prompt_being_typed_is_not_the_agent_working() {
        // Every keystroke opens its own grace window, so a prompt typed
        // steadily paints as many frames, as spread out, as a running turn.
        let mut model = agent_session();
        let start = Instant::now();
        for step in 0..4 * AGENT_BUSY_REPAINTS as u64 {
            let typed = start + Duration::from_millis(120 * step);
            model.open_echo_window(PaneId(1), typed, AGENT_ECHO_GRACE);
            model.note_agent_output(PaneId(1), &IDENTITIES, typed + Duration::from_millis(10));
        }
        assert_eq!(
            model.agent_tab_status(PaneId(1), false),
            AgentTabStatus::Idle
        );
    }

    #[test]
    fn a_paste_the_harness_lays_out_is_not_the_agent_working() {
        // Dropping an image into a prompt makes the harness reflow its
        // whole box — a burst of repaints as sustained as any animation,
        // arriving well after the pasted bytes did.
        let mut model = agent_session();
        let start = Instant::now();
        model.open_echo_window(PaneId(1), start, AGENT_PASTE_GRACE);
        animate(&mut model, PaneId(1), start);
        assert_eq!(
            model.agent_tab_status(PaneId(1), false),
            AgentTabStatus::Idle
        );
    }

    #[test]
    fn typing_over_a_running_turn_cannot_extend_it() {
        // Echo suppression holds whether or not a turn is running: the
        // user's own keystrokes are never evidence the agent is still
        // working, so the turn still ends on its own quiet window.
        let mut model = agent_session();
        let start = Instant::now();
        model.note_agent_prompt_submission(PaneId(1), &IDENTITIES, Some("hello"));
        for step in 0..4 * AGENT_BUSY_REPAINTS as u64 {
            let typed = start + Duration::from_millis(120 * step);
            model.open_echo_window(PaneId(1), typed, AGENT_ECHO_GRACE);
            model.note_agent_output(PaneId(1), &IDENTITIES, typed + Duration::from_millis(10));
        }

        assert!(model.expire_agent_activity(start + Duration::from_secs(4)));
        assert_ne!(
            model.agent_tab_status(PaneId(1), false),
            AgentTabStatus::Working
        );
    }

    #[test]
    fn output_during_a_turn_carries_it_past_the_quiet_window() {
        // The other direction: an agent still animating two seconds in is
        // still working, and must not be declared done on the strength of
        // when its prompt was submitted.
        let mut model = agent_session();
        model.note_agent_prompt_submission(PaneId(1), &IDENTITIES, Some("hello"));

        let later = Instant::now() + Duration::from_secs(2);
        animate(&mut model, PaneId(1), later);
        assert!(!model.expire_agent_activity(later + Duration::from_secs(2)));
        assert_eq!(
            model.agent_tab_status(PaneId(1), false),
            AgentTabStatus::Working
        );
    }

    #[test]
    fn a_shell_pane_never_receives_agent_activity() {
        let mut model = WorkspaceModel {
            session: Some(Session::new(
                WorkspaceId("workspace".into()),
                "/tmp".into(),
                80,
                24,
            )),
            ..WorkspaceModel::default()
        };
        model.note_agent_prompt_submission(PaneId(1), &IDENTITIES, Some("hello"));
        animate(&mut model, PaneId(1), Instant::now());
        assert!(model.agent_activity.is_empty());
    }

    #[test]
    fn completed_background_agent_keeps_a_check_until_its_tab_is_opened() {
        let mut session = Session::new(WorkspaceId("workspace".into()), "/tmp".into(), 80, 24);
        let agent_pane = session.add_tab(
            session.workspace.selected_space,
            "Agent".into(),
            None,
            80,
            24,
            "/tmp".into(),
        );
        let agent_tab = session.workspace.spaces[0].selected_tab;
        session.workspace.spaces[0].selected_tab = TabId(1);
        let mut model = WorkspaceModel {
            session: Some(session),
            ..WorkspaceModel::default()
        };
        model.note_agent_prompt_submission(agent_pane, &IDENTITIES, Some("hello"));
        assert!(model.expire_agent_activity(Instant::now() + Duration::from_secs(4)));
        assert_eq!(
            model.agent_tab_status(agent_pane, false),
            AgentTabStatus::Completed
        );

        model.acknowledge_completed_agent_tab(agent_tab);
        assert_eq!(
            model.agent_tab_status(agent_pane, false),
            AgentTabStatus::Idle
        );
    }

    #[test]
    fn a_check_clears_as_soon_as_its_pane_is_the_one_on_screen() {
        // Whichever way the user reached the tab — a click, Alt+n, a space
        // switch, a restored selection — the check has to go once they are
        // looking at it. Clearing it only at the call sites that happened to
        // know about it is what made "done" survive on a tab already open.
        let mut session = Session::new(WorkspaceId("workspace".into()), "/tmp".into(), 80, 24);
        let agent_pane = session.add_tab(
            session.workspace.selected_space,
            "Agent".into(),
            None,
            80,
            24,
            "/tmp".into(),
        );
        let agent_tab = session.workspace.spaces[0].selected_tab;
        session.workspace.spaces[0].selected_tab = TabId(1);
        let mut model = WorkspaceModel {
            session: Some(session),
            ..WorkspaceModel::default()
        };
        model.note_agent_prompt_submission(agent_pane, &IDENTITIES, Some("hello"));
        assert!(model.expire_agent_activity(Instant::now() + Duration::from_secs(4)));
        assert_eq!(
            model.agent_tab_status(agent_pane, false),
            AgentTabStatus::Completed
        );

        if let Some(session) = model.session.as_mut() {
            session.workspace.spaces[0].selected_tab = agent_tab;
        }
        assert!(model.expire_agent_activity(Instant::now() + Duration::from_secs(4)));
        assert_eq!(
            model.agent_tab_status(agent_pane, false),
            AgentTabStatus::Idle
        );
    }

    #[test]
    fn a_closed_tab_leaves_no_status_behind_for_the_next_pane() {
        let mut session = Session::new(WorkspaceId("workspace".into()), "/tmp".into(), 80, 24);
        let agent_pane = session.add_tab(
            session.workspace.selected_space,
            "Agent".into(),
            None,
            80,
            24,
            "/tmp".into(),
        );
        let agent_tab = session.workspace.spaces[0].selected_tab;
        session.workspace.spaces[0].selected_tab = TabId(1);
        let mut model = WorkspaceModel {
            session: Some(session),
            ..WorkspaceModel::default()
        };
        model.note_agent_prompt_submission(agent_pane, &IDENTITIES, Some("hello"));
        model.note_pane_input(agent_pane);

        if let Some(session) = model.session.as_mut() {
            session.remove_tab(agent_tab);
        }
        model.expire_agent_activity(Instant::now());
        assert!(model.agent_activity.is_empty());
        assert!(model.completed_agent_panes.is_empty());
        assert!(model.input_echo_until.is_empty());
    }

    #[test]
    fn pane_relative_is_1_indexed_and_excludes_anything_outside_the_pane() {
        let pane = Rect::new(10, 2, 40, 20);
        assert_eq!(
            pane_relative(mouse_at(10, 2, MouseEventKind::Moved), pane),
            Some((1, 1))
        );
        assert_eq!(
            pane_relative(mouse_at(49, 21, MouseEventKind::Moved), pane),
            Some((40, 20))
        );
        // One past the pane's own bottom-right corner in either axis, and
        // anything left of/above its origin (the sidebar, tab strip) — all
        // outside.
        assert_eq!(
            pane_relative(mouse_at(50, 21, MouseEventKind::Moved), pane),
            None
        );
        assert_eq!(
            pane_relative(mouse_at(49, 22, MouseEventKind::Moved), pane),
            None
        );
        assert_eq!(
            pane_relative(mouse_at(9, 5, MouseEventKind::Moved), pane),
            None
        );
        assert_eq!(
            pane_relative(mouse_at(15, 1, MouseEventKind::Moved), pane),
            None
        );
    }

    #[test]
    fn encode_mouse_sgr_matches_the_documented_wire_format() {
        assert_eq!(
            encode_mouse(MouseEventKind::Down(MouseButton::Left), 3, 5, true),
            Some(b"\x1b[<0;3;5M".to_vec())
        );
        assert_eq!(
            encode_mouse(MouseEventKind::Up(MouseButton::Left), 3, 5, true),
            Some(b"\x1b[<0;3;5m".to_vec())
        );
        assert_eq!(
            encode_mouse(MouseEventKind::Drag(MouseButton::Left), 3, 5, true),
            Some(b"\x1b[<32;3;5M".to_vec())
        );
        assert_eq!(
            encode_mouse(MouseEventKind::ScrollUp, 3, 5, true),
            Some(b"\x1b[<64;3;5M".to_vec())
        );
        // Unsupported buttons/kinds (right/middle click, plain motion) stay
        // unforwarded rather than guessing at an encoding for them.
        assert_eq!(
            encode_mouse(MouseEventKind::Down(MouseButton::Right), 3, 5, true),
            None
        );
    }

    #[test]
    fn encode_mouse_legacy_x10_saturates_instead_of_overflowing_past_223() {
        assert_eq!(
            encode_mouse(MouseEventKind::Down(MouseButton::Left), 1, 1, false),
            Some(vec![0x1b, b'[', b'M', 32, 33, 33])
        );
        assert_eq!(
            encode_mouse(MouseEventKind::Up(MouseButton::Left), 1, 1, false),
            Some(vec![0x1b, b'[', b'M', 32 + 3, 33, 33])
        );
        assert_eq!(
            encode_mouse(MouseEventKind::Down(MouseButton::Left), 999, 999, false),
            Some(vec![0x1b, b'[', b'M', 32, 32 + 223, 32 + 223])
        );
    }

    fn mouse_at(column: u16, row: u16, kind: MouseEventKind) -> crossterm::event::MouseEvent {
        crossterm::event::MouseEvent {
            kind,
            column,
            row,
            modifiers: crossterm::event::KeyModifiers::empty(),
        }
    }

    fn identities() -> Vec<AgentIdentity> {
        vec![
            AgentIdentity {
                binary: "claude",
                integration: "claude-code",
                display_name: "Claude Code",
            },
            AgentIdentity {
                binary: "codex",
                integration: "codex",
                display_name: "Codex",
            },
        ]
    }

    fn tab_with(label: &str, process: &str) -> Tab {
        let pane = Pane {
            id: PaneId(1),
            cwd: "/tmp".into(),
            columns: 80,
            rows: 24,
            process: process.to_owned(),
        };
        Tab {
            id: TabId(1),
            label: label.to_owned(),
            agent: None,
            layout: Layout::Pane(pane),
            focus: Focus { pane: PaneId(1) },
        }
    }

    #[test]
    fn recognizes_a_shim_launched_process_by_its_live_alias() {
        // What `UZE_SHIM_NAME` resolves `pane.process` to for a shim-
        // launched pane (see `src/shim.rs`) — a plain shell tab where
        // someone manually typed `claude`, unrelated to the picker.
        let tab = tab_with("shell 2", "claude");
        assert_eq!(agent_identity_for_tab(&identities(), &tab), Some("claude"));
    }

    #[test]
    fn new_agent_labels_are_numbered_independently_of_harnesses() {
        let mut session = Session::new(WorkspaceId("workspace".into()), "/tmp".into(), 80, 24);
        let model = WorkspaceModel {
            session: Some(session.clone()),
            ..WorkspaceModel::default()
        };
        assert_eq!(next_agent_label(&model), "agent 1");

        session.add_tab(
            session.workspace.selected_space,
            "agent 1".into(),
            None,
            80,
            24,
            "/tmp".into(),
        );
        let model = WorkspaceModel {
            session: Some(session),
            ..WorkspaceModel::default()
        };
        assert_eq!(next_agent_label(&model), "agent 2");
    }

    #[test]
    fn a_lone_agent_can_close_when_it_is_replaced_by_a_shell() {
        let mut session = Session::new(WorkspaceId("workspace".into()), "/tmp".into(), 80, 24);
        session.workspace.spaces[0].tabs[0].label = "Claude Code".into();
        let tab = session.workspace.spaces[0].selected_tab;
        let model = WorkspaceModel {
            session: Some(session),
            ..WorkspaceModel::default()
        };

        assert!(tab_needs_replacement_shell(&model, &identities(), tab));
        assert!(can_close_tab_from_menu(&model, &identities(), tab));
    }

    #[test]
    fn a_lone_plain_shell_stays_non_closable() {
        let session = Session::new(WorkspaceId("workspace".into()), "/tmp".into(), 80, 24);
        let tab = session.workspace.spaces[0].selected_tab;
        let model = WorkspaceModel {
            session: Some(session),
            ..WorkspaceModel::default()
        };

        assert!(!tab_needs_replacement_shell(&model, &identities(), tab));
        assert!(!can_close_tab_from_menu(&model, &identities(), tab));
    }

    #[test]
    fn a_plain_shell_matches_neither_signal() {
        let tab = tab_with("shell", "zsh");
        assert_eq!(agent_identity_for_tab(&identities(), &tab), None);
    }

    #[test]
    fn new_tabs_use_the_selected_panes_live_directory() {
        let mut session = Session::new(WorkspaceId("workspace".into()), "/tmp/root".into(), 80, 24);
        assert!(session.update_pane_status(PaneId(1), "/tmp/project/src".into(), "zsh".into()));
        let model = WorkspaceModel {
            session: Some(session),
            ..WorkspaceModel::default()
        };

        assert_eq!(selected_pane_cwd(&model), Some("/tmp/project/src".into()));
    }

    #[test]
    fn an_unrecognized_process_name_does_not_match() {
        // The exact motivating case: Claude Code's live comm resolves to
        // its own version string, not `claude` — recognizable only via the
        // shim-identity signal (`claude` from `UZE_SHIM_NAME`), not this
        // raw process read alone.
        let tab = tab_with("shell", "2.1.251");
        assert_eq!(agent_identity_for_tab(&identities(), &tab), None);
    }

    #[test]
    fn damage_updates_the_tracked_panes_mouse_and_bracketed_paste_mode() {
        // Regression: `mouse`/`bracketed_paste` ride along on every
        // `PaneDamage`, not just the initial full `Snapshot` — a pane's own
        // program typically turns these on shortly after it starts, which
        // is after the client's one-time first snapshot already fired. A
        // client that only reads these off `Snapshot` would forward mouse
        // clicks and pastes into the pane forever as if it never asked.
        let mut model = WorkspaceModel {
            panes: [(PaneId(1), blank_pane(PaneId(1), 80, 24))].into(),
            ..WorkspaceModel::default()
        };
        assert!(!model.panes[&PaneId(1)].mouse.reports_clicks);
        assert!(!model.panes[&PaneId(1)].bracketed_paste);

        model.apply(
            ClientEvent::Damage(PaneDamage {
                pane: PaneId(1),
                columns: 80,
                rows: 24,
                cursor: Cursor { column: 0, row: 0 },
                alternate_screen: false,
                mouse: MouseMode {
                    reports_clicks: true,
                    reports_drag: false,
                    sgr: true,
                },
                bracketed_paste: true,
                changed: Vec::new(),
            }),
            &[],
        );

        assert!(model.panes[&PaneId(1)].mouse.reports_clicks);
        assert!(model.panes[&PaneId(1)].bracketed_paste);
    }

    #[test]
    fn forward_paste_frames_the_bytes_only_when_the_pane_asked_for_bracketed_paste() {
        let mut plain = blank_pane(PaneId(1), 80, 24);
        plain.bracketed_paste = false;
        let plain_model = WorkspaceModel {
            session: Some(Session::new(
                WorkspaceId("workspace".into()),
                "/tmp".into(),
                80,
                24,
            )),
            panes: [(PaneId(1), plain)].into(),
            ..WorkspaceModel::default()
        };
        let mut stream = Vec::new();
        forward_paste(&mut stream, &plain_model, "hello");
        assert_eq!(decode_input_bytes(&stream), b"hello".to_vec());

        let mut bracketed = blank_pane(PaneId(1), 80, 24);
        bracketed.bracketed_paste = true;
        let bracketed_model = WorkspaceModel {
            session: Some(Session::new(
                WorkspaceId("workspace".into()),
                "/tmp".into(),
                80,
                24,
            )),
            panes: [(PaneId(1), bracketed)].into(),
            ..WorkspaceModel::default()
        };
        let mut stream = Vec::new();
        forward_paste(&mut stream, &bracketed_model, "hello");
        assert_eq!(
            decode_input_bytes(&stream),
            b"\x1b[200~hello\x1b[201~".to_vec()
        );
    }

    #[test]
    fn scroll_uses_arrow_keys_for_an_alternate_screen_without_mouse_reporting() {
        let mut pane = blank_pane(PaneId(1), 80, 24);
        pane.alternate_screen = true;
        let model = WorkspaceModel {
            session: Some(Session::new(
                WorkspaceId("workspace".into()),
                "/tmp".into(),
                80,
                24,
            )),
            panes: [(PaneId(1), pane)].into(),
            ..WorkspaceModel::default()
        };
        let mut stream = Vec::new();
        forward_scroll(
            &mut stream,
            &model,
            Rect::new(0, 0, 80, 24),
            mouse_at(4, 5, MouseEventKind::ScrollUp),
        );
        assert_eq!(decode_input_bytes(&stream), b"\x1b[A".to_vec());
    }

    #[test]
    fn scroll_uses_terminal_scrollback_for_a_normal_screen_without_mouse_reporting() {
        let model = WorkspaceModel {
            session: Some(Session::new(
                WorkspaceId("workspace".into()),
                "/tmp".into(),
                80,
                24,
            )),
            panes: [(PaneId(1), blank_pane(PaneId(1), 80, 24))].into(),
            ..WorkspaceModel::default()
        };
        let mut stream = Vec::new();
        forward_scroll(
            &mut stream,
            &model,
            Rect::new(0, 0, 80, 24),
            mouse_at(4, 5, MouseEventKind::ScrollDown),
        );
        assert_eq!(
            decode_request(&stream),
            ClientRequest::Scroll {
                pane: PaneId(1),
                lines: -3,
            }
        );
    }

    /// Mirrors `uze_terminal::runtime`'s length-prefixed bincode framing
    /// (a 4-byte little-endian length, then the payload) — `send_request`
    /// writes real wire frames, not bare JSON, so a test reading `stream`
    /// back has to strip the same prefix.
    fn decode_input_bytes(stream: &[u8]) -> Vec<u8> {
        match decode_request(stream) {
            ClientRequest::Input { bytes, .. } => bytes,
            other => panic!("expected ClientRequest::Input, got {other:?}"),
        }
    }

    fn decode_request(stream: &[u8]) -> ClientRequest {
        let (len_bytes, payload) = stream.split_at(4);
        let len = u32::from_le_bytes(len_bytes.try_into().unwrap()) as usize;
        assert_eq!(payload.len(), len, "one ClientRequest frame");
        bincode::deserialize(payload).expect("one ClientRequest frame")
    }

    /// A Ctrl+O round trip to management is a detach and a fresh attach.
    /// What the client resolved on its own — the sidebar's tasks,
    /// branches and a completion noticed while the user was elsewhere —
    /// must come back with it, while the server's view of the session and
    /// the presentation state of the attach that ended must not.
    #[test]
    fn memory_carries_what_the_client_resolved_across_attaches() {
        let mut model = agent_with_task(TaskStateView::Ready, 1);
        model
            .branches
            .insert(PathBuf::from("/repo"), "agent/ai".to_owned());
        model.completed_agent_panes.insert(PaneId(1));
        model.error = Some("stale".to_owned());
        model
            .hits
            .push((Rect::new(0, 0, 1, 1), WorkspaceHit::NewSpace));

        let model = WorkspaceModel::recall(model.remember());

        assert_eq!(model.tasks[&PathBuf::from("/repo")].len(), 1);
        assert_eq!(
            model
                .branches
                .get(&PathBuf::from("/repo"))
                .map(String::as_str),
            Some("agent/ai")
        );
        assert!(model.completed_agent_panes.contains(&PaneId(1)));
        assert!(model.session.is_none());
        assert!(model.error.is_none());
        assert!(model.hits.is_empty());
    }
}

mod prompt_buffer_tests {
    use super::PromptBuffer;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn typed(buffer: &mut PromptBuffer, text: &str) {
        for character in text.chars() {
            buffer.apply(key(KeyCode::Char(character)));
        }
    }

    #[test]
    fn plain_typing_round_trips() {
        let mut buffer = PromptBuffer::default();
        typed(&mut buffer, "hello world");
        assert_eq!(buffer.submit().as_deref(), Some("hello world"));
    }

    #[test]
    fn the_buffer_is_empty_again_after_submitting() {
        let mut buffer = PromptBuffer::default();
        typed(&mut buffer, "first");
        buffer.submit();
        typed(&mut buffer, "second");
        assert_eq!(buffer.submit().as_deref(), Some("second"));
    }

    #[test]
    fn editing_mid_line_reconstructs_the_real_text() {
        let mut buffer = PromptBuffer::default();
        typed(&mut buffer, "helo");
        buffer.apply(key(KeyCode::Left));
        typed(&mut buffer, "l");
        buffer.apply(key(KeyCode::Home));
        typed(&mut buffer, "> ");
        buffer.apply(key(KeyCode::End));
        typed(&mut buffer, "!");
        assert_eq!(buffer.submit().as_deref(), Some("> hello!"));
    }

    #[test]
    fn backspace_and_delete_remove_around_the_cursor() {
        let mut buffer = PromptBuffer::default();
        typed(&mut buffer, "abcd");
        buffer.apply(key(KeyCode::Backspace));
        buffer.apply(key(KeyCode::Left));
        buffer.apply(key(KeyCode::Delete));
        assert_eq!(buffer.submit().as_deref(), Some("ab"));
    }

    #[test]
    fn deleting_past_either_edge_is_a_no_op() {
        let mut buffer = PromptBuffer::default();
        buffer.apply(key(KeyCode::Backspace));
        buffer.apply(key(KeyCode::Delete));
        typed(&mut buffer, "x");
        buffer.apply(key(KeyCode::Right));
        buffer.apply(key(KeyCode::Delete));
        assert_eq!(buffer.submit().as_deref(), Some("x"));
    }

    // The agent's own line editor owns these keys, and what it does with
    // them is invisible from here — so nothing is recorded at all rather
    // than a prompt the user never typed.
    #[test]
    fn history_recall_discards_the_reconstruction() {
        for code in [KeyCode::Up, KeyCode::Down] {
            let mut buffer = PromptBuffer::default();
            typed(&mut buffer, "typed");
            buffer.apply(key(code));
            assert_eq!(buffer.submit(), None, "{code:?} must not be recorded");
        }
    }

    #[test]
    fn completion_and_escape_discard_the_reconstruction() {
        for code in [KeyCode::Tab, KeyCode::Esc] {
            let mut buffer = PromptBuffer::default();
            typed(&mut buffer, "typed");
            buffer.apply(key(code));
            assert_eq!(buffer.submit(), None, "{code:?} must not be recorded");
        }
    }

    #[test]
    fn a_control_or_alt_chord_discards_the_reconstruction() {
        for modifiers in [KeyModifiers::CONTROL, KeyModifiers::ALT] {
            let mut buffer = PromptBuffer::default();
            typed(&mut buffer, "typed");
            buffer.apply(KeyEvent::new(KeyCode::Char('u'), modifiers));
            typed(&mut buffer, " more");
            assert_eq!(buffer.submit(), None, "{modifiers:?} must not be recorded");
        }
    }

    #[test]
    fn distrust_does_not_outlive_the_line_it_applied_to() {
        let mut buffer = PromptBuffer::default();
        buffer.apply(key(KeyCode::Tab));
        assert_eq!(buffer.submit(), None);
        typed(&mut buffer, "clean line");
        assert_eq!(buffer.submit().as_deref(), Some("clean line"));
    }

    #[test]
    fn a_trailing_backslash_continues_the_line_instead_of_submitting() {
        let mut buffer = PromptBuffer::default();
        typed(&mut buffer, "first\\");
        assert_eq!(buffer.submit(), None);
        typed(&mut buffer, "second");
        assert_eq!(buffer.submit().as_deref(), Some("first\nsecond"));
    }

    #[test]
    fn a_paste_lands_at_the_cursor() {
        let mut buffer = PromptBuffer::default();
        typed(&mut buffer, "ab");
        buffer.apply(key(KeyCode::Left));
        buffer.paste("XY");
        assert_eq!(buffer.submit().as_deref(), Some("aXYb"));
    }

    #[test]
    fn a_pasted_carriage_return_becomes_a_newline_rather_than_a_submit() {
        let mut buffer = PromptBuffer::default();
        buffer.paste("one\r\ntwo");
        assert_eq!(buffer.submit().as_deref(), Some("one\n\ntwo"));
    }
}
