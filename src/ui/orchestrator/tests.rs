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
        WorkspaceModel, agent_identity_for_tab, blank_pane, can_close_tab_from_menu, encode_mouse,
        forward_paste, forward_scroll, next_agent_label, pane_relative, render::render_sidebar,
        selected_pane_cwd, tab_needs_replacement_shell, workspace_has_active_agent_operation,
    };
    use crossterm::event::{MouseButton, MouseEventKind};
    use ratatui::layout::Rect;
    use ratatui::{Terminal, backend::TestBackend};
    use std::time::{Duration, Instant};
    use uze_terminal::{
        CellAttributes, ClientEvent, ClientRequest, Cursor, Focus, Layout, MouseMode, Pane,
        PaneDamage, PaneId, RenderCell, Session, Tab, TabId, TerminalColor, WorkspaceId,
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
        session.add_space("second".into(), 80, 24);
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
        let mut terminal = Terminal::new(TestBackend::new(40, 24)).unwrap();
        terminal
            .draw(|frame| render_sidebar(frame, frame.area(), model, &IDENTITIES, hits))
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

    #[test]
    fn an_isolated_agent_is_marked_on_its_name_row_and_captioned_by_its_primary() {
        // The whole point: `.worktrees/<name>` is two more segments in a
        // column this narrow. The mark belongs beside the agent's name —
        // the caption below it just stops spelling out the tail.
        let model = agent_session_in("/repo/.worktrees/ai");
        let rows = sidebar_rows(&model, &mut Vec::new());
        let name_row = rows
            .iter()
            .find(|row| row.contains("Agent"))
            .expect("the agent is named in the tree");
        assert!(name_row.contains("(wt)"), "{name_row}");

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
    /// that agent is doing — and the marker must never take it over.
    #[test]
    fn the_marker_leaves_the_status_column_alone() {
        let model = agent_session_in("/repo/.worktrees/ai");
        let rows = sidebar_rows(&model, &mut Vec::new());
        let name_row = rows
            .iter()
            .find(|row| row.contains("Agent"))
            .expect("the agent is named in the tree");
        let status = name_row
            .find('\u{25cb}')
            .or_else(|| name_row.find('\u{25cf}'));
        let marker = name_row.find("(wt)");
        assert!(status.is_some(), "the status glyph still leads: {name_row}");
        assert!(status < marker, "the mark follows the name: {name_row}");
    }

    #[test]
    fn an_agent_in_the_primary_checkout_carries_no_marker() {
        let model = agent_session_in("/repo/src");
        let rows = sidebar_rows(&model, &mut Vec::new());
        assert!(
            rows.iter().any(|row| row.contains("/repo/src")),
            "the ordinary path is shown whole: {rows:?}"
        );
        assert!(
            !rows.iter().any(|row| row.contains("(wt)")),
            "nothing to mark: {rows:?}"
        );
    }

    /// `hits` resolves first match wins, so a marker hit pushed after the
    /// row-wide one it sits inside would never be reachable — clicking the
    /// marker would just select the tab.
    #[test]
    fn the_isolation_marker_outranks_the_row_it_sits_on() {
        let model = agent_session_in("/repo/.worktrees/ai");
        let mut hits = Vec::new();
        sidebar_rows(&model, &mut hits);
        let marker = hits
            .iter()
            .find(|(_, hit)| matches!(hit, WorkspaceHit::ShowIsolation(_)))
            .map(|(rect, _)| *rect)
            .expect("an isolated caption row offers its full path");
        let resolved = hits
            .iter()
            .find(|(rect, _)| {
                rect.x <= marker.x
                    && marker.x < rect.x + rect.width
                    && rect.y <= marker.y
                    && marker.y < rect.y + rect.height
            })
            .map(|(_, hit)| *hit);
        assert!(matches!(resolved, Some(WorkspaceHit::ShowIsolation(_))));
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
        let agent_pane = session.add_tab("Agent".into(), 80, 24, "/tmp".into());
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
        let agent_pane = session.add_tab("Agent".into(), 80, 24, "/tmp".into());
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
        let agent_pane = session.add_tab("Agent".into(), 80, 24, "/tmp".into());
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

        session.add_tab("agent 1".into(), 80, 24, "/tmp".into());
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
