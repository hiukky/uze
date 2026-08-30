//! Workspace client for the persistent local terminal runtime (ADR-038).
//!
//! Presentation deliberately reuses the management TUI's palette and layout
//! conventions (`super::BASE`/`ACCENT`/`BORDER`/…, hairline dividers, no
//! filled panels) so switching between the workspace and management
//! contexts with Ctrl+O reads as one product, not two.

use super::git_diff;
use crate::{Result, UzeError, UzeHome};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Padding, Paragraph},
};
use std::{
    collections::BTreeMap,
    io::{self, BufReader},
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use uze_integrations::registry::IntegrationRegistry;
use uze_terminal::{
    CellAttributes, ClientEvent, ClientRequest, Cursor, PROTOCOL_VERSION, PaneId, PaneSnapshot,
    RenderCell, Session, Space, SpaceId, Tab, TabId, TerminalColor, attach, read_event,
    send_request,
};

/// Input/redraw cadence. Unlike the pane content itself — which the server
/// now pushes on PTY output instead of the client polling for it (see
/// ADR-038 follow-up: the previous per-frame `Refresh` request serialized
/// every cell of every pane up to 60x/sec regardless of activity, which is
/// what made typing feel like it hung under any real system load) — this
/// timeout only bounds keyboard/mouse latency.
const POLL: Duration = Duration::from_millis(16);

/// Git inspection runs locally but still launches a process. Refresh often
/// enough to follow commands typed in the active pane without attaching that
/// cost to every PTY damage redraw.
const GIT_BADGE_REFRESH: Duration = Duration::from_millis(750);

pub(crate) enum WorkspaceExit {
    Management,
    Quit,
}

pub(crate) fn attach_workspace(
    terminal: &mut super::TerminalSession,
    root: &Path,
    sidebar_width: &mut Option<u16>,
    home: &UzeHome,
) -> Result<WorkspaceExit> {
    // The handshake below must ship the real terminal size: it sizes the PTY
    // used for the session's *already-selected* pane (e.g. a tab restored
    // from a prior attach), and the per-frame resize further down only
    // corrects the size actually visible in that loop's compute_layout call.
    // A placeholder here previously left a stale-selected pane pinned to a
    // wrong fixed size until something happened to trigger a fresh resize.
    // `*sidebar_width` carries over whatever the user last dragged it to —
    // in this mode or the management one, they share the one value (see
    // `super::run`) — so the pane starts at its real width immediately
    // instead of assuming the sidebar's responsive default.
    let size = terminal.size()?;
    let layout = compute_layout(Rect::new(0, 0, size.width, size.height), *sidebar_width);
    let (columns, rows) = (layout.pane.width, layout.pane.height);

    let mut stream = attach(root, columns, rows).map_err(runtime_error)?;
    let read_stream = stream.try_clone().map_err(io_error)?;
    send_request(
        &mut stream,
        &ClientRequest::Attach {
            version: PROTOCOL_VERSION,
            workspace: uze_terminal::WorkspaceId("client".into()),
            columns,
            rows,
        },
    )
    .map_err(runtime_error)?;
    let (events, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(read_stream);
        while let Ok(Some(event)) = read_event(&mut reader) {
            if events.send(event).is_err() {
                break;
            }
        }
    });
    let mut model = WorkspaceModel {
        dirty: true,
        last_size: (columns, rows),
        sidebar_width: *sidebar_width,
        ..WorkspaceModel::default()
    };
    // Built once per attach, not per frame — a registered harness set
    // doesn't change mid-session, and this loop's own redraw cadence is
    // deliberately kept off any per-frame filesystem/env work (see `POLL`
    // above).
    let identities = agent_identities(home);
    // The server's session/pane state is persistent — reattaching after a
    // Ctrl+O round trip to management finds the same shells exactly as they
    // were left. But the client's own model always starts empty, so without
    // this wait the very first frame renders before the server's initial
    // `Attached`/`Snapshot` reply lands, flashing the "starting shell…"
    // placeholder and repainting the whole pane a moment later — reading as
    // a lost/reset session even though nothing server-side ever was. A
    // generous timeout is still a safety net, not the expected path: this
    // is a local Unix socket round trip, normally sub-millisecond, and the
    // shared `TerminalSession` (see `super::TerminalSession`) is already
    // showing the same open alternate screen management just used, so this
    // blocks inside a continuously open uze, not a flash back to the shell.
    while model.session.is_none() || model.panes.is_empty() {
        match receiver.recv_timeout(Duration::from_millis(500)) {
            Ok(event) => model.apply(event),
            Err(_) => break,
        }
    }
    loop {
        while let Ok(event) = receiver.try_recv() {
            model.apply(event);
        }
        if let Some(view) = model.git_view.as_mut()
            && view.refresh_due()
        {
            view.refresh();
            model.dirty = true;
        }
        let size = terminal.size()?;
        let layout = compute_layout(
            Rect::new(0, 0, size.width, size.height),
            model.sidebar_width,
        );
        let (columns, rows) = (layout.pane.width, layout.pane.height);
        if (columns, rows) != model.last_size {
            model.last_size = (columns, rows);
            model.dirty = true;
            let _ = send_request(
                &mut stream,
                &ClientRequest::Resize {
                    pane: model.focused_pane(),
                    columns,
                    rows,
                },
            );
        }
        if model.dirty {
            model.refresh_git_badge();
            model.tick = model.tick.wrapping_add(1);
            let mut hits = Vec::new();
            terminal.draw(|frame| render(frame, &model, &identities, &mut hits))?;
            model.hits = hits;
            model.dirty = false;
        }
        if event::poll(POLL).map_err(io_error)? {
            match event::read().map_err(io_error)? {
                Event::Key(key) if model.renaming.is_some() => {
                    match key.code {
                        KeyCode::Enter => {
                            if let Some((target, buffer)) = model.renaming.take() {
                                let trimmed = buffer.trim().to_owned();
                                if !trimmed.is_empty() {
                                    let _ = send_request(
                                        &mut stream,
                                        &match target {
                                            RenameTarget::Tab(tab) => ClientRequest::RenameTab {
                                                tab,
                                                label: trimmed,
                                            },
                                            RenameTarget::Space(space) => {
                                                ClientRequest::RenameSpace {
                                                    space,
                                                    label: trimmed,
                                                }
                                            }
                                        },
                                    );
                                }
                            }
                        }
                        KeyCode::Esc => model.renaming = None,
                        KeyCode::Backspace => {
                            if let Some((_, buffer)) = model.renaming.as_mut() {
                                buffer.pop();
                            }
                        }
                        KeyCode::Char(c) => {
                            if let Some((_, buffer)) = model.renaming.as_mut() {
                                buffer.push(c);
                            }
                        }
                        _ => {}
                    }
                    model.dirty = true;
                }
                Event::Key(key) if model.agent_picker.is_some() => {
                    match key.code {
                        KeyCode::Up => {
                            if let Some(picker) = model.agent_picker.as_mut() {
                                picker.selected = picker.selected.saturating_sub(1);
                            }
                        }
                        KeyCode::Down => {
                            if let Some(picker) = model.agent_picker.as_mut() {
                                picker.selected = (picker.selected + 1)
                                    .min(picker.options.len().saturating_sub(1));
                            }
                        }
                        KeyCode::Enter => {
                            if let Some(picker) = model.agent_picker.take()
                                && let Some(option) = picker.options.get(picker.selected)
                            {
                                let _ = send_request(
                                    &mut stream,
                                    &ClientRequest::CreateTab {
                                        label: option.display_name.clone(),
                                        columns,
                                        rows,
                                        command: Some(option.command.clone()),
                                    },
                                );
                            }
                        }
                        // Esc, or anything else — the picker only reacts to
                        // Up/Down/Enter, so any other key just dismisses it
                        // rather than leaking through to the pane.
                        _ => model.agent_picker = None,
                    }
                    model.dirty = true;
                }
                Event::Key(key) if model.context_menu.is_some() => {
                    // Enter confirms the menu's one "close" row; anything
                    // else (Esc included) dismisses without acting — same
                    // "only reacts to its own actions" rule the agent
                    // picker above uses.
                    if key.code == KeyCode::Enter
                        && let Some(menu) = model.context_menu.take()
                    {
                        send_close_request(&mut stream, menu.target);
                    } else {
                        model.context_menu = None;
                    }
                    model.dirty = true;
                }
                Event::Key(key) if model.git_view.is_some() => {
                    if let Some(view) = model.git_view.as_mut()
                        && matches!(
                            git_diff::handle_key(view, key),
                            git_diff::GitViewOutcome::Close
                        )
                    {
                        model.git_view = None;
                    }
                    model.dirty = true;
                }
                Event::Key(key)
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('o') =>
                {
                    let _ = send_request(&mut stream, &ClientRequest::Detach);
                    return Ok(WorkspaceExit::Management);
                }
                Event::Key(key)
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('q') =>
                {
                    let _ = send_request(&mut stream, &ClientRequest::Detach);
                    return Ok(WorkspaceExit::Quit);
                }
                Event::Key(key)
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('t') =>
                {
                    let _ = send_request(
                        &mut stream,
                        &ClientRequest::CreateTab {
                            label: next_shell_label(&model),
                            columns,
                            rows,
                            command: None,
                        },
                    );
                }
                Event::Key(key)
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('w') =>
                {
                    if let Some(tab) = model.selected_tab() {
                        let _ = send_request(&mut stream, &ClientRequest::CloseTab { tab });
                    }
                }
                Event::Key(key)
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('g') =>
                {
                    open_git_view(&mut model);
                }
                Event::Key(key)
                    if key.modifiers.contains(KeyModifiers::ALT)
                        && matches!(key.code, KeyCode::Char('1'..='9')) =>
                {
                    let index = match key.code {
                        KeyCode::Char(value) => value as usize - '1' as usize,
                        _ => 0,
                    };
                    if let Some(tab) = model
                        .session
                        .as_ref()
                        .and_then(|session| session.selected_space().tabs.get(index))
                    {
                        let _ =
                            send_request(&mut stream, &ClientRequest::SelectTab { tab: tab.id });
                        let _ = send_request(
                            &mut stream,
                            &ClientRequest::Resize {
                                pane: tab.focus.pane,
                                columns,
                                rows,
                            },
                        );
                    }
                }
                Event::Key(key) => {
                    if let Some(bytes) = encode_key(key) {
                        let _ = send_request(
                            &mut stream,
                            &ClientRequest::Input {
                                pane: model.focused_pane(),
                                bytes,
                            },
                        );
                    }
                }
                Event::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                        && model.renaming.is_some() =>
                {
                    // Same rule the management TUI's overlays use: a click
                    // outside the thing being edited discards it rather
                    // than silently confirming or acting on the click.
                    model.renaming = None;
                    model.dirty = true;
                }
                Event::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                        && model.agent_picker.is_some() =>
                {
                    let hit = model
                        .hits
                        .iter()
                        .find(|(rect, _)| {
                            rect.x <= mouse.column
                                && mouse.column < rect.x + rect.width
                                && rect.y <= mouse.row
                                && mouse.row < rect.y + rect.height
                        })
                        .map(|(_, hit)| *hit);
                    match hit {
                        Some(WorkspaceHit::PickAgent(index)) => {
                            if let Some(picker) = model.agent_picker.take()
                                && let Some(option) = picker.options.get(index)
                            {
                                let _ = send_request(
                                    &mut stream,
                                    &ClientRequest::CreateTab {
                                        label: option.display_name.clone(),
                                        columns,
                                        rows,
                                        command: Some(option.command.clone()),
                                    },
                                );
                            }
                        }
                        // Click outside the picker's own rows discards it —
                        // same rule `renaming` uses.
                        _ => model.agent_picker = None,
                    }
                    model.dirty = true;
                }
                Event::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                        && model.context_menu.is_some() =>
                {
                    let hit = model
                        .hits
                        .iter()
                        .find(|(rect, _)| {
                            rect.x <= mouse.column
                                && mouse.column < rect.x + rect.width
                                && rect.y <= mouse.row
                                && mouse.row < rect.y + rect.height
                        })
                        .map(|(_, hit)| *hit);
                    // The popup's own row is the only place left that still
                    // raises `CloseSpace`/`CloseTab` — every ordinary row
                    // stopped pushing those hits once closing moved behind
                    // this confirmation, so finding one here means the
                    // click landed on the popup, not "outside" it.
                    if let Some(menu) = model.context_menu.take()
                        && matches!(
                            hit,
                            Some(WorkspaceHit::CloseSpace(_) | WorkspaceHit::CloseTab(_))
                        )
                    {
                        send_close_request(&mut stream, menu.target);
                    }
                    model.dirty = true;
                }
                Event::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                        && model.git_view.is_some() =>
                {
                    let hit = model
                        .hits
                        .iter()
                        .find(|(rect, _)| {
                            rect.x <= mouse.column
                                && mouse.column < rect.x + rect.width
                                && rect.y <= mouse.row
                                && mouse.row < rect.y + rect.height
                        })
                        .map(|(_, hit)| *hit);
                    // Mirrors `WorkspaceHit::ResizeSidebar` below: arms
                    // dragging instead of reaching `git_diff::handle_mouse`,
                    // whose `WorkspaceHit` match has no arm for a hit that
                    // isn't about the file tree or diff themselves.
                    if hit == Some(WorkspaceHit::ResizeGitTree) {
                        model.dragging_git_tree = true;
                    } else if let Some(view) = model.git_view.as_mut()
                        && matches!(
                            git_diff::handle_mouse(view, hit),
                            git_diff::GitViewOutcome::Close
                        )
                    {
                        model.git_view = None;
                    }
                    model.dirty = true;
                }
                Event::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Drag(MouseButton::Left)
                        && model.dragging_git_tree =>
                {
                    let frame_area = Rect::new(0, 0, size.width, size.height);
                    let (tree_column, diff_column, _footer) =
                        git_diff::content_columns(frame_area, model.git_tree_width);
                    let new_width = git_diff::clamp_tree_width(
                        mouse.column.saturating_sub(tree_column.x),
                        tree_column.width + diff_column.width,
                    );
                    if model.git_tree_width != Some(new_width) {
                        model.git_tree_width = Some(new_width);
                        model.dirty = true;
                    }
                }
                Event::Mouse(mouse)
                    if matches!(
                        mouse.kind,
                        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                    ) && model.git_view.is_some() =>
                {
                    if let Some(view) = model.git_view.as_mut() {
                        git_diff::handle_scroll(
                            view,
                            Rect::new(0, 0, size.width, size.height),
                            model.git_tree_width,
                            mouse,
                        );
                    }
                    model.dirty = true;
                }
                Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) => {
                    let Some((hit_rect, hit)) = model
                        .hits
                        .iter()
                        .find(|(rect, _)| {
                            rect.x <= mouse.column
                                && mouse.column < rect.x + rect.width
                                && rect.y <= mouse.row
                                && mouse.row < rect.y + rect.height
                        })
                        .map(|(rect, hit)| (*rect, *hit))
                    else {
                        model.last_click = None;
                        continue;
                    };
                    let now = std::time::Instant::now();
                    let is_double_click = model.last_click.is_some_and(|(at, previous)| {
                        previous == hit && now.duration_since(at) < DOUBLE_CLICK_WINDOW
                    });
                    model.last_click = Some((now, hit));
                    if is_double_click {
                        model.last_click = None;
                        match hit {
                            WorkspaceHit::SelectTab(tab) => {
                                let label = model
                                    .session
                                    .as_ref()
                                    .and_then(|session| {
                                        session
                                            .workspace
                                            .spaces
                                            .iter()
                                            .flat_map(|space| &space.tabs)
                                            .find(|t| t.id == tab)
                                    })
                                    .map(|t| t.label.clone())
                                    .unwrap_or_default();
                                model.renaming = Some((RenameTarget::Tab(tab), label));
                                model.dirty = true;
                            }
                            WorkspaceHit::SelectSpace(space) => {
                                let label = model
                                    .session
                                    .as_ref()
                                    .and_then(|session| {
                                        session.workspace.spaces.iter().find(|s| s.id == space)
                                    })
                                    .map(|s| s.label.clone())
                                    .unwrap_or_default();
                                model.renaming = Some((RenameTarget::Space(space), label));
                                model.dirty = true;
                            }
                            _ => {}
                        }
                        continue;
                    }
                    match hit {
                        WorkspaceHit::SelectTab(tab) => {
                            let _ = send_request(&mut stream, &ClientRequest::SelectTab { tab });
                            if let Some(pane) = model.pane_for_tab(tab) {
                                let _ = send_request(
                                    &mut stream,
                                    &ClientRequest::Resize {
                                        pane,
                                        columns,
                                        rows,
                                    },
                                );
                            }
                        }
                        WorkspaceHit::CloseTab(tab) => {
                            let _ = send_request(&mut stream, &ClientRequest::CloseTab { tab });
                        }
                        WorkspaceHit::NewTab => {
                            let _ = send_request(
                                &mut stream,
                                &ClientRequest::CreateTab {
                                    label: next_shell_label(&model),
                                    columns,
                                    rows,
                                    command: None,
                                },
                            );
                        }
                        WorkspaceHit::NewAgentMenu => {
                            model.agent_picker = Some(AgentPicker {
                                options: agent_options(home),
                                selected: 0,
                                anchor: hit_rect,
                            });
                            // Unlike every other arm here, this is a purely
                            // local state change with no server round trip
                            // to eventually mark the model dirty via
                            // `apply()` — without this the popup exists in
                            // `model` but the screen never redraws to show
                            // it.
                            model.dirty = true;
                        }
                        WorkspaceHit::PickAgent(_) => {
                            // Only reachable while the picker is open, which
                            // the guarded arm above already handles; a
                            // stale hit here (picker just closed) is a
                            // no-op.
                        }
                        WorkspaceHit::SelectSpace(space) => {
                            let _ =
                                send_request(&mut stream, &ClientRequest::SelectSpace { space });
                            // Resize the pane the same way `SelectTab` does
                            // — switching spaces switches which tab (and so
                            // which pane) is focused, same as switching
                            // tabs within one space already does.
                            if let Some(pane) = model.session.as_ref().and_then(|session| {
                                session
                                    .workspace
                                    .spaces
                                    .iter()
                                    .find(|s| s.id == space)
                                    .map(|s| s.selected_tab)
                                    .and_then(|tab| model.pane_for_tab(tab))
                            }) {
                                let _ = send_request(
                                    &mut stream,
                                    &ClientRequest::Resize {
                                        pane,
                                        columns,
                                        rows,
                                    },
                                );
                            }
                        }
                        WorkspaceHit::CloseSpace(_) => {
                            // Only reachable while the context menu is
                            // open, which the guarded arm above already
                            // handles — same as `PickAgent` above for the
                            // agent picker.
                        }
                        WorkspaceHit::NewSpace => {
                            let _ = send_request(
                                &mut stream,
                                &ClientRequest::CreateSpace {
                                    label: next_space_label(&model),
                                    columns,
                                    rows,
                                },
                            );
                        }
                        WorkspaceHit::OpenGitView => {
                            open_git_view(&mut model);
                        }
                        WorkspaceHit::GitSelectFile(_)
                        | WorkspaceHit::GitSelectWorktree(_)
                        | WorkspaceHit::ResizeGitTree
                        | WorkspaceHit::CloseGitView => {
                            // Only reachable while the git view is open,
                            // which its own guarded arm below already
                            // handles — same as `PickAgent`/`CloseSpace`
                            // above for the other two overlays.
                        }
                        WorkspaceHit::SwitchToManagement => {
                            let _ = send_request(&mut stream, &ClientRequest::Detach);
                            return Ok(WorkspaceExit::Management);
                        }
                        WorkspaceHit::ResizeSidebar => {
                            model.dragging_sidebar = true;
                        }
                    }
                }
                Event::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Drag(MouseButton::Left)
                        && model.dragging_sidebar =>
                {
                    let new_width = super::clamp_sidebar_width(
                        mouse.column.saturating_sub(layout.sidebar.x),
                        size.width,
                    );
                    if model.sidebar_width != Some(new_width) {
                        model.sidebar_width = Some(new_width);
                        // Written straight through to the shared value (not
                        // just kept on `model`) so a Ctrl+O switch to
                        // management picks up this width immediately,
                        // instead of only on the next drag.
                        *sidebar_width = model.sidebar_width;
                        model.dirty = true;
                    }
                }
                Event::Mouse(mouse) if mouse.kind == MouseEventKind::Up(MouseButton::Left) => {
                    model.dragging_sidebar = false;
                    model.dragging_git_tree = false;
                }
                Event::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Down(MouseButton::Right)
                        && model.renaming.is_none()
                        && model.agent_picker.is_none() =>
                {
                    // The only way to close a space or an agent tab: right-
                    // click it, then confirm in the popup this opens (see
                    // `ContextMenu`) — never a direct click, so a stray
                    // click can't kill a running agent or a whole space's
                    // worth of them by accident.
                    let hit = model
                        .hits
                        .iter()
                        .find(|(rect, _)| {
                            rect.x <= mouse.column
                                && mouse.column < rect.x + rect.width
                                && rect.y <= mouse.row
                                && mouse.row < rect.y + rect.height
                        })
                        .map(|(rect, hit)| (*rect, *hit));
                    if let Some((anchor, WorkspaceHit::SelectSpace(space))) = hit
                        && model
                            .session
                            .as_ref()
                            .is_some_and(|session| session.workspace.spaces.len() > 1)
                    {
                        model.context_menu = Some(ContextMenu {
                            target: CloseTarget::Space(space),
                            anchor,
                        });
                        model.dirty = true;
                    } else if let Some((anchor, WorkspaceHit::SelectTab(tab))) = hit
                        && model.session.as_ref().is_some_and(|session| {
                            session.workspace.spaces.iter().any(|space| {
                                space.tabs.len() > 1 && space.tabs.iter().any(|t| t.id == tab)
                            })
                        })
                    {
                        model.context_menu = Some(ContextMenu {
                            target: CloseTarget::Tab(tab),
                            anchor,
                        });
                        model.dirty = true;
                    }
                }
                Event::Resize(_, _) | Event::Mouse(_) => {}
                _ => {}
            }
        }
    }
}

/// `pub(super)` (not private) so [`git_diff`] — a sibling module under
/// `ui`, not a child of `orchestrator` — can construct Git-view hits
/// from its own render function and push them into the same `hits` vec
/// every other overlay already shares, rather than this workspace client
/// threading a second, parallel hit-testing vec just for one overlay.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum WorkspaceHit {
    SelectTab(TabId),
    CloseTab(TabId),
    NewTab,
    /// Opens the agent picker (`WorkspaceModel::agent_picker`) — the tab
    /// strip's "✦" button, creating a new agent tab inside the selected
    /// space.
    NewAgentMenu,
    /// One row of the open agent picker, by index into its `options`.
    PickAgent(usize),
    /// A space's header row in the sidebar — click selects it (switching
    /// which space's tabs the tab strip and pane show).
    SelectSpace(SpaceId),
    CloseSpace(SpaceId),
    /// The sidebar's "+ new" row — creates a new space directly (no
    /// picker; unlike an agent tab, a space has no "kind" to choose).
    NewSpace,
    /// The tab strip's right-corner button — opens the git changes view
    /// (`WorkspaceModel::git_view`), scoped to the active tab's live `cwd`.
    OpenGitView,
    /// One row of the open git changes view's changed-files list, by index.
    GitSelectFile(usize),
    /// A primary or linked worktree heading in the Git changes view.
    GitSelectWorktree(usize),
    /// The Git changes view's tree/diff divider — mirrors `ResizeSidebar`
    /// below, same mousedown-arms/`Drag`-events-move-it shape, just for
    /// `WorkspaceModel::git_tree_width` instead of `sidebar_width`.
    ResizeGitTree,
    /// The "×" in the Git changes view's own top-right corner — the
    /// click-driven counterpart to `Esc`/the shortcut that opened it (see
    /// `GitViewOutcome::Close`, which both funnel through).
    CloseGitView,
    SwitchToManagement,
    ResizeSidebar,
}

/// What [`WorkspaceModel::renaming`] is currently editing — a tab or a
/// space header both use the exact same inline-edit interaction (double-
/// click to enter, Enter/Esc/Backspace/typing to edit, click-away to
/// discard), so one buffer serves both; this just says which request to
/// send on commit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RenameTarget {
    Tab(TabId),
    Space(SpaceId),
}

/// A second `Down(Left)` on the same hit within this window counts as a
/// double-click (enters tab rename); slower than this, it's just another
/// single click.
const DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(400);

/// One selectable row of the agent picker: what to show, and the `argv` to
/// launch in the new pane if chosen.
struct AgentOption {
    display_name: String,
    command: Vec<String>,
}

/// Open state of the "+ new agent" popup (`WorkspaceHit::NewAgentMenu`) —
/// built fresh each time it opens from `agent_options`, never persisted.
struct AgentPicker {
    options: Vec<AgentOption>,
    selected: usize,
    /// The tab strip's "✦" button's own rect — the popup anchors just
    /// under it.
    anchor: Rect,
}

/// What a right-click-opened [`ContextMenu`] would close.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CloseTarget {
    Space(SpaceId),
    Tab(TabId),
}

/// Open state of the close-confirmation popup a right-click on a space
/// header or agent tab raises. A space/tab is never closable from a plain
/// left click any more — right-click, then this menu's own "close" row —
/// deliberately two steps, so an accidental click can't kill a running
/// agent or an entire space's worth of them. Built fresh on each
/// right-click, never persisted.
struct ContextMenu {
    target: CloseTarget,
    /// The right-clicked row's own rect — the popup anchors just under it,
    /// same placement rule [`AgentPicker::anchor`] uses.
    anchor: Rect,
}

/// One built-in harness's identity for recognizing which pane (if any) is
/// running it — the same alias/id vocabulary [`agent_options`] offers in the
/// agent picker, kept as its own small table so the sidebar/tab-strip
/// classification that runs on every dirty frame (see
/// [`agent_identity_for_tab`]) doesn't rebuild the registry each time; this
/// is built once per [`attach_workspace`] call instead.
struct AgentIdentity {
    /// The short, typed name — an alias when the harness has one (`claude`,
    /// where the stable id is `claude-code`), else its id (`codex`,
    /// `opencode`, whose id already is their binary name). Also the exact
    /// value `UZE_SHIM_NAME` carries (see `src/shim.rs`), which is what a
    /// shim-launched pane's live process name (see
    /// `uze_terminal::PaneRuntime::foreground_status`) resolves to.
    binary: &'static str,
    display_name: &'static str,
}

/// Resolved entirely through the generic `IntegrationPort` contract
/// (`.id()`/`.display_name()`/`.aliases()`) — never a hardcoded vendor list,
/// which `src/` is not allowed to hold (see
/// `tests/integrations/identity.rs::cli_and_tui_never_names_a_vendor_harness`).
/// A registry that fails to construct (rare — see `src/shim.rs`'s identical
/// `.ok()` fallback) just yields no identities rather than failing the
/// whole workspace session.
fn agent_identities(home: &UzeHome) -> Vec<AgentIdentity> {
    let Ok(registry) = IntegrationRegistry::builtin(home) else {
        return Vec::new();
    };
    registry
        .iter()
        .map(|integration| AgentIdentity {
            binary: integration
                .aliases()
                .first()
                .copied()
                .unwrap_or_else(|| integration.id()),
            display_name: integration.display_name(),
        })
        .collect()
}

/// The harnesses the agent picker offers — one row per
/// [`AgentIdentity`], `command` set to launch that identity's `binary`.
fn agent_options(home: &UzeHome) -> Vec<AgentOption> {
    agent_identities(home)
        .into_iter()
        .map(|identity| AgentOption {
            display_name: identity.display_name.to_owned(),
            command: vec![identity.binary.to_owned()],
        })
        .collect()
}

/// The recognized agent, if any, running in `tab`'s focused pane — matched
/// against `identities` primarily by live foreground process name (a
/// shim-launched process reports its invoked alias there via
/// `UZE_SHIM_NAME`, not its raw `comm` — see
/// `uze_terminal::PaneRuntime::foreground_status`), falling back to the
/// tab's own label for the brief window right after it opens through the
/// agent picker (label is seeded to the harness's display name) before
/// the first status probe lands. Returns the harness's short binary/alias
/// name (`claude`, `codex`, …) — what the sidebar and tab strip show in
/// place of the raw process string, and what decides whether a tab lists
/// under "agents" or "shell" at all.
fn agent_identity_for_tab<'a>(identities: &'a [AgentIdentity], tab: &Tab) -> Option<&'a str> {
    let process = pane_in_layout(&tab.layout, tab.focus.pane).map(|pane| pane.process.as_str());
    identities
        .iter()
        .find(|identity| {
            process.is_some_and(|process| process.eq_ignore_ascii_case(identity.binary))
                || tab.label.eq_ignore_ascii_case(identity.display_name)
        })
        .map(|identity| identity.binary)
}

#[derive(Default)]
struct WorkspaceModel {
    session: Option<Session>,
    panes: BTreeMap<PaneId, PaneSnapshot>,
    last_size: (u16, u16),
    error: Option<String>,
    tick: usize,
    hits: Vec<(Rect, WorkspaceHit)>,
    /// Set whenever applying an event (or a resize) changes what should be
    /// on screen; the input loop only redraws when this is true. Redrawing
    /// unconditionally at the input-poll rate was the other half of the
    /// workspace client's earlier CPU/latency problem (see [`POLL`]):
    /// ratatui re-copying and diffing a full grid ~60x/sec regardless of
    /// whether anything changed.
    dirty: bool,
    /// User-dragged sidebar width; `None` falls back to `sidebar_width_for`.
    /// Client-local presentation state — never sent to the server.
    sidebar_width: Option<u16>,
    dragging_sidebar: bool,
    /// What's being renamed (a tab or a space) and its live edit buffer.
    /// While set, all keyboard input edits this instead of reaching the
    /// pane, and any click elsewhere cancels it (same "click outside
    /// discards" rule the management TUI's overlays use).
    renaming: Option<(RenameTarget, String)>,
    last_click: Option<(std::time::Instant, WorkspaceHit)>,
    /// Open state of the "+ new agent" popup; `None` when closed. Same
    /// "click outside discards" rule as `renaming`.
    agent_picker: Option<AgentPicker>,
    /// Open state of the right-click close-confirmation popup; `None` when
    /// closed. Same "click outside discards" rule as `renaming`.
    context_menu: Option<ContextMenu>,
    /// Open state of the git changes overlay; `None` when closed. Unlike
    /// `renaming`/`agent_picker`/`context_menu` there is no "click outside
    /// discards" rule — it covers the full frame, so there is no outside;
    /// `Esc` (or the same shortcut that opened it) is the only dismissal.
    git_view: Option<git_diff::GitView>,
    /// User-dragged Git changes tree width; `None` falls back to its own
    /// responsive default. Mirrors `sidebar_width`/`dragging_sidebar`
    /// above, kept on the model rather than on `GitView` itself so it
    /// survives closing and reopening the overlay within the same
    /// session, the same way the sidebar's width survives switching tabs.
    git_tree_width: Option<u16>,
    dragging_git_tree: bool,
    /// Cached Git summary for the selected agent/shell tab's live cwd.
    /// Stored client-side because it is display chrome, not terminal session
    /// state that belongs in `uze-terminal`.
    git_badge: Option<GitBadge>,
}

struct GitBadge {
    cwd: PathBuf,
    summary: Option<git_diff::GitChangeSummary>,
    checked_at: Instant,
}
impl WorkspaceModel {
    fn apply(&mut self, event: ClientEvent) {
        if !matches!(event, ClientEvent::Error { .. }) {
            self.error = None;
        }
        self.dirty = true;
        match event {
            ClientEvent::Attached { session } => self.session = Some(session),
            ClientEvent::Snapshot { session, panes } => {
                self.session = Some(session);
                self.panes = panes.into_iter().map(|pane| (pane.pane, pane)).collect();
            }
            ClientEvent::SessionUpdated { session } => {
                self.session = Some(session);
            }
            ClientEvent::Damage(damage) => {
                let entry = self
                    .panes
                    .entry(damage.pane)
                    .or_insert_with(|| blank_pane(damage.pane, damage.columns, damage.rows));
                if entry.columns != damage.columns || entry.rows != damage.rows {
                    *entry = blank_pane(damage.pane, damage.columns, damage.rows);
                }
                entry.cursor = damage.cursor;
                entry.alternate_screen = damage.alternate_screen;
                for (row, column, cell) in damage.changed {
                    let index =
                        usize::from(row) * usize::from(damage.columns) + usize::from(column);
                    if let Some(slot) = entry.cells.get_mut(index) {
                        *slot = cell;
                    }
                }
            }
            ClientEvent::Error { message } => self.error = Some(message),
            ClientEvent::Detached | ClientEvent::Stopped => {}
        }
    }
    fn focused_pane(&self) -> PaneId {
        self.session
            .as_ref()
            .map(|session| session.selected_tab().focus.pane)
            .unwrap_or(PaneId(1))
    }
    fn selected_tab(&self) -> Option<TabId> {
        self.session
            .as_ref()
            .map(|session| session.selected_tab().id)
    }
    fn pane_for_tab(&self, tab: TabId) -> Option<PaneId> {
        self.session.as_ref().and_then(|session| {
            session
                .workspace
                .spaces
                .iter()
                .flat_map(|space| &space.tabs)
                .find(|candidate| candidate.id == tab)
                .map(|candidate| candidate.focus.pane)
        })
    }
    fn refresh_git_badge(&mut self) {
        let cwd = self.session.as_ref().and_then(|session| {
            let tab = session.selected_tab();
            pane_in_layout(&tab.layout, tab.focus.pane).map(|pane| pane.cwd.clone())
        });
        let now = Instant::now();
        if self.git_badge.as_ref().is_some_and(|badge| {
            cwd.as_ref().is_some_and(|cwd| cwd == &badge.cwd)
                && now.duration_since(badge.checked_at) < GIT_BADGE_REFRESH
        }) {
            return;
        }
        self.git_badge = cwd.map(|cwd| GitBadge {
            summary: git_diff::change_summary(&cwd),
            cwd,
            checked_at: now,
        });
    }
}

fn blank_pane(pane: PaneId, columns: u16, rows: u16) -> PaneSnapshot {
    PaneSnapshot {
        pane,
        columns,
        rows,
        cursor: Cursor { column: 0, row: 0 },
        alternate_screen: false,
        cells: vec![blank_cell(); usize::from(columns) * usize::from(rows)],
    }
}

fn blank_cell() -> RenderCell {
    RenderCell {
        character: ' ',
        foreground: TerminalColor::DefaultForeground,
        background: TerminalColor::DefaultBackground,
        attributes: CellAttributes::default(),
    }
}

// --- Layout --------------------------------------------------------------

struct WorkspaceLayout {
    sidebar: Rect,
    tab_strip: Rect,
    pane: Rect,
}

/// The one source of truth for workspace geometry — both the renderer and
/// the input loop's resize/CreateTab sizing call this, so the PTY dimensions
/// sent to the server always match the rect actually drawn into.
/// `sidebar_width_override` is the user's dragged width, if any (see
/// [`WorkspaceModel::sidebar_width`]); `None` uses the responsive default.
/// Only two areas span the full frame height — menu (sidebar) and main
/// container — there is no separate global header/footer row; the brand
/// and health chrome that used to live in a titlebar now opens the sidebar
/// itself (see [`render_sidebar`]), and this mode never shows the help
/// toolbar (see [`ui::render_footer`](crate::ui::render_footer) — that
/// stays exclusive to the management TUI).
fn compute_layout(frame_area: Rect, sidebar_width_override: Option<u16>) -> WorkspaceLayout {
    // Flush against the top row, not inset by one — the mode toggle is
    // this TUI's own top edge, and floating it a row down from the real
    // terminal top just read as wasted vertical space. One blank row is
    // still kept at the *bottom* (`saturating_sub(1)`, not `2`), matching
    // `management::compute_layout`'s identical rationale there: unlike the
    // top, that gap keeps the last row from reading as clipped.
    let area = Rect::new(
        frame_area.x,
        frame_area.y,
        frame_area.width,
        frame_area.height.saturating_sub(1),
    );
    let sidebar_width = sidebar_width_override
        .map(|width| super::clamp_sidebar_width(width, area.width))
        .unwrap_or_else(|| super::sidebar_width_for(area.width));
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(sidebar_width), Constraint::Min(10)])
        .split(area);
    let content_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(columns[1]);
    // No left inset either, matching the sidebar's own flush
    // `Padding::new(1, 0, 0, 0)` on its side of the same divider — the two
    // panes' content used to sit at mismatched distances from it (sidebar
    // text 1 column away, pane cells flush) until the sidebar's own inset
    // dropped to 0; keeping both at 0 here is what makes the divider read
    // as one straight line with even margins on both sides again, not a
    // lopsided one. The right side keeps its 1-column margin — that's
    // independent, matching the tab strip's own right padding against the
    // frame's outer edge, nothing to do with the divider. This is the rect
    // the PTY is actually sized to (see the resize logic that reads
    // `layout.pane.width/height`), so insetting it here — not just where
    // it's drawn — keeps what the shell thinks its size is in sync with
    // what's visible.
    let pane = Rect::new(
        content_rows[1].x,
        content_rows[1].y,
        content_rows[1].width.saturating_sub(1),
        content_rows[1].height,
    );
    WorkspaceLayout {
        sidebar: columns[0],
        tab_strip: content_rows[0],
        pane,
    }
}

// --- Rendering -------------------------------------------------------------

fn render(
    frame: &mut ratatui::Frame<'_>,
    model: &WorkspaceModel,
    identities: &[AgentIdentity],
    hits: &mut Vec<(Rect, WorkspaceHit)>,
) {
    frame.render_widget(
        Block::default().style(Style::default().bg(super::BASE).fg(super::TEXT_PRIMARY)),
        frame.area(),
    );
    // The git changes overlay covers the entire frame when open (see
    // `git_diff::render`) — everything below would just be drawn and
    // immediately hidden underneath it, so skip it outright rather than
    // paying for a sidebar/tab-strip/pane render this frame will never
    // show.
    if let Some(view) = &model.git_view {
        git_diff::render(frame, view, frame.area(), model.git_tree_width, hits);
        return;
    }
    let layout = compute_layout(frame.area(), model.sidebar_width);
    render_sidebar(frame, layout.sidebar, model, identities, hits);
    // The sidebar's own hairline right border doubles as a drag handle —
    // it sits just past `inner` (which `render_sidebar` never draws into),
    // so this can't collide with any row hit pushed there.
    hits.push((
        Rect::new(
            layout.sidebar.right().saturating_sub(1),
            layout.sidebar.y,
            1,
            layout.sidebar.height,
        ),
        WorkspaceHit::ResizeSidebar,
    ));
    render_tab_strip(frame, layout.tab_strip, model, identities, hits);
    render_pane(frame, layout.pane, model);
    // Drawn last so it sits on top of the pane — same ordering the
    // management TUI's overlays use in its own `render`. Anchored to
    // `picker.anchor` (the "✦" button's own rect) rather than centered on
    // the whole frame — a dropdown hanging off the thing you clicked, not a
    // modal interrupting the screen.
    if let Some(picker) = &model.agent_picker {
        render_agent_picker(frame, frame.area(), picker.anchor, picker, hits);
    }
    if let Some(menu) = &model.context_menu {
        render_context_menu(frame, frame.area(), menu, hits);
    }
}

/// A small popup listing `agent_options`, opened by the tab strip's "✦"
/// button — a dropdown anchored just below it, creating the
/// picked agent as a new tab in the currently selected space. Not built on
/// the management TUI's `render_modal`/`modal_block` (those are shaped for
/// static text, not a selectable, hit-testable list) — this is
/// self-contained, styled by hand to match the same palette.
fn render_agent_picker(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    anchor: Rect,
    picker: &AgentPicker,
    hits: &mut Vec<(Rect, WorkspaceHit)>,
) {
    let content_width = picker
        .options
        .iter()
        .map(|option| option.display_name.chars().count() as u16)
        .max()
        .unwrap_or(16)
        .max("no harnesses found".len() as u16);
    let width = (content_width + 6).min(area.width);
    let height = (picker.options.len().max(1) as u16 + 2).min(area.height);
    let popup = Rect::new(
        anchor.x.min((area.x + area.width).saturating_sub(width)),
        (anchor.y + anchor.height).min((area.y + area.height).saturating_sub(height)),
        width,
        height,
    );
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" new agent ")
        .title_style(
            Style::default()
                .fg(super::ACCENT)
                .add_modifier(Modifier::BOLD),
        )
        .borders(Borders::ALL)
        .border_style(Style::default().fg(super::BORDER))
        .style(Style::default().bg(super::BASE));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    if picker.options.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "no harnesses found",
                Style::default().fg(super::MUTED),
            )),
            inner,
        );
        return;
    }
    for (index, option) in picker.options.iter().enumerate() {
        if index as u16 >= inner.height {
            break;
        }
        let row = Rect::new(inner.x, inner.y + index as u16, inner.width, 1);
        let selected = index == picker.selected;
        // A filled bar for the selected row, not just bold text — the same
        // narrowly-scoped exception to this design's usual no-filled-
        // surfaces rule the Work/Manage toggle already makes, for the same
        // reason: a keyboard-navigable menu needs the affordance.
        let (style, text) = if selected {
            let style = Style::default()
                .bg(super::ACCENT)
                .fg(super::BASE)
                .add_modifier(Modifier::BOLD);
            let text = format!(
                " {:<width$}",
                option.display_name,
                width = inner.width.saturating_sub(1) as usize
            );
            (style, text)
        } else {
            let style = Style::default().fg(super::NAV_INACTIVE);
            (style, format!(" {}", option.display_name))
        };
        frame.render_widget(Paragraph::new(Span::styled(text, style)), row);
        hits.push((row, WorkspaceHit::PickAgent(index)));
    }
}

/// The right-click close-confirmation popup — one "close" row, styled in
/// `DANGER` to read as destructive, closing whatever [`ContextMenu::target`]
/// names. Same anchoring mechanics as [`render_agent_picker`] (anchored
/// just under the right-clicked row); see [`ContextMenu`]'s own doc comment
/// for why this exists instead of a direct click.
fn render_context_menu(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    menu: &ContextMenu,
    hits: &mut Vec<(Rect, WorkspaceHit)>,
) {
    let (title, hit) = match menu.target {
        CloseTarget::Space(space) => (" close space ", WorkspaceHit::CloseSpace(space)),
        CloseTarget::Tab(tab) => (" close agent ", WorkspaceHit::CloseTab(tab)),
    };
    let label = "close";
    let width = (title.len().max(label.len() + 2) as u16 + 2).min(area.width);
    let height = 3.min(area.height);
    let popup = Rect::new(
        menu.anchor
            .x
            .min((area.x + area.width).saturating_sub(width)),
        (menu.anchor.y + menu.anchor.height).min((area.y + area.height).saturating_sub(height)),
        width,
        height,
    );
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(title)
        .title_style(
            Style::default()
                .fg(super::DANGER)
                .add_modifier(Modifier::BOLD),
        )
        .borders(Borders::ALL)
        .border_style(Style::default().fg(super::BORDER))
        .style(Style::default().bg(super::BASE));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let row = Rect::new(inner.x, inner.y, inner.width, 1);
    frame.render_widget(
        Paragraph::new(Span::styled(
            format!(" {label}"),
            Style::default().fg(super::DANGER),
        )),
        row,
    );
    hits.push((row, hit));
}

/// A two-level tree, one block per space the user has created (blank-line
/// separated — see the loop below), each expanded (no collapse/accordion)
/// into the agent tabs [`agent_identity_for_tab`] recognizes as running
/// inside it — `●`/`○` for selected/unselected plus its label, and a dim
/// caption line underneath with its pane's live `cwd · alias` (the alias in
/// place of the raw process name — see [`agent_identity_for_tab`]). A space
/// with no agent tabs shows its current `cwd` alone in place of the tree,
/// so an empty space still reads as "somewhere", not blank. Plain shell
/// tabs (and anything else not recognized as an agent) never appear here;
/// they still exist in the tab strip above the pane (see
/// [`render_tab_strip`]), scoped to whichever space is selected. The
/// underlying workspace/directory this client is attached to (see
/// `Workspace` in `uze-terminal`) is deliberately never shown — it's
/// infrastructure the user never organizes by; spaces are the only unit
/// that matters here.
fn render_sidebar(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &WorkspaceModel,
    identities: &[AgentIdentity],
    hits: &mut Vec<(Rect, WorkspaceHit)>,
) {
    let border_color = if model.dragging_sidebar {
        super::ACCENT
    } else {
        super::BORDER_FAINT
    };
    // No top padding: the mode toggle must land on the exact row the tab
    // strip's own content does (that block has none either), or the two
    // panes' dividers drift out of alignment by one row. No right padding
    // either: sidebar content (the right-aligned "+ new" in particular)
    // sits flush against the divider instead of floating a column away
    // from it — only the left side keeps its 1-column inset.
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(border_color))
        .padding(Padding::new(1, 0, 0, 0));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut y = inner.y;
    let bottom = inner.y + inner.height;
    let mut row = |height: u16| -> Option<Rect> {
        if y + height > bottom {
            return None;
        }
        let rect = Rect::new(inner.x, y, inner.width, height);
        y += height;
        Some(rect)
    };

    // Mode toggle, one line: this used to be a global titlebar (brand +
    // status + Ctrl+O hint + path) spanning the whole frame; with only menu
    // + main container left, the menu opens with just enough chrome to
    // match the tab strip's height on the other TUI mode — a segmented
    // "Work" / "Manage" control standing in for the Ctrl+O keybinding
    // (still live, just no longer spelled out as text) instead of the old
    // prose hint.
    if let Some(rect) = row(1) {
        let (_work_rect, manage_rect) = super::render_mode_toggle(frame, rect, true);
        hits.push((manage_rect, WorkspaceHit::SwitchToManagement));
    }
    if let Some(error) = &model.error
        && let Some(rect) = row(1)
    {
        frame.render_widget(
            Paragraph::new(Span::styled(
                error.clone(),
                Style::default()
                    .fg(super::DANGER)
                    .add_modifier(Modifier::BOLD),
            )),
            rect,
        );
    }
    if let Some(rect) = row(1) {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "─".repeat(rect.width as usize),
                Style::default().fg(super::BORDER_FAINT),
            )),
            rect,
        );
    }

    let Some(session) = &model.session else {
        return;
    };

    // A blank row below "+ new" — the same 1-row breathing room a space
    // block gets after it (see the `row(1)` at the bottom of the loop
    // below) — so it doesn't read as glued to the first space's name.
    // Nothing above it: the divider row already separates it from the
    // mode toggle, and stacking a second blank row there just reads as
    // too much dead air for a compact menu.
    if let Some(rect) = row(1) {
        // Right-aligned, not tucked under the workspace name like the space
        // list below it — this reads as a header-row action (the same
        // place the "+"/"✦" buttons sit in the tab strip above the pane)
        // rather than as another tree item. Creates a space directly — a
        // space has no "kind" to pick, unlike an agent tab, so no picker
        // is needed here.
        let label = "+ new";
        let label_x = rect.x + rect.width.saturating_sub(label.len() as u16);
        frame.render_widget(
            Paragraph::new(Span::styled(label, Style::default().fg(super::ACCENT)))
                .alignment(Alignment::Right),
            rect,
        );
        hits.push((
            Rect::new(label_x, rect.y, label.len() as u16, 1),
            WorkspaceHit::NewSpace,
        ));
    }
    row(1);

    for space in &session.workspace.spaces {
        let is_active_space = space.id == session.workspace.selected_space;
        let Some(header_rect) = row(1) else { break };
        render_space_header(frame, header_rect, session, space, model, hits);

        let agent_tabs: Vec<&Tab> = space
            .tabs
            .iter()
            .filter(|tab| agent_identity_for_tab(identities, tab).is_some())
            .collect();

        if agent_tabs.is_empty() {
            // Nothing running here yet — show where this space currently
            // is instead of an empty gap under its header, so it still
            // reads as "somewhere", not blank. Reads off the space's own
            // selected tab (its bootstrap shell, absent any agent) rather
            // than the workspace root, so it tracks a plain `cd` the same
            // way an agent tab's own detail line already does.
            if let Some(cwd_rect) = row(1) {
                let cwd = space
                    .tabs
                    .iter()
                    .find(|tab| tab.id == space.selected_tab)
                    .and_then(|tab| pane_in_layout(&tab.layout, tab.focus.pane))
                    .map(|pane| super::display_project_path(&pane.cwd))
                    .unwrap_or_default();
                let mut spans = vec![Span::styled(
                    format!("  {cwd}"),
                    Style::default().fg(super::TEXT_DIM),
                )];
                if is_active_space {
                    fill_row_bg(&mut spans, cwd_rect.width, super::SURFACE_OVERLAY);
                }
                frame.render_widget(Paragraph::new(Line::from(spans)), cwd_rect);
            }
        }
        for (index, tab) in agent_tabs.iter().enumerate() {
            let is_last = index + 1 == agent_tabs.len();
            // One extra level of indent versus a flat list — these tabs
            // read as children of the space header row just drawn above.
            let connector = if is_last { "  └─ " } else { "  ├─ " };
            let Some(label_rect) = row(1) else { break };

            let selected = tab.id == space.selected_tab;
            let renaming_this = model
                .renaming
                .as_ref()
                .filter(|(target, _)| *target == RenameTarget::Tab(tab.id))
                .map(|(_, buffer)| buffer.as_str());
            let dot_fg = if selected {
                super::ACCENT
            } else {
                super::TEXT_FAINT
            };
            // Bright but not bold — bold is the space header's own marker
            // for "this is the active space" (see `render_space_header`);
            // an agent tab nested under it competing for the same weight
            // read as two different things both shouting "I'm the one".
            // The `●` dot above already says which tab is active.
            let label_style = Style::default().fg(if selected {
                super::TEXT_BRIGHT
            } else {
                super::NAV_INACTIVE
            });
            let connector_span = Span::styled(connector, Style::default().fg(super::TEXT_FAINT));
            let label = match renaming_this {
                Some(buffer) => Span::styled(
                    format!("{buffer}▏"),
                    Style::default()
                        .fg(super::TEXT_BRIGHT)
                        .add_modifier(Modifier::BOLD),
                ),
                None => Span::styled(tab.label.clone(), label_style),
            };
            let mut spans = vec![
                connector_span,
                Span::styled(
                    if selected { "● " } else { "○ " },
                    Style::default().fg(dot_fg),
                ),
                label,
            ];
            if is_active_space {
                fill_row_bg(&mut spans, label_rect.width, super::SURFACE_OVERLAY);
            }
            frame.render_widget(Paragraph::new(Line::from(spans)), label_rect);
            hits.push((label_rect, WorkspaceHit::SelectTab(tab.id)));

            if let Some(detail_rect) = row(1) {
                let continuation = if is_last { "     " } else { "  │  " };
                // The alias in place of the raw process name — this list only
                // ever holds tabs `agent_identity_for_tab` already resolved, so
                // it never falls back to showing something like a bare version
                // string (see that function's doc comment).
                let alias = agent_identity_for_tab(identities, tab).unwrap_or_default();
                let cwd = pane_in_layout(&tab.layout, tab.focus.pane)
                    .map(|pane| super::display_project_path(&pane.cwd))
                    .unwrap_or_default();
                let continuation_span =
                    Span::styled(continuation, Style::default().fg(super::TEXT_FAINT));
                let cwd_span = Span::styled(cwd, Style::default().fg(super::TEXT_DIM));
                let alias_span = Span::styled(alias, Style::default().fg(super::TEXT_DIM));
                // Right-aligned, not tacked onto the cwd behind a "·" —
                // cwd (where this tab lives) and the running agent are two
                // different facts, and pinning the agent to the row's own
                // right edge keeps its column stable as different tabs'
                // cwds vary in length, instead of drifting with the text
                // it used to follow. A 1-column trailing pad keeps it off
                // the sidebar's own flush-right divider (see
                // `render_sidebar`'s `Padding::new(1, 0, 0, 0)`) — that
                // padding drop suits a button glued to the edge, not a
                // plain text label.
                const TRAILING_PAD: u16 = 1;
                let used = continuation_span.width() as u16
                    + cwd_span.width() as u16
                    + alias_span.width() as u16
                    + TRAILING_PAD;
                let gap = detail_rect.width.saturating_sub(used);
                let mut spans = vec![
                    continuation_span,
                    cwd_span,
                    Span::raw(" ".repeat(gap as usize)),
                    alias_span,
                    Span::raw(" ".repeat(TRAILING_PAD as usize)),
                ];
                if is_active_space {
                    fill_row_bg(&mut spans, detail_rect.width, super::SURFACE_OVERLAY);
                }
                frame.render_widget(Paragraph::new(Line::from(spans)), detail_rect);
                // The label and its dim cwd/process caption read as one tree
                // item — clicking the caption line must select the tab too,
                // not just the label text above it.
                hits.push((detail_rect, WorkspaceHit::SelectTab(tab.id)));
            }
            // No blank row between tabs — a full row read as too much air once
            // tried (each item is already only 2 rows tall), and the "├─"/"└─"
            // connector on the next label is enough on its own to read as a new
            // sibling starting, the same way `tree`/git-log-graph style output
            // never blank-lines between nodes.
        }
        // One blank row *between* spaces (not between a tab and its own
        // detail line, which stays tight per the comment above) — each
        // space is its own block, and needs the breathing room a flat
        // tab list didn't.
        row(1);
    }
}

/// One space's header row in the sidebar tree — plain label for the rest,
/// bright/bold for the active one. The active space's whole envelope
/// (this header plus every tab/detail/cwd row nested under it — see the
/// `is_active_space` fill in [`render_sidebar`]) gets a neutral
/// [`super::SURFACE_OVERLAY`] background instead of a left accent bar, so
/// the highlight reads as "this whole block is where you are" rather than
/// a thin per-row marker or an on-brand "selected" tint (deliberately not
/// `SELECTED_BG` — that one borrows the accent hue for a different kind of
/// selection). Its own small function (unlike the tab row, which stays
/// inline in [`render_sidebar`]) purely to keep that function's now-nested
/// loop readable — this has no reuse motivation beyond that.
fn render_space_header(
    frame: &mut ratatui::Frame<'_>,
    rect: Rect,
    session: &Session,
    space: &Space,
    model: &WorkspaceModel,
    hits: &mut Vec<(Rect, WorkspaceHit)>,
) {
    let selected = space.id == session.workspace.selected_space;
    let renaming_this = model
        .renaming
        .as_ref()
        .filter(|(target, _)| *target == RenameTarget::Space(space.id))
        .map(|(_, buffer)| buffer.as_str());
    let mut label_style = Style::default().fg(if selected {
        super::TEXT_BRIGHT
    } else {
        super::NAV_INACTIVE
    });
    if selected {
        label_style = label_style.add_modifier(Modifier::BOLD);
    }
    let label = match renaming_this {
        Some(buffer) => Span::styled(
            format!(" {buffer}▏"),
            Style::default()
                .fg(super::TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ),
        None => Span::styled(format!(" {}", space.label), label_style),
    };
    let mut spans = vec![label];
    if selected {
        fill_row_bg(&mut spans, rect.width, super::SURFACE_OVERLAY);
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), rect);
    hits.push((rect, WorkspaceHit::SelectSpace(space.id)));
}

/// Stamps `bg` onto every span already in the row, then appends a
/// trailing background-filled run of spaces so the highlight spans the
/// row's full width instead of stopping at the last glyph — same pattern
/// `render_plugin_row`/`render_marketplace_row` use for their own
/// selected-row backgrounds.
fn fill_row_bg<'a>(spans: &mut Vec<Span<'a>>, width: u16, bg: Color) {
    for span in spans.iter_mut() {
        span.style = span.style.bg(bg);
    }
    let used: usize = spans.iter().map(Span::width).sum();
    let gap = (width as usize).saturating_sub(used);
    spans.push(Span::styled(" ".repeat(gap), Style::default().bg(bg)));
}

/// The label a plain "$ shell" tab opens with — numbered off the selected
/// space's current tab count (shells are per-space, same as everything
/// else in the tab strip) so opening several in a row reads as "shell 2",
/// "shell 3", … instead of every one showing the identical generic "shell".
fn next_shell_label(model: &WorkspaceModel) -> String {
    let count = model
        .session
        .as_ref()
        .map_or(0, |session| session.selected_space().tabs.len());
    format!("shell {}", count + 1)
}

/// The label a fresh space opens with — same numbering convention as
/// [`next_shell_label`], off the workspace's current space count.
fn next_space_label(model: &WorkspaceModel) -> String {
    let count = model
        .session
        .as_ref()
        .map_or(0, |session| session.workspace.spaces.len());
    format!("space {}", count + 1)
}

/// The one action a [`ContextMenu`] ever confirms — sent from both the
/// popup's own click zone and its keyboard Enter shortcut, so the request
/// this actually builds only needs writing once.
fn send_close_request<W: io::Write>(stream: &mut W, target: CloseTarget) {
    let _ = send_request(
        stream,
        &match target {
            CloseTarget::Space(space) => ClientRequest::CloseSpace { space },
            CloseTarget::Tab(tab) => ClientRequest::CloseTab { tab },
        },
    );
}

fn pane_in_layout(layout: &uze_terminal::Layout, wanted: PaneId) -> Option<&uze_terminal::Pane> {
    match layout {
        uze_terminal::Layout::Pane(pane) if pane.id == wanted => Some(pane),
        uze_terminal::Layout::Pane(_) => None,
        uze_terminal::Layout::Split { first, second, .. } => {
            pane_in_layout(first, wanted).or_else(|| pane_in_layout(second, wanted))
        }
    }
}

/// Opens the git changes overlay scoped to the *currently selected tab's*
/// live `cwd` — the hierarchy the user gave for this feature is
/// `Workspace > Space > Agent/Shell > Git`, one level further down than
/// the space itself. Snapshotted once here; the view doesn't track further
/// `cd`s in that tab while it's open (see `git_diff`'s own module doc).
fn open_git_view(model: &mut WorkspaceModel) {
    let Some(session) = model.session.as_ref() else {
        return;
    };
    let tab = session.selected_tab();
    let Some(pane) = pane_in_layout(&tab.layout, tab.focus.pane) else {
        return;
    };
    model.git_view = Some(git_diff::GitView::open(pane.cwd.clone()));
    model.dirty = true;
}

/// The horizontal tab strip above the pane: the *selected space's* shell
/// tabs only — agent tabs live exclusively in the sidebar now (see
/// [`render_sidebar`]), so a tab [`agent_identity_for_tab`] recognizes
/// never appears here, the same way a shell tab never appears in the
/// sidebar; other spaces' shell tabs don't appear here either, only the
/// currently selected space's. An active-tab marker in `ACCENT`/bold-bright
/// text, wrapped in the same neutral [`super::SURFACE_OVERLAY`] chip the
/// sidebar already uses for "this is where you are" (its active space's
/// envelope, its agent tab rows) — this strip used to skip that fill and
/// lean on text weight alone, which read as a lighter kind of "selected"
/// than everywhere else in the TUI. A dim `×` close affordance per tab once
/// more than one exists in the selected space, and trailing "+"/"✦" actions
/// to open another of either kind (both land in the selected space).
fn render_tab_strip(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &WorkspaceModel,
    identities: &[AgentIdentity],
    hits: &mut Vec<(Rect, WorkspaceHit)>,
) {
    // No left padding: the pane below sits flush against the divider (see
    // `compute_layout`'s own `content_rows[1].x`, with no left inset
    // either), so the first tab's marker has to start at that same column
    // or it reads as offset from whatever the pane shows directly under it
    // — a shell prompt in particular, which starts flush at column 0 too.
    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(super::BORDER_FAINT))
        .padding(Padding::new(0, 1, 0, 0));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(session) = &model.session else {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "connecting…",
                Style::default().fg(super::MUTED),
            )),
            inner,
        );
        return;
    };

    // Scoped to the selected space — switching spaces (sidebar) switches
    // which shells this strip shows, the actual "don't mix projects"
    // payoff of spaces existing at all.
    let space = session.selected_space();
    // Closability is a per-space rule (the server refuses to remove a
    // space's only tab — see `Session::remove_tab`), so it's judged
    // against every tab in the selected space, not just the shell ones
    // this strip goes on to show.
    let can_close = space.tabs.len() > 1;
    let mut spans = Vec::new();
    let mut x = inner.x;
    for tab in space
        .tabs
        .iter()
        .filter(|tab| agent_identity_for_tab(identities, tab).is_none())
    {
        if x >= inner.right() {
            break;
        }
        let selected = tab.id == space.selected_tab;
        let marker_fg = if selected {
            super::ACCENT
        } else {
            super::TEXT_FAINT
        };
        let label_style = if selected {
            Style::default()
                .fg(super::TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(super::NAV_INACTIVE)
        };
        let marker = Span::styled(
            if selected { "● " } else { "○ " },
            Style::default().fg(marker_fg),
        );
        let renaming_this = model
            .renaming
            .as_ref()
            .filter(|(target, _)| *target == RenameTarget::Tab(tab.id))
            .map(|(_, buffer)| buffer.as_str());
        let tab_label = match renaming_this {
            Some(buffer) => Span::styled(
                format!("{buffer}▏"),
                Style::default()
                    .fg(super::TEXT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
            None => Span::styled(tab.label.clone(), label_style),
        };
        let show_close = renaming_this.is_none() && can_close;
        let content_width =
            marker.width() as u16 + tab_label.width() as u16 + if show_close { 2 } else { 0 }; // " ×"
        // 1 column of padding on each side, reserved whether or not this
        // tab is selected — only the SURFACE_OVERLAY fill toggles with
        // `selected`, never the width. Sizing the chip itself to
        // `selected` used to mean every tab shifted horizontally the
        // moment selection moved past it, reading as the whole strip
        // "resizing" on every tab switch instead of just recoloring.
        const PAD: u16 = 1;
        let chip_start = x;
        let chip_width = content_width + 2 * PAD;

        let mut chip = vec![Span::raw(" ")];
        chip.push(marker);
        chip.push(tab_label);
        if show_close {
            chip.push(Span::raw(" "));
            chip.push(Span::styled("×", Style::default().fg(super::TEXT_DIM)));
            hits.push((
                Rect::new(chip_start + PAD + content_width - 1, inner.y, 1, 1),
                WorkspaceHit::CloseTab(tab.id),
            ));
        }
        chip.push(Span::raw(" "));
        if selected {
            fill_row_bg(&mut chip, chip_width, super::SURFACE_OVERLAY);
        }
        hits.push((
            Rect::new(chip_start, inner.y, chip_width, 1),
            WorkspaceHit::SelectTab(tab.id),
        ));
        spans.extend(chip);
        // Just 1 column between chips, not 3 — each chip already reserves
        // its own 1-column pad on both sides (see `PAD` above), so a full
        // 3-column gap on top of that read as too much air once every tab
        // carried that padding, not just the selected one.
        spans.push(Span::raw(" "));
        x += chip_width + 1;
    }
    // A "/" separates the tab list from the action buttons that follow —
    // without it the gap before them read as just another inter-tab gap,
    // not a boundary between two different kinds of thing. No leading
    // space of its own — the loop above already ends on one (the last
    // chip's trailing gap) — only a trailing one, so it sits exactly 1
    // neutral column off the tab side and 1 off the button side; baking a
    // space into both ends of `" / "` double-counted the left side and
    // left it looking closer to the buttons than to the tabs. `MUTED`, not
    // `BORDER_FAINT` — sitting on the plain backdrop out here (not a
    // filled chip the way the "│" below does), `BORDER_FAINT` read as a
    // near-invisible hairline.
    if x < inner.right() {
        spans.push(Span::styled("/", Style::default().fg(super::MUTED)));
        spans.push(Span::raw(" "));
        x += 2;
    }
    // One button, split by a divider — not two separate chips: a bold "+"
    // creates a new shell tab directly (the fast, default action), a "✦"
    // beside it opens the agent picker for anything else. "✦" carries the
    // accent (it's the one that summons an agent); "+" stays neutral,
    // just bolder, since it's the plain/default action. The divider stays
    // `BORDER_FAINT`, unlike the "/" above — it sits on this button's own
    // `SURFACE_OVERLAY_BRIGHT` fill, not the plain backdrop, so it already
    // has contrast `BORDER_FAINT` alone doesn't get out on the strip;
    // `MUTED` here read as too bright against that lighter background,
    // clashing with the plain "+"/"✦" glyphs it separates.
    // `SURFACE_OVERLAY_BRIGHT` backs the whole pair: at the plain
    // `SURFACE_OVERLAY` strength the icons read as barely there, since
    // unlike the sidebar's filled rows this pair has no bold/color weight
    // of its own otherwise carrying it.
    let button_width: u16 = 7; // " + │ ✦ "
    if x + button_width <= inner.right() {
        let action_start = x;
        let mut actions = vec![
            Span::raw(" "),
            Span::styled(
                "+",
                Style::default()
                    .fg(super::NAV_INACTIVE)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled("│", Style::default().fg(super::BORDER_FAINT)),
            Span::raw(" "),
            Span::styled("✦", Style::default().fg(super::ACCENT)),
            Span::raw(" "),
        ];
        hits.push((Rect::new(action_start, inner.y, 3, 1), WorkspaceHit::NewTab));
        hits.push((
            Rect::new(action_start + 4, inner.y, 3, 1),
            WorkspaceHit::NewAgentMenu,
        ));
        fill_row_bg(&mut actions, button_width, super::SURFACE_OVERLAY_BRIGHT);
        spans.extend(actions);
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), inner);

    // The status badge belongs to the active agent/shell tab's `cwd`, not
    // the workspace root. It is intentionally absent for a clean directory
    // or one outside Git; when it is present, it remains the entry point to
    // the full changes overlay. The `SURFACE_OVERLAY` chip (1 column of
    // padding on each side, same shape as an active tab's own chip) reads
    // as clickable the way plain colored text on the bare backdrop
    // doesn't.
    if let Some(summary) = model.git_badge.as_ref().and_then(|badge| badge.summary) {
        let mut badge = vec![
            Span::raw(" "),
            Span::styled(
                format!("+{}", summary.additions),
                Style::default().fg(super::SUCCESS),
            ),
            Span::raw(" "),
            Span::styled(
                format!("-{}", summary.deletions),
                Style::default().fg(super::DANGER),
            ),
            Span::raw(" "),
        ];
        let badge_width = badge.iter().map(Span::width).sum::<usize>() as u16;
        fill_row_bg(&mut badge, badge_width, super::SURFACE_OVERLAY);
        let badge_rect = Rect::new(
            inner.x + inner.width.saturating_sub(badge_width),
            inner.y,
            badge_width,
            1,
        );
        frame.render_widget(Paragraph::new(Line::from(badge)), badge_rect);
        hits.push((badge_rect, WorkspaceHit::OpenGitView));
    }
}

fn render_pane(frame: &mut ratatui::Frame<'_>, area: Rect, model: &WorkspaceModel) {
    let Some(snapshot) = model.panes.get(&model.focused_pane()) else {
        frame.render_widget(
            Paragraph::new(model.error.as_deref().unwrap_or(" starting shell…"))
                .style(Style::default().fg(super::MUTED)),
            area,
        );
        return;
    };
    let width = area.width.min(snapshot.columns);
    let height = area.height.min(snapshot.rows);
    let buffer = frame.buffer_mut();
    let mut encoded = [0u8; 4];
    for row in 0..height {
        for column in 0..width {
            let index = usize::from(row) * usize::from(snapshot.columns) + usize::from(column);
            if let Some(cell) = snapshot.cells.get(index) {
                buffer[(area.x + column, area.y + row)]
                    .set_symbol(cell.character.encode_utf8(&mut encoded))
                    .set_style(cell_style(cell));
            }
        }
    }
    if snapshot.cursor.row < height && snapshot.cursor.column < width {
        buffer[(
            area.x + snapshot.cursor.column,
            area.y + snapshot.cursor.row,
        )]
            .set_style(Style::default().bg(super::TEXT_BRIGHT).fg(super::BASE));
    }
}

fn cell_style(cell: &uze_terminal::RenderCell) -> Style {
    let mut style = Style::default()
        .fg(color(cell.foreground))
        .bg(color(cell.background));
    if cell.attributes.bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    if cell.attributes.dim {
        style = style.add_modifier(Modifier::DIM);
    }
    if cell.attributes.italic {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if cell.attributes.underline {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    if cell.attributes.inverse {
        style = style.add_modifier(Modifier::REVERSED);
    }
    if cell.attributes.hidden {
        style = style.add_modifier(Modifier::HIDDEN);
    }
    if cell.attributes.strikeout {
        style = style.add_modifier(Modifier::CROSSED_OUT);
    }
    style
}
fn color(color: TerminalColor) -> Color {
    match color {
        TerminalColor::DefaultForeground => super::TEXT_PRIMARY,
        TerminalColor::DefaultBackground => super::BASE,
        TerminalColor::Rgb { red, green, blue } => Color::Rgb(red, green, blue),
        TerminalColor::Indexed(index) => Color::Indexed(index),
    }
}
fn encode_key(key: KeyEvent) -> Option<Vec<u8>> {
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Char(character) if control && character.is_ascii_alphabetic() => {
            Some(vec![(character.to_ascii_lowercase() as u8) - b'a' + 1])
        }
        KeyCode::Char(character) => Some(character.to_string().into_bytes()),
        KeyCode::Enter => Some(vec![b'\r']),
        KeyCode::Backspace => Some(vec![0x7f]),
        KeyCode::Tab => Some(vec![b'\t']),
        KeyCode::Esc => Some(vec![0x1b]),
        KeyCode::Up => Some(b"\x1b[A".to_vec()),
        KeyCode::Down => Some(b"\x1b[B".to_vec()),
        KeyCode::Right => Some(b"\x1b[C".to_vec()),
        KeyCode::Left => Some(b"\x1b[D".to_vec()),
        KeyCode::Home => Some(b"\x1b[H".to_vec()),
        KeyCode::End => Some(b"\x1b[F".to_vec()),
        KeyCode::Delete => Some(b"\x1b[3~".to_vec()),
        _ => None,
    }
}
fn io_error(source: io::Error) -> UzeError {
    UzeError::Write {
        path: "terminal".into(),
        source,
    }
}
fn runtime_error(error: uze_terminal::RuntimeError) -> UzeError {
    UzeError::AcquisitionFailed(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{AgentIdentity, agent_identity_for_tab};
    use uze_terminal::{Focus, Layout, Pane, PaneId, Tab, TabId};

    fn identities() -> Vec<AgentIdentity> {
        vec![
            AgentIdentity {
                binary: "claude",
                display_name: "Claude Code",
            },
            AgentIdentity {
                binary: "codex",
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
    fn recognizes_a_freshly_opened_agent_tab_by_its_seeded_label() {
        // Right after the agent picker creates the tab, before the first
        // status probe has resolved `pane.process` past the server's
        // "shell" placeholder.
        let tab = tab_with("Codex", "shell");
        assert_eq!(agent_identity_for_tab(&identities(), &tab), Some("codex"));
    }

    #[test]
    fn a_plain_shell_matches_neither_signal() {
        let tab = tab_with("shell", "zsh");
        assert_eq!(agent_identity_for_tab(&identities(), &tab), None);
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
}
