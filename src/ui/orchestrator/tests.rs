//! Tests for the workspace client.
//!
//! Moved out of `orchestrator.rs` alongside the `render`/`input` split:
//! they were the last ~500 lines standing between a reader and the
//! session-driving code the file is actually about.

use super::*;

mod workspace_tests {
    use crate::ui::theme::{self, Token};

    /// What a pane's own program is told the terminal's colours are is the
    /// theme's, not a transcription of it.
    ///
    /// The two constants this replaced lived in another crate, copied from
    /// the palette by hand — so a colour changed here left a program inside
    /// a pane picking a light- or dark-adapted UI against a background
    /// nobody was drawing.
    #[test]
    fn the_palette_a_pane_is_told_about_is_the_one_being_drawn() {
        let theme = uze_theme::active();
        let palette = super::super::active_palette();
        let triple = |token| {
            let rgb = theme.color(token);
            (rgb.0, rgb.1, rgb.2)
        };
        assert_eq!(palette.foreground, triple(Token::TextPrimary));
        assert_eq!(palette.background, triple(Token::SurfaceBackground));
        for (index, entry) in palette.ansi.iter().enumerate() {
            assert_eq!(*entry, triple(Token::ANSI[index]), "ansi.{index}");
        }
    }

    use super::WorkspaceHit;
    use super::{
        AGENT_BUSY_REPAINTS, AGENT_ECHO_GRACE, AGENT_PASTE_GRACE, AgentIdentity, AgentTabStatus,
        Attach, CommitDetailPopup, CommitDetailResolution, CompletionBehavior, DeliveryResolution,
        DraggingTab, ExtensionHit, Flow, GitAnswer, GitBadge, GitResolution, NOTICE_TTL,
        PendingDrop, PlacementResolution, PreservedOverlay, RootPicker, ScrollDirection,
        TabDragGroup, TaskResolution, TaskStateView, TaskView, UpstreamSync, Viewport,
        WorkspaceModel, adopt_agent_labels, agent_activity_frame, agent_identity_for_tab,
        answered_or, blank_pane, can_close_tab_from_menu, checkout_lost, encode_mouse,
        evaluation_key, forward_paste, forward_scroll, next_agent_label, next_shell_label,
        open_code, open_commit_detail, pane_relative, pending_tab_drop,
        render::{
            self, FrameMetrics, WorkspaceLayout, compute_layout, render_commit_detail,
            render_preserved, render_sidebar, render_status_catalog, render_tab_strip, task_mark,
            timeline_height,
        },
        scroll_timeline, scroll_tree, selected_pane_cwd, space_context_agent, space_cwd,
        space_own_tab, strip_tabs, sync_slot_occupancy, tab_drag_group, tab_drag_group_members,
        tab_needs_replacement_shell, toggle_timeline, workspace_has_active_agent_operation,
    };
    use crossterm::event::{MouseButton, MouseEventKind};
    use ratatui::layout::Rect;
    use ratatui::style::Color;
    use ratatui::{Terminal, backend::TestBackend};
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};
    use uze_core::UzeHome;
    use uze_extensions::view::ViewHit;
    use uze_terminal::{
        CellAttributes, ClientEvent, ClientRequest, Cursor, MouseMode, Pane, PaneDamage, PaneId,
        RenderCell, Session, SpaceId, Tab, TabId, TerminalColor,
    };

    /// A fresh one-space session over `root`, at the size every test
    /// frame is drawn at.
    fn session(root: impl AsRef<Path>, kind: uze_terminal::SpaceKind) -> Session {
        Session::new(
            uze_terminal::SpaceSeat {
                root: root.as_ref().to_path_buf(),
                kind,
            },
            80,
            24,
        )
    }

    /// A model attached to `session` and nothing else.
    fn model_of(session: Session) -> WorkspaceModel {
        WorkspaceModel {
            session: Some(session),
            ..WorkspaceModel::default()
        }
    }

    fn identities_fixture() -> Vec<AgentIdentity> {
        vec![AgentIdentity {
            binary: "agent",
            integration: "agent",
            display_name: "Agent",
            launch: std::path::PathBuf::from("agent"),
            continuity_gap: None,
        }]
    }

    /// A one-tab session whose only tab `agent_identity_for_tab` resolves
    /// to the fixture identity by its probed process.
    fn agent_session() -> WorkspaceModel {
        let mut session = session("/tmp", uze_terminal::SpaceKind::Worktree);
        session.workspace.spaces[0].tabs[0].label = "Agent".into();
        session.workspace.spaces[0].tabs[0].pane.process = "agent".into();
        model_of(session)
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
            model.note_agent_output(
                pane,
                &identities_fixture(),
                start + Duration::from_millis(120 * step),
            );
        }
    }

    #[test]
    fn only_the_active_spaces_agent_carries_the_selected_dot() {
        // A background space keeps a `selected_tab` of its own — where it
        // would resume, not where the user is. Drawing the dot from that
        // alone gave the sidebar one "this is the agent you are talking to"
        // per open space.
        let mut session = session("/tmp", uze_terminal::SpaceKind::Worktree);
        session.workspace.spaces[0].tabs[0].label = "Agent".into();
        session.workspace.spaces[0].tabs[0].pane.process = "agent".into();
        session.create_space(
            Some("second".into()),
            uze_terminal::SpaceSeat {
                root: "/tmp/second".into(),
                kind: uze_terminal::SpaceKind::Worktree,
            },
            80,
            24,
        );
        session.workspace.spaces[1].tabs[0].label = "Agent".into();
        session.workspace.spaces[1].tabs[0].pane.process = "agent".into();
        let model = model_of(session);

        let mut terminal = Terminal::new(TestBackend::new(40, 24)).unwrap();
        let mut hits = Vec::new();
        terminal
            .draw(|frame| {
                render_sidebar(
                    frame,
                    frame.area(),
                    &model,
                    &identities_fixture(),
                    &mut hits,
                    &mut FrameMetrics::default(),
                )
            })
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
            completion: CompletionBehavior::Merge,
            ahead,
            published_as: None,
            published_request: None,
            unsynced: None,
            created_at_unix: 1,
        }
    }

    /// A one-agent session in the slot `/repo/.worktrees/ai`, whose task is
    /// in `state`.
    fn agent_with_task(state: TaskStateView, ahead: usize) -> WorkspaceModel {
        let mut model = agent_session_in("/repo/.worktrees/ai");
        stamp_first_tab(&mut model, "t1");
        model.remembered.tasks.insert(
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
            .draw(|frame| {
                render_tab_strip(frame, frame.area(), model, &identities_fixture(), &mut hits)
            })
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

    /// The colours one header chip is drawn in: the surface under its
    /// padding, and the label's own foreground. Read off the cells rather
    /// than off the skin function, so the test proves what reaches the
    /// screen.
    fn chip_colors(model: &WorkspaceModel, rect: Rect) -> (Color, Color) {
        let mut terminal = Terminal::new(TestBackend::new(80, 3)).unwrap();
        let mut hits = Vec::new();
        terminal
            .draw(|frame| {
                render_tab_strip(frame, frame.area(), model, &identities_fixture(), &mut hits)
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (buffer[(rect.x + 1, rect.y)].fg, buffer[(rect.x, rect.y)].bg)
    }

    /// Where the strip drew the given control last frame.
    fn hit_rect(model: &WorkspaceModel, wanted: WorkspaceHit) -> Rect {
        let (_, hits) = tab_strip(model);
        hits.iter()
            .find(|(_, hit)| *hit == wanted)
            .map(|(rect, _)| *rect)
            .unwrap_or_else(|| panic!("{wanted:?} is not on the strip"))
    }

    /// A space holding two agents, each with one shell of its own, plus
    /// the shell the space was born with. Returns the model and the two
    /// agent tabs, in creation order.
    fn two_agents_with_shells() -> (WorkspaceModel, TabId, TabId) {
        let mut session = session("/repo", uze_terminal::SpaceKind::Worktree);
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
        let model = model_of(session);
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

    /// A tab number means "that chip", so it is counted along the strip —
    /// which is contextual — and never along the space's own tab list.
    ///
    /// The list held every agent in the space and both their shells, so a
    /// number walked past the chips on screen and landed on another
    /// agent: a gesture that only ever meant "the second one here" changed
    /// which agent the workspace was about. One list now answers for both
    /// the chips and the numbers, which is the only way they can agree.
    #[test]
    fn a_tab_number_counts_along_the_strip_and_not_past_it() {
        let (mut model, first, second) = two_agents_with_shells();
        model.session.as_mut().expect("session").select_tab(second);
        let identities = identities_fixture();
        let session = model.session.as_ref().expect("session");
        let space = session.selected_space();
        let strip = strip_tabs(space, space_context_agent(space, &identities), &identities);

        assert_eq!(strip.len(), 2, "the agent in front, and its own shell");
        assert_eq!(strip[0].id, second, "the agent leads its own strip");
        assert_ne!(
            strip[1].id, first,
            "and the other agent is not on it at any position"
        );

        assert!(
            space.tabs.len() > strip.len(),
            "the space holds more than the strip shows — the bootstrap \
             shell and the other agent's context"
        );
        assert_ne!(
            space.tabs[1].id, strip[1].id,
            "so counting along the space would land somewhere no chip is"
        );
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
        let own = first_tab(&model).id;
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

        assert_eq!(next_shell_label(&model, &identities_fixture()), "shell 2");

        let own = first_tab(&model).id;
        model.session.as_mut().expect("session").select_tab(own);
        assert_eq!(
            next_shell_label(&model, &identities_fixture()),
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
            space_own_tab(&session.workspace.spaces[0], &identities_fixture()),
            Some(own),
            "from an agent, back to the space's own shell"
        );

        session.select_tab(own);
        assert_eq!(
            space_own_tab(&session.workspace.spaces[0], &identities_fixture()),
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
        let mut metrics = render::FrameMetrics::default();
        terminal
            .draw(|frame| {
                render::render(frame, model, &identities_fixture(), &mut hits, &mut metrics)
            })
            .unwrap();
        model.hits = hits;
        model.absorb_manage_frame(metrics.manage);
        compute_layout(area, model.sidebar_width)
    }

    /// The whole frame as text, row by row — for asserting not just that
    /// something was drawn, but where.
    fn frame_rows(model: &mut WorkspaceModel) -> Vec<String> {
        let area = Rect::new(0, 0, 80, 24);
        let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
        terminal
            .draw(|frame| {
                render::render(
                    frame,
                    model,
                    &identities_fixture(),
                    &mut Vec::new(),
                    &mut render::FrameMetrics::default(),
                )
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..buffer.area.height)
            .map(|row| {
                (0..buffer.area.width)
                    .map(|column| buffer[(column, row)].symbol())
                    .collect()
            })
            .collect()
    }

    /// Glance at the code, close it, come back: the commonest gesture in
    /// the product, and the one that used to cost the whole walk down the
    /// tree again. The place is per checkout, so another agent's surface
    /// is another place rather than the same one moved.
    #[test]
    fn coming_back_to_a_checkouts_code_returns_to_where_it_was_left() {
        use uze_extensions::{DirEntry, code};

        let (mut model, first, second) = two_agents_with_shells();
        let answer_listing = |model: &mut WorkspaceModel, root: &str| {
            let view = model.code.as_mut().expect("the surface is open");
            view.take_request();
            view.absorb(code::FileAnswer::Listed {
                path: PathBuf::from(root),
                entries: Ok(vec![
                    DirEntry {
                        directory: false,
                        name: "a.rs".to_owned(),
                    },
                    DirEntry {
                        directory: false,
                        name: "b.rs".to_owned(),
                    },
                ]),
            });
        };

        let space = crate::ui::extension_view::content_space(
            Rect::new(0, 0, 120, 40),
            model.code_tree_width,
        );

        model.session.as_mut().expect("session").select_tab(first);
        open_code(&mut model, code::ContentMode::Contents);
        answer_listing(&mut model, "/repo/.worktrees/a");
        // Walked away from the row the tree opened on, which is the part
        // that must survive the round trip.
        code::handle_command(
            model.code.as_mut().expect("open"),
            uze_extensions::view::Command::SelectNext,
            space,
        );
        let walked_to = model.code.as_ref().expect("open").place();

        model.close_code();
        assert!(model.code.is_none());

        // Another agent's checkout is a different place, not this one.
        model.session.as_mut().expect("session").select_tab(second);
        open_code(&mut model, code::ContentMode::Contents);
        answer_listing(&mut model, "/repo/.worktrees/b");
        assert_ne!(
            model.code.as_ref().expect("open").place(),
            walked_to,
            "a checkout never visited opens on its own first row"
        );
        model.close_code();

        model.session.as_mut().expect("session").select_tab(first);
        open_code(&mut model, code::ContentMode::Contents);
        assert_eq!(
            model.code.as_ref().expect("open").place(),
            walked_to,
            "and the one left mid-walk is where it was left"
        );
    }

    /// A click inside the explorer has to resolve to the row the frame
    /// drew, not to something laid out under it. The overlay covers the
    /// whole frame and pushes its own hits into the shared table, so
    /// "does a click reach the extension" is a question only the real
    /// render can answer.
    #[test]
    fn a_click_inside_the_explorer_reaches_the_extension() {
        use uze_extensions::{DirEntry, ExtensionHit, code, view::ViewHit};

        let root = PathBuf::from("/repo");
        let (mut model, first, _second) = two_agents_with_shells();
        model.session.as_mut().expect("session").select_tab(first);

        let mut view = code::CodeView::opening(
            root.clone(),
            "/repo".to_owned(),
            code::ContentMode::Contents,
        );
        // Answered by hand rather than off a disk: what is under test is
        // where the click lands, and a temp directory would only add a
        // way for the test to fail for reasons of its own.
        view.take_request();
        view.absorb(code::FileAnswer::Listed {
            path: root,
            entries: Ok(vec![
                DirEntry {
                    directory: true,
                    name: "src".to_owned(),
                },
                DirEntry {
                    directory: false,
                    name: "README.md".to_owned(),
                },
            ]),
        });
        model.code = Some(view);
        full_frame(&mut model);

        let row = model
            .hits
            .iter()
            .find(|(_, hit)| {
                matches!(
                    hit,
                    WorkspaceHit::Extension(ExtensionHit::Code(ViewHit::SelectItem(_)))
                )
            })
            .expect("the explorer drew a clickable file row")
            .0;
        assert!(
            matches!(
                model.hit_at(row.x + 2, row.y),
                Some(WorkspaceHit::Extension(ExtensionHit::Code(
                    ViewHit::SelectItem(_)
                )))
            ),
            "a click on the row resolves to that row, not to something under it"
        );

        let caret = model.hits.iter().find(|(_, hit)| {
            matches!(
                hit,
                WorkspaceHit::Extension(ExtensionHit::Code(ViewHit::PlaceCaret { .. }))
            )
        });
        assert!(
            caret.is_none(),
            "with no file open there is nothing to put a caret in"
        );
    }

    /// The changes chip is a badge that is also a door: it says how much
    /// changed, so with nothing to say it says nothing. What that must
    /// not cost is reachability — the code chip is always there, and the
    /// diff is one mode switch away inside the surface it opens.
    #[test]
    fn the_changes_chip_comes_with_the_work_and_the_code_chip_never_leaves() {
        let (mut model, first, _second) = two_agents_with_shells();
        model.session.as_mut().expect("session").select_tab(first);

        model.remembered.git_badge = None;
        full_frame(&mut model);
        assert!(
            model
                .hits
                .iter()
                .any(|(_, hit)| *hit == WorkspaceHit::OpenFiles),
            "a clean checkout still offers its files"
        );
        assert!(
            !model
                .hits
                .iter()
                .any(|(_, hit)| *hit == WorkspaceHit::OpenChanges),
            "and says nothing about changes rather than saying zero"
        );

        model.remembered.git_badge = Some(GitBadge {
            cwd: PathBuf::from("/repo/.worktrees/a"),
            summary: Some(uze_extensions::code::ChangeSummary {
                additions: 3,
                deletions: 1,
            }),
            timeline: None,
            timeline_checked_at: Instant::now(),
            checked_at: Instant::now(),
        });
        full_frame(&mut model);
        let changes = model
            .hits
            .iter()
            .find(|(_, hit)| *hit == WorkspaceHit::OpenChanges)
            .expect("work arriving brings the badge")
            .0;
        let files = model
            .hits
            .iter()
            .find(|(_, hit)| *hit == WorkspaceHit::OpenFiles)
            .expect("the code chip is still there")
            .0;
        assert!(
            files.right() <= changes.x,
            "the badge arrives to the right of the code chip, so it never moves it"
        );
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
            tab_drag_group(&model, &identities_fixture(), &layout, sidebar_rect, first),
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
            tab_drag_group(&model, &identities_fixture(), &layout, strip_rect, first),
            Some(TabDragGroup::Strip(space, Some(first))),
            "the very same tab, but its strip chip's rect names the strip's group"
        );

        assert_eq!(
            tab_drag_group(&model, &identities_fixture(), &layout, layout.pane, first),
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

        let agents = tab_drag_group_members(
            &model,
            &identities_fixture(),
            &layout,
            TabDragGroup::Agents(space),
        );
        assert_eq!(
            agents.iter().map(|(_, tab)| *tab).collect::<Vec<_>>(),
            vec![first, second],
            "sidebar rows top to bottom, one entry per agent despite each pushing two hits"
        );

        let strip = tab_drag_group_members(
            &model,
            &identities_fixture(),
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
        let mut session = session("/repo", uze_terminal::SpaceKind::Worktree);
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
        let mut model = model_of(session);
        let layout = full_frame(&mut model);
        let dragged = agent_ids[0];

        let all = tab_drag_group_members(
            &model,
            &identities_fixture(),
            &layout,
            TabDragGroup::Agents(space),
        );
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

        let Sidebar {
            rows, hits, buffer, ..
        } = sidebar(&model, &identities_fixture());
        let second_row = hits
            .iter()
            .find(|(_, hit)| matches!(hit, WorkspaceHit::SelectTab(tab) if *tab == second))
            .map(|(rect, _)| rect.y)
            .expect("the drop target's own row");
        let column = gutter_column(&hits);
        assert!(
            lit_gutter_rows(&buffer, column, uze_terminal::SpaceKind::Worktree)
                .contains(&second_row),
            "indicator on the target row: {rows:?}"
        );

        model.dragging_tab = model
            .dragging_tab
            .map(|d| DraggingTab { armed: false, ..d });
        let buffer = sidebar(&model, &identities_fixture()).buffer;
        assert!(
            !lit_gutter_rows(&buffer, column, uze_terminal::SpaceKind::Worktree)
                .contains(&second_row),
            "no indicator before the drag is armed"
        );
    }

    #[test]
    fn a_ready_task_names_its_row_marks_it_and_offers_delivery() {
        let model = agent_with_task(TaskStateView::Ready, 3);
        let rows = sidebar(&model, &identities_fixture()).rows;
        let name_row = rows
            .iter()
            .find(|row| row.contains("Agent"))
            .expect("the agent names its own row: {rows:?}");
        let (ready, _) = task_mark(&TaskStateView::Ready).expect("ready is marked");
        assert!(
            name_row.contains(&ready),
            "ready carries its own mark: {name_row}"
        );
        assert!(
            rows.iter().any(|row| row.contains("agent")),
            "and what runs it reads underneath it: {rows:?}"
        );

        let (rows, hits) = tab_strip(&model);
        assert!(
            rows.iter().any(|row| row.contains("3 merge → main")),
            "{rows:?}"
        );
        assert!(
            hits.iter()
                .any(|(_, hit)| matches!(hit, WorkspaceHit::Deliver(_))),
            "the button is a hit"
        );
    }

    /// A control that looks identical idle, pointed at and pressed is one
    /// the operator presses twice. Every header button is drawn as a
    /// filled chip that lifts under the pointer and inverts while the
    /// press flash lasts — the press's own answer, given where the finger
    /// is rather than wherever the work will show up.
    #[test]
    fn a_header_button_answers_the_pointer_and_the_press() {
        let mut model = agent_with_task(TaskStateView::Ready, 3);
        let deliver = WorkspaceHit::Deliver(model.selected_tab().expect("a selected tab"));
        let rect = hit_rect(&model, deliver);
        assert!(
            rect.width > 4,
            "the chip is padded around its label: {rect:?}"
        );

        assert_eq!(
            chip_colors(&model, rect),
            (
                theme::color(Token::Accent),
                theme::color(Token::SurfaceRaised)
            ),
            "at rest: the hue, raised off the strip"
        );

        model.hovered = Some(deliver);
        assert_eq!(
            chip_colors(&model, rect),
            (
                theme::color(Token::Accent),
                theme::color(Token::SurfaceHover)
            ),
            "under the pointer: one step brighter, and only this control"
        );

        model.pressed = Some((deliver, Instant::now()));
        assert_eq!(
            chip_colors(&model, rect),
            (
                theme::color(Token::SurfaceBackground),
                theme::color(Token::Accent)
            ),
            "pressed: the hue becomes the button"
        );
    }

    /// The header draws reports in the same row as its controls, and the
    /// shape has to tell them apart: a report is recessed and answers no
    /// pointer, where a button is raised and does.
    #[test]
    fn a_report_in_the_actions_row_is_not_dressed_as_a_button() {
        let mut model = agent_with_task(TaskStateView::Integrating, 3);
        model.tick = 3;
        let (rows, hits) = tab_strip(&model);
        let row = rows.join("\n");
        assert!(
            row.contains(&format!("{} delivering", agent_activity_frame(3))),
            "{row}"
        );
        assert!(
            !hits
                .iter()
                .any(|(_, hit)| matches!(hit, WorkspaceHit::Deliver(_))),
            "a delivery in flight is not pressed again"
        );

        let spinner = agent_activity_frame(3);
        let column = rows[0]
            .find(&spinner)
            .map(|index| rows[0][..index].chars().count() as u16)
            .expect("the report is on the strip");
        let mut terminal = Terminal::new(TestBackend::new(80, 3)).unwrap();
        terminal
            .draw(|frame| {
                render_tab_strip(
                    frame,
                    frame.area(),
                    &model,
                    &identities_fixture(),
                    &mut Vec::new(),
                )
            })
            .unwrap();
        assert_eq!(
            terminal.backend().buffer()[(column, 0)].bg,
            theme::color(Token::SurfaceRecessed),
            "recessed, not raised"
        );
    }

    /// The button says what pressing it does. One verb over three
    /// completions read the same whether it was about to fast-forward the
    /// target under you, open a pull request against it, or touch nothing
    /// outside the branch.
    #[test]
    fn the_delivery_button_names_the_ending_the_project_asked_for() {
        let ending = |completion| {
            let mut model = agent_with_task(TaskStateView::Ready, 3);
            for task in model.remembered.tasks.values_mut().flatten() {
                task.completion = completion;
            }
            let (rows, _) = tab_strip(&model);
            rows.join("\n")
        };

        assert!(
            ending(CompletionBehavior::Merge).contains("3 merge → main"),
            "{}",
            ending(CompletionBehavior::Merge)
        );
        assert!(
            ending(CompletionBehavior::Pr).contains("3 pr → main"),
            "{}",
            ending(CompletionBehavior::Pr)
        );
        assert!(
            ending(CompletionBehavior::Handoff).contains("3 hand off"),
            "a completion that writes to nothing names no target: {}",
            ending(CompletionBehavior::Handoff)
        );
    }

    /// Readiness is marked once, in the sidebar; the delivery button says
    /// it in words — the count and where a press sends it.
    ///
    /// The two used to share a mark, and drifted into two marks for one
    /// state the moment a glyph set drew them differently. The `nerd` set
    /// then drew a pull-request icon in front of every button, including
    /// the ones that merge or only hand off. Words cannot drift from the
    /// sidebar, and cannot claim a request the press will not open.
    #[test]
    fn a_ready_task_is_marked_in_the_sidebar_and_named_on_its_button() {
        let state = TaskStateView::Ready;
        let model = agent_with_task(state.clone(), 3);
        let (mark, _) = super::render::task_mark(&state).expect("ready is marked");
        let sidebar = sidebar(&model, &identities_fixture()).rows;
        assert!(
            sidebar.iter().any(|row| row.contains(mark.trim())),
            "{sidebar:#?}"
        );
        let (rows, _) = tab_strip(&model);
        let strip = rows.join("\n");
        assert!(strip.contains("3 merge → main"), "{strip}");
        assert!(
            !strip.contains(mark.trim()),
            "the button carries no mark: {strip}"
        );
    }

    /// `pr` is two actions over a task's life, and the button is how the
    /// operator tells them apart: an errand while no request exists, a
    /// sync onto a named one once it does.
    #[test]
    fn a_published_request_turns_the_delivery_button_into_a_sync() {
        let mut model = agent_with_task(TaskStateView::Ready, 4);
        for task in model.remembered.tasks.values_mut().flatten() {
            task.completion = CompletionBehavior::Pr;
        }
        let (before, _) = tab_strip(&model);
        let before = before.join("\n");
        assert!(before.contains("4 pr → main"), "{before}");
        assert!(
            !before.contains(&crate::ui::theme::glyph(
                crate::ui::theme::Symbol::TaskReady
            )),
            "the button's words say what a press does; no mark in front of them: {before}"
        );

        for task in model.remembered.tasks.values_mut().flatten() {
            task.published_request = Some(11);
        }
        let (after, _) = tab_strip(&model);
        let after = after.join("\n");
        assert!(after.contains("4 #11"), "{after}");
        assert!(
            !after.contains("pr → main"),
            "a request that exists is not opened again: {after}"
        );
    }

    /// The count on the button is what pressing it would send, and a
    /// branch level with its request would send nothing. Counting commits
    /// against the target instead left `6 #20` standing on a request that
    /// already carried all six — a merge's question asked of a sync.
    #[test]
    fn a_branch_level_with_its_request_reports_the_sync_instead_of_a_count() {
        let mut model = agent_with_task(TaskStateView::Published, 6);
        for task in model.remembered.tasks.values_mut().flatten() {
            task.completion = CompletionBehavior::Pr;
            task.published_as = Some("fix-auth-redirect".into());
            task.published_request = Some(20);
            task.unsynced = Some(0);
        }
        let (synced, hits) = tab_strip(&model);
        let synced = synced.join("\n");
        assert!(synced.contains("✓ #20"), "{synced}");
        assert!(
            !synced.contains("6 #20"),
            "the target is still six commits away, and that is not this button's question: {synced}"
        );
        assert!(
            hits.iter()
                .any(|(_, hit)| matches!(hit, WorkspaceHit::Deliver(_))),
            "a synced branch still follows a target that moves"
        );

        // Two commits later the button counts those two, not the six the
        // request has carried since the last sync.
        for task in model.remembered.tasks.values_mut().flatten() {
            task.state = TaskStateView::Ready;
            task.unsynced = Some(2);
        }
        let (behind_by_two, _) = tab_strip(&model);
        let behind_by_two = behind_by_two.join("\n");
        assert!(behind_by_two.contains("2 #20"), "{behind_by_two}");
    }

    /// The sidebar and the header answer the same question, so they had
    /// better answer it the same way. Reading only `Ready`, the row went
    /// on wearing the "there is work to hand over" mark for the whole life
    /// of an open request, one column away from a button that had already
    /// stopped saying it.
    #[test]
    fn a_published_task_is_marked_as_gone_not_as_waiting_to_be_delivered() {
        let mut model = agent_with_task(TaskStateView::Published, 6);
        for task in model.remembered.tasks.values_mut().flatten() {
            task.completion = CompletionBehavior::Pr;
            task.published_request = Some(20);
            task.unsynced = Some(0);
        }
        let rows = sidebar(&model, &identities_fixture()).rows;
        let name_row = rows
            .iter()
            .find(|row| row.contains("Agent"))
            .expect("the agent names its own row");
        let (published, _) = task_mark(&TaskStateView::Published).expect("published is marked");
        let (ready, _) = task_mark(&TaskStateView::Ready).expect("ready is marked");
        assert!(
            name_row.contains(&published) && !name_row.contains(&ready),
            "the work is with its reviewer, not waiting on the operator: {name_row}"
        );
    }

    /// The one state no evaluation can ever report: `Integrating` is set
    /// in memory by `landing::deliver` and overwritten by the outcome
    /// before the store is saved, so a surface reading only the record
    /// showed `ready` for the whole delivery — a gate may take half an
    /// hour. The client that started it is the party that knows.
    #[test]
    fn a_delivery_in_flight_is_drawn_from_the_client_that_started_it() {
        let mut model = agent_with_task(TaskStateView::Ready, 3);
        let task = model
            .remembered
            .tasks
            .values()
            .flatten()
            .next()
            .expect("the fixture has a task")
            .clone();
        assert_eq!(model.drawn_state(&task), TaskStateView::Ready);

        model.remembered.delivery_pending.insert(task.id.clone());
        assert_eq!(model.drawn_state(&task), TaskStateView::Integrating);

        let (delivering, _) = task_mark(&TaskStateView::Integrating).expect("delivering is marked");
        let rows = sidebar(&model, &identities_fixture()).rows;
        assert!(
            rows.iter().any(|row| row.contains(&delivering)),
            "the row says a delivery is running: {rows:?}"
        );

        let (strip, hits) = tab_strip(&model);
        let strip = strip.join("\n");
        assert!(strip.contains("delivering"), "{strip}");
        assert!(
            !hits
                .iter()
                .any(|(_, hit)| matches!(hit, WorkspaceHit::Deliver(_))),
            "and it cannot be pressed again while it runs"
        );
    }

    /// A delivery that came back with nothing still releases the task it
    /// was started for.
    ///
    /// Releasing by walking the reports is only correct while there is
    /// always a report, and three ordinary endings produce none: the
    /// checkout was removed under the agent, the store no longer holds
    /// the id, the application would not open. Each of those left the
    /// task drawn as "delivering" for the rest of the session —
    /// undeliverable again, and repainting every 120 ms forever, because
    /// a pending delivery is one of the three things that keep the
    /// spinner's clock turning.
    #[test]
    fn a_delivery_that_answered_nothing_still_gives_the_task_back() {
        let home = UzeHome::at(uze_testkit::temp::scratch("orchestrator-delivery-silence"));
        let mut driven = driven(agent_with_task(TaskStateView::Ready, 3), &home);
        let task = driven
            .attach
            .model
            .remembered
            .tasks
            .values()
            .flatten()
            .next()
            .expect("the fixture has a task")
            .clone();

        driven
            .attach
            .model
            .remembered
            .delivery_pending
            .insert(task.id.clone());
        assert_eq!(
            driven.attach.model.drawn_state(&task),
            TaskStateView::Integrating
        );

        driven
            .attach
            .channels
            .deliveries
            .sender
            .send(DeliveryResolution {
                cwd: PathBuf::from("/repo/.worktrees/ai"),
                reserved: Some(task.id.clone()),
                reports: Vec::new(),
            })
            .unwrap();
        driven.pump();

        assert_eq!(
            driven.attach.model.drawn_state(&task),
            task.state,
            "the task is drawn from its record again"
        );
        assert!(
            driven.attach.model.remembered.delivery_pending.is_empty(),
            "and nothing is left holding the spinner on"
        );
    }

    /// Discarding a preserved task is asked for, not performed here.
    ///
    /// A discard is `git worktree remove`, `git branch -D` and a
    /// recursive removal of the checkout; both it and `finish` used to run
    /// on the thread that owns the frame, so a slot holding a build
    /// directory froze the client and every pane in it until the
    /// filesystem was done. Held twice: by the architecture rule that
    /// now forbids `tui_application` under `orchestrator/`, and by this —
    /// a second confirmation while the first is still out starts no
    /// second removal.
    #[test]
    fn discarding_a_preserved_task_is_asked_for_rather_than_done_on_the_keystroke() {
        let home = UzeHome::at(uze_testkit::temp::scratch("orchestrator-discard-async"));
        let mut model = agent_with_task(TaskStateView::Ready, 1);
        let mut parked = task_in(
            "/repo/.worktrees/old",
            "yesterday",
            TaskStateView::Parked,
            0,
        );
        parked.id = "t2".into();
        model
            .remembered
            .tasks
            .get_mut(Path::new("/repo"))
            .unwrap()
            .push(parked);
        model.preserved = Some(PreservedOverlay {
            selected: 0,
            confirm_discard: false,
        });
        let mut driven = driven(model, &home);

        let keymap = uze_keys::active();
        let scopes = [uze_keys::Scope::PreservedWork];
        let ask = keymap
            .chord_for(uze_keys::Action::DiscardTask, &scopes)
            .expect("discard is bound here");
        let confirm = keymap
            .chord_for(uze_keys::Action::ConfirmDiscard, &scopes)
            .expect("confirmation is bound here");

        for _ in 0..2 {
            driven.press_key(key_event(ask));
            driven.press_key(key_event(confirm));
        }

        assert_eq!(
            driven.attach.model.remembered.task_mutation_pending.len(),
            1,
            "the second confirmation starts no second removal"
        );
        assert!(
            driven
                .attach
                .model
                .remembered
                .notice
                .as_ref()
                .is_some_and(|notice| notice.busy),
            "and the operator is told something is running"
        );

        // The keystroke started a real thread against a checkout that does
        // not exist; its answer is the ending this test waits for, rather
        // than one sent alongside it — two answers on one channel arrive
        // in whichever order the scheduler picks, and the last one drawn
        // is the notice.
        let deadline = Instant::now() + Duration::from_secs(10);
        while !driven
            .attach
            .model
            .remembered
            .task_mutation_pending
            .is_empty()
        {
            assert!(
                Instant::now() < deadline,
                "the mutation thread must answer, whatever it found"
            );
            std::thread::sleep(Duration::from_millis(10));
            driven.pump();
        }
        let notice = driven
            .attach
            .model
            .remembered
            .notice
            .as_ref()
            .expect("an ending");
        assert!(
            notice.text.contains("t2") || notice.text.contains("yesterday"),
            "the ending names the task it was about: {}",
            notice.text
        );
    }

    /// A background read whose work panicked still answers.
    ///
    /// Otherwise there is no reservation left to release and nothing on
    /// screen says so: the message used to go straight into a live
    /// alternate screen, which ratatui repaints by difference — so it was
    /// both corrupting and, a frame later, gone. The message is readable
    /// now (`ui::run` restores the terminal before the hook prints), and
    /// the client carries on with the answer the read would have given
    /// had it found nothing.
    ///
    /// The panic printed while this runs is the subject of the test, not
    /// a failure in it.
    #[test]
    fn a_read_that_panicked_answers_what_it_would_have_answered_empty() {
        let silence: Option<GitBadge> = None;
        assert!(
            answered_or(|| panic!("syntect, over whatever the tree listed"), silence).is_none(),
            "a panicked read answers, so its key is released"
        );
    }

    /// The terminal server going away is noticed, said, and left.
    ///
    /// The reader thread ends — dropping its sender — when the socket
    /// stops answering, and a `while let Ok(..)` reads that as "nothing
    /// arrived this tick". Every pane on screen is then a frozen image in
    /// a client that still redraws, scrolls and accepts keys, with no
    /// message and no way back but quitting.
    #[test]
    fn a_terminal_runtime_that_went_away_is_said_rather_than_waited_on() {
        let home = UzeHome::at(uze_testkit::temp::scratch("orchestrator-runtime-gone"));
        let mut driven = driven(agent_with_task(TaskStateView::Ready, 3), &home);
        assert!(
            matches!(driven.pump(), Flow::Continue),
            "a live runtime is just a quiet one"
        );

        driven.runtime_gone();

        assert!(
            matches!(
                driven.pump(),
                Flow::Exit(super::WorkspaceExit::Disconnected)
            ),
            "the client leaves rather than spinning against a dead socket"
        );
        let notice = driven
            .attach
            .model
            .remembered
            .notice
            .as_ref()
            .expect("it says why it left");
        assert!(notice.text.contains("disconnected"), "{}", notice.text);
    }

    /// The press is answered where the state lives, and nowhere else. The
    /// button and the mark read `delivery_pending`, so a notice announcing
    /// the same delivery put the word on the header twice — once beside
    /// the button, once *as* the button — which is the two-sources shape
    /// this state was folded into `drawn_state` to remove.
    #[test]
    fn pressing_deliver_says_it_once_and_leaves_no_message_behind() {
        let home = UzeHome::at(uze_testkit::temp::scratch("orchestrator-deliver-once"));
        let mut driven = driven(agent_with_task(TaskStateView::Ready, 3), &home);
        let deliver =
            WorkspaceHit::Deliver(driven.attach.model.selected_tab().expect("a selected tab"));
        let rect = hit_rect(&driven.attach.model, deliver);
        driven.frame();
        driven.press(rect.x, rect.y);

        assert!(
            driven.attach.model.remembered.notice.is_none(),
            "the button already says it: {:?}",
            driven
                .attach
                .model
                .remembered
                .notice
                .as_ref()
                .map(|notice| &notice.text)
        );
        let (rows, _) = tab_strip(&driven.attach.model);
        assert_eq!(
            rows.join("\n").matches("delivering").count(),
            1,
            "one delivery, one word for it: {rows:?}"
        );
    }

    fn label_every_tab(model: &mut WorkspaceModel, label: &str) {
        if let Some(session) = model.session.as_mut() {
            for space in &mut session.workspace.spaces {
                for tab in &mut space.tabs {
                    tab.label = label.to_owned();
                }
            }
        }
    }

    /// A task that acquired a name renames the tab that is running it —
    /// without this the name is stored everywhere except where a person
    /// looks, since the strip and the sidebar read the *tab's* label.
    #[test]
    fn a_task_that_acquired_a_name_renames_its_tab() {
        let mut model = agent_with_task(TaskStateView::Ready, 3);
        for tasks in model.remembered.tasks.values_mut() {
            tasks[0].branch = "fix/branch-naming".to_owned();
            tasks[0].label = "branch naming".to_owned();
        }
        label_every_tab(&mut model, "agent 1");

        let requests = super::super::adopt_task_names(&mut model);

        assert!(
            requests.iter().any(|request| matches!(
                request,
                uze_terminal::ClientRequest::RenameTab { label, .. } if label == "branch naming"
            )),
            "the tab takes the name the work acquired: {requests:?}"
        );
        // Once the session echoes the rename, the tab already says what the
        // task says and nothing more is owed.
        label_every_tab(&mut model, "branch naming");
        assert!(
            super::super::adopt_task_names(&mut model).is_empty(),
            "a tab already carrying its task's name is left alone"
        );
    }

    /// A task renamed again — by its agent, or by the operator's own
    /// `git branch -m` — carries the tab with it, because the label on
    /// that tab is one this mechanism put there.
    #[test]
    fn a_task_renamed_again_carries_the_tab_it_already_named() {
        let mut model = agent_with_task(TaskStateView::Ready, 3);
        for tasks in model.remembered.tasks.values_mut() {
            tasks[0].label = "branch naming".to_owned();
        }
        label_every_tab(&mut model, "agent 1");
        let first = super::super::adopt_task_names(&mut model);
        assert_eq!(first.len(), 1);
        label_every_tab(&mut model, "branch naming");

        for tasks in model.remembered.tasks.values_mut() {
            tasks[0].label = "renamed by hand".to_owned();
        }
        let second = super::super::adopt_task_names(&mut model);

        assert!(
            second.iter().any(|request| matches!(
                request,
                uze_terminal::ClientRequest::RenameTab { label, .. } if label == "renamed by hand"
            )),
            "the tab follows the new name: {second:?}"
        );
    }

    /// A label the user chose is theirs and stays — the same rule the shell
    /// adoption already follows.
    #[test]
    fn a_tab_the_user_named_is_never_renamed_by_its_task() {
        let mut model = agent_with_task(TaskStateView::Ready, 3);
        for tasks in model.remembered.tasks.values_mut() {
            tasks[0].label = "branch naming".to_owned();
        }
        label_every_tab(&mut model, "my own name");

        assert!(super::super::adopt_task_names(&mut model).is_empty());
    }

    /// A task still carrying its generated identifier has no name to give.
    #[test]
    fn an_unnamed_task_renames_nothing() {
        let mut model = agent_with_task(TaskStateView::Ready, 3);
        for tasks in model.remembered.tasks.values_mut() {
            let id = tasks[0].id.clone();
            tasks[0].label = id;
        }
        label_every_tab(&mut model, "agent 1");

        assert!(super::super::adopt_task_names(&mut model).is_empty());
    }

    /// A named task reads as its name, once. The label *is* the branch's
    /// subject (`worktree::label_of`), so the caption that used to carry
    /// the branch was the same words with the type in front. This is where
    /// a claim about what the *screen says* belongs — a journey may only
    /// gate on screen text, never assert it.
    #[test]
    fn a_named_task_reads_as_its_name_in_the_sidebar() {
        let mut model = agent_with_task(TaskStateView::Ready, 3);
        for tasks in model.remembered.tasks.values_mut() {
            tasks[0].branch = "fix/branch-naming".to_owned();
            tasks[0].label = "branch naming".to_owned();
        }
        // The tab carries the name the task took, which is what
        // `adopt_task_names` puts there in the product.
        label_every_tab(&mut model, "branch naming");

        let rows = sidebar(&model, &identities_fixture()).rows;

        assert!(
            rows.iter().any(|row| row.contains("branch naming")),
            "the name the agent chose is the row: {rows:?}"
        );
        assert!(
            !rows.iter().any(|row| row.contains("fix/branch-naming")),
            "and the branch it was derived from is not repeated under it: {rows:?}"
        );
        assert!(
            !rows.iter().any(|row| row.contains("agent/")),
            "nor is the generated identifier anywhere on the screen: {rows:?}"
        );
    }

    /// A caption too long for the column is elided, not cut. It used to
    /// run under the row's own right-aligned caption and off the sidebar,
    /// taking that caption's meaning with it and ending mid-word with
    /// nothing to say it had been shortened.
    #[test]
    fn a_long_caption_is_elided_rather_than_run_off_the_sidebar() {
        const LONG: &str = "a-harness-named-longer-than-any-sidebar-column-could-hold";
        let mut model = agent_with_task(TaskStateView::Ready, 3);
        for space in &mut model.session.as_mut().unwrap().workspace.spaces {
            for tab in &mut space.tabs {
                tab.pane.process = LONG.to_owned();
            }
        }
        let identities = vec![AgentIdentity {
            binary: LONG,
            integration: "long",
            display_name: "Long",
            launch: std::path::PathBuf::from(LONG),
            continuity_gap: None,
        }];

        let rows = sidebar(&model, &identities).rows;
        let caption = rows
            .iter()
            .find(|row| row.contains("a-harness-named"))
            .expect("what runs there reads under the agent's name");

        // Past the caption sits the sidebar's own divider, which is the
        // proof nothing ran over the column's edge.
        assert!(
            caption
                .trim_end()
                .trim_end_matches('│')
                .trim_end()
                .ends_with('…'),
            "the name is elided, and says so: {caption}"
        );
        assert!(
            !caption.contains(LONG),
            "so the whole name cannot be on the row: {caption}"
        );
    }

    /// A slot outlives the tasks that run in it, and a task that ended
    /// keeps naming the slot it ran in — so a reused directory is named by
    /// two tasks at once. The row belongs to whoever is in it now; reading
    /// the first match handed the new agent the previous one's delivered
    /// arrow, which is the mark this reads for.
    #[test]
    fn a_reused_slot_reads_the_task_in_it_now_not_the_one_before() {
        let mut model = agent_session_in("/repo/.worktrees/ai");
        stamp_first_tab(&mut model, "now");
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
            .remembered
            .tasks
            .insert(PathBuf::from("/repo"), vec![before, now]);

        let rows = sidebar(&model, &identities_fixture()).rows;
        let (delivered, _) = task_mark(&TaskStateView::Integrated).expect("integrated is marked");
        let name_row = rows
            .iter()
            .find(|row| row.contains("Agent"))
            .expect("the agent names its own row");
        assert!(
            !name_row.contains(&delivered),
            "a new agent delivered nothing: {name_row}"
        );
    }

    /// A workspace space with two agents, one per harness: `agent 1` running
    /// `claude` (the space's context agent, selected) and `agent 2` running
    /// `codex`, both in the space's own root.
    fn workspace_space_session() -> WorkspaceModel {
        let mut session = session("/repo", uze_terminal::SpaceKind::Workspace);
        let space = session.workspace.selected_space;
        let first = session.add_tab(space, "agent 1".into(), None, 80, 24, "/repo".into());
        session.update_pane_status(first, "/repo".into(), "claude".into());
        let second = session.add_tab(space, "agent 2".into(), None, 80, 24, "/repo".into());
        session.update_pane_status(second, "/repo".into(), "codex".into());
        session.workspace.spaces[0].selected_tab = TabId(3);
        let mut model = model_of(session);
        model
            .remembered
            .branches
            .insert(PathBuf::from("/repo"), "main".into());
        model
    }

    /// The two harnesses the tenants above run, so the sidebar is drawn
    /// among these rather than the one-identity fixture the tree tests
    /// share.
    fn tenant_identities() -> Vec<AgentIdentity> {
        vec![
            AgentIdentity {
                binary: "claude",
                integration: "claude-code",
                display_name: "Claude Code",
                launch: std::path::PathBuf::from("/uze/shims/claude"),
                continuity_gap: None,
            },
            AgentIdentity {
                binary: "codex",
                integration: "codex",
                display_name: "Codex",
                launch: std::path::PathBuf::from("codex"),
                continuity_gap: None,
            },
        ]
    }

    /// The rows the tree draws for a space's agents, by the row each
    /// `SelectTab` hit was pushed for.
    fn agent_rows(hits: &[(Rect, WorkspaceHit)]) -> Vec<u16> {
        let mut rows: Vec<u16> = hits
            .iter()
            .filter(|(_, hit)| matches!(hit, WorkspaceHit::SelectTab(_)))
            .map(|(rect, _)| rect.y)
            .collect();
        rows.dedup();
        rows
    }

    /// A tenant is the same two-row item a worktree agent is: its name,
    /// and beneath it the harness running it. Every tenant of the space
    /// stands in one directory on one branch, so the branch never told
    /// two rows apart and the harness always does.
    #[test]
    fn a_workspace_space_draws_each_agent_over_the_harness_it_runs() {
        let model = workspace_space_session();
        let Sidebar { rows, hits, .. } = sidebar(&model, &tenant_identities());
        let agents = agent_rows(&hits);
        assert_eq!(agents.len(), 4, "two rows per agent: {rows:?}");
        for (label, harness, row) in [
            ("agent 1", "claude", agents[0]),
            ("agent 2", "codex", agents[2]),
        ] {
            assert!(rows[row as usize].contains(label), "{rows:?}");
            assert!(
                rows[row as usize + 1].contains(harness),
                "what runs {label}, beneath it: {rows:?}"
            );
        }
        assert!(
            !rows.iter().any(|row| row.contains("main")),
            "and the branch they share is said by the space, not by each of them: {rows:?}"
        );
        assert_eq!(
            agents[2],
            agents[1] + 1,
            "the next agent follows directly: two rows per item, no gap: {rows:?}"
        );
        let branch = theme::glyph(theme::Symbol::TreeBranch);
        assert!(
            !rows.iter().any(|row| row.contains(&branch)),
            "flat: nothing branches off the gutter: {rows:?}"
        );
    }

    /// Two rows per agent and no gap between them, so what says which
    /// item the keyboard is on has to be the item itself: its two rows
    /// carry a trace of the space's own hue over the panel every other
    /// row sits on. Light enough to be read through — it is a selection,
    /// not a highlight — and the kind's, so the tint says what the space
    /// is as well as where you are.
    #[test]
    fn the_agent_receiving_keystrokes_wears_its_kinds_hue_over_the_space() {
        let model = workspace_space_session();
        let Sidebar {
            buffer, hits, rows, ..
        } = sidebar(&model, &tenant_identities());
        let agents = agent_rows(&hits);
        let plain = theme::color(Token::SurfaceRaisedSubtle);
        let tinted = crate::ui::theme::tinted(Token::SpaceWorkspace, Token::SurfaceRaisedSubtle);
        assert_ne!(tinted, plain, "the tint is a surface of its own");

        // `agent 2` is the space's context agent (see `workspace_space_session`).
        for row in [agents[2], agents[3]] {
            assert_eq!(
                buffer[(2, row)].bg,
                tinted,
                "both rows of the item in front: {rows:?}"
            );
        }
        for row in [agents[0], agents[1]] {
            assert_eq!(
                buffer[(2, row)].bg,
                plain,
                "and every other agent keeps the space's own panel: {rows:?}"
            );
        }
    }

    /// Outside a Git repository there is no branch to name, and the row
    /// reads exactly as it does inside one: the caption answers "what
    /// runs here", which is a question every directory has an answer to.
    #[test]
    fn a_tenant_outside_a_repository_reads_as_any_other_agent() {
        let mut model = workspace_space_session();
        model.remembered.branches.clear();
        let Sidebar { rows, hits, .. } = sidebar(&model, &tenant_identities());
        let agents = agent_rows(&hits);
        assert!(rows[agents[1] as usize].contains("claude"), "{rows:?}");
        assert!(
            !rows[agents[1] as usize].contains("/repo"),
            "no directory stands in for it: {rows:?}"
        );
    }

    #[test]
    fn tenants_wear_the_same_status_glyphs_as_worktree_agents() {
        let model = workspace_space_session();
        let Sidebar { rows, hits, .. } = sidebar(&model, &tenant_identities());
        let agents = agent_rows(&hits);
        let idle = theme::glyph(crate::ui::theme::Symbol::StatusIdle);
        let selected = theme::glyph(crate::ui::theme::Symbol::StatusSelected);
        assert!(rows[agents[0] as usize].contains(&idle), "{rows:?}");
        assert!(rows[agents[2] as usize].contains(&selected), "{rows:?}");
    }

    /// The agent receiving keystrokes lights its stretch of the gutter —
    /// both of its rows, and nothing else — in either kind of space.
    #[test]
    fn the_selected_agent_lights_its_stretch_of_the_gutter_and_nothing_else() {
        let model = workspace_space_session();
        let Sidebar {
            rows, hits, buffer, ..
        } = sidebar(&model, &tenant_identities());
        let agents = agent_rows(&hits);
        assert_eq!(
            lit_gutter_rows(
                &buffer,
                gutter_column(&hits),
                uze_terminal::SpaceKind::Workspace
            ),
            vec![agents[2], agents[3]],
            "a workspace space lights its own hue: {rows:?}"
        );

        let tree = agent_with_task(TaskStateView::Ready, 1);
        let Sidebar {
            rows, hits, buffer, ..
        } = sidebar(&tree, &identities_fixture());
        let agents = agent_rows(&hits);
        assert_eq!(
            lit_gutter_rows(
                &buffer,
                gutter_column(&hits),
                uze_terminal::SpaceKind::Worktree
            ),
            vec![agents[0], agents[1]],
            "and a worktree space its own: {rows:?}"
        );
    }

    #[test]
    fn an_empty_workspace_space_draws_its_root_as_a_caption() {
        let session = session("/repo", uze_terminal::SpaceKind::Workspace);
        let model = model_of(session);
        let Sidebar { rows, hits, .. } = sidebar(&model, &tenant_identities());
        assert!(
            rows.iter()
                .any(|row| row.trim_matches(|c: char| c == '│' || c == ' ') == "/repo"),
            "{rows:?}"
        );
        assert!(
            hits.iter()
                .filter(|(_, hit)| matches!(hit, WorkspaceHit::SelectSpace(_)))
                .count()
                >= 2,
            "the caption selects the space like the header does"
        );
    }

    #[test]
    fn a_tenant_whose_harness_exited_is_not_an_agent_row() {
        let mut model = workspace_space_session();
        model.session.as_mut().unwrap().update_pane_status(
            PaneId(3),
            "/repo".into(),
            "bash".into(),
        );
        let Sidebar { rows, hits, .. } = sidebar(&model, &tenant_identities());
        assert_eq!(agent_rows(&hits).len(), 2, "one two-row item: {rows:?}");
    }

    /// A tenant has no task, so its row has nothing to deliver and no state
    /// to mark; and every row selects the agent it names.
    #[test]
    fn a_tenant_row_selects_its_own_agent_and_offers_no_delivery() {
        let model = workspace_space_session();
        let Sidebar { rows, hits, .. } = sidebar(&model, &tenant_identities());
        assert!(
            !hits
                .iter()
                .any(|(_, hit)| matches!(hit, WorkspaceHit::Deliver(_))),
            "no deliver button: {rows:?}"
        );
        let marks = [
            theme::Symbol::PlusMinus,
            theme::Symbol::TaskReady,
            theme::Symbol::ArrowExternal,
            theme::Symbol::MarkAttention,
        ]
        .map(theme::glyph);
        assert!(
            !rows
                .iter()
                .any(|row| marks.iter().any(|mark| row.contains(mark.as_str()))),
            "no task mark: {rows:?}"
        );
        let space = &model.session.as_ref().unwrap().workspace.spaces[0];
        for (rect, hit) in &hits {
            let WorkspaceHit::SelectTab(tab) = hit else {
                continue;
            };
            let label = &space
                .tabs
                .iter()
                .find(|candidate| candidate.id == *tab)
                .expect("the hit names a tab of the space")
                .label;
            // The label row, or the caption row beneath it.
            let item = [rect.y, rect.y.saturating_sub(1)];
            assert!(
                item.iter()
                    .any(|row| rows[*row as usize].contains(label.as_str())),
                "the row at {} selects {label}: {rows:?}",
                rect.y
            );
        }
    }

    /// The agent chords walk a workspace space's agents in the order they
    /// are drawn, wrapping at the ends.
    #[test]
    fn the_agent_chords_walk_a_workspace_spaces_agents() {
        let home = UzeHome::at(uze_testkit::temp::scratch("orchestrator-flat-step"));
        let model = workspace_space_session();
        let Sidebar {
            rows, hits: drawn, ..
        } = sidebar(&model, &tenant_identities());
        let space = &model.session.as_ref().unwrap().workspace.spaces[0];
        let top = space
            .tabs
            .iter()
            .find(|tab| rows[agent_rows(&drawn)[0] as usize].contains(tab.label.as_str()))
            .expect("the top row names a tab")
            .id;
        assert_ne!(space.selected_tab, top, "the fixture selects the last row");
        let mut driven = driven(model, &home);
        driven.attach.identities = tenant_identities();
        let next = uze_keys::active()
            .chord_for(uze_keys::Action::NextAgent, &[uze_keys::Scope::Workspace])
            .expect("stepping agents is reachable from the keyboard");

        driven.press_key(key_event(next));

        assert!(
            driven
                .sent()
                .iter()
                .any(|request| matches!(request, ClientRequest::SelectTab { tab } if *tab == top)),
            "from the last row, the next agent is the top row"
        );
    }

    #[test]
    fn the_first_steps_keep_the_foot_beside_a_workspace_space() {
        let mut model = workspace_space_session();
        model.first_steps_collapsed = false;
        let rows = sidebar(&model, &tenant_identities()).rows;
        assert!(
            rows.iter().any(|row| row.contains("first steps")),
            "the foot is budgeted from the measure: {rows:?}"
        );
    }

    /// The picker offers both kinds until the root's profile answers, and
    /// what it answers decides which of them can be chosen.
    #[test]
    fn the_picker_offers_the_kinds_and_a_plain_directory_allows_only_a_tenancy() {
        let root = uze_testkit::temp::TempDir::new("sidebar-root-picker-kinds");
        std::fs::create_dir_all(root.join("plain")).unwrap();
        let mut model = agent_session_in("/repo");
        model.root_picker = Some(RootPicker::opened_in(
            &root.path().display().to_string(),
            None,
        ));
        let Sidebar { rows, hits, .. } = sidebar(&model, &identities_fixture());
        assert!(
            rows.iter().any(|row| row.contains("workspace")),
            "a plain directory names the tenancy from the start: {rows:?}"
        );
        assert!(
            hits.iter().any(|(_, hit)| matches!(
                hit,
                WorkspaceHit::PickSpaceKind(uze_terminal::SpaceKind::Worktree)
            )),
            "and the other kind is one click away"
        );

        let landed = model
            .root_picker
            .as_ref()
            .and_then(RootPicker::landed)
            .expect("a directory is landed on");
        model.root_picker.as_mut().unwrap().absorb_profile(
            landed,
            uze_application::RootProfile {
                slots_possible: false,
            },
        );
        let Sidebar {
            rows, hits, buffer, ..
        } = sidebar(&model, &identities_fixture());
        let segments = rows
            .iter()
            .position(|row| row.contains("workspace"))
            .expect("the kinds are offered");
        let column = rows[segments].find("workspace").expect("the segment") as u16;
        let chosen = &buffer[(column, segments as u16)];
        assert_eq!(
            chosen.fg,
            theme::color(Token::SpaceWorkspace),
            "a directory that is no repository lands on the tenancy, in that \
             kind's own hue: {rows:?}"
        );
        assert!(
            hits.iter().any(|(_, hit)| matches!(
                hit,
                WorkspaceHit::PickSpaceKind(uze_terminal::SpaceKind::Worktree)
            )),
            "and the other kind is one click away: {rows:?}"
        );
        assert_eq!(
            model
                .root_picker
                .as_ref()
                .and_then(RootPicker::chosen)
                .map(|(_, kind)| kind),
            Some(uze_terminal::SpaceKind::Workspace),
            "which is what it would create here"
        );

        // Looking for a repository from a directory that is not one is the
        // whole point of the other kind: the flip is taken, the listing
        // narrows to what a slot can be cut from and the folders leading
        // to one, and with neither there is nothing to create yet.
        model
            .root_picker
            .as_mut()
            .unwrap()
            .choose_kind(uze_terminal::SpaceKind::Worktree);
        assert_eq!(
            model.root_picker.as_ref().map(RootPicker::kind),
            Some(uze_terminal::SpaceKind::Worktree)
        );
        assert_eq!(
            model.root_picker.as_ref().map(RootPicker::match_count),
            Some(0),
            "no repository under it, and nothing under that either"
        );
        assert_eq!(
            model.root_picker.as_ref().and_then(RootPicker::chosen),
            None,
            "and nothing to create until one is found"
        );
    }

    /// The window between a placement and the evaluation that lists its
    /// task: the only task on record in the slot is the one before, and
    /// reading it by directory named the new agent's tab after it for good.
    /// The tab is for the agent its launch named, and nothing else.
    #[test]
    fn a_new_agent_never_takes_the_name_of_the_task_before_it_in_the_slot() {
        let mut model = agent_session_in("/repo/.worktrees/ai");
        let tab = first_tab(&model).id;
        stamp_first_tab(&mut model, "now");
        let before = TaskView {
            id: "before".into(),
            ..task_in(
                "/repo/.worktrees/ai",
                "nerd font symbols",
                TaskStateView::Integrated,
                2,
            )
        };
        model
            .remembered
            .tasks
            .insert(PathBuf::from("/repo"), vec![before]);

        assert!(
            model.tab_task(tab).is_none(),
            "the task placed here is not listed yet, and nothing stands in for it"
        );

        let now = TaskView {
            id: "now".into(),
            created_at_unix: 2,
            ..task_in("/repo/.worktrees/ai", "now", TaskStateView::Running, 0)
        };
        model
            .remembered
            .tasks
            .get_mut(Path::new("/repo"))
            .unwrap()
            .push(now);
        assert_eq!(
            model.tab_task(tab).map(|task| task.id.as_str()),
            Some("now")
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
            sidebar(&model, &identities_fixture())
                .rows
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
            TaskStateView::Published,
            TaskStateView::Integrating,
            TaskStateView::Conflicted {
                files: vec![PathBuf::from("src/lib.rs")],
            },
            TaskStateView::GateFailed,
            TaskStateView::Integrated,
            TaskStateView::Parked,
        ]
    }

    /// The marks are symbols, not emoji, and this is the narrow form of that
    /// rule: a status mark is one column, so it is held to U+2300 — below
    /// the blocks where the pictographs start — rather than merely to being
    /// non-emoji.
    ///
    /// `uze_theme::load::tests::no_bundled_glyph_is_an_emoji` is the wider
    /// rule, over every glyph UZE ships rather than only these.
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
    /// sharing `theme::color(Token::TextDim)` meant the column read as one undifferentiated
    /// smudge. The glyphs are distinct for the same reason, and `Ready`
    /// specifically must not reuse the `✓` the agent column already spends
    /// on `Completed`.
    #[test]
    fn each_status_mark_carries_a_glyph_and_a_hue_of_its_own() {
        let marks: Vec<(String, Color)> = every_task_state().iter().filter_map(task_mark).collect();
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

    /// The task mark is the one click target that opens the catalog: the
    /// glyphs are the row's only wordless vocabulary, so the row has to
    /// carry the way to look them up — once. The agent's own status glyph
    /// in front of the name is not a second door to the same popup.
    #[test]
    fn only_the_task_mark_opens_the_catalog() {
        let model = agent_with_task(TaskStateView::Ready, 1);
        let Sidebar { rows, hits, .. } = sidebar(&model, &identities_fixture());
        let anchors: Vec<Rect> = hits
            .iter()
            .filter_map(|(_, hit)| match hit {
                WorkspaceHit::OpenStatusCatalog(anchor) => Some(*anchor),
                _ => None,
            })
            .collect();
        assert_eq!(anchors.len(), 1, "one door, the task mark: {rows:?}");
        // The anchor is the glyph's own cell, not the row: a click on the
        // name still selects the tab. Read back out of the drawn rows, so
        // a hit that drifts off its glyph fails here rather than opening
        // the catalog from a blank column.
        let anchor = anchors[0];
        assert_eq!((anchor.width, anchor.height), (1, 1));
        let glyph = rows[anchor.y as usize]
            .chars()
            .nth(anchor.x as usize)
            .expect("the anchor is inside the row")
            .to_string();
        let (ready, _) = task_mark(&TaskStateView::Ready).expect("ready is marked");
        assert_eq!(glyph, ready, "the anchor is the task mark: {rows:?}");
        assert!(
            hits.iter()
                .any(|(rect, hit)| matches!(hit, WorkspaceHit::SelectTab(_)) && rect.width > 1),
            "the row itself still selects the tab"
        );
    }

    /// The task mark stands at the row's right edge, in the column the
    /// space header's `⇄` does, and the row no longer spends that edge on
    /// naming the harness.
    #[test]
    fn a_task_mark_is_pinned_to_the_sidebars_right_column() {
        let model = agent_with_task(TaskStateView::Ready, 1);
        let Sidebar { rows, hits, .. } = sidebar(&model, &identities_fixture());
        let mark = hits
            .iter()
            .find_map(|(_, hit)| match hit {
                WorkspaceHit::OpenStatusCatalog(anchor) => Some(*anchor),
                _ => None,
            })
            .expect("the task is marked");
        let toggle = hits
            .iter()
            .find_map(|(rect, hit)| {
                matches!(hit, WorkspaceHit::ToggleSpaceRoot(_)).then_some(*rect)
            })
            .expect("the space header has its toggle");
        assert_eq!(mark.x, toggle.x, "one right-hand column: {rows:#?}");
        let alias = crate::ui::small_caps("agent");
        // Past the header block — its label and its count name the column,
        // not a harness.
        assert!(
            !rows.iter().skip(3).any(|row| row.contains(&alias)),
            "the harness is not named in the sidebar: {rows:#?}"
        );
    }

    /// An agent's caption says what runs there, and never the branch: a
    /// task's label is derived from its branch, so the caption used to
    /// repeat the name above it — `agent/t1` under `t1` for work nobody
    /// named, `fix/thing` under `thing` for work somebody did. The
    /// space's `⇄` is the header's alone and does not touch it.
    #[test]
    fn an_agents_caption_names_the_harness_running_it_never_its_branch() {
        let mut model = agent_with_task(TaskStateView::Running, 0);
        let Sidebar { rows, hits, .. } = sidebar(&model, &identities_fixture());
        let caption = agent_rows(&hits)[1] as usize;
        assert!(
            rows[caption].contains("agent") && !rows[caption].contains("agent/t1"),
            "the harness id, not the branch: {rows:#?}"
        );
        assert!(
            !rows.iter().any(|row| row.contains("agent/t1")),
            "and the branch is nowhere in the column: {rows:#?}"
        );

        let space = model.session.as_ref().unwrap().workspace.selected_space;
        model.remembered.roots_shown.insert(space);
        let rows = sidebar(&model, &identities_fixture()).rows;
        assert!(
            rows[caption].contains("agent") && !rows[caption].contains("agent/t1"),
            "which the header's own toggle leaves alone: {rows:#?}"
        );
    }

    /// A space header says one thing at a time — its name, or where its
    /// work lives — and the `⇄` behind the text is the one way between
    /// them: a click on the name itself still selects the space.
    #[test]
    fn the_root_toggle_flips_a_space_header_between_label_and_root() {
        let mut model = agent_session_in("/repo");
        // A name the root does not contain, so each reads as itself alone.
        let label = "workbench".to_owned();
        let space = {
            let session = model.session.as_mut().unwrap();
            let id = session.workspace.selected_space;
            session.workspace.spaces[0].label = label.clone();
            id
        };
        let header_row = |rows: &[String]| {
            rows.iter()
                .find(|row| row.contains(&label) || row.contains("/repo"))
                .cloned()
                .expect("the space header is drawn")
        };

        let Sidebar { rows, hits, .. } = sidebar(&model, &identities_fixture());
        let row = header_row(&rows);
        assert!(row.contains(&label) && !row.contains("/repo"), "{row}");
        let toggles: Vec<Rect> = hits
            .iter()
            .filter_map(|(rect, hit)| match hit {
                WorkspaceHit::ToggleSpaceRoot(id) if *id == space => Some(*rect),
                _ => None,
            })
            .collect();
        assert_eq!(toggles.len(), 1, "one toggle per header: {rows:?}");
        let toggle = toggles[0];
        assert_eq!((toggle.width, toggle.height), (1, 1));
        let glyph = rows[toggle.y as usize]
            .chars()
            .nth(toggle.x as usize)
            .expect("the toggle is inside the row");
        assert_eq!(glyph, '⇄', "the hit is the toggle glyph: {rows:?}");
        assert!(
            hits.iter().any(|(rect, hit)| {
                matches!(hit, WorkspaceHit::SelectSpace(id) if *id == space) && rect.width > 1
            }),
            "the row itself still selects the space"
        );

        model.remembered.roots_shown.insert(space);
        let rows = sidebar(&model, &identities_fixture()).rows;
        let row = header_row(&rows);
        assert!(row.contains("/repo") && !row.contains(&label), "{row}");
    }

    /// A tab is bound to the task its launch named, once an evaluation
    /// lists it, wherever its pane stands — in the slot, in a removed
    /// checkout the kernel spells ` (deleted)`, or anywhere else — and the
    /// binding outlives the task's checkout, which is what offers the
    /// resume.
    #[test]
    fn a_stamped_tab_is_bound_to_its_task_wherever_its_pane_stands() {
        for cwd in [
            "/repo/.worktrees/ai",
            "/repo/.worktrees/ai (deleted)",
            "/elsewhere",
        ] {
            let mut model = agent_session_in(cwd);
            stamp_first_tab(&mut model, "t1");
            let (tab, pane) = (first_tab(&model).id, first_tab(&model).pane.id);
            assert!(model.tab_task(tab).is_none(), "nothing listed yet: {cwd}");

            let mut task = task_in("/repo/.worktrees/ai", "fix-auth", TaskStateView::Running, 0);
            model
                .remembered
                .tasks
                .insert(PathBuf::from("/repo"), vec![task.clone()]);
            assert_eq!(
                model.tab_task(tab).map(|task| task.id.as_str()),
                Some("t1"),
                "{cwd}"
            );

            task.checkout = None;
            task.state = TaskStateView::Parked;
            model
                .remembered
                .tasks
                .insert(PathBuf::from("/repo"), vec![task]);
            model.remembered.lost_checkouts.insert(pane);
            assert_eq!(
                model.tab_task(tab).map(|task| task.id.as_str()),
                Some("t1"),
                "the binding outlives the checkout: {cwd}"
            );
            assert!(
                model.lost_task(tab).is_some(),
                "and offers the resume: {cwd}"
            );
        }
    }

    /// A tab launched for nobody is bound to nothing, however much its
    /// directory says.
    #[test]
    fn an_unstamped_tab_is_bound_to_nothing() {
        let mut model = agent_session_in("/repo/.worktrees/ai");
        let (tab, pane) = (first_tab(&model).id, first_tab(&model).pane.id);
        model.remembered.tasks.insert(
            PathBuf::from("/repo"),
            vec![task_in(
                "/repo/.worktrees/ai",
                "fix-auth",
                TaskStateView::Running,
                0,
            )],
        );
        model.remembered.lost_checkouts.insert(pane);
        assert!(model.tab_task(tab).is_none());
        assert!(model.lost_task(tab).is_none());
    }

    /// A tenant holds no checkout, so no slot is released when its tab
    /// closes: the agent leaving the tabs is the only sign it has ended,
    /// and it must reconcile the space roots the tenant is keyed by.
    #[test]
    fn an_agent_losing_its_last_tab_reconciles_the_space_roots() {
        let mut model = model_of(session("/plain", uze_terminal::SpaceKind::Workspace));
        stamp_first_tab(&mut model, "ten4nt");
        let home = UzeHome::at(uze_testkit::temp::scratch("sidebar-tenant-left"));
        let (sender, _receiver) = std::sync::mpsc::channel();
        let (evaluations, _answers) = std::sync::mpsc::channel();

        model.occupancy_stale = true;
        sync_slot_occupancy(&mut model, &home, &sender, &evaluations);
        model.occupancy_pending = false;

        model.occupancy_stale = true;
        sync_slot_occupancy(&mut model, &home, &sender, &evaluations);
        assert!(
            !model.occupancy_pending,
            "nothing changed, so nothing is reconciled"
        );

        first_tab_mut(&mut model).env.clear();
        model.occupancy_stale = true;
        sync_slot_occupancy(&mut model, &home, &sender, &evaluations);
        assert!(
            model.occupancy_pending,
            "the tenant's roots are reconciled once its agent has no tab"
        );
    }

    /// A checkout removed from under a live pane is the one change to a
    /// repository nothing else asks about: the pane is still there, so no
    /// slot was released and no reconciliation is due. Unasked, the row
    /// keeps drawing a task view that still believes it has a checkout —
    /// which is exactly what the way back into it is gated on, so the
    /// "resume" never appeared however long the operator waited.
    #[test]
    fn a_checkout_that_vanished_under_a_pane_asks_its_repository_again() {
        let mut model = agent_session_in("/repo/.worktrees/ai");
        let pane = first_tab(&model).pane.id;
        model
            .remembered
            .pane_checkouts
            .insert(pane, PathBuf::from("/repo/.worktrees/ai"));
        let home = UzeHome::at(uze_testkit::temp::scratch("sidebar-vanished"));
        let (sender, _receiver) = std::sync::mpsc::channel();
        let (evaluations, _answers) = std::sync::mpsc::channel();
        model.occupancy_stale = true;
        sync_slot_occupancy(&mut model, &home, &sender, &evaluations);

        assert!(model.remembered.lost_checkouts.contains(&pane));
        assert!(
            model
                .remembered
                .task_eval_pending
                .contains(Path::new("/repo")),
            "the repository is re-read: {:?}",
            model.remembered.task_eval_pending
        );
    }

    /// A worktree removed by hand leaves the agent standing in a directory
    /// that no longer exists. The row says so, in words, instead of
    /// showing the kernel's own ` (deleted)` path — and carries the way
    /// back in: a "resume" that puts the task into a slot of its own,
    /// offered only while the task is still waiting for one.
    #[test]
    fn an_agent_whose_checkout_was_removed_says_so_and_offers_to_resume() {
        let removed = uze_testkit::temp::scratch("sidebar-lost-checkout");
        std::fs::remove_dir_all(&removed).unwrap();
        assert!(checkout_lost(
            Some(&removed),
            Path::new("/repo/.worktrees/x")
        ));
        assert!(checkout_lost(
            None,
            Path::new("/repo/.worktrees/x (deleted)")
        ));
        assert!(!checkout_lost(None, Path::new("/repo/.worktrees/x")));

        // Bound while the task still had its checkout — the pane's own
        // binding is what survives the reconciliation, which strips an
        // orphaned task of both its checkout and its checkout id.
        let mut model = agent_session_in("/repo/.worktrees/ai (deleted)");
        let pane = first_tab(&model).pane.id;
        model
            .remembered
            .pane_checkouts
            .insert(pane, PathBuf::from("/repo/.worktrees/ai"));
        stamp_first_tab(&mut model, "t1");
        model.remembered.lost_checkouts.insert(pane);
        let mut parked = task_in("/repo/.worktrees/ai", "fix-auth", TaskStateView::Parked, 2);
        parked.checkout = None;
        model
            .remembered
            .tasks
            .insert(PathBuf::from("/repo"), vec![parked.clone()]);

        let Sidebar { rows, hits, .. } = sidebar(&model, &identities_fixture());
        assert!(
            rows.iter().any(|row| row.contains("checkout removed")),
            "{rows:?}"
        );
        assert!(
            !rows.iter().any(|row| row.contains("(deleted)")),
            "{rows:?}"
        );
        let resume: Vec<Rect> = hits
            .iter()
            .filter_map(|(rect, hit)| match hit {
                WorkspaceHit::ResumeLostCheckout(_) => Some(*rect),
                _ => None,
            })
            .collect();
        assert_eq!(resume.len(), 1, "one way back in: {rows:?}");
        let label: String = rows[resume[0].y as usize]
            .chars()
            .skip(resume[0].x as usize)
            .take(resume[0].width as usize)
            .collect();
        assert_eq!(label, "resume", "the hit is the word itself: {rows:?}");

        // Resumed: the task has a slot again, and this row — still the
        // dead one — no longer offers a second agent for it.
        let mut resumed = parked;
        resumed.checkout = Some(PathBuf::from("/repo/.worktrees/b2"));
        resumed.state = TaskStateView::Running;
        model
            .remembered
            .tasks
            .insert(PathBuf::from("/repo"), vec![resumed]);
        let Sidebar { rows, hits, .. } = sidebar(&model, &identities_fixture());
        assert!(
            !hits
                .iter()
                .any(|(_, hit)| matches!(hit, WorkspaceHit::ResumeLostCheckout(_))),
            "{rows:?}"
        );
        assert!(
            rows.iter().any(|row| row.contains("checkout removed")),
            "{rows:?}"
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
            "published",
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
                assert!(text.contains(&mark), "{mark} is missing from: {text}");
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

    /// Every message the workspace makes is drawn beside the header's own
    /// controls, and nowhere else. A line pinned over the bottom of the
    /// pane was a second place to look for something that is usually two
    /// words, and it sat on top of the agent's own output while it did.
    #[test]
    fn a_notice_is_drawn_beside_the_controls_and_never_over_the_pane() {
        let mut model = agent_with_task(TaskStateView::Ready, 3);
        model.set_notice("nothing ready".to_owned());
        let rows = frame_rows(&mut model);
        let layout = compute_layout(Rect::new(0, 0, 80, 24), model.sidebar_width);
        assert!(
            rows[layout.tab_strip.y as usize].contains("nothing ready"),
            "{:?}",
            rows[layout.tab_strip.y as usize]
        );
        assert!(
            !rows[layout.pane.bottom() as usize - 1].contains("nothing ready"),
            "{:?}",
            rows[layout.pane.bottom() as usize - 1]
        );
    }

    /// A notice about the selected tab's own task needs no label — the tab
    /// already says whose agent this is — and stands left of the actions
    /// behind the zone divider, not in place of any of them.
    #[test]
    fn a_notice_about_the_task_on_screen_needs_no_label() {
        let mut model = agent_with_task(TaskStateView::Ready, 3);
        model.set_task_notice("t1", "fix-auth-redirect", "merged → main".to_owned());
        let (rows, hits) = tab_strip(&model);
        assert!(rows[0].contains("merged → main │"), "{rows:?}");
        assert!(!rows[0].contains("fix-auth-redirect"), "{rows:?}");
        assert!(
            hits.iter()
                .any(|(_, hit)| matches!(hit, WorkspaceHit::Deliver(_))),
            "its own button is still there to press"
        );
    }

    /// The header is two zones, and the message is never allowed into the
    /// other one: whatever the workspace has to say, every action keeps
    /// the exact rect it had — including one about the very task the
    /// message is about, which is the case that used to take the button
    /// away mid-click.
    #[test]
    fn a_message_never_moves_an_action() {
        let mut model = agent_with_task(TaskStateView::Ready, 3);
        model.remembered.git_badge = Some(GitBadge {
            cwd: PathBuf::from("/repo/.worktrees/ai"),
            summary: Some(uze_extensions::code::ChangeSummary {
                additions: 12,
                deletions: 3,
            }),
            timeline: None,
            timeline_checked_at: Instant::now(),
            checked_at: Instant::now(),
        });
        let (_, quiet) = tab_strip(&model);

        model.set_busy_notice("delivering all".to_owned());
        let (rows, speaking) = tab_strip(&model);

        assert!(rows[0].contains("delivering all │"), "{rows:?}");
        assert_eq!(
            quiet.len(),
            speaking.len(),
            "the same actions are offered either way"
        );
        for (quiet, speaking) in quiet.iter().zip(&speaking) {
            assert_eq!(
                (quiet.0, format!("{:?}", quiet.1)),
                (speaking.0, format!("{:?}", speaking.1)),
                "an action moved under the message"
            );
        }
    }

    /// One about a task that is *not* on screen carries the label, and
    /// leaves the selected task's own button alone: it is not about it.
    #[test]
    fn a_notice_about_another_task_carries_its_label_and_keeps_the_button() {
        let mut model = agent_with_task(TaskStateView::Ready, 3);
        model.set_task_notice("t2", "other", "back to its agent".to_owned());
        let (rows, hits) = tab_strip(&model);
        assert!(rows[0].contains("other: back to its agent"), "{rows:?}");
        assert!(rows[0].contains("3 merge → main"), "{rows:?}");
        assert!(
            hits.iter()
                .any(|(_, hit)| matches!(hit, WorkspaceHit::Deliver(_)))
        );
    }

    /// Work still running says so by moving: a spinner rides in front of
    /// it, which is what buys the message the right to be two words.
    #[test]
    fn running_work_carries_a_spinner() {
        let mut model = agent_with_task(TaskStateView::Ready, 3);
        model.set_busy_notice("delivering all".to_owned());
        model.tick = 3;
        let (rows, _) = tab_strip(&model);
        assert!(
            rows[0].contains(&format!("{} delivering all", agent_activity_frame(3))),
            "{rows:?}"
        );
        model.set_task_notice("t1", "fix-auth-redirect", "merged → main".to_owned());
        let (settled, _) = tab_strip(&model);
        assert!(
            !settled[0].contains(&agent_activity_frame(3)),
            "an ending does not spin: {settled:?}"
        );
    }

    /// A message about work in flight outlives the notice clock — the
    /// alternative is silence while the thing it announced is still
    /// running — and is retired by the outcome that replaces it.
    #[test]
    fn a_running_notice_outlives_the_deadline_a_finished_one_keeps() {
        let home = UzeHome::at(uze_testkit::temp::scratch("orchestrator-notice-ttl"));
        let mut driven = driven(agent_with_task(TaskStateView::Ready, 3), &home);
        let aged = Instant::now() - NOTICE_TTL - Duration::from_secs(1);

        driven
            .attach
            .model
            .set_busy_notice("delivering all".to_owned());
        driven
            .attach
            .model
            .remembered
            .notice
            .as_mut()
            .unwrap()
            .since = aged;
        driven.pump();
        assert!(
            driven.attach.model.remembered.notice.is_some(),
            "work still in flight is not swept"
        );

        driven.attach.model.set_notice("nothing ready".to_owned());
        driven
            .attach
            .model
            .remembered
            .notice
            .as_mut()
            .unwrap()
            .since = aged;
        driven.pump();
        assert!(
            driven.attach.model.remembered.notice.is_none(),
            "an ending ages out"
        );
    }

    #[test]
    fn a_running_task_offers_no_delivery_and_carries_no_mark() {
        let model = agent_with_task(TaskStateView::Running, 0);
        let rows = sidebar(&model, &identities_fixture()).rows;
        let name_row = rows.iter().find(|row| row.contains("Agent")).unwrap();
        assert!(
            !name_row.contains('\u{2713}') && !name_row.contains('\u{26a0}'),
            "{name_row}"
        );
        let (rows, hits) = tab_strip(&model);
        assert!(
            !rows
                .iter()
                .any(|row| row.contains("merge →") || row.contains("pr →")),
            "{rows:?}"
        );
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
        let rows = sidebar(&model, &identities_fixture()).rows;
        let name_row = rows.iter().find(|row| row.contains("Agent")).unwrap();
        let (conflict, _) = task_mark(&TaskStateView::Conflicted { files: Vec::new() })
            .expect("a conflict is marked");
        assert!(name_row.contains(&conflict), "{name_row}");
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
            .remembered
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
        let discard = uze_keys::active()
            .chord_for(
                uze_keys::Action::DiscardTask,
                &[uze_keys::Scope::Global, uze_keys::Scope::PreservedWork],
            )
            .expect("discard is bound here");
        assert!(
            text.contains(&format!(
                "{discard} {}",
                uze_keys::Action::DiscardTask.label().to_lowercase()
            )),
            "the key it names is the one the keymap binds: {text}"
        );
    }

    /// The first tab of the first space — the one tab most fixtures have.
    fn first_tab(model: &WorkspaceModel) -> &Tab {
        &model.session.as_ref().expect("a session").workspace.spaces[0].tabs[0]
    }

    fn first_tab_mut(model: &mut WorkspaceModel) -> &mut Tab {
        &mut model.session.as_mut().expect("a session").workspace.spaces[0].tabs[0]
    }

    /// Marks the first tab as launched for `id`: what the server echoes
    /// back for a tab the client created with that identity stamped.
    fn stamp_first_tab(model: &mut WorkspaceModel, id: &str) {
        let tab = first_tab_mut(model);
        tab.env = vec![(
            uze_terminal::launch::AGENT_IDENTITY_VARIABLE.to_owned(),
            id.to_owned(),
        )];
    }

    /// A one-agent session whose only tab runs in `cwd`.
    fn agent_session_in(cwd: &str) -> WorkspaceModel {
        let mut session = session("/repo", uze_terminal::SpaceKind::Worktree);
        let tab = &mut session.workspace.spaces[0].tabs[0];
        tab.label = "Agent".into();
        tab.pane.process = "agent".into();
        tab.pane.cwd = cwd.into();
        model_of(session)
    }

    /// Two agents in one space, the first of them selected: `Agent` in
    /// `first`, `Second` in `second`. The second resolves by its pane's
    /// process rather than its label, so the two never answer to the same
    /// row search.
    fn two_agent_session(first: &str, second: &str) -> WorkspaceModel {
        let mut model = agent_session_in(first);
        let space = &mut model.session.as_mut().unwrap().workspace.spaces[0];
        let mut tab = space.tabs[0].clone();
        tab.id = TabId(2);
        tab.label = "Second".into();
        tab.pane = Pane {
            id: PaneId(2),
            cwd: second.into(),
            columns: 80,
            rows: 24,
            process: "agent".to_owned(),
        };
        space.tabs.push(tab);
        model
    }

    /// Moves the keystrokes to the second agent [`two_agent_session`] built.
    fn select_second_agent(model: &mut WorkspaceModel) {
        model.session.as_mut().unwrap().workspace.spaces[0].selected_tab = TabId(2);
    }

    /// A session whose one space is rooted at `root` — a real directory,
    /// so the picker and the client compare the same canonical path.
    fn session_rooted_at(root: &Path) -> WorkspaceModel {
        model_of(session(root, uze_terminal::SpaceKind::Worktree))
    }

    /// A one-agent session in `/repo` whose checkout's history is
    /// `subjects`, newest first, every commit landed `3h` ago.
    fn session_with_timeline(subjects: &[&str]) -> WorkspaceModel {
        let mut model = agent_session_in("/repo");
        model.remembered.git_badge = Some(GitBadge {
            cwd: PathBuf::from("/repo"),
            summary: None,
            timeline: Some(uze_extensions::code::Timeline {
                branch: "main".to_owned(),
                commits: subjects
                    .iter()
                    .enumerate()
                    .map(|(index, subject)| uze_extensions::code::Commit {
                        hash: format!("{index:07x}"),
                        subject: (*subject).to_owned(),
                        age: "3h".to_owned(),
                        ahead: false,
                    })
                    .collect(),
            }),
            timeline_checked_at: Instant::now(),
            checked_at: Instant::now(),
        });
        model
    }

    /// Scheduling a read never answers one.
    ///
    /// The point of the whole background path: `git status` and `git log`
    /// launch processes, and the loop that calls this is the loop that
    /// draws. It reserves the checkout and returns; the badge is whatever
    /// it already was.
    #[test]
    fn scheduling_a_git_read_reserves_the_checkout_and_answers_nothing() {
        let mut model = agent_session_in("/repo");
        let (sender, receiver) = std::sync::mpsc::channel();

        model.schedule_git_read(&sender);

        assert_eq!(
            model.remembered.git_pending.as_deref(),
            Some(Path::new("/repo")),
            "the checkout is reserved while its read is out"
        );
        assert!(
            model.remembered.git_badge.is_none(),
            "nothing is read on the caller's thread"
        );
        assert!(
            receiver.try_recv().is_err() || model.remembered.git_pending.is_some(),
            "the answer arrives on the channel, not from the call"
        );

        // A reservation is what stops the next tick asking again.
        model.schedule_git_read(&sender);
        assert_eq!(
            model.remembered.git_pending.as_deref(),
            Some(Path::new("/repo"))
        );
    }

    /// An answer about a checkout the selection has left is released and
    /// dropped — not drawn over the checkout now in front of the viewer.
    #[test]
    fn a_git_answer_for_another_checkout_is_released_and_dropped() {
        let mut model = agent_session_in("/repo");
        model.remembered.git_pending = Some(PathBuf::from("/elsewhere"));

        let changed = model.absorb_git_read(GitResolution {
            cwd: PathBuf::from("/elsewhere"),
            answer: GitAnswer::Full {
                summary: None,
                timeline: Some(uze_extensions::code::Timeline {
                    branch: "other".to_owned(),
                    commits: Vec::new(),
                }),
            },
        });

        assert!(!changed, "nothing on screen changed");
        assert!(
            model.remembered.git_pending.is_none(),
            "the key is released whatever the answer, or the checkout is \
             never asked about again"
        );
        assert!(
            model.remembered.git_badge.is_none(),
            "no badge for a checkout nobody is on"
        );
    }

    /// The two cadences are independent: a summary-only answer keeps the
    /// history the badge already had, rather than blanking the timeline
    /// every 750ms between the 3s reads that fill it.
    #[test]
    fn a_summary_only_answer_keeps_the_history_already_read() {
        let mut model = session_with_timeline(&["landed"]);
        let read_at = model
            .remembered
            .git_badge
            .as_ref()
            .map(|badge| badge.timeline_checked_at);

        let changed = model.absorb_git_read(GitResolution {
            cwd: PathBuf::from("/repo"),
            answer: GitAnswer::Summary(Some(uze_extensions::code::ChangeSummary {
                additions: 2,
                deletions: 1,
            })),
        });

        assert!(changed);
        let badge = model.remembered.git_badge.as_ref().expect("a badge");
        assert_eq!(
            badge
                .timeline
                .as_ref()
                .map(|timeline| timeline.commits.len()),
            Some(1),
            "the timeline survives a summary-only read"
        );
        assert_eq!(
            Some(badge.timeline_checked_at),
            read_at,
            "and keeps its own read time, so its own cadence still governs it"
        );
        assert!(badge.summary.is_some());
    }

    /// A commit account is asked for, not read inline, and an answer
    /// nobody is waiting for never opens over them.
    #[test]
    fn a_commit_account_arrives_only_for_the_row_last_clicked() {
        let mut model = session_with_timeline(&["newest", "older"]);
        let (sender, _receiver) = std::sync::mpsc::channel();

        open_commit_detail(&mut model, 1, Rect::new(0, 0, 10, 1), &sender);
        assert!(
            model.commit_detail.is_none(),
            "the popup opens when the read lands, never from the click"
        );
        let asked = model.commit_detail_pending.clone().expect("a pending hash");

        let stale = model.absorb_commit_detail(CommitDetailResolution {
            hash: "deadbee".to_owned(),
            anchor: Rect::new(0, 0, 10, 1),
            target: None,
            detail: None,
        });
        assert!(!stale, "an answer for another commit is dropped");
        assert_eq!(model.commit_detail_pending.as_deref(), Some(asked.as_str()));

        // Dismissing while the read is still out cancels it, so it cannot
        // open behind the viewer's back when it lands.
        model.dismiss_commit_detail();
        assert!(model.commit_detail_pending.is_none());
        assert!(!model.commit_detail_open());
    }

    /// Everything the timeline puts on screen is the extension's, and it
    /// says so in the extension's own vocabulary.
    ///
    /// The section used to be drawn by hand from `git::Timeline` with
    /// three `WorkspaceHit` variants of its own, which made it half an
    /// extension: the palette, the eliding and the hit rectangles were
    /// all decided on the host's side of a boundary whose whole point is
    /// that they are not.
    #[test]
    fn the_timeline_speaks_only_the_extensions_vocabulary() {
        let model = session_with_timeline(&["feat: newest", "chore: older"]);
        let Sidebar { rows, hits, .. } = sidebar(&model, &identities_fixture());

        let header = timeline_hit(&hits).expect("the header folds the section");
        let divider = resize_hit(&hits).expect("the divider resizes it");
        assert_eq!(divider.y, header.y + 1, "the handle sits under the header");

        let commits: Vec<usize> = hits
            .iter()
            .filter_map(|(_, hit)| match hit {
                WorkspaceHit::Extension(ExtensionHit::CodeTimeline(ViewHit::SelectItem(index))) => {
                    Some(*index)
                }
                _ => None,
            })
            .collect();
        assert_eq!(commits, vec![0, 1], "one hit per commit, in order");

        // Nothing in the section reaches the host's own hit vocabulary.
        // Nothing in the section reaches the host's own hit vocabulary.
        let timeline_top = header.y;
        assert!(
            hits.iter()
                .filter(|(rect, _)| rect.y >= timeline_top)
                .all(|(_, hit)| matches!(
                    hit,
                    WorkspaceHit::Extension(ExtensionHit::CodeTimeline(_))
                )),
            "a host hit escaped into the extension's section: {hits:?}"
        );
        assert!(
            rows.iter().any(|row| row.contains("feat: newest")),
            "and the rows are actually drawn: {rows:?}"
        );
    }

    /// A sidebar row without its right-hand divider and the padding
    /// before it.
    fn inside(row: &str) -> &str {
        row.trim_end_matches('│').trim_end()
    }

    fn timeline_hit(hits: &[(Rect, WorkspaceHit)]) -> Option<Rect> {
        hits.iter()
            .find(|(_, hit)| {
                *hit == WorkspaceHit::Extension(ExtensionHit::CodeTimeline(ViewHit::ToggleSection))
            })
            .map(|(rect, _)| *rect)
    }

    fn resize_hit(hits: &[(Rect, WorkspaceHit)]) -> Option<Rect> {
        hits.iter()
            .find(|(_, hit)| {
                *hit == WorkspaceHit::Extension(ExtensionHit::CodeTimeline(ViewHit::ResizeSection))
            })
            .map(|(rect, _)| *rect)
    }

    /// Dragged, the section shows the rows asked for — no fewer than one,
    /// no more than the history has, and never into the tree's own
    /// minimum — where left alone it stops at half the column.
    #[test]
    fn dragging_the_timeline_sets_how_many_commits_show() {
        let subjects: Vec<String> = (0..20).map(|index| format!("commit {index}")).collect();
        let subjects: Vec<&str> = subjects.iter().map(String::as_str).collect();
        let mut model = session_with_timeline(&subjects);

        model.timeline_rows = Some(2);
        let rows = sidebar(&model, &identities_fixture()).rows;
        let drawn = rows.iter().filter(|row| row.contains("commit ")).count();
        assert_eq!(drawn, 2, "{rows:?}");

        model.timeline_rows = None;
        let rows = sidebar(&model, &identities_fixture()).rows;
        let default = rows.iter().filter(|row| row.contains("commit ")).count();

        model.timeline_rows = Some(u16::MAX);
        let rows = sidebar(&model, &identities_fixture()).rows;
        let drawn = rows.iter().filter(|row| row.contains("commit ")).count();
        assert!(drawn > default, "past the half-column default: {rows:?}");
        assert!(
            rows.iter().any(|row| row.contains("Agent")),
            "the tree keeps its rows: {rows:?}"
        );

        let timeline = model
            .remembered
            .git_badge
            .as_ref()
            .unwrap()
            .timeline
            .as_ref()
            .unwrap();
        assert_eq!(
            timeline_height(timeline, false, Some(0), 24),
            3,
            "never fewer than one"
        );
        assert_eq!(
            timeline_height(timeline, true, Some(9), 24),
            1,
            "folded is the header alone"
        );
    }

    /// The wheel moves the section a row at a time and never past the
    /// page that ends on the oldest commit; every row drawn is a target
    /// for the commit it shows, by its place in the history.
    #[test]
    fn the_timeline_scrolls_by_rows_within_its_history() {
        let subjects: Vec<String> = (0..20).map(|index| format!("commit {index}")).collect();
        let subjects: Vec<&str> = subjects.iter().map(String::as_str).collect();
        let mut model = session_with_timeline(&subjects);

        model.timeline_scroll = 5;
        let Sidebar { rows, hits, .. } = sidebar(&model, &identities_fixture());
        let drawn: Vec<&String> = rows.iter().filter(|row| row.contains("commit ")).collect();
        assert!(drawn[0].contains("● commit 5"), "{rows:?}");
        assert!(
            rows.iter().all(|row| !row.contains('◉')),
            "HEAD scrolled off: {rows:?}"
        );
        let targets: Vec<usize> = hits
            .iter()
            .filter_map(|(_, hit)| match hit {
                WorkspaceHit::Extension(ExtensionHit::CodeTimeline(ViewHit::SelectItem(index))) => {
                    Some(*index)
                }
                _ => None,
            })
            .collect();
        assert_eq!(targets[0], 5);
        assert_eq!(targets.len(), drawn.len());

        model.timeline_scroll = 100;
        let rows = sidebar(&model, &identities_fixture()).rows;
        assert!(rows.last().unwrap().contains("commit 19"), "{rows:?}");

        model.timeline_scroll = 0;
        model.hits = hits;
        let shown = model.timeline_rows_shown();
        for _ in 0..50 {
            scroll_timeline(&mut model, ScrollDirection::Down);
        }
        assert_eq!(model.timeline_scroll, 20 - shown);
        for _ in 0..50 {
            scroll_timeline(&mut model, ScrollDirection::Up);
        }
        assert_eq!(model.timeline_scroll, 0);
    }

    fn commit_popup(anchor: Rect) -> CommitDetailPopup {
        CommitDetailPopup {
            detail: uze_extensions::code::CommitDetail {
                hash: "0ebf3b8000000000000000000000000000000000".to_owned(),
                short_hash: "0ebf3b8".to_owned(),
                author: "Ada".to_owned(),
                age: "14 minutes ago".to_owned(),
                date: "2026-09-03 19:39".to_owned(),
                refs: vec![
                    "agent/task".to_owned(),
                    "main".to_owned(),
                    "origin/main".to_owned(),
                ],
                subject: "docs(openspec): archive five completed changes".to_owned(),
                body: "Every task done.\n\nThree decisions cleared the ADR bar.".to_owned(),
                files_changed: 8,
                insertions: 350,
                deletions: 4,
            },
            target: Some("main".to_owned()),
            anchor,
            scroll: 0,
        }
    }

    fn popup_rows(width: u16, height: u16, popup: &CommitDetailPopup) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| render_commit_detail(frame, frame.area(), popup))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..buffer.area.height)
            .map(|row| {
                (0..buffer.area.width)
                    .map(|column| buffer[(column, row)].symbol())
                    .collect()
            })
            .collect()
    }

    /// The popup is the commit's account — who, when, what it said, what
    /// it touched, and what stands at it — beside the row it opened from,
    /// in the pane's columns.
    #[test]
    fn a_commit_popup_stands_beside_its_row_and_gives_its_account() {
        let anchor = Rect::new(1, 20, 38, 1);
        let rows = popup_rows(120, 30, &commit_popup(anchor));
        let text = rows.join("\n");

        assert!(text.contains("commit"), "{text}");
        assert!(
            text.contains("Ada · 14 minutes ago · 2026-09-03 19:39"),
            "{text}"
        );
        assert!(
            text.contains("docs(openspec): archive five completed changes"),
            "{text}"
        );
        assert!(
            text.contains("Three decisions cleared the ADR bar."),
            "{text}"
        );
        assert!(text.contains("8 files changed  +350  −4"), "{text}");
        assert!(text.contains(" agent/task   main   origin/main "), "{text}");
        assert!(text.contains("0ebf3b8"), "{text}");
        let border_row = rows
            .iter()
            .position(|row| row.contains('╭') || row.contains('┌'))
            .expect("the popup has a frame");
        let left = rows[border_row]
            .chars()
            .position(|c| c == '╭' || c == '┌')
            .unwrap();
        assert_eq!(
            left as u16,
            anchor.right() + 1,
            "beside the sidebar's divider"
        );
        assert!(
            border_row <= 20,
            "level with its row, or pulled up to fit: {border_row}"
        );
    }

    /// Among the refs at a commit, the delivery target and its
    /// remote-tracking twin wear the target's gold; any other branch is
    /// blue, the way the timeline colours a commit still ahead.
    #[test]
    fn the_target_ref_wears_gold_and_the_others_blue() {
        let popup = commit_popup(Rect::new(1, 2, 38, 1));
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal
            .draw(|frame| render_commit_detail(frame, frame.area(), &popup))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let rows = popup_rows(120, 30, &popup);
        let row = rows
            .iter()
            .position(|row| row.contains(" agent/task "))
            .unwrap();
        let bg_of = |needle: &str| {
            let column = rows[row].find(needle).unwrap() + 1;
            buffer[(column as u16, row as u16)].bg
        };
        assert_eq!(bg_of("agent/task"), theme::color(Token::StateInfo));
        assert_eq!(bg_of(" main "), theme::color(Token::StateWarning));
        assert_eq!(bg_of("origin/main"), theme::color(Token::StateWarning));
    }

    /// A long message scrolls inside the popup rather than growing it
    /// over the pane, and the wheel stops where the text does.
    #[test]
    fn a_long_commit_message_scrolls_inside_a_bounded_popup() {
        let mut popup = commit_popup(Rect::new(1, 2, 38, 1));
        popup.detail.body = (0..40)
            .map(|index| format!("paragraph {index}"))
            .collect::<Vec<_>>()
            .join("\n\n");
        let area = Rect::new(0, 0, 120, 60);
        let layout = render::commit_detail_layout(area, &popup);
        assert_eq!(layout.rect.height, 20, "no taller than a hover card");
        assert!(layout.scroll_limit() > 0);
        assert_eq!(
            layout.scroll_limit() + layout.inner.height,
            layout.content_rows
        );

        let rows = popup_rows(120, 60, &popup);
        assert!(rows.join("\n").contains("paragraph 0"));
        assert!(!rows.join("\n").contains("paragraph 39"), "{rows:?}");
        popup.scroll = u16::MAX;
        let rows = popup_rows(120, 60, &popup);
        let text = rows.join("\n");
        assert!(
            text.contains("0ebf3b8"),
            "held to the end of the text: {text}"
        );
        assert_eq!(
            rows.iter().filter(|row| row.contains('│')).count(),
            18,
            "the frame keeps its height: {rows:?}"
        );
    }

    /// A frame with no room beside the sidebar puts the popup over the
    /// pane, inset, rather than clipping it against the edge.
    #[test]
    fn a_narrow_frame_centres_the_commit_popup() {
        let rows = popup_rows(60, 30, &commit_popup(Rect::new(1, 5, 38, 1)));
        let border_row = rows
            .iter()
            .position(|row| row.contains('╭') || row.contains('┌'))
            .expect("the popup has a frame");
        let left = rows[border_row]
            .chars()
            .position(|c| c == '╭' || c == '┌')
            .unwrap();
        assert_eq!(left, 2, "{:?}", rows[border_row]);
        assert!(rows.join("\n").contains("0ebf3b8"));
    }

    /// While a commit is open the wheel scrolls its text, not the pane
    /// underneath, and any click or key puts it away.
    #[test]
    fn an_open_commit_is_a_modal_like_the_support_dropdown() {
        let mut model = agent_session_in("/repo");
        assert!(model.no_modal_open());
        model.commit_detail = Some(commit_popup(Rect::new(1, 5, 38, 1)));
        assert!(!model.no_modal_open());
    }

    /// A commit's dot says where it stands: blue while it is still ahead
    /// of the base, the target's gold once it has landed there. The ring
    /// says `HEAD`, whichever colour it wears.
    #[test]
    fn a_commits_dot_wears_its_standing() {
        let mut model = session_with_timeline(&["feat: ahead", "fix: also ahead", "chore: landed"]);
        let commits = &mut model
            .remembered
            .git_badge
            .as_mut()
            .unwrap()
            .timeline
            .as_mut()
            .unwrap()
            .commits;
        commits[0].ahead = true;
        commits[1].ahead = true;

        let buffer = sidebar(&model, &identities_fixture()).buffer;
        let rows = sidebar(&model, &identities_fixture()).rows;
        let dot_of = |needle: &str| {
            let row = rows.iter().position(|row| row.contains(needle)).unwrap();
            let column = rows[row]
                .chars()
                .position(|c| c == '◉' || c == '●')
                .unwrap();
            (
                rows[row].chars().nth(column).unwrap(),
                buffer[(column as u16, row as u16)].fg,
            )
        };
        assert_eq!(dot_of("feat: ahead"), ('◉', theme::color(Token::StateInfo)));
        assert_eq!(
            dot_of("fix: also ahead"),
            ('●', theme::color(Token::StateInfo))
        );
        assert_eq!(
            dot_of("chore: landed"),
            ('●', theme::color(Token::StateWarning))
        );
    }

    /// The header is the section's one heading: filled and bold, where the
    /// commit rows under it are plain.
    #[test]
    fn the_timeline_header_stands_out_from_its_rows() {
        let model = session_with_timeline(&["feat: only"]);
        let buffer = sidebar(&model, &identities_fixture()).buffer;
        let rows = sidebar(&model, &identities_fixture()).rows;
        let header = rows
            .iter()
            .position(|row| row.contains("timeline"))
            .expect("the header is drawn");
        let column = rows[header].chars().position(|c| c == 't').unwrap() as u16;

        let cell = &buffer[(column, header as u16)];
        assert_eq!(cell.bg, theme::color(Token::SurfaceRaised));
        assert!(cell.modifier.contains(ratatui::style::Modifier::BOLD));
        let commit = &buffer[(column, header as u16 + 2)];
        assert_ne!(commit.bg, theme::color(Token::SurfaceRaised));
    }

    /// A folded section's title is plain: bold is for a heading over
    /// content, and folded there is none under it.
    #[test]
    fn a_folded_sections_title_is_not_bold() {
        let mut model = session_with_timeline(&["feat: only"]);
        let title_is_bold = |model: &WorkspaceModel| {
            let Sidebar { rows, buffer, .. } = sidebar(model, &identities_fixture());
            let header = rows
                .iter()
                .position(|row| row.contains("timeline"))
                .expect("the header is drawn");
            let column = rows[header]
                .find("timeline")
                .map(|byte| rows[header][..byte].chars().count())
                .unwrap() as u16;
            buffer[(column, header as u16)]
                .modifier
                .contains(ratatui::style::Modifier::BOLD)
        };
        model.timeline_collapsed = false;
        assert!(title_is_bold(&model), "open, it heads its rows");
        model.timeline_collapsed = true;
        assert!(!title_is_bold(&model), "folded, it is plain");
    }

    /// Both sections stack at the foot of the column, the steps on the
    /// history, and each takes its rows before the tree is laid out — so
    /// opening one pushes the other rather than being drawn over it.
    #[test]
    fn the_two_sections_stack_at_the_foot_and_push_each_other() {
        let mut model = session_with_timeline(&["feat: one", "fix: two", "chore: three"]);
        model.timeline_collapsed = true;
        model.first_steps_collapsed = true;
        let rows = sidebar(&model, &identities_fixture()).rows;

        let steps = rows
            .iter()
            .position(|row| row.contains("first steps"))
            .expect("the steps are at the foot");
        let timeline = rows
            .iter()
            .position(|row| row.contains("timeline"))
            .expect("and the history under them");
        assert_eq!(timeline, steps + 1, "in that order, adjacent: {rows:?}");
        assert_eq!(timeline, rows.len() - 1, "and nothing below: {rows:?}");

        // Opening the steps pushes the history down the column, never over
        // it: both headers are still on screen, still in that order.
        model.first_steps_collapsed = false;
        let rows = sidebar(&model, &identities_fixture()).rows;
        let steps = rows
            .iter()
            .position(|row| row.contains("first steps"))
            .expect("still there");
        let timeline = rows
            .iter()
            .position(|row| row.contains("timeline"))
            .expect("and so is the history");
        assert_eq!(
            timeline,
            steps + 1 + render::FIRST_STEPS.len() + 1,
            "the steps came between them, with a blank row closing them off \
             so the last one does not sit against the next header: {rows:?}"
        );
        assert!(
            inside(&rows[timeline - 1]).trim().is_empty(),
            "and that row is blank: {rows:?}"
        );
        assert_eq!(timeline, rows.len() - 1, "{rows:?}");
    }

    /// One open at a time. They stack in the same column and each takes
    /// its rows from the tree, so two open at once spends the sidebar on
    /// what sits under the spaces rather than on the spaces.
    #[test]
    fn opening_one_section_folds_the_other() {
        let mut model = session_with_timeline(&["feat: one"]);
        model.timeline_collapsed = true;
        model.first_steps_collapsed = true;

        toggle_timeline(&mut model);
        assert!(!model.timeline_collapsed);
        assert!(model.first_steps_collapsed, "the steps gave way");

        // And back: the section that opens is the one that was asked for.
        model.first_steps_collapsed = false;
        model.timeline_collapsed = true;
        toggle_timeline(&mut model);
        assert!(!model.timeline_collapsed);
        assert!(model.first_steps_collapsed);

        // Folding one leaves the other alone — an accordion closes nothing
        // on the way to closing itself.
        model.first_steps_collapsed = false;
        model.timeline_collapsed = true;
        toggle_timeline(&mut model);
        toggle_timeline(&mut model);
        assert!(model.timeline_collapsed);
        assert!(model.first_steps_collapsed);
    }

    /// A dialog is held to a reading width, and its own selection does not
    /// decide that width. Filling the selected row to the frame's edge
    /// before anything had measured the popup made the popup the width of
    /// the terminal.
    #[test]
    fn the_preserved_dialog_keeps_a_reading_width() {
        let model = WorkspaceModel {
            preserved: Some(PreservedOverlay {
                selected: 0,
                confirm_discard: false,
            }),
            ..WorkspaceModel::default()
        };
        let overlay = model.preserved.as_ref().expect("open");
        let mut terminal = Terminal::new(TestBackend::new(200, 20)).unwrap();
        terminal
            .draw(|frame| render_preserved(frame, frame.area(), &model, overlay))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let drawn: Vec<String> = (0..buffer.area.height)
            .map(|row| {
                (0..buffer.area.width)
                    .map(|column| buffer[(column, row)].symbol())
                    .collect()
            })
            .collect();
        let top = drawn
            .iter()
            .find(|row| row.contains('┌'))
            .expect("the dialog drew a border");
        let width = top.trim_end().chars().count()
            - top.find('┌').map_or(0, |byte| top[..byte].chars().count());
        assert!(
            (30..=72).contains(&width),
            "held to a reading width, not the terminal's: {width}"
        );
    }

    /// Every key this dialog names comes from the keymap. Five of them
    /// were written into the string by hand — the last place in the client
    /// that claimed a key nothing had resolved.
    #[test]
    fn the_preserved_dialog_reads_its_keys_off_the_keymap() {
        let model = WorkspaceModel {
            preserved: Some(PreservedOverlay {
                selected: 0,
                confirm_discard: false,
            }),
            ..WorkspaceModel::default()
        };
        let overlay = model.preserved.as_ref().expect("open");
        let mut terminal = Terminal::new(TestBackend::new(120, 20)).unwrap();
        terminal
            .draw(|frame| render_preserved(frame, frame.area(), &model, overlay))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let drawn: String = (0..buffer.area.height)
            .flat_map(|row| (0..buffer.area.width).map(move |column| (column, row)))
            .map(|position| buffer[position].symbol().to_owned())
            .collect();

        let keymap = uze_keys::active();
        let scopes = [uze_keys::Scope::Global, uze_keys::Scope::PreservedWork];
        for action in [
            uze_keys::Action::ResumeTask,
            uze_keys::Action::DeliverTask,
            uze_keys::Action::FinishTask,
            uze_keys::Action::DiscardTask,
            uze_keys::Action::Dismiss,
        ] {
            let chord = keymap
                .chord_for(action, &scopes)
                .unwrap_or_else(|| panic!("{action} is bound here"));
            assert!(
                drawn.contains(&format!("{chord} {}", action.label().to_lowercase())),
                "{action} is named with the key the keymap binds: {drawn}"
            );
        }
    }

    /// The release notice sits on the sections holding the foot, not under
    /// them, and every part of it is whole at a real version's length.
    #[test]
    fn a_release_notice_sits_on_the_sections_at_the_foot() {
        let mut model = session_with_timeline(&["feat: one"]);
        model.timeline_collapsed = true;
        let hits = sidebar(&model, &identities_fixture()).hits;
        assert!(
            !hits.iter().any(|(_, hit)| matches!(
                hit,
                WorkspaceHit::OpenReleaseNotes | WorkspaceHit::DismissRelease
            )),
            "no release, no notice"
        );

        model.release = Some(crate::self_update::Notice::Installed(
            "0.0.0-alpha.14".to_owned(),
        ));
        let Sidebar { rows, hits, .. } = sidebar(&model, &identities_fixture());
        let (mark, _) = hits
            .iter()
            .find(|(_, hit)| matches!(hit, WorkspaceHit::DismissRelease))
            .expect("its mark puts it away");
        let y = usize::from(mark.y);
        assert!(
            rows[y].contains("v0.0.0-alpha.14")
                && rows[y].contains(&theme::glyph(theme::Symbol::MarkClose)),
            "the version, whole, with the mark on its row: {rows:?}"
        );
        assert!(rows[y + 1].contains("restart uze to use it"), "{rows:?}");
        assert_eq!(
            hits.iter()
                .filter(|(_, hit)| matches!(hit, WorkspaceHit::OpenReleaseNotes))
                .count(),
            2,
            "two rows, and each opens the notes"
        );
        let steps = rows
            .iter()
            .position(|line| line.contains("first steps"))
            .expect("the steps are still there");
        assert!(y < steps, "and the notice sits on them: {rows:?}");
    }

    /// The header folds it, and it stays folded: a section that came back
    /// open every run would be one nobody could put away.
    #[test]
    fn the_first_steps_section_folds_to_its_header() {
        let mut model = session_with_timeline(&["feat: one"]);
        model.timeline_collapsed = true;
        let Sidebar { rows, hits, .. } = sidebar(&model, &identities_fixture());
        assert!(
            rows.iter().any(|row| row.contains("first steps")),
            "{rows:?}"
        );
        assert_eq!(
            hits.iter()
                .filter(|(_, hit)| matches!(hit, WorkspaceHit::QuickAction(_)))
                .count(),
            render::FIRST_STEPS.len(),
            "every step is a target"
        );

        model.first_steps_collapsed = true;
        let Sidebar { rows, hits, .. } = sidebar(&model, &identities_fixture());
        assert!(
            rows.iter().any(|row| row.contains("first steps")),
            "the header stays: {rows:?}"
        );
        assert!(
            !hits
                .iter()
                .any(|(_, hit)| matches!(hit, WorkspaceHit::QuickAction(_))),
            "and its steps are folded away: {rows:?}"
        );
        assert!(
            model.shape().first_steps.collapsed,
            "and the shape this client hands back says so"
        );
    }

    /// The list finishes. Once every step has been taken the header offers
    /// a mark that puts it away for good — and only then: a list of things
    /// to try that could be dismissed before trying any of them would be
    /// onboarding nobody ever sees.
    #[test]
    fn a_finished_list_offers_to_leave() {
        let mut model = session_with_timeline(&["feat: one"]);
        model.timeline_collapsed = true;
        model.steps_taken = [render::FIRST_STEPS[0].name()].into_iter().collect();

        let Sidebar { rows, hits, .. } = sidebar(&model, &identities_fixture());
        assert!(
            !hits
                .iter()
                .any(|(_, hit)| *hit == WorkspaceHit::CloseFirstSteps),
            "unfinished, so nothing to close: {rows:?}"
        );

        model.steps_taken = render::FIRST_STEPS
            .iter()
            .map(|action| action.name())
            .collect();
        let Sidebar { rows, hits, .. } = sidebar(&model, &identities_fixture());
        let close = hits
            .iter()
            .find_map(|(rect, hit)| (*hit == WorkspaceHit::CloseFirstSteps).then_some(*rect))
            .expect("finished, so the header offers the way out");
        let header = rows
            .iter()
            .position(|row| row.contains("first steps"))
            .expect("the header is drawn");
        assert_eq!(usize::from(close.y), header, "on the header itself");
        // Resolved the way an ordinary click is — `hit_rect_at`, first
        // rect wins — and not by the reversed search the modal guards use.
        // Asking the wrong one is why this shipped folding instead of
        // closing.
        model.hits = hits.clone();
        assert_eq!(
            model.hit_rect_at(close.x, close.y).map(|(_, hit)| hit),
            Some(WorkspaceHit::CloseFirstSteps),
            "and the header underneath does not swallow it"
        );

        model.first_steps_closed = true;
        let rows = sidebar(&model, &identities_fixture()).rows;
        assert!(
            !rows.iter().any(|row| row.contains("first steps")),
            "closed for good, header and all: {rows:?}"
        );
        assert!(
            rows.iter().any(|row| row.contains("timeline")),
            "and the history took the rows back: {rows:?}"
        );
        assert!(model.shape().first_steps.closed);
    }

    // --- The management modal --------------------------------------------

    fn manage_chord() -> uze_keys::Chord {
        uze_keys::active()
            .chord_for(
                uze_keys::Action::SwitchMode,
                &[uze_keys::Scope::Global, uze_keys::Scope::Workspace],
            )
            .expect("the modal is reachable from the keyboard")
    }

    fn manage_route(driven: &Driven<'_>) -> crate::ui::model::Route {
        driven
            .attach
            .model
            .manage
            .as_ref()
            .expect("the modal is open")
            .route
    }

    /// Creating a space is reachable from the keyboard, and the chord opens
    /// exactly what the pointer's `new` opens: the picker, listing where
    /// the selected space's neighbours are and standing on the space.
    #[test]
    fn the_new_space_chord_opens_the_picker_the_pointer_opens() {
        let home = UzeHome::at(uze_testkit::temp::scratch("orchestrator-new-space-chord"));
        let mut driven = driven(agent_session_in("/repo"), &home);
        let chord = uze_keys::active()
            .chord_for(uze_keys::Action::NewSpace, &[uze_keys::Scope::Workspace])
            .expect("space creation is reachable from the keyboard");

        driven.press_key(key_event(chord));

        let picker = driven
            .attach
            .model
            .root_picker
            .as_ref()
            .expect("the picker opened");
        // Where a project beside this one would be. Whether the space's
        // own root is then marked is a question about real directories,
        // and `root_picker`'s own tests answer it over a temp tree — this
        // session's `/repo` is a name, not a directory.
        assert_eq!(
            picker.base(),
            Path::new("/"),
            "listing where a project beside this one would be"
        );
        assert!(
            picker.input().is_empty(),
            "with nothing typed for the operator"
        );
    }

    /// The management surface is a modal over the workspace, not a mode
    /// beside it: the action that opens it closes it again, the frame
    /// draws it over everything, and the client behind it stays attached.
    #[test]
    fn the_manage_action_opens_the_modal_and_closes_it_again() {
        let home = UzeHome::at(uze_testkit::temp::scratch("orchestrator-manage-toggle"));
        let mut driven = driven(agent_with_task(TaskStateView::Ready, 1), &home);

        driven.press_key(key_event(manage_chord()));
        assert!(driven.attach.model.manage.is_some(), "the modal opened");
        driven.frame();
        let chrome = driven
            .attach
            .model
            .manage_chrome
            .expect("the frame drew the modal");
        assert!(
            driven.attach.model.hits[..2]
                .iter()
                .any(|(rect, hit)| *hit == WorkspaceHit::ManageSurface && *rect == chrome.area),
            "the modal answers for its own rectangle ahead of everything under it"
        );
        assert!(
            driven.attach.model.session.is_some(),
            "the workspace behind it is still attached"
        );

        driven.press_key(key_event(manage_chord()));
        assert!(
            driven.attach.model.manage.is_none(),
            "the same key closes it"
        );
        driven.frame();
        assert!(driven.attach.model.manage_chrome.is_none());
    }

    /// The header's trailing control opens the modal; a click inside it
    /// is the modal's own, a click beside it closes it.
    #[test]
    fn the_header_control_opens_the_modal_and_a_click_beside_it_closes_it() {
        let home = UzeHome::at(uze_testkit::temp::scratch("orchestrator-manage-click"));
        let mut driven = driven(agent_with_task(TaskStateView::Ready, 1), &home);
        driven.frame();
        let more = driven
            .attach
            .model
            .hits
            .iter()
            .find_map(|(rect, hit)| (*hit == WorkspaceHit::OpenManage).then_some(*rect))
            .expect("the header offers the modal");
        driven.press(more.x, more.y);
        assert!(
            driven.attach.model.manage.is_some(),
            "the control opened it"
        );
        assert_eq!(manage_route(&driven), crate::ui::model::Route::Overview);

        driven.frame();
        let plugins = driven
            .attach
            .model
            .manage
            .as_ref()
            .expect("open")
            .hits
            .iter()
            .find_map(|(rect, hit)| {
                matches!(
                    hit,
                    crate::ui::hit::Hit::Route(crate::ui::model::Route::Plugins)
                )
                .then_some(*rect)
            })
            .expect("the modal's menu lists Plugins");
        driven.press(plugins.x, plugins.y);
        assert_eq!(
            manage_route(&driven),
            crate::ui::model::Route::Plugins,
            "a click inside the modal reaches the modal"
        );

        let chrome = driven.attach.model.manage_chrome.expect("drawn");
        assert!(
            chrome.area.x > 0,
            "the modal leaves the workspace visible beside it"
        );
        driven.press(0, 0);
        assert!(
            driven.attach.model.manage.is_none(),
            "a click beside the modal closes it"
        );
        assert!(
            driven.attach.model.root_picker.is_none(),
            "and reaches nothing underneath"
        );
    }

    /// The mark on the modal's title closes it.
    #[test]
    fn the_close_mark_on_the_modal_closes_it() {
        let home = UzeHome::at(uze_testkit::temp::scratch("orchestrator-manage-close-mark"));
        let mut driven = driven(agent_with_task(TaskStateView::Ready, 1), &home);
        driven.press_key(key_event(manage_chord()));
        driven.frame();
        let close = driven.attach.model.manage_chrome.expect("drawn").close;
        driven.press(close.x, close.y);
        assert!(driven.attach.model.manage.is_none());
    }

    /// Inside the modal the keyboard is the modal's: a key that moves its
    /// screens moves them, and one that backs out of everything backs out
    /// of the modal when nothing inside it is open.
    #[test]
    fn keys_inside_the_modal_are_the_modals() {
        let home = UzeHome::at(uze_testkit::temp::scratch("orchestrator-manage-keys"));
        let mut driven = driven(agent_with_task(TaskStateView::Ready, 1), &home);
        driven.press_key(key_event(manage_chord()));
        let keymap = uze_keys::active();
        let next = keymap
            .chord_for(
                uze_keys::Action::NextScreen,
                &[uze_keys::Scope::Global, uze_keys::Scope::Management],
            )
            .expect("screens are walked from the keyboard");
        driven.press_key(key_event(next));
        assert_ne!(
            manage_route(&driven),
            crate::ui::model::Route::Overview,
            "the modal's own screen moved"
        );
        assert!(
            driven.attach.model.action_index.is_none()
                && driven.attach.model.agent_picker.is_none()
                && driven.attach.model.root_picker.is_none(),
            "nothing of the workspace answered"
        );

        // Esc leaves the modal. A screen's detail drawer is a column of
        // it, not a layer over it, so there is nothing for Esc to back out
        // of first — which is what makes one press enough.
        let esc = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        );
        driven.press_key(esc);
        assert!(
            driven.attach.model.manage.is_none(),
            "Esc closes the modal from the screen it was on"
        );
    }

    /// The modal is about the project the operator is standing in.
    ///
    /// The process's own directory is not that project: a shell opens at
    /// home and the work is in a repository, so the Overview read its
    /// prompt history — and its context status, and the project's
    /// plugins — against a directory nothing had been recorded for, and
    /// said "no history yet" over a full file.
    #[test]
    fn the_modal_is_about_the_space_it_was_opened_over() {
        let home = UzeHome::at(uze_testkit::temp::scratch("orchestrator-manage-root"));
        let mut driven = driven(agent_session_in("/repo"), &home);
        driven.press_key(key_event(manage_chord()));

        assert_eq!(
            driven
                .attach
                .model
                .manage
                .as_ref()
                .expect("the modal opened")
                .context_root,
            PathBuf::from("/repo"),
            "the modal speaks about the space, not about where uze was started"
        );
    }

    /// The modal reopens on the screen it was closed on, and the layout
    /// the client hands back carries that screen for the next run.
    #[test]
    fn the_modal_reopens_where_it_was_closed() {
        let home = UzeHome::at(uze_testkit::temp::scratch("orchestrator-manage-memory"));
        let mut driven = driven(agent_with_task(TaskStateView::Ready, 1), &home);
        driven.press_key(key_event(manage_chord()));
        let next = uze_keys::active()
            .chord_for(
                uze_keys::Action::NextScreen,
                &[uze_keys::Scope::Global, uze_keys::Scope::Management],
            )
            .expect("bound");
        driven.press_key(key_event(next));
        let moved_to = manage_route(&driven);
        driven.press_key(key_event(manage_chord()));
        assert_eq!(
            driven.attach.model.management_layout.route.as_deref(),
            Some(moved_to.id()),
            "closing keeps the screen in the layout the client owns"
        );
        assert_eq!(
            driven.attach.model.shape().management.route.as_deref(),
            Some(moved_to.id()),
            "and it is what the layout file is written from"
        );

        driven.press_key(key_event(manage_chord()));
        assert_eq!(manage_route(&driven), moved_to, "reopening lands on it");
    }

    /// The sidebar opens with the surface's name, the way to grow it and the
    /// way into the other one, a rule between the two controls.
    #[test]
    fn the_sidebar_header_names_the_column_and_offers_the_modal() {
        let mut model = agent_with_task(TaskStateView::Ready, 1);
        let rows = frame_rows(&mut model);
        let header = &rows[0];
        assert!(header.contains("work"), "the surface is named: {header:?}");
        assert!(
            header.contains(&theme::glyph(crate::ui::theme::Symbol::Manage)),
            "the control that opens the modal ends the row: {header:?}"
        );
        assert!(
            rows[1]
                .trim_start()
                .starts_with(&theme::glyph(crate::ui::theme::Symbol::TreeDivider).repeat(4)),
            "a hairline closes the header: {:?}",
            rows[1]
        );
        assert!(
            header.contains("new"),
            "the way to grow the column rides the header: {header:?}"
        );
        let rule = header
            .find(&theme::glyph(crate::ui::theme::Symbol::TreeColumnDivider))
            .expect("a rule between the two controls");
        assert!(
            header[..rule].contains("new"),
            "with the rule between them: {header:?}"
        );
    }

    /// The word is where the prompt came from: while that prompt is open it
    /// is spent, and says so by going quiet until it closes.
    #[test]
    fn the_way_to_grow_the_column_goes_quiet_while_its_prompt_is_open() {
        let hue_of_new = |model: &mut WorkspaceModel| {
            let area = Rect::new(0, 0, 80, 24);
            let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
            terminal
                .draw(|frame| {
                    render::render(
                        frame,
                        model,
                        &identities_fixture(),
                        &mut Vec::new(),
                        &mut render::FrameMetrics::default(),
                    )
                })
                .unwrap();
            let buffer = terminal.backend().buffer().clone();
            let header = buffer_rows(&buffer)[0].clone();
            let column = header.find("new").expect("the control is drawn") as u16;
            buffer[(column, 0)].fg
        };
        let mut model = agent_with_task(TaskStateView::Ready, 1);
        assert_eq!(hue_of_new(&mut model), theme::color(Token::Accent));

        model.root_picker = Some(RootPicker::opened_in("~", None));

        assert_eq!(
            hue_of_new(&mut model),
            theme::color(Token::TextMuted),
            "spent while the prompt it opened is open"
        );
    }

    /// The keystroke a chord is: the inverse of `keys::chord_of`, so a test
    /// can press what the keymap says rather than a key typed by hand.
    fn key_event(chord: uze_keys::Chord) -> crossterm::event::KeyEvent {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        use uze_keys::Key;
        let code = match chord.key {
            Key::Char(character) => KeyCode::Char(character),
            Key::F(number) => KeyCode::F(number),
            Key::Enter => KeyCode::Enter,
            Key::Esc => KeyCode::Esc,
            Key::Tab => KeyCode::Tab,
            Key::Space => KeyCode::Char(' '),
            Key::Backspace => KeyCode::Backspace,
            Key::Delete => KeyCode::Delete,
            Key::Insert => KeyCode::Insert,
            Key::Up => KeyCode::Up,
            Key::Down => KeyCode::Down,
            Key::Left => KeyCode::Left,
            Key::Right => KeyCode::Right,
            Key::Home => KeyCode::Home,
            Key::End => KeyCode::End,
            Key::PageUp => KeyCode::PageUp,
            Key::PageDown => KeyCode::PageDown,
        };
        let mut modifiers = KeyModifiers::NONE;
        if chord.mods.ctrl {
            modifiers |= KeyModifiers::CONTROL;
        }
        if chord.mods.alt {
            modifiers |= KeyModifiers::ALT;
        }
        if chord.mods.shift {
            modifiers |= KeyModifiers::SHIFT;
        }
        KeyEvent::new(code, modifiers)
    }

    /// The property the list needs, in this mode too: a step must be
    /// takeable from where the list is drawn, and taking it must tick it.
    ///
    /// Moving between agents did not. Its evidence was "the selected tab
    /// changed", and this client asks the server for a tab and is answered
    /// frames later — so at the moment the question was asked the answer
    /// was always no, however many times the step was taken.
    #[test]
    fn every_first_step_is_ticked_when_it_is_taken() {
        let home = UzeHome::at(uze_testkit::temp::scratch("first-steps-ticked"));
        let (model, first, _) = two_agents_with_shells();
        let mut driven = driven(model, &home);
        driven
            .attach
            .model
            .session
            .as_mut()
            .expect("session")
            .select_tab(first);

        let keymap = uze_keys::active();
        for action in render::FIRST_STEPS {
            let chord = keymap
                .chord_for(action, render::FIRST_STEP_SCOPES)
                .unwrap_or_else(|| panic!("{action} is bound in this mode"));
            driven.press_key(key_event(chord));
            assert!(
                driven.attach.model.steps_taken.contains(&action.name()),
                "{action} was taken with {chord} and never ticked"
            );
            // Whatever it opened goes away before the next one is tried.
            driven.press_key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyModifiers::NONE,
            ));
        }
    }

    /// A step is ticked once it has been taken, and the header counts them.
    #[test]
    fn a_step_taken_is_marked_and_counted() {
        let mut model = session_with_timeline(&["feat: one"]);
        model.timeline_collapsed = true;
        model.steps_taken = [render::FIRST_STEPS[0].name()].into_iter().collect();
        let rows = sidebar(&model, &identities_fixture()).rows;

        let header = rows
            .iter()
            .find(|row| row.contains("first steps"))
            .expect("the section names itself");
        assert!(
            header.contains(&format!("1 of {}", render::FIRST_STEPS.len())),
            "{header:?}"
        );

        let tick = theme::glyph(theme::Symbol::MarkDone);
        let taken = rows
            .iter()
            .find(|row| row.contains(&render::FIRST_STEPS[0].label()))
            .expect("the step is listed");
        assert!(taken.contains(&tick), "{taken:?}");
        let untaken = rows
            .iter()
            .find(|row| row.contains(&render::FIRST_STEPS[1].label()))
            .expect("and so is the next one");
        assert!(!untaken.contains(&tick), "{untaken:?}");
    }

    /// Folded, the header carries no band. A filled row across the column
    /// says "this is a heading over content", and a folded section has
    /// none — two of them stacked at the foot of the sidebar read as a
    /// toolbar rather than as two things you can open.
    #[test]
    fn a_folded_section_header_carries_no_band() {
        let mut model = session_with_timeline(&["feat: only"]);
        model.first_steps_collapsed = true;

        for (collapsed, banded) in [(false, true), (true, false)] {
            model.timeline_collapsed = collapsed;
            let buffer = sidebar(&model, &identities_fixture()).buffer;
            let rows = sidebar(&model, &identities_fixture()).rows;
            let header = rows
                .iter()
                .position(|row| row.contains("timeline"))
                .expect("the header is drawn");
            let column = rows[header]
                .chars()
                .position(|glyph| glyph == 't')
                .expect("its title") as u16;
            assert_eq!(
                buffer[(column, header as u16)].bg == theme::color(Token::SurfaceRaised),
                banded,
                "collapsed={collapsed}: {rows:?}"
            );
        }
    }

    /// The timeline keeps the foot of the column, under the spaces, with
    /// its header naming the branch and `HEAD` ringed at the top of the
    /// list — wherever the tree above happens to end.
    #[test]
    fn the_timeline_keeps_the_foot_of_the_column() {
        let model = session_with_timeline(&["feat: third", "fix: second", "chore: first"]);
        let Sidebar { rows, hits, .. } = sidebar(&model, &identities_fixture());
        let last = rows.len() - 1;

        assert!(rows[last].contains("● chore: first"), "{rows:?}");
        assert!(rows[last - 1].contains("● fix: second"), "{rows:?}");
        assert!(rows[last - 2].contains("◉ feat: third"), "{rows:?}");
        assert!(inside(&rows[last - 2]).ends_with("3h"), "{rows:?}");
        assert!(
            inside(&rows[last - 3]).trim().chars().all(|c| c == '─'),
            "a divider parts the header from its rows: {rows:?}"
        );
        assert_eq!(
            resize_hit(&hits).map(|rect| rect.y),
            Some((last - 3) as u16),
            "the divider is the handle"
        );
        let header = &rows[last - 4];
        assert!(header.contains("▾ timeline"), "{header}");
        assert!(inside(header).ends_with("main"), "{header}");
        let space_row = rows
            .iter()
            .position(|row| row.contains("Agent"))
            .expect("the agent stays in the tree above");
        assert!(space_row < last - 4, "{rows:?}");
        assert_eq!(
            timeline_hit(&hits).map(|rect| rect.y),
            Some((last - 4) as u16)
        );
    }

    /// Folded, the section is its header alone — still at the foot, still
    /// the one target that opens it back up.
    #[test]
    fn folding_the_timeline_keeps_only_its_header() {
        let mut model = session_with_timeline(&["feat: third", "fix: second"]);
        model.timeline_collapsed = true;
        let Sidebar { rows, hits, .. } = sidebar(&model, &identities_fixture());
        let last = rows.len() - 1;

        assert!(rows[last].contains("▸ timeline"), "{rows:?}");
        assert!(
            rows.iter()
                .all(|row| !row.contains("feat:") && !row.contains("fix:")),
            "{rows:?}"
        );
        let hit = timeline_hit(&hits).expect("the header folds and unfolds");
        assert_eq!((hit.y, hit.height), (last as u16, 1));
        assert_eq!(resize_hit(&hits), None, "nothing to resize while folded");
    }

    /// The spaces are what the sidebar is for: however long the history,
    /// the section takes at most half of what the column has left, and
    /// the newest commits are the ones that fit.
    #[test]
    fn the_timeline_takes_at_most_half_the_column() {
        let subjects: Vec<String> = (0..20).map(|index| format!("commit {index}")).collect();
        let subjects: Vec<&str> = subjects.iter().map(String::as_str).collect();
        let model = session_with_timeline(&subjects);
        let rows = sidebar(&model, &identities_fixture()).rows;

        let drawn: Vec<&String> = rows.iter().filter(|row| row.contains("commit ")).collect();
        assert!(drawn.len() < 20, "{rows:?}");
        assert!(drawn.len() * 2 <= rows.len(), "{rows:?}");
        assert!(drawn[0].contains("commit 0"), "newest first: {rows:?}");
        assert_eq!(
            timeline_height(
                model
                    .remembered
                    .git_badge
                    .as_ref()
                    .unwrap()
                    .timeline
                    .as_ref()
                    .unwrap(),
                false,
                None,
                3
            ),
            0,
            "a column too short for the header shows nothing"
        );
    }

    /// The subject gives way before the age, so the column that says when
    /// stays a column however long the commit message runs.
    #[test]
    fn a_long_subject_gives_way_before_its_age() {
        let model = session_with_timeline(&[
            "feat(tui): a subject long enough to run past the sidebar's width",
        ]);
        let rows = sidebar(&model, &identities_fixture()).rows;
        let row = rows
            .iter()
            .find(|row| row.contains("◉"))
            .expect("the commit is drawn");

        assert!(row.contains('…'), "{row}");
        assert!(inside(row).ends_with("3h"), "{row:?}");
        assert!(row.contains("feat(tui): a subject"), "{row}");
    }

    /// Folding the timeline is a preference, not a transient: the next
    /// run is told, rather than opening the section again over the spaces.
    #[test]
    fn folding_the_timeline_is_kept_for_the_next_run() {
        let (recorder, recorded) = std::sync::mpsc::channel();
        let mut model = session_with_timeline(&["feat: newest"]);
        model.layout_recorder = Some(recorder);
        model.timeline_collapsed = false;
        model.timeline_rows = Some(4);

        toggle_timeline(&mut model);

        let shape = recorded.try_recv().expect("the fold is recorded");
        assert!(shape.workspace.timeline_collapsed);
        assert_eq!(
            shape.workspace.timeline_rows,
            Some(4),
            "the height it was left at"
        );
        assert_eq!(
            shape.sidebar.width, model.sidebar_width,
            "the whole column's shape, not the one field that changed"
        );
    }

    /// A tree taller than the column scrolls rather than ending wherever
    /// the column ran out: the spaces past the foot — under a long tree,
    /// or under the timeline that holds that foot — were unreachable, not
    /// merely out of view.
    #[test]
    fn the_space_tree_scrolls_to_what_the_column_cannot_show() {
        let mut session = session("/tmp", uze_terminal::SpaceKind::Worktree);
        session.workspace.spaces[0].tabs[0].label = "Agent".into();
        session.workspace.spaces[0].tabs[0].pane.process = "agent".into();
        for index in 1..8 {
            session.create_space(
                Some(format!("space {index}")),
                uze_terminal::SpaceSeat {
                    root: format!("/tmp/{index}").into(),
                    kind: uze_terminal::SpaceKind::Worktree,
                },
                80,
                24,
            );
            session.workspace.spaces[index].tabs[0].label = "Agent".into();
            session.workspace.spaces[index].tabs[0].pane.process = "agent".into();
        }
        let mut model = model_of(session);

        let Sidebar { rows, metrics, .. } = sidebar(&model, &identities_fixture());
        assert!(metrics.tree_overflow > 0, "the tree outgrows the column");
        assert!(
            !rows.iter().any(|row| row.contains("space 7")),
            "the last space starts past the foot: {rows:?}"
        );

        model.tree_overflow = metrics.tree_overflow;
        for _ in 0..metrics.tree_overflow {
            scroll_tree(&mut model, ScrollDirection::Down);
        }
        let rows = sidebar(&model, &identities_fixture()).rows;
        assert!(
            rows.iter().any(|row| row.contains("space 7")),
            "scrolled to the foot: {rows:?}"
        );
        assert!(
            !rows.iter().any(|row| row.contains("space 1")),
            "the head scrolled out of view: {rows:?}"
        );

        scroll_tree(&mut model, ScrollDirection::Down);
        assert_eq!(
            model.remembered.tree_scroll, metrics.tree_overflow,
            "the wheel stops at the foot"
        );
        for _ in 0..=metrics.tree_overflow {
            scroll_tree(&mut model, ScrollDirection::Up);
        }
        assert_eq!(
            model.remembered.tree_scroll, 0,
            "and comes back to the head"
        );
    }

    /// No history, no section — a checkout with nothing committed, or no
    /// checkout at all, leaves the column to the spaces.
    #[test]
    fn without_history_the_sidebar_ends_with_the_spaces() {
        let model = agent_session_in("/repo");
        let Sidebar { rows, hits, .. } = sidebar(&model, &identities_fixture());

        assert!(rows.iter().all(|row| !row.contains("timeline")), "{rows:?}");
        assert_eq!(timeline_hit(&hits), None);
    }

    /// The sidebar drawn once, among `identities`: what it looks like, as
    /// cells and as text, and what the frame recorded while drawing it —
    /// the hits a click resolves against and the bounds only the render
    /// knows, like the tree's own scroll overflow.
    struct Sidebar {
        buffer: ratatui::buffer::Buffer,
        rows: Vec<String>,
        hits: Vec<(Rect, WorkspaceHit)>,
        metrics: FrameMetrics,
    }

    fn sidebar(model: &WorkspaceModel, identities: &[AgentIdentity]) -> Sidebar {
        let mut terminal = Terminal::new(TestBackend::new(40, 24)).unwrap();
        let mut hits = Vec::new();
        let mut metrics = FrameMetrics::default();
        terminal
            .draw(|frame| {
                render_sidebar(
                    frame,
                    frame.area(),
                    model,
                    identities,
                    &mut hits,
                    &mut metrics,
                )
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        Sidebar {
            rows: buffer_rows(&buffer),
            buffer,
            hits,
            metrics,
        }
    }

    /// The rows whose gutter is lit — drawn in the hue of the space's own
    /// kind, in the sidebar's leading column `column`, which every space's
    /// gutter runs down.
    fn lit_gutter_rows(
        buffer: &ratatui::buffer::Buffer,
        column: u16,
        kind: uze_terminal::SpaceKind,
    ) -> Vec<u16> {
        let hue = theme::color(match kind {
            uze_terminal::SpaceKind::Worktree => Token::SpaceWorktree,
            uze_terminal::SpaceKind::Workspace => Token::SpaceWorkspace,
        });
        (0..buffer.area.height)
            .filter(|row| {
                let cell = &buffer[(column, *row)];
                cell.symbol() != " " && cell.fg == hue
            })
            .collect()
    }

    /// The column every space's gutter runs down, by the first header drawn.
    fn gutter_column(hits: &[(Rect, WorkspaceHit)]) -> u16 {
        hits.iter()
            .find(|(_, hit)| matches!(hit, WorkspaceHit::SelectSpace(_)))
            .map(|(rect, _)| rect.x)
            .expect("a space header is drawn")
    }

    fn buffer_rows(buffer: &ratatui::buffer::Buffer) -> Vec<String> {
        (0..buffer.area.height)
            .map(|row| {
                (0..buffer.area.width)
                    .map(|column| buffer[(column, row)].symbol())
                    .collect()
            })
            .collect()
    }

    /// The foreground the caption under the agent labelled `agent` — the
    /// row beneath its name — is drawn in, checked to be captioning `text`.
    fn caption_color_of(model: &WorkspaceModel, agent: &str, text: &str) -> Color {
        let Sidebar { buffer, rows, .. } = sidebar(model, &identities_fixture());
        let row = rows
            .iter()
            .position(|row| row.contains(agent))
            .unwrap_or_else(|| panic!("{agent} is named in the tree: {rows:?}"))
            + 1;
        let offset = rows[row]
            .find(text)
            .unwrap_or_else(|| panic!("{text} captions {agent}: {rows:?}"));
        // A byte offset is not a column once the caption holds small caps
        // or subscript digits: one cell, several bytes.
        let column = rows[row][..offset].chars().count();
        buffer[(column as u16, row as u16)].fg
    }

    /// Every agent has a slot, so a slot is nothing to announce: neither
    /// the `.worktrees/<id>` tail — two more segments in a column this
    /// narrow — nor a mark of its own on the name row.
    #[test]
    fn an_agent_in_a_slot_is_left_unmarked_and_says_where_nowhere() {
        let model = agent_session_in("/repo/.worktrees/ai");
        let Sidebar { rows, hits, .. } = sidebar(&model, &identities_fixture());
        let caption = &rows[agent_rows(&hits)[1] as usize];
        assert!(caption.contains("agent"), "what runs there: {caption}");
        assert!(!caption.contains(".worktrees"), "{caption}");
        assert!(!caption.contains("/repo"), "{caption}");
        assert!(
            !caption.contains('\u{22d4}'),
            "one mark, not two: {caption}"
        );
    }

    /// The column in front of an agent's name answers one question — how
    /// that agent is doing — so which agent the keystrokes reach is left
    /// to the caption's hue, wherever that agent stands: a slot of its
    /// own or the operator's tree, both read the same way.
    #[test]
    fn the_agent_receiving_keystrokes_is_the_one_captioned_in_the_warning_hue() {
        let mut model = two_agent_session("/repo/.worktrees/ai", "/repo/src");
        let rows = sidebar(&model, &identities_fixture()).rows;
        let name_row = rows
            .iter()
            .find(|row| row.contains("Agent"))
            .expect("the agent is named in the tree");
        assert!(
            name_row.contains('\u{25cb}') || name_row.contains('\u{25cf}'),
            "the status glyph still leads: {name_row}"
        );
        assert_eq!(
            caption_color_of(&model, "Agent", "agent"),
            theme::color(Token::StateWarning),
            "a slot is no exception — the selected agent wears the hue"
        );
        assert_eq!(
            caption_color_of(&model, "Second", "agent"),
            theme::color(Token::TextDim),
            "and every other agent stays dim, slot or not"
        );

        select_second_agent(&mut model);
        assert_eq!(
            caption_color_of(&model, "Agent", "agent"),
            theme::color(Token::TextDim)
        );
        assert_eq!(
            caption_color_of(&model, "Second", "agent"),
            theme::color(Token::StateWarning)
        );
    }

    /// An agent outside any slot reads as one inside it — what runs
    /// there, and nothing about where. The branch its directory was
    /// evaluated on belongs to the space, which says it once.
    #[test]
    fn an_agent_outside_any_slot_reads_as_one_inside_it() {
        let mut model = agent_session_in("/repo/src");
        model
            .remembered
            .branches
            .insert(PathBuf::from("/repo/src"), "feature/x".into());
        let Sidebar { rows, hits, .. } = sidebar(&model, &identities_fixture());
        let caption = &rows[agent_rows(&hits)[1] as usize];
        assert!(caption.contains("agent"), "what runs there: {caption}");
        assert!(
            !caption.contains("feature/x") && !caption.contains("/repo/src"),
            "neither the branch nor the directory: {caption}"
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
        model
            .remembered
            .branches
            .insert(PathBuf::from("/repo"), "main".into());
        model
            .remembered
            .upstream_syncs
            .insert(PathBuf::from("/repo"), UpstreamSync { pull: 1, push: 12 });
        let rows = sidebar(&model, &identities_fixture()).rows;
        let agent = rows
            .iter()
            .position(|row| row.contains("Agent"))
            .expect("the agent's row");
        let caption = &rows[agent + 1];
        assert!(
            caption.contains("agent"),
            "the harness captions the agent: {caption:?}"
        );
        assert!(
            caption.ends_with("\u{21e3}\u{2081} \u{21e1}\u{2081}\u{2082} \u{2502}"),
            "⇣₁ ⇡₁₂ sit at the right edge, one pad off the divider: {caption:?}"
        );
        assert_eq!(
            caption_color_of(&model, "Agent", "agent"),
            theme::color(Token::StateWarning)
        );
        assert_eq!(
            caption_color_of(&model, "Agent", "\u{21e3}"),
            theme::color(Token::StateDanger)
        );
        assert_eq!(
            caption_color_of(&model, "Agent", "\u{21e1}"),
            theme::color(Token::StateSuccess)
        );

        model
            .remembered
            .upstream_syncs
            .insert(PathBuf::from("/repo"), UpstreamSync { pull: 0, push: 3 });
        let rows = sidebar(&model, &identities_fixture()).rows;
        let agent = rows.iter().position(|row| row.contains("Agent")).unwrap();
        let caption = &rows[agent + 1];
        assert!(
            !caption.contains('\u{21e3}') && caption.ends_with("\u{21e1}\u{2083} \u{2502}"),
            "nothing to pull, three to push: {caption:?}"
        );

        model
            .remembered
            .upstream_syncs
            .insert(PathBuf::from("/repo"), UpstreamSync::default());
        let rows = sidebar(&model, &identities_fixture()).rows;
        let agent = rows.iter().position(|row| row.contains("Agent")).unwrap();
        let caption = &rows[agent + 1];
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
            .remembered
            .upstream_syncs
            .insert(PathBuf::from("/repo"), UpstreamSync { pull: 2, push: 2 });
        let rows = sidebar(&model, &identities_fixture()).rows;
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

        let rows = sidebar(&model, &identities_fixture()).rows;
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
            tab.pane.process = "agent".into();
        }

        let requests = adopt_agent_labels(&mut model, &identities_fixture());
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
            adopt_agent_labels(&mut model, &identities_fixture()).is_empty(),
            "each tab is asked once"
        );

        let session = model.session.as_mut().unwrap();
        assert!(session.rename_tab(TabId(2), "agent 2".into()));
        assert!(session.rename_tab(TabId(4), "agent 3".into()));
        assert!(adopt_agent_labels(&mut model, &identities_fixture()).is_empty());
        assert!(
            model.remembered.label_adoptions.is_empty(),
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
        assert!(adopt_agent_labels(&mut model, &identities_fixture()).is_empty());
    }

    /// The `new` prompt is a chooser, not a text field: the sidebar
    /// itself lists the directories the typed segment still matches, and
    /// clicking one is the same choice Enter makes.
    #[test]
    fn the_new_space_prompt_lists_the_directories_it_matches() {
        let root = uze_testkit::temp::TempDir::new("sidebar-root-picker");
        for directory in ["engine", "extensions", "docs"] {
            std::fs::create_dir_all(root.join(directory)).unwrap();
        }
        let mut model = agent_session_in("/repo");
        model.root_picker = Some(RootPicker::opened_in(
            &root.path().display().to_string(),
            None,
        ));

        let Sidebar { rows, hits, .. } = sidebar(&model, &identities_fixture());
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
        let rows = sidebar(&model, &identities_fixture()).rows;
        assert!(
            rows.iter().any(|row| row.contains("extensions")),
            "what matches stays: {rows:?}"
        );
        assert!(
            !rows.iter().any(|row| row.contains("docs")),
            "what stopped matching is gone: {rows:?}"
        );
    }

    /// One grid down the column: a section at the foot folds from the
    /// column a space folds from and names itself in the column a space
    /// names itself in, with a row of air between the tree and the foot so
    /// a tree that grows to meet it still reads as two things.
    #[test]
    fn the_foot_sections_stand_on_the_columns_own_grid() {
        let mut model = three_spaces();
        model.first_steps_collapsed = false;
        let Sidebar { rows, hits, .. } = sidebar(&model, &identities_fixture());
        let column = |row: &str, text: &str| row[..row.find(text).unwrap()].chars().count();
        let header = space_header(&hits, SpaceId(1)).y as usize;
        let steps = rows
            .iter()
            .position(|row| row.contains("first steps"))
            .expect("the steps are at the foot");

        assert_eq!(
            column(&rows[steps], "first steps"),
            column(&rows[header], "one"),
            "a section names itself where a space does: {rows:?}"
        );
        assert_eq!(
            column(
                &rows[steps],
                &theme::glyph(crate::ui::theme::Symbol::ChevronExpanded)
            ),
            column(
                &rows[header],
                &theme::glyph(crate::ui::theme::Symbol::ChevronExpanded)
            ),
            "and folds from the column a space folds from: {rows:?}"
        );
        assert!(
            rows[steps - 1].trim_end_matches('│').trim().is_empty(),
            "a row of air over the foot: {rows:?}"
        );
    }

    /// The picker is a small table: the kinds as one control over what is
    /// being typed and the directories it matches, all in one column, with
    /// the way out at the right edge of the row it opens on.
    /// A name the query is the head of says so in the query's own hue; one
    /// matched further in is left alone.
    #[test]
    fn the_picker_lines_its_rows_up_in_columns() {
        let root = uze_testkit::temp::TempDir::new("sidebar-picker-grid");
        for directory in ["craude", "cortex", "scribble"] {
            std::fs::create_dir_all(root.join(directory)).unwrap();
        }
        let mut model = agent_session_in("/repo");
        let mut picker = RootPicker::opened_in(&root.path().display().to_string(), None);
        for character in "cr".chars() {
            picker.typed(character);
        }
        model.root_picker = Some(picker);

        let Sidebar { rows, buffer, .. } = sidebar(&model, &identities_fixture());
        let kind = rows
            .iter()
            .position(|row| row.contains("workspace"))
            .expect("the kind is named");
        let column = |row: &str, text: &str| row[..row.find(text).unwrap()].chars().count();
        // The kinds lead their row, then what is typed and the directories
        // it matches, all in one column.
        let values = column(&rows[kind], "workspace");
        assert_eq!(column(&rows[kind + 1], "cr"), values, "{rows:?}");
        for offset in 2..=3 {
            let row = &rows[kind + offset];
            let name = row.trim_start();
            assert_eq!(
                column(row, name.split(' ').next().unwrap()),
                values,
                "every row answers in one column: {rows:?}"
            );
        }
        assert!(
            rows[kind]
                .trim_end_matches('│')
                .trim_end()
                .ends_with(&theme::glyph(crate::ui::theme::Symbol::ArrowSwap)),
            "the control that flips the kind ends its row: {rows:?}"
        );
        assert_eq!(
            buffer[(0, kind as u16 + 1)].fg,
            theme::color(Token::SpaceWorkspace),
            "the row being typed into is marked in the hue of the kind it \
             would create"
        );

        // "craude" is what "cr" is the head of; "scribble" matched further in.
        let head = |row: usize, at: u16| buffer[(values as u16 + at, row as u16)].fg;
        assert_eq!(head(kind + 2, 0), theme::color(Token::Accent), "{rows:?}");
        assert_eq!(
            head(kind + 3, 0),
            theme::color(Token::TextInactive),
            "{rows:?}"
        );
    }

    /// The control that flips the kind answers the click: the picker stays
    /// open, and a flip the root cannot honour leaves it open too — a
    /// control the click falls through closes the prompt instead.
    #[test]
    fn clicking_the_kind_control_flips_it_and_keeps_the_picker_open() {
        let home = UzeHome::at(uze_testkit::temp::scratch("picker-kind-click"));
        let root = uze_testkit::temp::TempDir::new("sidebar-picker-kind-click");
        std::fs::create_dir_all(root.join("plain")).unwrap();
        let mut model = agent_session_in("/repo");
        model.root_picker = Some(RootPicker::opened_in(
            &root.path().display().to_string(),
            None,
        ));
        let mut driven = driven(model, &home);
        driven.frame();
        let control = driven.hit(|hit| matches!(hit, WorkspaceHit::PickSpaceKind(_)));

        driven.press(control.x, control.y);

        let picker = driven
            .attach
            .model
            .root_picker
            .as_ref()
            .expect("the picker is still open");
        assert_eq!(picker.kind(), uze_terminal::SpaceKind::Worktree);

        driven.frame();
        let control = driven.hit(|hit| matches!(hit, WorkspaceHit::PickSpaceKind(_)));
        driven.press(control.x, control.y);

        let picker = driven
            .attach
            .model
            .root_picker
            .as_ref()
            .expect("and open again after flipping back");
        assert_eq!(picker.kind(), uze_terminal::SpaceKind::Workspace);
    }

    /// A listing taller than the column is reached with the wheel too: it
    /// walks the directories the way the arrow keys do, and the window
    /// follows the one selected.
    #[test]
    fn the_wheel_walks_the_directories_the_picker_offers() {
        let home = UzeHome::at(uze_testkit::temp::scratch("picker-wheel"));
        let root = uze_testkit::temp::TempDir::new("sidebar-picker-wheel");
        for index in 0..40 {
            std::fs::create_dir_all(root.join(format!("directory-{index:02}"))).unwrap();
        }
        let mut model = agent_session_in("/repo");
        model.root_picker = Some(RootPicker::opened_in(
            &root.path().display().to_string(),
            None,
        ));
        let mut driven = driven(model, &home);
        driven.frame();
        let last_row = |driven: &Driven<'_>| {
            let rows = sidebar(&driven.attach.model, &identities_fixture()).rows;
            rows.iter()
                .rev()
                .find(|row| row.contains("directory-"))
                .cloned()
                .expect("the listing is drawn")
        };
        let before = last_row(&driven);

        for _ in 0..25 {
            driven.mouse(1, 6, MouseEventKind::ScrollDown);
        }

        let picker = driven
            .attach
            .model
            .root_picker
            .as_ref()
            .expect("still open");
        assert_eq!(
            picker.selection(),
            Some(24),
            "the first turn takes the row in front, and the rest walk it"
        );
        assert_ne!(last_row(&driven), before, "and the window follows it");

        for _ in 0..40 {
            driven.mouse(1, 6, MouseEventKind::ScrollUp);
        }
        let picker = driven
            .attach
            .model
            .root_picker
            .as_ref()
            .expect("still open");
        assert_eq!(picker.selection(), Some(0), "and stops at the top");
    }

    /// The picker asks in the order it is answered — what kind of space,
    /// then where — standing where the spaces it would join stand, with the
    /// directories directly under it and no blank row wedged between them.
    #[test]
    fn the_prompt_stands_where_the_spaces_it_would_join_stand() {
        let root = uze_testkit::temp::TempDir::new("sidebar-root-place");
        for directory in ["engine", "extensions"] {
            std::fs::create_dir_all(root.join(directory)).unwrap();
        }
        let mut model = agent_session_in("/repo");
        let rows = sidebar(&model, &identities_fixture()).rows;
        let spaces_row = rows
            .iter()
            .position(|row| row.contains("repo"))
            .expect("the first space's header is drawn");

        model.root_picker = Some(RootPicker::opened_in(
            &root.path().display().to_string(),
            None,
        ));
        let Sidebar { rows, buffer, .. } = sidebar(&model, &identities_fixture());
        let kind_row = rows
            .iter()
            .position(|row| row.contains("workspace"))
            .expect("the kind is named");

        assert_eq!(
            kind_row, spaces_row,
            "the prompt stands where the spaces do: {rows:?}"
        );
        assert!(
            rows[kind_row + 1].contains(&theme::glyph(crate::ui::theme::Symbol::CursorText)),
            "the prompt follows the kinds: {rows:?}"
        );
        assert!(
            rows[kind_row + 2].contains("engine"),
            "and the listing starts under them: {rows:?}"
        );
        // The panel on the darker of the two surfaces, and the two rows the
        // keyboard answers to — what is typed, and where it has landed —
        // lifted onto the lighter one.
        let surface = |row: usize| buffer[(2, row as u16)].bg;
        assert_eq!(
            surface(kind_row),
            theme::color(Token::SurfaceRaisedSubtle),
            "the kinds are part of the panel: {rows:?}"
        );
        assert_eq!(
            surface(kind_row + 1),
            theme::color(Token::SurfaceRaised),
            "what is typed is lifted: {rows:?}"
        );
        // Nothing is chosen in the listing yet, so no row of it is lifted.
        for row in [kind_row + 2, kind_row + 3] {
            assert_eq!(
                surface(row),
                theme::color(Token::SurfaceRaisedSubtle),
                "the listing is the panel: {rows:?}"
            );
        }
    }

    /// The prompt opens with nothing in it: the directory it is rooted at
    /// is the row's own context, not text waiting to be deleted.
    #[test]
    fn the_prompt_opens_empty_over_the_directory_it_is_rooted_at() {
        let mut model = agent_session_in("/repo");
        model.root_picker = Some(RootPicker::opened_in("~", None));

        let rows = sidebar(&model, &identities_fixture()).rows;
        let cursor = theme::glyph(crate::ui::theme::Symbol::CursorText);
        let prompt = rows
            .iter()
            .find(|row| row.contains(&cursor))
            .expect("the prompt row is drawn");
        let typed = prompt
            .trim_start()
            .trim_start_matches(&theme::glyph(crate::ui::theme::Symbol::TreeVertical))
            .trim_start();
        assert!(
            typed.starts_with(&cursor),
            "nothing is typed for the operator: {prompt}"
        );
        assert!(
            prompt.contains('~'),
            "and the row says where it is: {prompt}"
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

        model.root_picker = Some(RootPicker::opened_in(
            &root.path().display().to_string(),
            None,
        ));
        let rows = sidebar(&model, &identities_fixture()).rows;
        assert!(
            rows.iter().any(|row| row.contains("engine")),
            "the directories are what is on offer: {rows:?}"
        );
        assert!(
            !rows.iter().any(|row| row.contains("Agent")),
            "the agents step aside: {rows:?}"
        );

        model.root_picker = None;
        let rows = sidebar(&model, &identities_fixture()).rows;
        assert!(
            rows.iter().any(|row| row.contains("Agent")),
            "and come back when it closes: {rows:?}"
        );
    }

    /// A root several levels deep is longer than the sidebar is wide, so
    /// its head gives way — and the moment something is typed the line is
    /// only that. The root stood pinned to the right of the line all the
    /// way through, saying where the prompt was in a second place; the
    /// two could disagree, and the one that went stale was the pinned
    /// half.
    #[test]
    fn a_long_root_gives_way_and_then_gives_the_line_over_to_what_is_typed() {
        let root = uze_testkit::temp::TempDir::new("sidebar-root-elide");
        std::fs::create_dir_all(root.join("a-very-long-directory-name/inner")).unwrap();
        let mut model = agent_session_in("/repo");
        let cursor = theme::glyph(crate::ui::theme::Symbol::CursorText);
        let prompt_row = |model: &WorkspaceModel| {
            sidebar(model, &identities_fixture())
                .rows
                .into_iter()
                .find(|row| row.contains(&cursor))
                .expect("the prompt row is drawn")
        };

        let mut picker = RootPicker::opened_in(
            &root
                .join("a-very-long-directory-name")
                .display()
                .to_string(),
            None,
        );
        model.root_picker = Some(picker);
        let prompt = prompt_row(&model);
        assert!(
            prompt.contains('\u{2026}'),
            "where the typing starts from, head first to give way: {prompt}"
        );

        picker = model.root_picker.take().expect("the prompt is open");
        for character in "inn".chars() {
            picker.typed(character);
        }
        model.root_picker = Some(picker);
        let prompt = prompt_row(&model);
        assert!(prompt.contains("inn\u{258f}"), "{prompt}");
        assert!(
            !prompt.contains('\u{2026}'),
            "and the root is not repeated beside what was typed: {prompt}"
        );
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

        model.note_agent_prompt_submission(PaneId(1), &identities_fixture(), Some("hello"));
        assert_eq!(
            model.agent_tab_status(PaneId(1), true),
            AgentTabStatus::Working
        );

        model.remembered.agent_activity.remove(&PaneId(1));
        model.remembered.completed_agent_panes.insert(PaneId(1));
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
        assert_eq!(AgentTabStatus::Idle.color(), theme::color(Token::TextFaint));
        assert_eq!(
            AgentTabStatus::Selected.color(),
            theme::color(Token::Accent)
        );
    }

    #[test]
    fn a_submitted_agent_prompt_works_until_its_pane_goes_quiet() {
        let mut model = agent_session();
        model.note_agent_prompt_submission(PaneId(1), &identities_fixture(), Some("hello"));
        assert_eq!(
            model.agent_tab_status(PaneId(1), false),
            AgentTabStatus::Working
        );
        assert!(workspace_has_active_agent_operation(
            &model,
            &identities_fixture()
        ));

        assert!(!model.expire_agent_activity(Instant::now() + Duration::from_secs(1)));
        assert_eq!(
            model.agent_tab_status(PaneId(1), false),
            AgentTabStatus::Working
        );

        assert!(model.expire_agent_activity(Instant::now() + Duration::from_secs(4)));
        assert!(!workspace_has_active_agent_operation(
            &model,
            &identities_fixture()
        ));
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

        model.apply(painted(PaneId(1)), &identities_fixture());
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
                &identities_fixture(),
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
                &identities_fixture(),
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
            model.apply(repainted_whole_grid(PaneId(1)), &identities_fixture());
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
        model.note_agent_prompt_submission(PaneId(1), &identities_fixture(), Some("hello"));
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
            model.note_agent_output(
                PaneId(1),
                &identities_fixture(),
                typed + Duration::from_millis(10),
            );
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
        model.note_agent_prompt_submission(PaneId(1), &identities_fixture(), Some("hello"));
        for step in 0..4 * AGENT_BUSY_REPAINTS as u64 {
            let typed = start + Duration::from_millis(120 * step);
            model.open_echo_window(PaneId(1), typed, AGENT_ECHO_GRACE);
            model.note_agent_output(
                PaneId(1),
                &identities_fixture(),
                typed + Duration::from_millis(10),
            );
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
        model.note_agent_prompt_submission(PaneId(1), &identities_fixture(), Some("hello"));

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
        let mut model = model_of(session("/tmp", uze_terminal::SpaceKind::Worktree));
        model.note_agent_prompt_submission(PaneId(1), &identities_fixture(), Some("hello"));
        animate(&mut model, PaneId(1), Instant::now());
        assert!(model.remembered.agent_activity.is_empty());
    }

    #[test]
    fn completed_background_agent_keeps_a_check_until_its_tab_is_opened() {
        let mut session = session("/tmp", uze_terminal::SpaceKind::Worktree);
        let agent_pane = session.add_tab(
            session.workspace.selected_space,
            "Agent".into(),
            None,
            80,
            24,
            "/tmp".into(),
        );
        session.update_pane_status(agent_pane, "/tmp".into(), "agent".into());
        let agent_tab = session.workspace.spaces[0].selected_tab;
        session.workspace.spaces[0].selected_tab = TabId(1);
        let mut model = model_of(session);
        model.note_agent_prompt_submission(agent_pane, &identities_fixture(), Some("hello"));
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
        let mut session = session("/tmp", uze_terminal::SpaceKind::Worktree);
        let agent_pane = session.add_tab(
            session.workspace.selected_space,
            "Agent".into(),
            None,
            80,
            24,
            "/tmp".into(),
        );
        session.update_pane_status(agent_pane, "/tmp".into(), "agent".into());
        let agent_tab = session.workspace.spaces[0].selected_tab;
        session.workspace.spaces[0].selected_tab = TabId(1);
        let mut model = model_of(session);
        model.note_agent_prompt_submission(agent_pane, &identities_fixture(), Some("hello"));
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
        let mut session = session("/tmp", uze_terminal::SpaceKind::Worktree);
        let agent_pane = session.add_tab(
            session.workspace.selected_space,
            "Agent".into(),
            None,
            80,
            24,
            "/tmp".into(),
        );
        session.update_pane_status(agent_pane, "/tmp".into(), "agent".into());
        let agent_tab = session.workspace.spaces[0].selected_tab;
        session.workspace.spaces[0].selected_tab = TabId(1);
        let mut model = model_of(session);
        model.note_agent_prompt_submission(agent_pane, &identities_fixture(), Some("hello"));
        model.note_pane_input(agent_pane);

        if let Some(session) = model.session.as_mut() {
            session.remove_tab(agent_tab);
        }
        model.expire_agent_activity(Instant::now());
        assert!(model.remembered.agent_activity.is_empty());
        assert!(model.remembered.completed_agent_panes.is_empty());
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
                launch: std::path::PathBuf::from("/uze/shims/claude"),
                continuity_gap: None,
            },
            AgentIdentity {
                binary: "codex",
                integration: "codex",
                display_name: "Codex",
                launch: std::path::PathBuf::from("codex"),
                continuity_gap: Some("no launcher".to_owned()),
            },
        ]
    }

    /// An agent launched through UZE's own launcher is still the same agent
    /// on the tab: what a pane is recognized by is the process it is
    /// running, which the launcher preserves, and never the path it was
    /// started from.
    #[test]
    fn launching_through_the_launcher_leaves_the_pane_recognizable() {
        let tab = tab_with("agent 1", "claude");
        assert_eq!(agent_identity_for_tab(&identities(), &tab), Some("claude"));
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
            env: Vec::new(),
            pane,
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
        let mut session = session("/tmp", uze_terminal::SpaceKind::Worktree);
        let model = model_of(session.clone());
        assert_eq!(next_agent_label(&model), "agent 1");

        session.add_tab(
            session.workspace.selected_space,
            "agent 1".into(),
            None,
            80,
            24,
            "/tmp".into(),
        );
        let model = model_of(session);
        assert_eq!(next_agent_label(&model), "agent 2");
    }

    #[test]
    fn a_lone_agent_can_close_when_it_is_replaced_by_a_shell() {
        let mut session = session("/tmp", uze_terminal::SpaceKind::Worktree);
        session.workspace.spaces[0].tabs[0].label = "Claude Code".into();
        session.workspace.spaces[0].tabs[0].pane.process = "claude".into();
        let tab = session.workspace.spaces[0].selected_tab;
        let model = model_of(session);

        assert!(tab_needs_replacement_shell(&model, &identities(), tab));
        assert!(can_close_tab_from_menu(&model, &identities(), tab));
    }

    #[test]
    fn a_lone_plain_shell_stays_non_closable() {
        let session = session("/tmp", uze_terminal::SpaceKind::Worktree);
        let tab = session.workspace.spaces[0].selected_tab;
        let model = model_of(session);

        assert!(!tab_needs_replacement_shell(&model, &identities(), tab));
        assert!(!can_close_tab_from_menu(&model, &identities(), tab));
    }

    /// A space always keeps a shell of its own: closing the last one beside
    /// its agents opens another first, while a shell with a sibling of its
    /// kind, or an agent whose own shells become the space's, needs none.
    #[test]
    fn closing_a_spaces_last_own_shell_is_replaced_and_no_other_close_is() {
        let session_of_one_agent = || {
            let mut solo = session("/tmp", uze_terminal::SpaceKind::Worktree);
            solo.workspace.spaces[0].tabs[0].pane.process = "claude".into();
            solo
        };
        let mut session = session("/tmp", uze_terminal::SpaceKind::Worktree);
        let space = session.workspace.selected_space;
        let shell = session.workspace.spaces[0].tabs[0].id;
        let pane = session.add_tab(space, "Claude Code".into(), None, 80, 24, "/tmp".into());
        session.update_pane_status(pane, "/tmp".into(), "claude".into());
        let agent = session.workspace.spaces[0].tabs[1].id;
        let model = model_of(session.clone());
        assert!(
            tab_needs_replacement_shell(&model, &identities(), shell),
            "the space's only shell of its own"
        );
        assert!(
            !tab_needs_replacement_shell(&model, &identities(), agent),
            "the agent goes, the shell stays"
        );

        session.add_tab(space, "shell 2".into(), None, 80, 24, "/tmp".into());
        let model = model_of(session.clone());
        assert!(
            !tab_needs_replacement_shell(&model, &identities(), shell),
            "another shell of its own remains"
        );

        let mut solo = session_of_one_agent();
        let lone = solo.workspace.spaces[0].tabs[0].id;
        let space = solo.workspace.selected_space;
        solo.add_tab(space, "shell 2".into(), Some(lone), 80, 24, "/tmp".into());
        let model = model_of(solo);
        assert!(
            !tab_needs_replacement_shell(&model, &identities(), lone),
            "the agent's own shell becomes the space's when it goes"
        );
    }

    /// Closed from the keyboard, the last shell of a space's own is
    /// replaced before it goes, so the header still has somewhere to land.
    #[test]
    fn the_close_chord_on_a_spaces_last_shell_opens_another_first() {
        let home = UzeHome::at(uze_testkit::temp::scratch("orchestrator-last-shell"));
        let mut session = session("/repo", uze_terminal::SpaceKind::Worktree);
        let space = session.workspace.selected_space;
        let shell = session.workspace.spaces[0].tabs[0].id;
        let pane = session.add_tab(space, "agent 1".into(), None, 80, 24, "/repo".into());
        session.update_pane_status(pane, "/repo".into(), "agent".into());
        session.workspace.spaces[0].selected_tab = shell;
        let mut driven = driven(model_of(session), &home);
        let close = uze_keys::active()
            .chord_for(uze_keys::Action::CloseTab, &[uze_keys::Scope::Workspace])
            .expect("closing a tab is reachable from the keyboard");

        driven.press_key(key_event(close));

        let sent = driven.sent();
        let opened = sent.iter().position(|request| {
            matches!(
                request,
                ClientRequest::CreateTab {
                    agent: None,
                    command: None,
                    ..
                }
            )
        });
        let closed = sent.iter().position(
            |request| matches!(request, ClientRequest::CloseTab { tab } if *tab == shell),
        );
        assert!(
            matches!((opened, closed), (Some(opened), Some(closed)) if opened < closed),
            "a shell of its own opens before the last one closes: {sent:?}"
        );
    }

    #[test]
    fn a_plain_shell_matches_neither_signal() {
        let tab = tab_with("shell", "zsh");
        assert_eq!(agent_identity_for_tab(&identities(), &tab), None);
    }

    #[test]
    fn new_tabs_use_the_selected_panes_live_directory() {
        let mut session = session("/tmp/root", uze_terminal::SpaceKind::Worktree);
        assert!(session.update_pane_status(PaneId(1), "/tmp/project/src".into(), "zsh".into()));
        let model = model_of(session);

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
            session: Some(session("/tmp", uze_terminal::SpaceKind::Worktree)),
            panes: [(PaneId(1), plain)].into(),
            ..WorkspaceModel::default()
        };
        let mut stream = Vec::new();
        forward_paste(&mut stream, &plain_model, "hello");
        assert_eq!(decode_input_bytes(&stream), b"hello".to_vec());

        let mut bracketed = blank_pane(PaneId(1), 80, 24);
        bracketed.bracketed_paste = true;
        let bracketed_model = WorkspaceModel {
            session: Some(session("/tmp", uze_terminal::SpaceKind::Worktree)),
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
            session: Some(session("/tmp", uze_terminal::SpaceKind::Worktree)),
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
            session: Some(session("/tmp", uze_terminal::SpaceKind::Worktree)),
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
            .remembered
            .branches
            .insert(PathBuf::from("/repo"), "agent/ai".to_owned());
        model.remembered.completed_agent_panes.insert(PaneId(1));
        model.error = Some("stale".to_owned());
        model
            .hits
            .push((Rect::new(0, 0, 1, 1), WorkspaceHit::NewSpace));

        let model = WorkspaceModel {
            remembered: model.remembered,
            ..WorkspaceModel::default()
        };

        assert_eq!(model.remembered.tasks[&PathBuf::from("/repo")].len(), 1);
        assert_eq!(
            model
                .remembered
                .branches
                .get(&PathBuf::from("/repo"))
                .map(String::as_str),
            Some("agent/ai")
        );
        assert!(model.remembered.completed_agent_panes.contains(&PaneId(1)));
        assert!(model.session.is_none());
        assert!(model.error.is_none());
        assert!(model.hits.is_empty());
    }

    /// One attached client, driven the way the real loop drives it: hits
    /// from a real frame, a socket pair standing in for the server, and
    /// the channels a background read answers through.
    struct Driven<'a> {
        attach: Attach<'a>,
        server: std::os::unix::net::UnixStream,
        events: std::sync::mpsc::Receiver<ClientEvent>,
        /// The reader thread's end, held so the channel stays connected.
        /// Dropping it is exactly what the real reader does when the
        /// socket stops answering, which is how this client learns the
        /// terminal server is gone.
        events_sender: Option<std::sync::mpsc::Sender<ClientEvent>>,
    }

    impl Driven<'_> {
        /// Draws the frame the next click is tested against, storing its
        /// hits on the model exactly as the attach loop does.
        fn frame(&mut self) {
            full_frame(&mut self.attach.model);
        }

        fn press(&mut self, column: u16, row: u16) {
            self.mouse(column, row, MouseEventKind::Down(MouseButton::Left));
        }

        /// Any other mouse event at the same viewport the click helpers
        /// use — the rest of a drag, which `press` alone cannot say.
        fn mouse(&mut self, column: u16, row: u16, kind: MouseEventKind) {
            let area = Rect::new(0, 0, 80, 24);
            let layout = compute_layout(area, self.attach.model.sidebar_width);
            let viewport = Viewport {
                size: ratatui::layout::Size::new(area.width, area.height),
                columns: layout.pane.width,
                rows: layout.pane.height,
                layout,
            };
            let event = crossterm::event::Event::Mouse(mouse_at(column, row, kind));
            let _ = self.attach.handle(event, &viewport);
        }

        /// One key, through the same dispatch the attach loop uses.
        fn press_key(&mut self, key: crossterm::event::KeyEvent) {
            let area = Rect::new(0, 0, 80, 24);
            let layout = compute_layout(area, self.attach.model.sidebar_width);
            let viewport = Viewport {
                size: ratatui::layout::Size::new(area.width, area.height),
                columns: layout.pane.width,
                rows: layout.pane.height,
                layout,
            };
            let _ = self
                .attach
                .handle(crossterm::event::Event::Key(key), &viewport);
        }

        /// One turn of everything that is not an event — what absorbs a
        /// placement once its thread has answered.
        fn pump(&mut self) -> Flow {
            self.attach.pump(&self.events)
        }

        /// The terminal server exiting under the client.
        fn runtime_gone(&mut self) {
            self.events_sender = None;
        }

        /// Every request written to the server since the last read.
        fn sent(&mut self) -> Vec<ClientRequest> {
            self.server.set_nonblocking(true).unwrap();
            let mut buffer = Vec::new();
            let mut chunk = [0u8; 8192];
            while let Ok(read) = std::io::Read::read(&mut self.server, &mut chunk) {
                if read == 0 {
                    break;
                }
                buffer.extend_from_slice(&chunk[..read]);
            }
            let mut requests = Vec::new();
            let mut rest = buffer.as_slice();
            while rest.len() >= 4 {
                let (length, payload) = rest.split_at(4);
                let length = u32::from_le_bytes(length.try_into().unwrap()) as usize;
                assert!(payload.len() >= length, "a whole frame");
                requests.push(bincode::deserialize(&payload[..length]).expect("a request"));
                rest = &payload[length..];
            }
            requests
        }

        /// Hands one already-received placement back to the client, the
        /// way the loop's own `pump` absorbs it.
        fn placements_answered(&mut self, resolution: PlacementResolution) {
            self.attach
                .channels
                .placements
                .sender
                .send(resolution)
                .unwrap();
            self.pump();
        }

        /// The rect of the one hit of its kind the last frame drew.
        fn hit(&self, wanted: impl Fn(&WorkspaceHit) -> bool) -> Rect {
            let found: Vec<Rect> = self
                .attach
                .model
                .hits
                .iter()
                .filter(|(_, hit)| wanted(hit))
                .map(|(rect, _)| *rect)
                .collect();
            assert_eq!(found.len(), 1, "exactly one such hit: {found:?}");
            found[0]
        }
    }

    fn driven(model: WorkspaceModel, home: &UzeHome) -> Driven<'_> {
        let (client, server) = std::os::unix::net::UnixStream::pair().unwrap();
        let (events, events_rx) = std::sync::mpsc::channel();
        Driven {
            attach: Attach {
                model,
                stream: client,
                home,
                identities: identities_fixture(),
                // Leaked like the management memory below: the attach
                // borrows its channels for as long as it lives.
                channels: Box::leak(Box::default()),
                spinner: indicatif::ProgressBar::hidden(),
                next_tick: Instant::now(),
                asked_for_a_tab: false,
                // Leaked on purpose: the attach borrows the memory for as
                // long as it lives, and a test's lives until the process
                // does.
                manage_memory: Box::leak(Box::new(
                    crate::ui::management::ManagementMemory::unresolved(),
                )),
                keyboard: crate::ui::keys::KeyboardSupport::default(),
            },
            server,
            events: events_rx,
            events_sender: Some(events),
        }
    }

    /// A space's own row lands on a shell of the space's, not on whichever
    /// agent the strip was showing: it is the way back to the space's
    /// shells. A space of nothing but agents has no such tab, so the click
    /// is a plain switch.
    #[test]
    fn a_space_row_lands_on_its_own_shell_and_otherwise_switches_the_space() {
        let home = UzeHome::at(uze_testkit::temp::scratch("orchestrator-space-row"));
        let click_space_row = |model: WorkspaceModel| {
            let mut driven = driven(model, &home);
            driven.frame();
            let (row, space) = driven
                .attach
                .model
                .hits
                .iter()
                .find_map(|(rect, hit)| match hit {
                    WorkspaceHit::SelectSpace(space) => Some((*rect, *space)),
                    _ => None,
                })
                .expect("the space row is a target");
            driven.press(row.x + 4, row.y);
            let sent = driven.sent();
            (space, sent)
        };

        let mut with_shell = session("/repo", uze_terminal::SpaceKind::Worktree);
        let space = with_shell.selected_space().id;
        let shell = with_shell.selected_tab().id;
        let agent_pane = with_shell.add_tab(
            space,
            "agent 1".into(),
            None,
            80,
            24,
            PathBuf::from("/repo"),
        );
        with_shell.update_pane_status(agent_pane, PathBuf::from("/repo"), "agent".into());
        let (_, sent) = click_space_row(model_of(with_shell));
        assert!(
            sent.iter().any(
                |request| matches!(request, ClientRequest::SelectTab { tab } if *tab == shell)
            ),
            "the space's own shell was not selected: {sent:?}"
        );

        let mut only_agents = session("/repo", uze_terminal::SpaceKind::Worktree);
        let pane = only_agents.selected_tab().pane.id;
        only_agents.update_pane_status(pane, PathBuf::from("/repo"), "agent".into());
        let (space, sent) = click_space_row(model_of(only_agents));
        assert!(
            sent.iter()
                .any(|request| matches!(request, ClientRequest::SelectSpace { space: selected } if *selected == space)),
            "a space of agents alone was not switched to: {sent:?}"
        );
    }

    /// Three worktree spaces, `one`, `two` and `three`, one agent each; the
    /// last one created is selected.
    fn three_spaces() -> WorkspaceModel {
        let mut session = session("/one", uze_terminal::SpaceKind::Worktree);
        let pane = session.selected_tab().pane.id;
        session.update_pane_status(pane, "/one".into(), "agent".into());
        for name in ["two", "three"] {
            let root = PathBuf::from(format!("/{name}"));
            session.create_space(
                Some(name.into()),
                uze_terminal::SpaceSeat {
                    root: root.clone(),
                    kind: uze_terminal::SpaceKind::Worktree,
                },
                80,
                24,
            );
            let pane = session.selected_tab().pane.id;
            session.update_pane_status(pane, root, "agent".into());
        }
        model_of(session)
    }

    /// The header row a space was drawn at, by the frame's own hits.
    fn space_header(hits: &[(Rect, WorkspaceHit)], wanted: SpaceId) -> Rect {
        hits.iter()
            .filter(|(_, hit)| matches!(hit, WorkspaceHit::SelectSpace(space) if *space == wanted))
            .map(|(rect, _)| *rect)
            .min_by_key(|rect| rect.y)
            .expect("the space has a header")
    }

    fn agent_rows_of(
        model: &WorkspaceModel,
        hits: &[(Rect, WorkspaceHit)],
        space: SpaceId,
    ) -> usize {
        let session = model.session.as_ref().unwrap();
        let space = session
            .workspace
            .spaces
            .iter()
            .find(|candidate| candidate.id == space)
            .unwrap();
        hits.iter()
            .filter(|(_, hit)| {
                matches!(hit, WorkspaceHit::SelectTab(tab) if space.tabs.iter().any(|candidate| candidate.id == *tab))
            })
            .count()
    }

    /// Every row of a space hangs off one muted gutter down its leading
    /// column, and a worktree space's items branch straight off it, at no
    /// level of indent; only the selected agent's stretch of it is heavier
    /// and in the accent.
    #[test]
    fn a_space_is_one_block_down_its_gutter() {
        let mut model = three_spaces();
        // All three spaces in the column, with the steps folded at the foot.
        model.first_steps_collapsed = true;
        let Sidebar {
            rows, hits, buffer, ..
        } = sidebar(&model, &identities_fixture());
        use crate::ui::theme::{Symbol, Token};
        let muted = theme::color(Token::TextMuted);
        let accent = theme::color(Token::Accent);
        // Space 1 is in the background; space 3 is selected, on its agent.
        for (space, branch, caption, hues) in [
            (
                SpaceId(1),
                Symbol::TreeBranch,
                Symbol::TreeVertical,
                [muted, muted, muted],
            ),
            (
                SpaceId(3),
                Symbol::TreeBranch,
                Symbol::TreeVertical,
                [muted, accent, accent],
            ),
        ] {
            let header = space_header(&hits, space);
            // The header, then the agent's two rows.
            let (column, span) = (header.x, header.y..header.y + 3);
            for (row, hue) in span.clone().zip(hues) {
                assert_eq!(
                    buffer[(column, row)].fg,
                    hue,
                    "row {row} of {space:?}: {rows:?}"
                );
            }
            let item = &rows[span.start as usize + 1];
            assert!(
                item.trim_start().starts_with(&theme::glyph(branch)),
                "the agent branches off the gutter: {item:?}"
            );
            let below = &rows[span.start as usize + 2];
            assert!(
                below.trim_start().starts_with(&theme::glyph(caption)),
                "and its caption runs down it: {below:?}"
            );
            assert_eq!(
                buffer[(column, span.end)].symbol(),
                " ",
                "the blank row after the space is outside its gutter: {rows:?}"
            );
        }

        // The fill runs under the gutter: the line is drawn inside the
        // block rather than alongside it. Filling up to the line and no
        // further left the line sitting on the column's own background,
        // which reads as a decoration outside the card with a gap between
        // them — the card's edge is where the fill ends, and the fill has
        // to end past the line for the line to be in it.
        let header = space_header(&hits, SpaceId(3));
        for row in header.y..header.y + 3 {
            assert_eq!(
                buffer[(header.x, row)].bg,
                buffer[(header.x + 1, row)].bg,
                "row {row}: the gutter sits outside the block's fill: {rows:?}"
            );
        }
        let outside = space_header(&hits, SpaceId(1));
        assert_ne!(
            buffer[(outside.x, outside.y)].bg,
            buffer[(header.x, header.y)].bg,
            "a space nobody is in is not filled at all: {rows:?}"
        );
    }

    /// The column reads space > agent: the header's fold against the
    /// gutter and its name just after; each agent's status glyph a blank
    /// column past the connector, its name after that, its caption under
    /// that name — the same in either kind of space.
    #[test]
    fn agents_sit_one_step_inside_their_space() {
        let column_of = |row: &str, text: &str| {
            let byte = row
                .find(text)
                .unwrap_or_else(|| panic!("{text:?} in {row:?}"));
            row[..byte].chars().count()
        };
        let idle = theme::glyph(crate::ui::theme::Symbol::StatusIdle);
        let tree = three_spaces();
        let Sidebar { rows, hits, .. } = sidebar(&tree, &identities_fixture());
        let header = space_header(&hits, SpaceId(1)).y as usize;
        let name = column_of(&rows[header], "one");
        assert_eq!(column_of(&rows[header + 1], &idle), name, "{rows:?}");
        let agent = column_of(&rows[header + 1], "shell");
        assert_eq!(agent, name + 2, "{rows:?}");
        assert_eq!(column_of(&rows[header + 2], "agent"), agent, "{rows:?}");

        let flat = workspace_space_session();
        let Sidebar { rows, hits, .. } = sidebar(&flat, &tenant_identities());
        let header = space_header(&hits, SpaceId(1)).y as usize;
        let name = column_of(&rows[header], "repo");
        assert_eq!(column_of(&rows[header + 1], &idle), name, "{rows:?}");
        assert_eq!(
            column_of(&rows[header + 1], "agent 1"),
            name + 2,
            "{rows:?}"
        );
        assert_eq!(column_of(&rows[header + 2], "claude"), name + 2, "{rows:?}");
    }

    /// A space is where its own shell is: a `cd` there moves what the space
    /// names and what its agents are placed from. With no shell of its own
    /// — every tab an agent — it is the root it was opened at.
    #[test]
    fn a_cd_in_a_spaces_own_shell_moves_the_space() {
        let mut session = session("/repo", uze_terminal::SpaceKind::Workspace);
        let space = session.workspace.selected_space;
        let shell = session.workspace.spaces[0].tabs[0].pane.id;
        let agent = session.add_tab(space, "agent 1".into(), None, 80, 24, "/repo".into());
        session.update_pane_status(agent, "/repo".into(), "claude".into());
        assert_eq!(
            space_cwd(&session.workspace.spaces[0], &tenant_identities()),
            PathBuf::from("/repo")
        );

        session.update_pane_status(shell, "/repo/services/api".into(), "bash".into());
        let moved = PathBuf::from("/repo/services/api");
        assert_eq!(
            space_cwd(&session.workspace.spaces[0], &tenant_identities()),
            moved,
            "the space followed its own shell"
        );

        let mut model = model_of(session.clone());
        model.remembered.collapsed_spaces.insert(space);
        let Sidebar { rows, hits, .. } = sidebar(&model, &tenant_identities());
        let header = space_header(&hits, space).y as usize;
        assert!(
            rows[header + 1].contains("api"),
            "the caption names where it is now: {rows:?}"
        );
        model.remembered.roots_shown.insert(space);
        let rows = sidebar(&model, &tenant_identities()).rows;
        assert!(
            rows[header].contains("api"),
            "and so does the header's own toggle: {rows:?}"
        );

        // Nothing of its own left to follow: the root it was opened at.
        session.update_pane_status(shell, "/repo/services/api".into(), "claude".into());
        assert_eq!(
            space_cwd(&session.workspace.spaces[0], &tenant_identities()),
            PathBuf::from("/repo"),
            "every tab an agent, so the space is its root again"
        );
    }

    /// A space's root and each of its agents' directories are read as soon
    /// as the sidebar names them — once each, answered or still asked.
    #[test]
    fn every_directory_the_sidebar_names_is_read_once() {
        let mut model = three_spaces();
        let unread = |model: &WorkspaceModel| {
            let mut unread = model.unread_named_directories(&identities_fixture());
            unread.sort();
            unread
        };
        assert_eq!(
            unread(&model),
            ["/one", "/three", "/two"].map(PathBuf::from).to_vec(),
            "each root, which is also where each agent runs"
        );

        model.remembered.evaluated.insert(PathBuf::from("/one"));
        model
            .remembered
            .task_eval_pending
            .insert(PathBuf::from("/two"));
        assert_eq!(unread(&model), vec![PathBuf::from("/three")]);

        let session = model.session.as_mut().unwrap();
        let space = session.workspace.selected_space;
        let pane = session.add_tab(space, "agent 2".into(), None, 80, 24, "/three".into());
        session.update_pane_status(pane, "/three/.worktrees/x".into(), "agent".into());
        model.remembered.evaluated.insert(PathBuf::from("/three"));
        assert!(
            unread(&model).is_empty(),
            "a slot is answered by its repository's one evaluation"
        );
    }

    /// The fold minimizes a space to its header and opens it again, on this
    /// client alone: nothing is asked of the server.
    #[test]
    fn the_fold_minimizes_a_space_to_its_header_and_opens_it_again() {
        let home = UzeHome::at(uze_testkit::temp::scratch("orchestrator-space-fold"));
        let mut driven = driven(three_spaces(), &home);
        driven.frame();
        let one = SpaceId(1);
        assert_eq!(
            agent_rows_of(&driven.attach.model, &driven.attach.model.hits, one),
            2
        );
        let fold = driven
            .hit(|hit| matches!(hit, WorkspaceHit::ToggleSpaceCollapsed(space) if *space == one));

        driven.press(fold.x, fold.y);
        driven.frame();
        let hits = driven.attach.model.hits.clone();
        assert_eq!(
            agent_rows_of(&driven.attach.model, &hits, one),
            0,
            "the agent rows are folded away"
        );
        let two = space_header(&hits, SpaceId(2));
        assert_eq!(
            two.y,
            space_header(&hits, one).y + 3,
            "the next space follows the header, its caption and its blank row"
        );
        assert!(
            driven.sent().is_empty(),
            "folding is this client's own view"
        );

        driven.press(fold.x, fold.y);
        driven.frame();
        let hits = driven.attach.model.hits.clone();
        assert_eq!(
            agent_rows_of(&driven.attach.model, &hits, one),
            2,
            "and back"
        );
    }

    /// Folded, a space still says that something in it wants a look, and
    /// its header — all there is of it — carries the selection.
    #[test]
    fn a_folded_space_says_through_its_header_what_its_agents_did() {
        let mut model = three_spaces();
        let three = model.session.as_ref().unwrap().workspace.selected_space;
        let pane = model.session.as_ref().unwrap().selected_tab().pane.id;
        model.remembered.completed_agent_panes.insert(pane);
        let completed = theme::glyph(crate::ui::theme::Symbol::StatusCompleted);
        let header_row = |model: &WorkspaceModel| {
            let Sidebar {
                rows, hits, buffer, ..
            } = sidebar(model, &identities_fixture());
            let header = space_header(&hits, three).y;
            let lit = lit_gutter_rows(
                &buffer,
                gutter_column(&hits),
                uze_terminal::SpaceKind::Worktree,
            )
            .contains(&header);
            (rows[header as usize].clone(), lit)
        };
        let (open, lit) = header_row(&model);
        assert!(
            !open.contains(&completed),
            "open, the agent row says it: {open:?}"
        );
        assert!(!lit, "and the agent carries the selection: {open:?}");

        model.remembered.collapsed_spaces.insert(three);
        let (folded, lit) = header_row(&model);
        assert!(folded.contains(&completed), "{folded:?}");
        assert!(lit, "{folded:?}");
    }

    /// A minimized space keeps a caption under its header saying where its
    /// work is — the branch its root is on, else the root, pinned under its
    /// `⇄`, never an agent; open over its agents, it has none.
    #[test]
    fn a_folded_space_caption_names_where_it_is_and_an_open_one_has_none() {
        let rows_at = |model: &WorkspaceModel, space: SpaceId| {
            let Sidebar { rows, hits, .. } = sidebar(model, &identities_fixture());
            let header = space_header(&hits, space).y as usize;
            (rows[header].clone(), rows[header + 1].clone())
        };
        let mut model = three_spaces();
        let one = SpaceId(1);
        let (_, open) = rows_at(&model, one);
        assert!(
            open.contains("shell"),
            "open, the agents follow the header: {open:?}"
        );

        model.remembered.collapsed_spaces.insert(one);
        let (header, row) = rows_at(&model, one);
        assert!(
            row.contains("/one") && !row.contains("shell"),
            "no branch, the root: {row:?}"
        );
        let column = |row: &str, text: &str| row[..row.find(text).unwrap()].chars().count();
        assert_eq!(
            column(&row, "/one") + 3,
            column(&header, "⇄"),
            "pinned under the toggle"
        );
        model
            .remembered
            .branches
            .insert(PathBuf::from("/one"), "main".into());
        let (_, row) = rows_at(&model, one);
        assert!(row.contains("main"), "the branch: {row:?}");
    }

    /// The scroll bound is measured with the folds, so a column of folded
    /// spaces does not scroll past rows that are no longer drawn.
    #[test]
    fn a_folded_space_measures_its_header_alone() {
        let mut model = three_spaces();
        let session = model.session.as_mut().unwrap();
        for index in 4..12 {
            let root = PathBuf::from(format!("/{index}"));
            session.create_space(
                None,
                uze_terminal::SpaceSeat {
                    root: root.clone(),
                    kind: uze_terminal::SpaceKind::Worktree,
                },
                80,
                24,
            );
            let pane = session.selected_tab().pane.id;
            session.update_pane_status(pane, root, "agent".into());
        }
        let open = sidebar(&model, &identities_fixture()).metrics.tree_overflow;
        model.remembered.collapsed_spaces.insert(SpaceId(1));
        let folded = sidebar(&model, &identities_fixture()).metrics.tree_overflow;
        assert!(open > 2, "the column overflows: {open}");
        assert_eq!(
            open - folded,
            1,
            "the agent's two rows give way to one caption"
        );
    }

    /// A space is carried by its header: while it moves, a line shows where
    /// it lands, and letting go there asks the server to put it there.
    #[test]
    fn a_space_dragged_by_its_header_lands_where_the_line_said() {
        let home = UzeHome::at(uze_testkit::temp::scratch("orchestrator-space-drag"));
        let mut driven = driven(three_spaces(), &home);
        driven.frame();
        let hits = driven.attach.model.hits.clone();
        let one = space_header(&hits, SpaceId(1));
        let three = space_header(&hits, SpaceId(3));

        driven.press(three.x + 4, three.y);
        let _ = driven.sent();
        driven.mouse(three.x + 4, one.y, MouseEventKind::Drag(MouseButton::Left));
        let rows = frame_rows(&mut driven.attach.model);
        let line = theme::glyph(crate::ui::theme::Symbol::TreeDivider).repeat(8);
        assert!(
            rows[one.y as usize - 1].contains(&line),
            "the line is drawn above the space it lands before: {rows:?}"
        );

        driven.mouse(three.x + 4, one.y, MouseEventKind::Up(MouseButton::Left));
        let sent = driven.sent();
        assert!(
            sent.iter().any(|request| matches!(
                request,
                ClientRequest::ReorderSpace { space, before: Some(before) }
                    if *space == SpaceId(3) && *before == SpaceId(1)
            )),
            "{sent:?}"
        );
        assert!(driven.attach.model.dragging_space.is_none());
    }

    /// A header clicked, or barely moved, stays a click.
    #[test]
    fn a_click_on_a_space_header_moves_no_space() {
        let home = UzeHome::at(uze_testkit::temp::scratch("orchestrator-space-click"));
        let mut driven = driven(three_spaces(), &home);
        driven.frame();
        let three = space_header(&driven.attach.model.hits.clone(), SpaceId(3));

        driven.press(three.x + 4, three.y);
        driven.mouse(
            three.x + 4,
            three.y - 1,
            MouseEventKind::Drag(MouseButton::Left),
        );
        driven.mouse(
            three.x + 4,
            three.y - 1,
            MouseEventKind::Up(MouseButton::Left),
        );

        let sent = driven.sent();
        assert!(
            !sent
                .iter()
                .any(|request| matches!(request, ClientRequest::ReorderSpace { .. })),
            "{sent:?}"
        );
    }

    /// The space row is a context of its own — its shells — so while the
    /// operator is there its gutter is lit, in either kind, and
    /// gives it up once an agent of the space is selected.
    #[test]
    fn the_space_row_carries_the_bar_while_its_own_shells_are_selected() {
        let header_lit = |model: &WorkspaceModel, kind| {
            let Sidebar { hits, buffer, .. } = sidebar(model, &tenant_identities());
            let header = hits
                .iter()
                .find(|(_, hit)| matches!(hit, WorkspaceHit::SelectSpace(_)))
                .map(|(rect, _)| rect.y)
                .expect("the space header");
            lit_gutter_rows(&buffer, gutter_column(&hits), kind).contains(&header)
        };
        for kind in [
            uze_terminal::SpaceKind::Worktree,
            uze_terminal::SpaceKind::Workspace,
        ] {
            let mut on_shell = session("/repo", kind);
            let space = on_shell.selected_space().id;
            let shell = on_shell.selected_tab().id;
            let agent = on_shell.add_tab(space, "agent 1".into(), None, 80, 24, "/repo".into());
            on_shell.update_pane_status(agent, "/repo".into(), "claude".into());
            on_shell.workspace.spaces[0].selected_tab = shell;
            assert!(header_lit(&model_of(on_shell.clone()), kind), "{kind:?}");

            let agent_tab = on_shell.workspace.spaces[0]
                .tabs
                .iter()
                .find(|tab| tab.pane.id == agent)
                .expect("the agent's tab")
                .id;
            on_shell.workspace.spaces[0].selected_tab = agent_tab;
            assert!(!header_lit(&model_of(on_shell), kind), "{kind:?}");
        }
    }

    /// The context menu answers the keyboard like the agent picker does:
    /// the selection moves within the items and stops at their ends,
    /// Enter acts on the highlighted one and closes the menu.
    #[test]
    fn the_context_menu_is_driven_by_the_keyboard() {
        let home = UzeHome::at(uze_testkit::temp::scratch("orchestrator-menu-keys"));
        let mut driven = driven(
            model_of(session("/repo", uze_terminal::SpaceKind::Worktree)),
            &home,
        );
        driven.frame();
        let row = driven
            .attach
            .model
            .hits
            .iter()
            .find_map(|(rect, hit)| matches!(hit, WorkspaceHit::SelectSpace(_)).then_some(*rect))
            .expect("the space row is a target");
        driven.mouse(row.x + 1, row.y, MouseEventKind::Down(MouseButton::Right));
        let scopes = [
            uze_keys::Scope::Global,
            uze_keys::Scope::Workspace,
            uze_keys::Scope::ContextMenu,
        ];
        let press = |driven: &mut Driven<'_>, action| {
            let chord = uze_keys::active()
                .chord_for(action, &scopes)
                .expect("the menu is reachable from the keyboard");
            driven.press_key(key_event(chord));
        };
        let selected = |driven: &Driven<'_>| {
            driven
                .attach
                .model
                .context_menu
                .as_ref()
                .map(|menu| menu.selected)
        };

        press(&mut driven, uze_keys::Action::SelectPrevious);
        assert_eq!(selected(&driven), Some(0), "it stops at the first item");
        press(&mut driven, uze_keys::Action::SelectNext);
        press(&mut driven, uze_keys::Action::SelectNext);
        assert_eq!(selected(&driven), Some(1), "and at the last");

        let _ = driven.sent();
        press(&mut driven, uze_keys::Action::Activate);
        assert!(
            driven.attach.model.context_menu.is_none(),
            "acting closes it"
        );
        assert!(
            driven
                .sent()
                .iter()
                .any(|request| matches!(request, ClientRequest::CloseSpace { .. })),
            "Enter acted on the highlighted item, delete"
        );
    }

    /// A lone space can be deleted like any other: its menu offers it, and
    /// deleting it names a space at home to take its place, so the
    /// workspace is never left with nowhere to land.
    #[test]
    fn a_lone_space_offers_delete_and_is_replaced_by_home() {
        let home = UzeHome::at(uze_testkit::temp::scratch("orchestrator-close-last-space"));
        let model = model_of(session("/repo", uze_terminal::SpaceKind::Worktree));
        let mut driven = driven(model, &home);
        driven.frame();
        let (row, space) = driven
            .attach
            .model
            .hits
            .iter()
            .find_map(|(rect, hit)| match hit {
                WorkspaceHit::SelectSpace(space) => Some((*rect, *space)),
                _ => None,
            })
            .expect("the space row is a target");

        driven.mouse(row.x + 1, row.y, MouseEventKind::Down(MouseButton::Right));
        let menu = driven
            .attach
            .model
            .context_menu
            .as_ref()
            .expect("a menu opened on the space row");
        assert_eq!(
            menu.items,
            vec![
                uze_keys::Action::RenameSelection,
                uze_keys::Action::CloseTab
            ]
        );

        let mut requests = Vec::new();
        crate::ui::orchestrator::dispatch_menu_action(
            &mut requests,
            &mut driven.attach.model,
            &identities_fixture(),
            crate::ui::orchestrator::MenuTarget::Space(space),
            uze_keys::Action::CloseTab,
        );
        let request: ClientRequest =
            bincode::deserialize(&requests[4..]).expect("one request was written");
        let ClientRequest::CloseSpace {
            space: closed,
            replacement,
            ..
        } = request
        else {
            panic!("expected CloseSpace, got {request:?}");
        };
        assert_eq!(closed, space);
        assert_eq!(replacement.kind, uze_terminal::SpaceKind::Workspace);
        assert_eq!(
            Some(replacement.root.as_os_str()),
            std::env::var_os("HOME").as_deref(),
            "the workspace lands at home"
        );
    }

    /// Starting `uze` somewhere is a request for a space there, except
    /// where nobody chose the directory. A shell opens at home, so a
    /// launch from home is "start the app", not "add my home directory to
    /// the workspace" — and a home space closed on purpose used to be
    /// remade by the next launch, which reads as the close not working.
    #[test]
    fn starting_at_home_lands_in_the_workspace_rather_than_adding_to_it() {
        use uze_terminal::{Seating, SpaceKind, SpaceSeat};

        let home = PathBuf::from(std::env::var_os("HOME").expect("a home directory"));
        let seat = |root: &Path| SpaceSeat {
            root: root.to_path_buf(),
            kind: SpaceKind::Workspace,
        };

        assert_eq!(
            crate::ui::orchestrator::seating_at(seat(&home)),
            Seating::At(seat(&home)),
            "the home directory is where a shell starts, not a space to open"
        );
        let project = home.join("some-project");
        assert_eq!(
            crate::ui::orchestrator::seating_at(seat(&project)),
            Seating::Open(seat(&project)),
            "a directory somebody chose is a request for a space in it"
        );
    }

    /// Walking away from an agent and coming back returns to the tab it
    /// was left on. A space holds one selection, so a shell opened beside
    /// an agent used to be forgotten the moment the user looked at
    /// another agent — they came back to the agent's own tab and had to
    /// find their shell again in the strip.
    #[test]
    fn an_agent_is_re_entered_on_the_tab_it_was_left_on() {
        let home = UzeHome::at(uze_testkit::temp::scratch("orchestrator-strip-memory"));
        let (mut model, first, second) = two_agents_with_shells();
        let shell = model.session.as_ref().expect("session").workspace.spaces[0]
            .tabs
            .iter()
            .find(|tab| tab.agent == Some(first))
            .expect("the first agent has a shell")
            .id;

        // Left working in the first agent's shell, then away to the second.
        for tab in [shell, second] {
            let mut session = model.session.clone().expect("session");
            session.select_tab(tab);
            model.apply(
                ClientEvent::SessionUpdated { session },
                &identities_fixture(),
            );
        }

        let mut driven = driven(model, &home);
        driven.frame();
        let layout = compute_layout(Rect::new(0, 0, 80, 24), driven.attach.model.sidebar_width);
        let row = driven
            .attach
            .model
            .hits
            .iter()
            .find(|(rect, hit)| {
                rect.x < layout.sidebar.right()
                    && matches!(hit, WorkspaceHit::SelectTab(tab) if *tab == first)
            })
            .map(|(rect, _)| *rect)
            .expect("the first agent has a sidebar row");
        driven.press(row.x + 4, row.y);

        assert!(
            driven.sent().iter().any(
                |request| matches!(request, ClientRequest::SelectTab { tab } if *tab == shell)
            ),
            "the shell it was left in, not the agent tab"
        );
    }

    /// The same click, when the user is already inside that agent: it
    /// means the agent's own tab, and the strip is right there for
    /// anything else.
    #[test]
    fn clicking_the_agent_you_are_already_in_selects_the_agent_itself() {
        let home = UzeHome::at(uze_testkit::temp::scratch("orchestrator-strip-same-agent"));
        let (mut model, first, _second) = two_agents_with_shells();
        let shell = model.session.as_ref().expect("session").workspace.spaces[0]
            .tabs
            .iter()
            .find(|tab| tab.agent == Some(first))
            .expect("the first agent has a shell")
            .id;
        let mut session = model.session.clone().expect("session");
        session.select_tab(shell);
        model.apply(
            ClientEvent::SessionUpdated { session },
            &identities_fixture(),
        );

        let mut driven = driven(model, &home);
        driven.frame();
        let layout = compute_layout(Rect::new(0, 0, 80, 24), driven.attach.model.sidebar_width);
        let row = driven
            .attach
            .model
            .hits
            .iter()
            .find(|(rect, hit)| {
                rect.x < layout.sidebar.right()
                    && matches!(hit, WorkspaceHit::SelectTab(tab) if *tab == first)
            })
            .map(|(rect, _)| *rect)
            .expect("the first agent has a sidebar row");
        driven.press(row.x + 4, row.y);

        assert!(
            driven.sent().iter().any(
                |request| matches!(request, ClientRequest::SelectTab { tab } if *tab == first)
            ),
            "the agent's own tab"
        );
    }

    /// A session whose one agent sits in `checkout`, removed from under
    /// it and bound to `task` — the state the "resume" is drawn from.
    fn agent_over_a_lost_checkout(
        checkout: &Path,
        primary: &Path,
        task: TaskView,
    ) -> WorkspaceModel {
        let mut model = agent_session_in(&format!("{} (deleted)", checkout.display()));
        let pane = first_tab(&model).pane.id;
        // Rows under the one that lost its checkout: what the picker
        // opens over, and what its own rows have to answer ahead of.
        if let Some(session) = model.session.as_mut() {
            let space = session.workspace.selected_space;
            for label in ["Agent two", "Agent three"] {
                let opened = session.add_tab(space, label.into(), None, 80, 24, "/repo".into());
                session.update_pane_status(opened, "/repo".into(), "agent".into());
            }
        }
        model
            .remembered
            .pane_checkouts
            .insert(pane, checkout.to_path_buf());
        stamp_first_tab(&mut model, &task.id);
        model.remembered.lost_checkouts.insert(pane);
        model
            .remembered
            .tasks
            .insert(primary.to_path_buf(), vec![task]);
        model
    }

    /// A task with no checkout left, waiting to be put back in one.
    fn parked_task(id: &str, branch: &str) -> TaskView {
        let mut task = task_in("/repo/.worktrees/ai", id, TaskStateView::Parked, 1);
        task.id = id.to_owned();
        task.branch = branch.to_owned();
        task.checkout = None;
        task
    }

    /// The picker opens over the tree it was asked from, so its rows sit
    /// on top of sidebar rows drawn — and pushed — before them. The click
    /// search the tree itself uses takes the first rect a point lands in,
    /// which is what puts a mark's own hit ahead of its row; an overlay
    /// has to take the last one instead, or the half of every option row
    /// standing over the tree belongs to the row underneath it.
    #[test]
    fn a_picker_row_over_the_tree_answers_its_own_click() {
        let home = UzeHome::at(uze_testkit::temp::scratch("orchestrator-picker-overlap"));
        let model = agent_over_a_lost_checkout(
            Path::new("/repo/.worktrees/ai"),
            Path::new("/repo"),
            parked_task("t1", "agent/t1"),
        );
        let mut driven = driven(model, &home);

        driven.frame();
        let resume = driven.hit(|hit| matches!(hit, WorkspaceHit::ResumeLostCheckout(_)));
        driven.press(resume.x, resume.y);
        assert!(
            driven.attach.model.agent_picker.is_some(),
            "the resume opens the picker"
        );

        driven.frame();
        let option = driven.hit(|hit| matches!(hit, WorkspaceHit::PickAgent(0)));
        let tree_ends = compute_layout(Rect::new(0, 0, 80, 24), None).pane.x;
        assert!(
            option.x < tree_ends,
            "the row this is about starts over the tree: {option:?}"
        );
        driven.press(option.x, option.y);
        assert!(
            driven.attach.model.placement_pending,
            "the harness under the pointer answered, not the row beneath it"
        );
    }

    /// A directory another space already holds is opened all the same:
    /// one repository is routinely worth two spaces (one per branch, one
    /// per thing being tried), and the prompt is an explicit request for
    /// one — not a lookup of what is already open.
    #[test]
    fn a_directory_a_space_already_holds_is_opened_again() {
        let home = UzeHome::at(uze_testkit::temp::scratch("orchestrator-space-again"));
        let root = uze_testkit::temp::TempDir::new("orchestrator-space-root");
        std::fs::create_dir_all(root.join("inner")).unwrap();
        let mut model = session_rooted_at(root.path());
        // The directory being listed is what an untouched prompt lands on
        // (see `RootPicker`) — here, the space's own root.
        model.root_picker = Some(RootPicker::opened_in(
            &root.path().display().to_string(),
            None,
        ));
        let mut driven = driven(model, &home);

        driven.frame();
        let enter = uze_keys::active()
            .chord_for(uze_keys::Action::Activate, &[uze_keys::Scope::RootPicker])
            .expect("the prompt is answered from the keyboard");
        driven.press_key(key_event(enter));

        let sent = driven.sent();
        assert!(
            sent.iter().any(|request| matches!(
                request,
                ClientRequest::CreateSpace { seat, .. } if seat.root == root.path()
            )),
            "the pick is asked for, not looked up: {sent:?}"
        );
    }

    #[test]
    fn a_directory_no_space_holds_is_opened_as_one() {
        let home = UzeHome::at(uze_testkit::temp::scratch("orchestrator-space-new"));
        let root = uze_testkit::temp::TempDir::new("orchestrator-space-new-root");
        std::fs::create_dir_all(root.join("inner")).unwrap();
        let mut model = session_rooted_at(root.path());
        model.root_picker = Some(RootPicker::opened_in(
            &root.path().display().to_string(),
            None,
        ));
        let mut driven = driven(model, &home);

        driven.frame();
        let row = driven.hit(|hit| matches!(hit, WorkspaceHit::PickSpaceRoot(_)));
        driven.press(row.x, row.y);

        let sent = driven.sent();
        assert!(
            sent.iter().any(|request| matches!(
                request,
                ClientRequest::CreateSpace { seat, .. } if seat.root == root.join("inner")
            )),
            "{sent:?}"
        );
    }

    /// Where the divider was let go outlives the run, like the timeline's
    /// own shape: both modes share this column, so both find it at the
    /// width it was left. Kept on release, not through the drag — the
    /// widths it swept past are not answers.
    #[test]
    fn the_dragged_sidebar_width_is_kept_for_the_next_run() {
        let home = UzeHome::at(uze_testkit::temp::scratch("orchestrator-sidebar-width"));
        let (recorder, recorded) = std::sync::mpsc::channel();
        let mut model = agent_session_in("/repo");
        model.layout_recorder = Some(recorder);
        let mut driven = driven(model, &home);
        driven.frame();
        let handle = driven.hit(|hit| matches!(hit, WorkspaceHit::ResizeSidebar));

        driven.press(handle.x, handle.y);
        driven.mouse(20, 5, MouseEventKind::Drag(MouseButton::Left));
        let dragged = driven.attach.model.sidebar_width;
        assert!(dragged.is_some(), "the drag moved the divider");
        assert!(recorded.try_recv().is_err(), "nothing is written mid-drag");

        driven.mouse(20, 5, MouseEventKind::Up(MouseButton::Left));

        let shape = recorded.try_recv().expect("the release is recorded");
        assert_eq!(shape.sidebar.width, dragged);
    }

    /// An agent that could not be placed as the space asked is not started
    /// anywhere else: the reason is said, and no tab is opened.
    #[test]
    fn an_agent_that_could_not_be_placed_says_why_and_opens_nothing() {
        let home = UzeHome::at(uze_testkit::temp::scratch("orchestrator-refused"));
        let mut driven = driven(agent_session_in("/repo"), &home);
        driven.placements_answered(PlacementResolution {
            label: "agent 2".to_owned(),
            command: vec!["claude".to_owned()],
            placement: Err("could not place the agent: no commit to branch from".to_owned()),
            replacing: None,
        });
        let notice = driven
            .attach
            .model
            .remembered
            .notice
            .as_ref()
            .expect("the refusal is said");
        assert!(
            notice.text.contains("agent 2") && notice.text.contains("no commit to branch from"),
            "{}",
            notice.text
        );
        assert!(
            !driven
                .sent()
                .iter()
                .any(|request| matches!(request, ClientRequest::CreateTab { .. })),
            "nothing was opened in the operator's tree"
        );
    }

    /// The operator's own sequence, end to end: an agent commits in its
    /// slot, the slot is removed by hand, and the row that says so is
    /// clicked back to life. What has to come back is *that* task, on its
    /// own branch with its own commits — not a second agent beside it.
    #[test]
    fn resume_clicked_on_a_lost_checkout_brings_the_task_back_with_its_commits() {
        let repository = uze_testkit::git::Repository::new("orchestrator-resume");
        let root = repository.root().to_path_buf();
        let home = UzeHome::at(uze_testkit::temp::scratch("orchestrator-resume-home"));
        let app = uze_application::UzeApplication::new(home.clone(), Vec::new());
        let placement = app
            .workspace()
            .place_new_agent(
                &root,
                uze_application::PlacementKind::Slot,
                "claude-code",
                &[],
            )
            .unwrap();
        let task_id = placement.placement.agent().as_str().to_owned();
        std::fs::write(placement.cwd.join("kept.rs"), b"fn kept() {}").unwrap();
        repository.git_in(&placement.cwd, &["add", "."]);
        repository.git_in(&placement.cwd, &["commit", "-qm", "kept"]);
        std::fs::remove_dir_all(&placement.cwd).unwrap();
        app.workspace().release_abandoned_tasks(&root, &[], &[]);

        let primary = root.canonicalize().unwrap();
        let task = app
            .workspace()
            .tasks(&primary)
            .into_iter()
            .find(|task| task.id == task_id)
            .expect("the task outlives its checkout");
        let model = agent_over_a_lost_checkout(&placement.cwd, &primary, task);
        let mut driven = driven(model, &home);

        driven.frame();
        let resume = driven.hit(|hit| matches!(hit, WorkspaceHit::ResumeLostCheckout(_)));
        driven.press(resume.x, resume.y);
        driven.frame();
        let option = driven.hit(|hit| matches!(hit, WorkspaceHit::PickAgent(0)));
        driven.press(option.x, option.y);

        let resolution = driven
            .attach
            .channels
            .placements
            .receiver
            .recv_timeout(Duration::from_secs(30))
            .expect("the placement answers");
        let placed = resolution
            .placement
            .as_ref()
            .expect("the task lands somewhere");
        assert!(
            matches!(
                &placed.placement,
                uze_application::Placement::Slot { task, branch, .. }
                    if task.as_str() == task_id && *branch == format!("agent/{task_id}")
            ),
            "the same task, on its own branch: {:?}",
            placed.placement
        );
        let slot = placed.cwd.clone();
        assert!(
            slot.join("kept.rs").is_file(),
            "with the commit it made: {}",
            slot.display()
        );

        // And the agent it took over from: the tab is opened first, then
        // the dead row it replaces is closed — the operator is left with
        // one agent for the task, not a corpse beside a copy.
        let lost_tab = first_tab(&driven.attach.model).id;
        driven.placements_answered(resolution);
        let sent = driven.sent();
        assert!(
            sent.iter().any(
                |request| matches!(request, ClientRequest::SelectTab { tab } if *tab == lost_tab)
            ),
            "the row is selected, so the revived agent opens in its space: {sent:?}"
        );
        let created = sent
            .iter()
            .position(|request| matches!(request, ClientRequest::CreateTab { cwd, .. } if cwd.as_deref() == Some(slot.as_path())))
            .expect("the revived agent opens in the slot");
        let closed = sent
            .iter()
            .position(
                |request| matches!(request, ClientRequest::CloseTab { tab } if *tab == lost_tab),
            )
            .expect("the row that lost its checkout closes");
        assert!(created < closed, "the new tab opens first: {sent:?}");
    }

    /// Resuming a preserved task whose checkout is still there opens the
    /// agent in that checkout *and* names the task on the launch: a tab
    /// without the stamp is an agent no reader can bind to its work.
    #[test]
    fn resuming_a_preserved_task_that_kept_its_checkout_opens_a_stamped_tab() {
        let repository = uze_testkit::git::Repository::new("orchestrator-resume-kept");
        let root = repository.root().to_path_buf();
        let home = UzeHome::at(uze_testkit::temp::scratch("orchestrator-resume-kept-home"));
        let app = uze_application::UzeApplication::new(home.clone(), Vec::new());
        let placement = app
            .workspace()
            .place_new_agent(
                &root,
                uze_application::PlacementKind::Slot,
                "claude-code",
                &[],
            )
            .unwrap();
        let task_id = placement.placement.agent().as_str().to_owned();
        let primary = root.canonicalize().unwrap();
        let mut model = agent_session_in("/elsewhere");
        model
            .remembered
            .tasks
            .insert(primary.clone(), app.workspace().tasks(&primary));
        model.preserved = Some(PreservedOverlay {
            selected: 0,
            confirm_discard: false,
        });
        let mut driven = driven(model, &home);

        let keymap = uze_keys::active();
        let resume = keymap
            .chord_for(
                uze_keys::Action::ResumeTask,
                &[uze_keys::Scope::PreservedWork],
            )
            .expect("resume is bound here");
        let pick = keymap
            .chord_for(uze_keys::Action::Activate, &[uze_keys::Scope::AgentPicker])
            .expect("picking is bound here");
        driven.press_key(key_event(resume));
        driven.press_key(key_event(pick));
        let resolution = driven
            .attach
            .channels
            .placements
            .receiver
            .recv_timeout(Duration::from_secs(30))
            .expect("the placement answers");
        driven.placements_answered(resolution);

        let identity = (
            uze_terminal::launch::AGENT_IDENTITY_VARIABLE.to_owned(),
            task_id,
        );
        let sent = driven.sent();
        assert!(
            sent.iter().any(|request| matches!(
                request,
                ClientRequest::CreateTab { cwd, env, .. }
                    if cwd.as_deref() == Some(placement.cwd.as_path()) && env.contains(&identity)
            )),
            "the agent opens in its checkout, launched for its task: {sent:?}"
        );
    }

    /// A message for an agent reaches the agent's own pane, never a shell
    /// that happens to stand in the same slot: typed into the shell, it
    /// would run as a command.
    #[test]
    fn a_notice_for_an_agent_skips_a_shell_standing_in_its_slot() {
        let home = UzeHome::at(uze_testkit::temp::scratch("orchestrator-notice-pane"));
        let mut model = agent_with_task(TaskStateView::Running, 0);
        let space = &mut model.session.as_mut().unwrap().workspace.spaces[0];
        let mut shell = space.tabs[0].clone();
        shell.id = TabId(2);
        shell.label = "shell".into();
        shell.env = Vec::new();
        shell.pane.id = PaneId(2);
        shell.pane.process = "zsh".into();
        space.tabs.insert(0, shell);
        let agent_pane = space.tabs[1].pane.id;
        let mut driven = driven(model, &home);

        driven
            .attach
            .channels
            .tasks
            .sender
            .send(TaskResolution {
                key: PathBuf::from("/repo"),
                answered: Some(super::EvaluationAnswer {
                    primary: PathBuf::from("/repo"),
                    branch: None,
                    target: None,
                    sync: None,
                    evaluation: uze_application::Evaluation {
                        notices: vec![uze_application::AgentNotice {
                            task: "t1".into(),
                            checkout: PathBuf::from("/repo/.worktrees/ai"),
                            message: "resolve the conflict".into(),
                        }],
                        ..uze_application::Evaluation::default()
                    },
                }),
            })
            .unwrap();
        driven.pump();

        let inputs: Vec<PaneId> = driven
            .sent()
            .into_iter()
            .filter_map(|request| match request {
                ClientRequest::Input { pane, .. } => Some(pane),
                _ => None,
            })
            .collect();
        assert_eq!(inputs, vec![agent_pane], "only the agent is told");
    }
}

mod prompt_buffer_tests {
    use super::PromptBuffer;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    /// A keystroke as the buffer receives one: a chord, since
    /// reconstructing what someone typed is the same vocabulary question
    /// as binding it.
    fn key(code: KeyCode) -> uze_keys::Chord {
        crate::ui::keys::chord_of(KeyEvent::new(code, KeyModifiers::NONE)).expect("a chord")
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
            buffer.apply(
                crate::ui::keys::chord_of(KeyEvent::new(KeyCode::Char('u'), modifiers))
                    .expect("a chord"),
            );
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

/// A door pressed twice closes. `ctrl+e` on a surface already showing
/// files used to re-show them, which is indistinguishable from a key that
/// does nothing — and the action is called a toggle.
#[test]
fn a_code_door_pressed_on_the_surface_it_opened_closes_it() {
    use super::session::{CodeDoor, code_door};
    use uze_extensions::code::ContentMode;

    assert_eq!(
        code_door(Some(ContentMode::Contents), ContentMode::Contents),
        CodeDoor::Close,
        "its own door, pressed again, has to close"
    );
    assert_eq!(
        code_door(Some(ContentMode::Diff), ContentMode::Diff),
        CodeDoor::Close
    );
    // The other door switches instead: reaching for the other half of the
    // same surface is not asking to leave it.
    assert_eq!(
        code_door(Some(ContentMode::Diff), ContentMode::Contents),
        CodeDoor::Switch
    );
    // With nothing open, this scope is not live at all — the workspace's
    // own binding is what opens the surface.
    assert_eq!(code_door(None, ContentMode::Contents), CodeDoor::Nothing);
}
