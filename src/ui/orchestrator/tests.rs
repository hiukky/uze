//! Tests for the workspace client.
//!
//! Moved out of `orchestrator.rs` alongside the `render`/`input` split:
//! they were the last ~500 lines standing between a reader and the
//! session-driving code the file is actually about.

use super::*;

mod workspace_tests {
    use super::{
        AgentIdentity, WorkspaceModel, agent_identity_for_tab, blank_pane, can_close_tab_from_menu,
        encode_mouse, forward_paste, forward_scroll, next_agent_label, pane_relative,
        selected_pane_cwd, tab_needs_replacement_shell, workspace_has_active_agent_operation,
    };
    use crossterm::event::{MouseButton, MouseEventKind};
    use ratatui::layout::Rect;
    use std::time::{Duration, Instant};
    use uze_terminal::{
        ClientEvent, ClientRequest, Cursor, Focus, Layout, MouseMode, Pane, PaneDamage, PaneId,
        Session, Tab, TabId, WorkspaceId,
    };

    #[test]
    fn only_submitted_agent_prompts_receive_activity_status() {
        let identities = [AgentIdentity {
            binary: "agent",
            integration: "agent",
            display_name: "Agent",
        }];
        let mut session = Session::new(WorkspaceId("workspace".into()), "/tmp".into(), 80, 24);
        session.workspace.spaces[0].tabs[0].label = "Agent".into();
        let mut model = WorkspaceModel {
            session: Some(session),
            ..WorkspaceModel::default()
        };
        model.note_agent_prompt_submission(PaneId(1), &identities, Some("hello"));
        assert!(workspace_has_active_agent_operation(&model, &identities));

        assert!(!model.expire_agent_activity(Instant::now() + Duration::from_secs(2)));
        assert!(workspace_has_active_agent_operation(&model, &identities));

        assert!(model.expire_agent_activity(Instant::now() + Duration::from_secs(11)));
        assert!(!workspace_has_active_agent_operation(&model, &identities));
    }

    #[test]
    fn completed_background_agent_keeps_a_check_until_its_tab_is_opened() {
        let identities = [AgentIdentity {
            binary: "agent",
            integration: "agent",
            display_name: "Agent",
        }];
        let mut session = Session::new(WorkspaceId("workspace".into()), "/tmp".into(), 80, 24);
        let agent_pane = session.add_tab("Agent".into(), 80, 24, "/tmp".into());
        let agent_tab = session.workspace.spaces[0].selected_tab;
        session.workspace.spaces[0].selected_tab = TabId(1);
        let mut model = WorkspaceModel {
            session: Some(session),
            ..WorkspaceModel::default()
        };
        model.note_agent_prompt_submission(agent_pane, &identities, Some("hello"));
        assert!(model.expire_agent_activity(Instant::now() + Duration::from_secs(11)));
        assert!(model.completed_agent_panes.contains(&agent_pane));

        model.acknowledge_completed_agent_tab(agent_tab);
        assert!(!model.completed_agent_panes.contains(&agent_pane));
    }

    #[test]
    fn submitted_shell_commands_do_not_receive_agent_activity_status() {
        let mut model = WorkspaceModel {
            session: Some(Session::new(
                WorkspaceId("workspace".into()),
                "/tmp".into(),
                80,
                24,
            )),
            ..WorkspaceModel::default()
        };
        let identities = [AgentIdentity {
            binary: "agent",
            integration: "agent",
            display_name: "Agent",
        }];
        model.note_agent_prompt_submission(PaneId(1), &identities, Some("hello"));
        assert!(model.agent_activity.is_empty());
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

        model.apply(ClientEvent::Damage(PaneDamage {
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
        }));

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
