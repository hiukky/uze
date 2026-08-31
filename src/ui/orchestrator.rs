//! Workspace client for the persistent local terminal runtime (ADR-038).
//!
//! Presentation deliberately reuses the management TUI's palette and layout
//! conventions (`super::BASE`/`ACCENT`/`BORDER`/…, hairline dividers, no
//! filled panels) so switching between the workspace and management
//! contexts with Ctrl+O reads as one product, not two.

use crate::{Result, UzeError, UzeHome};
use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Padding, Paragraph},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, BufReader},
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use uze_extensions::{ExtensionHit, git_diff};
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

/// The same frames configure the hidden `indicatif` spinner that schedules
/// this animation. Ratatui owns the alternate screen, so it paints the frame
/// instead of letting indicatif write to stderr.
const AGENT_ACTIVITY_FRAMES: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];
const AGENT_ACTIVITY_TICK: Duration = Duration::from_millis(120);
/// A submitted prompt stays active while its terminal keeps producing
/// output. Agent harnesses can spend several seconds silent while waiting
/// for a tool or network response, so this grace period favors not hiding
/// an in-flight operation over immediately clearing a returned prompt.
const AGENT_ACTIVITY_IDLE_AFTER: Duration = Duration::from_secs(10);

pub(crate) enum WorkspaceExit {
    Management,
    Quit,
}

/// Computes the harness-support read model in a background thread and
/// delivers it through `sender` — the same async path `attach_workspace`
/// uses at attach time, reused wherever the model needs a fresh read
/// instead of trusting a possibly stale earlier one (see the
/// `OpenAgentSupport` handler below).
fn spawn_support_refresh(
    home: &UzeHome,
    root: &Path,
    sender: mpsc::Sender<Result<Vec<super::agent_support::AgentSupport>>>,
) {
    let support_home = home.clone();
    let support_root = root.to_path_buf();
    thread::spawn(move || {
        let result = super::tui_application(support_home).map(|app| {
            let workspace = app.overview_workspace(&support_root).ok();
            let context_root = workspace
                .as_ref()
                .map(|workspace| workspace.root.as_path())
                .unwrap_or(&support_root);
            let context = app.context_inspect(context_root).ok();
            let agents_directory_loaded = workspace
                .as_ref()
                .is_some_and(|workspace| workspace.agents_directory_present);
            let profiles = app.list_profiles().unwrap_or_default();
            let active_profile = profiles.iter().find(|profile| profile.active);
            app.doctor()
                .harnesses
                .into_iter()
                .map(|health| {
                    super::agent_support::AgentSupport::from_health(
                        health,
                        context.as_ref(),
                        agents_directory_loaded,
                        active_profile,
                    )
                })
                .collect()
        });
        let _ = sender.send(result);
    });
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
    // A registered harness set doesn't change mid-session, so this is built
    // once per attach.
    let identities = agent_identities(home);
    // This is the exact `HarnessHealth` read model used by the Integrations
    // screen. It loads asynchronously so inspecting support never delays a
    // live terminal attach or a pane redraw. Unlike `identities` above, this
    // one *does* go stale — `AGENTS.md`/the runtime projection can change
    // underneath an open workspace (another session writing it, a race in
    // `claude_runtime_projection` resolving) — so `OpenAgentSupport` below
    // fires a fresh one on every open rather than trusting this attach-time
    // snapshot for the rest of the session.
    let (support_sender, support_receiver) = mpsc::channel();
    spawn_support_refresh(home, root, support_sender.clone());
    let activity_spinner = ProgressBar::new_spinner();
    activity_spinner.set_draw_target(ProgressDrawTarget::hidden());
    activity_spinner
        .set_style(ProgressStyle::default_spinner().tick_strings(&AGENT_ACTIVITY_FRAMES));
    let mut next_activity_tick = Instant::now();
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
        if let Ok(result) = support_receiver.try_recv() {
            model.agent_support = result.unwrap_or_default();
            model.dirty = true;
        }
        if let Some(view) = model.git_view.as_mut()
            && view.refresh_due()
        {
            view.refresh();
            model.dirty = true;
        }
        if model.expire_agent_activity(Instant::now()) {
            model.dirty = true;
        }
        if workspace_has_active_agent_operation(&model, &identities) {
            let now = Instant::now();
            if now >= next_activity_tick {
                activity_spinner.inc(1);
                model.tick = activity_spinner.position() as usize;
                next_activity_tick = now + AGENT_ACTIVITY_TICK;
                model.dirty = true;
            }
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
                Event::Paste(text) if model.renaming.is_some() => {
                    if let Some((_, buffer)) = model.renaming.as_mut() {
                        buffer.push_str(text.trim_end_matches(['\r', '\n']));
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
                                        cwd: selected_pane_cwd(&model),
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
                Event::Key(_) if model.support_dropdown.is_some() => {
                    model.support_dropdown = None;
                    model.dirty = true;
                }
                Event::Key(key) if model.context_menu.is_some() => {
                    // Up/Down move the selection; Enter confirms whichever
                    // row is selected; anything else (Esc included)
                    // dismisses without acting — same "only reacts to its
                    // own actions" rule the agent picker above uses.
                    match key.code {
                        KeyCode::Up => {
                            if let Some(menu) = model.context_menu.as_mut() {
                                menu.selected = menu.selected.saturating_sub(1);
                            }
                        }
                        KeyCode::Down => {
                            if let Some(menu) = model.context_menu.as_mut() {
                                menu.selected =
                                    (menu.selected + 1).min(menu.items.len().saturating_sub(1));
                            }
                        }
                        KeyCode::Enter => {
                            if let Some(menu) = model.context_menu.take()
                                && let Some(action) = menu.items.get(menu.selected).copied()
                            {
                                dispatch_menu_action(
                                    &mut stream,
                                    &mut model,
                                    &identities,
                                    menu.target,
                                    action,
                                );
                            }
                        }
                        _ => model.context_menu = None,
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
                            cwd: None,
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
                        .map(|tab| tab.id)
                    {
                        model.acknowledge_completed_agent_tab(tab);
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
                }
                Event::Key(key) => {
                    if let Some(bytes) = encode_key(key) {
                        let pane = model.focused_pane();
                        if bytes.contains(&b'\r') || bytes.contains(&b'\n') {
                            model.note_agent_prompt_submission(pane, &identities);
                        }
                        let _ = send_request(&mut stream, &ClientRequest::Input { pane, bytes });
                    }
                }
                Event::Paste(text) if model.no_modal_open() => {
                    forward_paste(&mut stream, &model, &text);
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
                                        cwd: selected_pane_cwd(&model),
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
                    if mouse.kind == MouseEventKind::Moved && model.agent_picker.is_some() =>
                {
                    // Keep this dropdown's pointer behavior aligned with the
                    // sidebar context menu: the highlighted option follows
                    // the cursor, while keyboard navigation remains intact.
                    let hit = model
                        .hits
                        .iter()
                        .rev()
                        .find(|(rect, _)| {
                            rect.x <= mouse.column
                                && mouse.column < rect.x + rect.width
                                && rect.y <= mouse.row
                                && mouse.row < rect.y + rect.height
                        })
                        .map(|(_, hit)| *hit);
                    if let Some(WorkspaceHit::PickAgent(index)) = hit
                        && let Some(picker) = model.agent_picker.as_mut()
                        && picker.selected != index
                    {
                        picker.selected = index;
                        model.dirty = true;
                    }
                }
                Event::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                        && model.support_dropdown.is_some() =>
                {
                    // Informational dropdown: every click simply dismisses
                    // it, preventing the click from leaking into the pane.
                    model.support_dropdown = None;
                    model.dirty = true;
                }
                Event::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                        && model.context_menu.is_some() =>
                {
                    // `.rev()`: the popup renders last, so its own rows sit
                    // at the tail of `hits` — searching forward could match
                    // an older, now visually-covered sidebar row underneath
                    // it instead (the tight, gapless sidebar packing meant
                    // this landed on a covered row far more often than not,
                    // which is what made the popup's own click feel
                    // intermittent — it depended on which row was
                    // right-clicked, not on timing).
                    let hit = model
                        .hits
                        .iter()
                        .rev()
                        .find(|(rect, _)| {
                            rect.x <= mouse.column
                                && mouse.column < rect.x + rect.width
                                && rect.y <= mouse.row
                                && mouse.row < rect.y + rect.height
                        })
                        .map(|(_, hit)| *hit);
                    let action = match hit {
                        Some(WorkspaceHit::ContextMenuAction(index)) => model
                            .context_menu
                            .as_ref()
                            .and_then(|menu| menu.items.get(index).copied()),
                        _ => None,
                    };
                    // Dismiss unconditionally (any click, on the popup or
                    // outside it, closes the menu) but only dispatch when
                    // the click actually resolved to one of its own rows —
                    // the two used to be one `if let` that discarded the
                    // menu before checking the hit, so a miss silently
                    // dismissed without acting instead of visibly no-oping.
                    let target = model.context_menu.take().map(|menu| menu.target);
                    if let (Some(target), Some(action)) = (target, action) {
                        dispatch_menu_action(&mut stream, &mut model, &identities, target, action);
                    }
                    model.dirty = true;
                }
                Event::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Moved && model.context_menu.is_some() =>
                {
                    // Hovering a row selects it, same as Up/Down — so the
                    // popup reads as a real menu (highlight follows the
                    // cursor) instead of only reacting to a click. Only
                    // marks the frame dirty when the hover actually moved
                    // onto a different row, so waving the mouse across the
                    // rest of the screen doesn't force a redraw every tick.
                    let hit = model
                        .hits
                        .iter()
                        .rev()
                        .find(|(rect, _)| {
                            rect.x <= mouse.column
                                && mouse.column < rect.x + rect.width
                                && rect.y <= mouse.row
                                && mouse.row < rect.y + rect.height
                        })
                        .map(|(_, hit)| *hit);
                    if let Some(WorkspaceHit::ContextMenuAction(index)) = hit
                        && let Some(menu) = model.context_menu.as_mut()
                        && menu.selected != index
                    {
                        menu.selected = index;
                        model.dirty = true;
                    }
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
                    // which only knows about `ExtensionHit`s that are its
                    // own — the resize handle's drag lifecycle belongs to
                    // this workspace client, not the extension.
                    let extension_hit = match hit {
                        Some(WorkspaceHit::Extension(extension_hit)) => Some(extension_hit),
                        _ => None,
                    };
                    if extension_hit == Some(ExtensionHit::ResizeTree) {
                        model.dragging_git_tree = true;
                    } else if let Some(view) = model.git_view.as_mut()
                        && matches!(
                            git_diff::handle_mouse(view, extension_hit),
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
                Event::Mouse(mouse)
                    if matches!(
                        mouse.kind,
                        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                    ) && model.no_modal_open() =>
                {
                    forward_scroll(&mut stream, &model, layout.pane, mouse);
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
                        forward_mouse(&mut stream, &model, layout.pane, mouse);
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
                                begin_rename(&mut model, MenuTarget::Tab(tab));
                                model.dirty = true;
                            }
                            WorkspaceHit::SelectSpace(space) => {
                                begin_rename(&mut model, MenuTarget::Space(space));
                                model.dirty = true;
                            }
                            _ => {}
                        }
                        continue;
                    }
                    match hit {
                        WorkspaceHit::SelectTab(tab) => {
                            model.acknowledge_completed_agent_tab(tab);
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
                                    cwd: None,
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
                            if let Some(tab) = model.session.as_ref().and_then(|session| {
                                session
                                    .workspace
                                    .spaces
                                    .iter()
                                    .find(|candidate| candidate.id == space)
                                    .map(|candidate| candidate.selected_tab)
                            }) {
                                model.acknowledge_completed_agent_tab(tab);
                            }
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
                        WorkspaceHit::ContextMenuAction(_) => {
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
                        WorkspaceHit::OpenAgentSupport(anchor) => {
                            model.support_dropdown = selected_agent_support(&model, &identities)
                                .map(|integration| AgentSupportDropdown {
                                    integration: integration.to_owned(),
                                    anchor,
                                });
                            // Refreshes `model.agent_support` in the
                            // background so a dropdown left open across a
                            // slow first read still catches up, and the
                            // next open past this one starts from live
                            // state instead of whatever was true at attach
                            // time.
                            spawn_support_refresh(home, root, support_sender.clone());
                            model.dirty = true;
                        }
                        WorkspaceHit::Extension(_) => {
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
                Event::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Drag(MouseButton::Left)
                        && !model.dragging_sidebar
                        && !model.dragging_git_tree
                        && model.no_modal_open() =>
                {
                    forward_mouse(&mut stream, &model, layout.pane, mouse);
                }
                Event::Mouse(mouse) if mouse.kind == MouseEventKind::Up(MouseButton::Left) => {
                    // A drag this client never owned (neither flag was set,
                    // and nothing modal was open to have owned it either) is
                    // one it was forwarding into the pane above — the
                    // matching release belongs there too, not just silently
                    // dropped the way it was before pane forwarding existed.
                    if !model.dragging_sidebar && !model.dragging_git_tree && model.no_modal_open()
                    {
                        forward_mouse(&mut stream, &model, layout.pane, mouse);
                    }
                    model.dragging_sidebar = false;
                    model.dragging_git_tree = false;
                }
                Event::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Down(MouseButton::Right)
                        && model.renaming.is_none()
                        && model.agent_picker.is_none()
                        && model.context_menu.is_none() =>
                {
                    // The only way to close a space or an agent tab: right-
                    // click it, then confirm in the popup this opens (see
                    // `ContextMenu`) — never a direct click, so a stray
                    // click can't kill a running agent or a whole space's
                    // worth of them by accident. Guarded on `context_menu`
                    // being closed too (like the left-click/Enter handlers
                    // already are) so right-clicking a different row while
                    // a menu is open can't silently swap its target instead
                    // of requiring the open menu be dismissed first.
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
                    // Anchored to the cursor itself, not the clicked row's
                    // rect — a row spans the sidebar's full width, so
                    // anchoring to `rect.x` always opened the menu at the
                    // row's left edge regardless of where along it you
                    // right-clicked, which read as the popup ignoring the
                    // mouse entirely.
                    let anchor = Rect::new(mouse.column, mouse.row, 1, 1);
                    // `rename` is always offered. A tab can close with a
                    // sibling as usual, and a lone agent can close because
                    // the action replaces it with a plain shell. Renaming a
                    // lone space or shell remains the only available action.
                    if let Some(WorkspaceHit::SelectSpace(space)) = hit {
                        let mut items = vec![MenuAction::Rename];
                        if model
                            .session
                            .as_ref()
                            .is_some_and(|session| session.workspace.spaces.len() > 1)
                        {
                            items.push(MenuAction::Close);
                        }
                        model.context_menu = Some(ContextMenu {
                            target: MenuTarget::Space(space),
                            items,
                            selected: 0,
                            anchor,
                        });
                        model.dirty = true;
                    } else if let Some(WorkspaceHit::SelectTab(tab)) = hit {
                        let mut items = vec![MenuAction::Rename];
                        if can_close_tab_from_menu(&model, &identities, tab) {
                            items.push(MenuAction::Close);
                        }
                        model.context_menu = Some(ContextMenu {
                            target: MenuTarget::Tab(tab),
                            items,
                            selected: 0,
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

/// `pub(super)` (not private) so `uze_extensions::git_diff` — a crate
/// this one depends on, not a child module of `orchestrator` — can
/// construct `ExtensionHit`s from its own render function; `Extension`
/// below wraps them into the same `hits` vec every other overlay already
/// shares, rather than this workspace client threading a second, parallel
/// hit-testing vec just for one extension.
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
    /// One row of the open [`ContextMenu`], by index into its `items` —
    /// generic over whatever action that row is, same pattern
    /// [`WorkspaceHit::PickAgent`] uses for the agent picker.
    ContextMenuAction(usize),
    /// The sidebar's "+ new" row — creates a new space directly (no
    /// picker; unlike an agent tab, a space has no "kind" to choose).
    NewSpace,
    /// The tab strip's right-corner button — opens the Git changes
    /// extension (`WorkspaceModel::git_view`), scoped to the active tab's
    /// live `cwd`.
    OpenGitView,
    /// Opens contextual support details for the selected agent tab.
    OpenAgentSupport(Rect),
    /// A hit the open extension's own render pass produced (a file row, a
    /// worktree header, its tree/diff resize handle, its close button —
    /// see `uze_extensions::ExtensionHit`), wrapped instead of given its
    /// own `WorkspaceHit` variant per extension. The one Git extension
    /// today owns every `ExtensionHit` variant that exists; a second
    /// extension adds to that enum, not to this one.
    Extension(ExtensionHit),
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

/// Open state for the informational support dropdown in an active agent
/// session. The integration id keeps it tied to the selected live agent,
/// rather than to a mutable display label or process name.
struct AgentSupportDropdown {
    integration: String,
    anchor: Rect,
}

/// What a right-click-opened [`ContextMenu`] targets — the space or tab its
/// [`MenuAction`]s act on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MenuTarget {
    Space(SpaceId),
    Tab(TabId),
}

/// One row a [`ContextMenu`] can offer — the menu itself (items, selection,
/// rendering) is generic over this enum, so adding a third action is adding
/// a variant plus a match arm here and in [`dispatch_menu_action`], not
/// restructuring the popup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MenuAction {
    Rename,
    Close,
}

impl MenuAction {
    fn label(self) -> &'static str {
        match self {
            MenuAction::Rename => "rename",
            MenuAction::Close => "close",
        }
    }
}

/// Open state of the right-click action menu a space header or agent tab
/// raises. Closing a space/tab is never one click any more — right-click,
/// then confirm the menu's own "close" row — deliberately two steps, so an
/// accidental click can't kill a running agent or an entire space's worth
/// of them; other, non-destructive actions this menu grows (like `rename`)
/// don't need that same two-step guard, but share the same
/// open/navigate/confirm mechanics.
/// Built fresh on each right-click, never persisted.
struct ContextMenu {
    target: MenuTarget,
    items: Vec<MenuAction>,
    /// Index into `items` the keyboard's Up/Down currently highlights —
    /// same role [`AgentPicker::selected`] plays.
    selected: usize,
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
    integration: &'static str,
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
            integration: integration.id(),
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

fn workspace_has_active_agent_operation(
    model: &WorkspaceModel,
    identities: &[AgentIdentity],
) -> bool {
    model.session.as_ref().is_some_and(|session| {
        session
            .workspace
            .spaces
            .iter()
            .flat_map(|space| &space.tabs)
            .any(|tab| {
                agent_identity_for_tab(identities, tab).is_some()
                    && model.agent_activity.contains_key(&tab.focus.pane)
            })
    })
}

fn selected_agent_support<'a>(
    model: &WorkspaceModel,
    identities: &'a [AgentIdentity],
) -> Option<&'a str> {
    let tab = model.session.as_ref()?.selected_tab();
    let binary = agent_identity_for_tab(identities, tab)?;
    identities
        .iter()
        .find(|identity| identity.binary == binary)
        .map(|identity| identity.integration)
}

#[derive(Default)]
struct WorkspaceModel {
    session: Option<Session>,
    panes: BTreeMap<PaneId, PaneSnapshot>,
    last_size: (u16, u16),
    error: Option<String>,
    tick: usize,
    /// Agent panes with a user prompt currently in flight. A pane only
    /// enters this map when the user submits a line; PTY output extends the
    /// entry, and a quiet terminal removes it again. Merely having an agent
    /// process open is deliberately not activity.
    agent_activity: BTreeMap<PaneId, Instant>,
    /// Agent panes that finished while their tab was not selected. The
    /// sidebar keeps their green check visible until the user opens the tab,
    /// making completion discoverable without leaving a stale busy spinner.
    completed_agent_panes: BTreeSet<PaneId>,
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
    /// Contextual support information for the active harness tab.
    support_dropdown: Option<AgentSupportDropdown>,
    /// Read once asynchronously from the Integrations screen's
    /// `HarnessHealth` source of truth.
    agent_support: Vec<super::agent_support::AgentSupport>,
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
                if let Some(last_output) = self.agent_activity.get_mut(&damage.pane) {
                    *last_output = Instant::now();
                }
                let entry = self
                    .panes
                    .entry(damage.pane)
                    .or_insert_with(|| blank_pane(damage.pane, damage.columns, damage.rows));
                if entry.columns != damage.columns || entry.rows != damage.rows {
                    *entry = blank_pane(damage.pane, damage.columns, damage.rows);
                }
                entry.cursor = damage.cursor;
                entry.alternate_screen = damage.alternate_screen;
                entry.mouse = damage.mouse;
                entry.bracketed_paste = damage.bracketed_paste;
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
    /// None of the modal overlays that own mouse input while they're open
    /// (rename buffer, agent picker, context menu, Git changes view) are
    /// currently up — the precondition for forwarding a drag/release/scroll
    /// that isn't already claimed by one of them straight into the focused
    /// pane's PTY instead of dropping it.
    fn no_modal_open(&self) -> bool {
        self.renaming.is_none()
            && self.agent_picker.is_none()
            && self.support_dropdown.is_none()
            && self.context_menu.is_none()
            && self.git_view.is_none()
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
    fn note_agent_prompt_submission(&mut self, pane: PaneId, identities: &[AgentIdentity]) {
        let is_agent = self.session.as_ref().is_some_and(|session| {
            session
                .workspace
                .spaces
                .iter()
                .flat_map(|space| &space.tabs)
                .any(|tab| {
                    tab.focus.pane == pane && agent_identity_for_tab(identities, tab).is_some()
                })
        });
        if is_agent {
            self.agent_activity.insert(pane, Instant::now());
            self.completed_agent_panes.remove(&pane);
            self.dirty = true;
        }
    }
    fn expire_agent_activity(&mut self, now: Instant) -> bool {
        let expired: Vec<PaneId> = self
            .agent_activity
            .iter()
            .filter_map(|(pane, last_output)| {
                (now.duration_since(*last_output) >= AGENT_ACTIVITY_IDLE_AFTER).then_some(*pane)
            })
            .collect();
        for pane in &expired {
            self.agent_activity.remove(pane);
            if *pane != self.focused_pane() {
                self.completed_agent_panes.insert(*pane);
            }
        }
        !expired.is_empty()
    }
    fn acknowledge_completed_agent_tab(&mut self, tab: TabId) {
        if let Some(pane) = self.pane_for_tab(tab)
            && self.completed_agent_panes.remove(&pane)
        {
            self.dirty = true;
        }
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
        mouse: uze_terminal::MouseMode::default(),
        bracketed_paste: false,
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
        // The extension pushes its own `ExtensionHit`s into a scratch vec
        // (it has no reason to know about `WorkspaceHit` at all — that
        // type lives one crate up); wrap each one on the way into the
        // shared `hits` vec, the one place that translation needs to
        // happen.
        let mut extension_hits = Vec::new();
        git_diff::render(
            frame,
            view,
            frame.area(),
            model.git_tree_width,
            &mut extension_hits,
        );
        hits.extend(
            extension_hits
                .into_iter()
                .map(|(rect, hit)| (rect, WorkspaceHit::Extension(hit))),
        );
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
    if let Some(dropdown) = &model.support_dropdown
        && let Some(support) = model
            .agent_support
            .iter()
            .find(|support| support.integration() == dropdown.integration)
    {
        super::agent_support::render(frame, frame.area(), dropdown.anchor, support);
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

/// The right-click action menu — one row per [`MenuAction`] in
/// `menu.items`, keyboard-navigable (Up/Down + Enter) and mouse-clickable,
/// same mechanics and neutral styling as [`render_agent_picker`] (anchored
/// just under the right-clicked row, selected row filled instead of just
/// bold — no action gets a special color of its own, `close` included, so
/// the menu reads as one consistent list rather than singling a row out).
/// See [`ContextMenu`]'s own doc comment for why closing specifically still
/// requires this menu instead of a direct click.
fn render_context_menu(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    menu: &ContextMenu,
    hits: &mut Vec<(Rect, WorkspaceHit)>,
) {
    const H_PAD: u16 = 2;
    const MIN_WIDTH: u16 = 14;
    let content_width = menu
        .items
        .iter()
        .map(|action| action.label().len())
        .max()
        .unwrap_or(0) as u16;
    let width = (content_width + 2 * H_PAD + 2)
        .max(MIN_WIDTH)
        .min(area.width);
    let height = (menu.items.len() as u16 + 2).min(area.height);
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
        .borders(Borders::ALL)
        .border_style(Style::default().fg(super::BORDER))
        .style(Style::default().bg(super::BASE));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    for (index, action) in menu.items.iter().enumerate() {
        if index as u16 >= inner.height {
            break;
        }
        let row = Rect::new(inner.x, inner.y + index as u16, inner.width, 1);
        let selected = index == menu.selected;
        // A filled bar for the selected row, same affordance
        // `render_agent_picker` uses — always in `ACCENT`, never a red
        // fill; every row shares the same neutral color otherwise.
        let style = if selected {
            Style::default()
                .bg(super::ACCENT)
                .fg(super::BASE)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(super::NAV_INACTIVE)
        };
        let label = format!("{:pad$}{}", "", action.label(), pad = H_PAD as usize);
        let text = format!("{label:<width$}", width = inner.width as usize);
        frame.render_widget(Paragraph::new(Span::styled(text, style)), row);
        hits.push((row, WorkspaceHit::ContextMenuAction(index)));
    }
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
                // The header and its cwd caption read as one tree item —
                // clicking the caption must select the space too, not just
                // the label text above it (same rule an agent tab's own
                // detail line already follows).
                hits.push((cwd_rect, WorkspaceHit::SelectSpace(space.id)));
            }
        }
        for (index, tab) in agent_tabs.iter().enumerate() {
            let is_last = index + 1 == agent_tabs.len();
            // One extra level of indent versus a flat list — these tabs
            // read as children of the space header row just drawn above.
            let connector = if is_last { "  └─ " } else { "  ├─ " };
            let Some(label_rect) = row(1) else { break };

            let selected = tab.id == space.selected_tab;
            let active = model.agent_activity.contains_key(&tab.focus.pane);
            let completed = model.completed_agent_panes.contains(&tab.focus.pane);
            let renaming_this = model
                .renaming
                .as_ref()
                .filter(|(target, _)| *target == RenameTarget::Tab(tab.id))
                .map(|(_, buffer)| buffer.as_str());
            let indicator = if active {
                format!("{} ", agent_activity_frame(model.tick))
            } else if completed {
                "✓ ".to_owned()
            } else if selected {
                "● ".to_owned()
            } else {
                "○ ".to_owned()
            };
            let indicator_fg = if active || completed || selected {
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
                Span::styled(indicator, Style::default().fg(indicator_fg)),
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

fn agent_activity_frame(tick: usize) -> &'static str {
    AGENT_ACTIVITY_FRAMES[tick % AGENT_ACTIVITY_FRAMES.len()]
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

/// A sidebar action may target an agent in a background space, so its
/// replacement shell must be numbered from that space rather than the local
/// selection.
fn next_shell_label_for_tab(model: &WorkspaceModel, tab: TabId) -> String {
    let count = model
        .session
        .as_ref()
        .and_then(|session| {
            session
                .workspace
                .spaces
                .iter()
                .find(|space| space.tabs.iter().any(|candidate| candidate.id == tab))
        })
        .map_or(0, |space| space.tabs.len());
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

/// Confirms one [`ContextMenu`] row against its `target` — sent from both
/// the popup's own click zone and its keyboard Enter shortcut, so each
/// [`MenuAction`] only needs writing once here as the menu grows.
fn dispatch_menu_action<W: io::Write>(
    stream: &mut W,
    model: &mut WorkspaceModel,
    identities: &[AgentIdentity],
    target: MenuTarget,
    action: MenuAction,
) {
    match action {
        MenuAction::Rename => begin_rename(model, target),
        MenuAction::Close => match target {
            MenuTarget::Space(space) => {
                let _ = send_request(stream, &ClientRequest::CloseSpace { space });
            }
            MenuTarget::Tab(tab) => {
                if tab_needs_replacement_shell(model, identities, tab) {
                    // The runtime refuses to leave a space without a focused tab.
                    // Select the target first: right-clicking a background agent must
                    // replace it in its own space, not in the currently selected one.
                    let _ = send_request(stream, &ClientRequest::SelectTab { tab });
                    let (columns, rows) = model.last_size;
                    let _ = send_request(
                        stream,
                        &ClientRequest::CreateTab {
                            label: next_shell_label_for_tab(model, tab),
                            columns,
                            rows,
                            cwd: tab_cwd(model, tab),
                            command: None,
                        },
                    );
                }
                let _ = send_request(stream, &ClientRequest::CloseTab { tab });
            }
        },
    }
}

/// A normal tab can close when it has a sibling. A lone recognized agent is
/// also closable from the sidebar menu: it is replaced by a plain shell so
/// the space remains usable after its process is stopped.
fn can_close_tab_from_menu(
    model: &WorkspaceModel,
    identities: &[AgentIdentity],
    tab: TabId,
) -> bool {
    model.session.as_ref().is_some_and(|session| {
        session.workspace.spaces.iter().any(|space| {
            space.tabs.iter().any(|candidate| candidate.id == tab)
                && (space.tabs.len() > 1 || tab_needs_replacement_shell(model, identities, tab))
        })
    })
}

fn tab_needs_replacement_shell(
    model: &WorkspaceModel,
    identities: &[AgentIdentity],
    tab: TabId,
) -> bool {
    model.session.as_ref().is_some_and(|session| {
        session.workspace.spaces.iter().any(|space| {
            space.tabs.len() == 1
                && space.tabs.first().is_some_and(|candidate| {
                    candidate.id == tab && agent_identity_for_tab(identities, candidate).is_some()
                })
        })
    })
}

/// Opens the inline rename editor (`WorkspaceModel::renaming`) for `target`,
/// seeded with its current label — shared by the tab-strip/sidebar
/// double-click gesture and the context menu's `rename` row so the lookup
/// only lives once.
fn begin_rename(model: &mut WorkspaceModel, target: MenuTarget) {
    let (rename_target, label) = match target {
        MenuTarget::Tab(tab) => {
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
            (RenameTarget::Tab(tab), label)
        }
        MenuTarget::Space(space) => {
            let label = model
                .session
                .as_ref()
                .and_then(|session| session.workspace.spaces.iter().find(|s| s.id == space))
                .map(|s| s.label.clone())
                .unwrap_or_default();
            (RenameTarget::Space(space), label)
        }
    };
    model.renaming = Some((rename_target, label));
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

/// The new-agent picker inherits the selected pane's live directory. The
/// runtime's workspace root is only a fallback for callers that omit it.
fn selected_pane_cwd(model: &WorkspaceModel) -> Option<PathBuf> {
    let session = model.session.as_ref()?;
    let tab = session.selected_tab();
    pane_in_layout(&tab.layout, tab.focus.pane).map(|pane| pane.cwd.clone())
}

fn tab_cwd(model: &WorkspaceModel, tab: TabId) -> Option<PathBuf> {
    let tab = model
        .session
        .as_ref()?
        .workspace
        .spaces
        .iter()
        .find_map(|space| space.tabs.iter().find(|candidate| candidate.id == tab))?;
    pane_in_layout(&tab.layout, tab.focus.pane).map(|pane| pane.cwd.clone())
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
    // the full changes overlay. Unlike the "+"/"✦" pair above, this button
    // is a bare icon with no filled chip behind it — it sits directly on
    // the plain backdrop.
    let mut trailing_right = inner.right();
    if selected_agent_support(model, identities).is_some() {
        let button = vec![Span::styled(
            "✦",
            Style::default()
                .fg(super::ACCENT)
                .add_modifier(Modifier::BOLD),
        )];
        let button_width = button.iter().map(Span::width).sum::<usize>() as u16;
        let button_rect = Rect::new(
            trailing_right.saturating_sub(button_width),
            inner.y,
            button_width,
            1,
        );
        frame.render_widget(Paragraph::new(Line::from(button)), button_rect);
        hits.push((button_rect, WorkspaceHit::OpenAgentSupport(button_rect)));
        trailing_right = button_rect.x.saturating_sub(1);
    }
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
            trailing_right.saturating_sub(badge_width),
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
/// Encodes and forwards a click/drag/scroll that missed every uze chrome
/// hit into the focused pane's PTY — the counterpart to `encode_key` for
/// mouse input. A no-op unless the pane's own program has actually turned
/// mouse reporting on (see `uze_terminal::MouseMode`): sending raw mouse
/// escape sequences into a plain shell prompt would just inject garbage
/// text at the cursor.
fn forward_mouse<W: io::Write>(
    stream: &mut W,
    model: &WorkspaceModel,
    pane: Rect,
    mouse: MouseEvent,
) {
    let Some(snapshot) = model.panes.get(&model.focused_pane()) else {
        return;
    };
    if !snapshot.mouse.reports_clicks {
        return;
    }
    if matches!(mouse.kind, MouseEventKind::Drag(_)) && !snapshot.mouse.reports_drag {
        return;
    }
    let Some((column, row)) = pane_relative(mouse, pane) else {
        return;
    };
    let Some(bytes) = encode_mouse(mouse.kind, column, row, snapshot.mouse.sgr) else {
        return;
    };
    let _ = send_request(
        stream,
        &ClientRequest::Input {
            pane: model.focused_pane(),
            bytes,
        },
    );
}

/// Forwards the wheel into a pane. Programs that requested xterm mouse
/// reports receive a real mouse sequence. Normal-screen programs receive a
/// terminal scrollback request, matching a physical terminal; alternate
/// screens retain the conventional arrow-key fallback because they have no
/// normal-screen history to display.
fn forward_scroll<W: io::Write>(
    stream: &mut W,
    model: &WorkspaceModel,
    pane: Rect,
    mouse: MouseEvent,
) {
    let Some(snapshot) = model.panes.get(&model.focused_pane()) else {
        return;
    };
    if snapshot.mouse.reports_clicks {
        forward_mouse(stream, model, pane, mouse);
        return;
    }
    if snapshot.alternate_screen {
        let bytes = match mouse.kind {
            MouseEventKind::ScrollUp => b"\x1b[A".to_vec(),
            MouseEventKind::ScrollDown => b"\x1b[B".to_vec(),
            _ => return,
        };
        let _ = send_request(
            stream,
            &ClientRequest::Input {
                pane: model.focused_pane(),
                bytes,
            },
        );
        return;
    }
    let lines = match mouse.kind {
        MouseEventKind::ScrollUp => 3,
        MouseEventKind::ScrollDown => -3,
        _ => return,
    };
    let _ = send_request(
        stream,
        &ClientRequest::Scroll {
            pane: model.focused_pane(),
            lines,
        },
    );
}

/// Forwards a physical paste into the focused pane's PTY — the counterpart
/// to `encode_key` for bulk pasted text. Framed with the same
/// `ESC[200~ … ESC[201~` bracket the pane's own program would have seen
/// pasting directly into a real terminal only when it actually turned
/// bracketed-paste mode on (`uze_terminal::PaneSnapshot::bracketed_paste`);
/// a plain shell that never asked for it would otherwise echo the bracket
/// markers themselves as literal garbage instead of treating them as
/// framing. This is also what lets a terminal's own clipboard-image-to-text
/// conversion (an image copied, then pasted) reach the pane at all — with
/// no bracketed-paste request mirrored onto the physical terminal in the
/// first place, most terminal emulators never attempt that conversion, and
/// pasting an image into an agent's input silently does nothing.
fn forward_paste<W: io::Write>(stream: &mut W, model: &WorkspaceModel, text: &str) {
    let Some(snapshot) = model.panes.get(&model.focused_pane()) else {
        return;
    };
    let bytes = if snapshot.bracketed_paste {
        let mut framed = Vec::with_capacity(text.len() + 12);
        framed.extend_from_slice(b"\x1b[200~");
        framed.extend_from_slice(text.as_bytes());
        framed.extend_from_slice(b"\x1b[201~");
        framed
    } else {
        text.as_bytes().to_vec()
    };
    let _ = send_request(
        stream,
        &ClientRequest::Input {
            pane: model.focused_pane(),
            bytes,
        },
    );
}

/// `mouse`'s position translated into 1-indexed coordinates relative to
/// `pane`'s own top-left — the coordinate space every mouse-tracking
/// protocol reports in — or `None` when it falls outside `pane` entirely
/// (over the sidebar, tab strip, or an overlay uze's own hit-testing
/// already would have claimed first).
fn pane_relative(mouse: MouseEvent, pane: Rect) -> Option<(u16, u16)> {
    if mouse.column < pane.x || mouse.row < pane.y {
        return None;
    }
    let column = mouse.column - pane.x;
    let row = mouse.row - pane.y;
    if column >= pane.width || row >= pane.height {
        return None;
    }
    Some((column + 1, row + 1))
}

/// The xterm mouse-tracking byte sequence for one click/drag/scroll event,
/// at pane-relative `column`/`row` (see `pane_relative`) — SGR (mode 1006)
/// when the pane asked for it, else the legacy X10 encoding every terminal
/// still understands as a fallback, whose single-byte coordinates saturate
/// at 223 rather than wrapping for anything larger.
fn encode_mouse(kind: MouseEventKind, column: u16, row: u16, sgr: bool) -> Option<Vec<u8>> {
    let (code, release) = match kind {
        MouseEventKind::Down(MouseButton::Left) => (0u8, false),
        MouseEventKind::Up(MouseButton::Left) => (0u8, true),
        MouseEventKind::Drag(MouseButton::Left) => (32u8, false),
        MouseEventKind::ScrollUp => (64u8, false),
        MouseEventKind::ScrollDown => (65u8, false),
        _ => return None,
    };
    if sgr {
        Some(
            format!(
                "\x1b[<{code};{column};{row}{}",
                if release { 'm' } else { 'M' }
            )
            .into_bytes(),
        )
    } else {
        // Legacy X10: release never carries a button, always code 3; both
        // axes are single bytes offset by 32, so anything past 223 would
        // overflow into control-character range instead of wrapping.
        let legacy_code = if release { 3 } else { code };
        let cb = 32u16 + u16::from(legacy_code);
        let cx = 32u16 + column.min(223);
        let cy = 32u16 + row.min(223);
        Some(vec![0x1b, b'[', b'M', cb as u8, cx as u8, cy as u8])
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
    use super::{
        AgentIdentity, WorkspaceModel, agent_identity_for_tab, blank_pane, can_close_tab_from_menu,
        encode_mouse, forward_paste, forward_scroll, pane_relative, selected_pane_cwd,
        tab_needs_replacement_shell, workspace_has_active_agent_operation,
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

        model.note_agent_prompt_submission(PaneId(1), &identities);
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

        model.note_agent_prompt_submission(agent_pane, &identities);
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

        model.note_agent_prompt_submission(PaneId(1), &identities);
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
    fn recognizes_a_freshly_opened_agent_tab_by_its_seeded_label() {
        // Right after the agent picker creates the tab, before the first
        // status probe has resolved `pane.process` past the server's
        // "shell" placeholder.
        let tab = tab_with("Codex", "shell");
        assert_eq!(agent_identity_for_tab(&identities(), &tab), Some("codex"));
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
    fn new_agent_uses_the_selected_panes_live_directory() {
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
