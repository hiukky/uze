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
use uze_core::prompt_history;
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
/// PTY damage arriving this soon after we forwarded the user's own
/// keystrokes into a pane is that pane echoing them back, not the agent
/// working. Without this window every character typed into an agent tab
/// would re-arm its busy spinner.
const LOCAL_ECHO_WINDOW: Duration = Duration::from_millis(250);

/// A submitted prompt stays active while its terminal keeps producing
/// output. Agent harnesses can spend several seconds silent while waiting
/// for a tool or network response, so this grace period favors not hiding
/// an in-flight operation over immediately clearing a returned prompt.
const AGENT_ACTIVITY_IDLE_AFTER: Duration = Duration::from_secs(10);

mod input;
mod render;
use input::*;
use render::*;

pub(crate) enum WorkspaceExit {
    Management,
    Quit,
}

/// Which harness, resolved against which directory. Both halves are the
/// identity of one support resolution: the same agent open in two panes
/// sitting in two different projects has two different answers, and a
/// resolution computed for one must never be shown for the other. This is
/// the whole reason the old session-wide "resolve once at the attach root"
/// read was wrong.
type SupportKey = (String, PathBuf);

/// A finished resolution, tagged with the key it answers. `support` is
/// `None` when the read failed outright — kept as a resolved-but-empty
/// answer rather than dropped, so a failing read cannot spin the refresh
/// loop by looking forever unresolved.
struct SupportResolution {
    key: SupportKey,
    support: Option<super::agent_support::AgentSupport>,
}

/// Computes one agent's support read model in a background thread and
/// delivers it through `sender`.
///
/// Everything about the answer comes from `key`: the harness the pane is
/// actually running, and that pane's own working directory. The
/// application resolves the project from there
/// (`UzeApplication::agent_context_for`), the same way the runtime shim
/// resolves it when it execs the harness from that directory — so the
/// popup reports the delivery a launch here would really perform, not the
/// one that would have happened wherever `uze` itself was started.
fn spawn_support_refresh(home: &UzeHome, key: SupportKey, sender: mpsc::Sender<SupportResolution>) {
    let support_home = home.clone();
    thread::spawn(move || {
        let support = super::tui_application(support_home).ok().and_then(|app| {
            let context = app.agent_context_for(&key.0, &key.1).ok()?;
            let health = app.harness_inspect(&key.0).ok()?;
            let profiles = app.list_profiles().unwrap_or_default();
            let active_profile = profiles.iter().find(|profile| profile.active);
            Some(super::agent_support::AgentSupport::resolve(
                health,
                &context,
                active_profile,
            ))
        });
        let _ = sender.send(SupportResolution { key, support });
    });
}

pub(crate) fn attach_workspace(
    terminal: &mut super::TerminalSession,
    root: &Path,
    sidebar_width: &mut Option<u16>,
    home: &UzeHome,
    pending_tab: Option<TabId>,
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
    // Prompt history is keyed by the *resolved* workspace root, the same
    // one the management Overview reads against — `root` here is the raw
    // cwd, which differs whenever uze is launched from a subdirectory of
    // the workspace. Writes run on their own thread so a keystroke never
    // waits on the filesystem, and the thread ends when `model` drops its
    // sender at the end of this attach.
    let history_root = uze_core::workspace::resolve_workspace(root)
        .map(|workspace| workspace.root)
        .unwrap_or_else(|_| root.to_path_buf());
    let (prompt_recorder, recorded_prompts) =
        mpsc::channel::<(prompt_history::PromptOrigin, String)>();
    thread::spawn({
        let home = home.clone();
        move || {
            while let Ok((origin, prompt)) = recorded_prompts.recv() {
                let _ = prompt_history::record(&home, &history_root, &origin, &prompt);
            }
        }
    });
    let mut model = WorkspaceModel {
        dirty: true,
        last_size: (columns, rows),
        sidebar_width: *sidebar_width,
        prompt_recorder: Some(prompt_recorder),
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
    // No attach-time prefetch: there is nothing to resolve until a tab is
    // recognized as running an agent, and what to resolve then depends on
    // that pane's own directory. The loop below kicks a refresh the moment
    // the selection names an agent whose answer is not already in hand —
    // the same moment the "✦" badge appears.
    let (support_sender, support_receiver) = mpsc::channel::<SupportResolution>();
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
    // `select_tab` moves the selected space too when the tab lives in
    // another one, so the space needs no separate request. A tab closed
    // since its prompt was logged simply selects nothing.
    if let Some(tab) = pending_tab {
        let _ = send_request(&mut stream, &ClientRequest::SelectTab { tab });
        // Resize the newly selected pane to the current layout size, same as
        // the manual SelectTab handler does, so the PTY size matches the
        // visible rect immediately.
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
    loop {
        while let Ok(event) = receiver.try_recv() {
            model.apply(event);
        }
        while let Ok(resolution) = support_receiver.try_recv() {
            if model.agent_support_pending.as_ref() == Some(&resolution.key) {
                model.agent_support_pending = None;
            }
            model.agent_support = Some(resolution);
            model.dirty = true;
        }
        // Contextual resolution: whatever the selection currently is, that
        // is what must be resolved. Keyed on `(harness, cwd)`, so this
        // fires exactly when the answer could have changed — a different
        // agent tab selected, or the server's live probe reporting the
        // pane moved — and never repeats for an answer already held.
        if let Some(key) = selected_agent_context(&model, &identities)
            && model.agent_support_pending.as_ref() != Some(&key)
            && model
                .agent_support
                .as_ref()
                .is_none_or(|resolution| resolution.key != key)
        {
            model.agent_support_pending = Some(key.clone());
            spawn_support_refresh(home, key, support_sender.clone());
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
                                        label: next_agent_label(&model),
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
                            cwd: selected_pane_cwd(&model),
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
                        // `encode_key` emits a bare CR for Enter and 0x03
                        // for Ctrl+C, so these are exact byte comparisons
                        // rather than a substring scan that a pasted or
                        // multi-byte sequence could trip.
                        let submitted = bytes.as_slice() == *b"\r";
                        let cancelled = bytes.as_slice() == [3u8];
                        let prompt = if submitted {
                            model.prompt_buffers.entry(pane).or_default().submit()
                        } else {
                            if cancelled {
                                model.prompt_buffers.remove(&pane);
                            } else {
                                model.prompt_buffers.entry(pane).or_default().apply(key);
                            }
                            None
                        };
                        // Forwarded before anything is recorded: the pane's
                        // own responsiveness must never wait on history.
                        let _ = send_request(&mut stream, &ClientRequest::Input { pane, bytes });
                        model.note_local_input(pane);
                        if submitted {
                            model.note_agent_prompt_submission(
                                pane,
                                &identities,
                                prompt.as_deref(),
                            );
                        }
                    }
                }
                Event::Paste(text) if model.no_modal_open() => {
                    let pane = model.focused_pane();
                    model.prompt_buffers.entry(pane).or_default().paste(&text);
                    forward_paste(&mut stream, &model, &text);
                    model.note_local_input(pane);
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
                                        label: next_agent_label(&model),
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
                                    cwd: selected_pane_cwd(&model),
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
                            model.support_dropdown = selected_agent_context(&model, &identities)
                                .map(|key| AgentSupportDropdown { key, anchor });
                            // Opening always re-reads, even when an answer
                            // for this key is already held: `AGENTS.md` and
                            // `.agents/` can change under an open workspace,
                            // and this is the one moment the user is
                            // actually looking at the answer.
                            if let Some(dropdown) = &model.support_dropdown {
                                model.agent_support_pending = Some(dropdown.key.clone());
                                spawn_support_refresh(
                                    home,
                                    dropdown.key.clone(),
                                    support_sender.clone(),
                                );
                            }
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
/// session. The `(harness, cwd)` key keeps it tied to the exact live agent
/// it was opened over, rather than to a mutable display label or process
/// name — and makes a resolution for some other pane unrenderable here.
struct AgentSupportDropdown {
    key: SupportKey,
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
/// tab's own label only for legacy tabs created before generic agent labels
/// were introduced. Returns the harness's short binary/alias
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

/// The selected tab's agent, paired with that tab's focused pane's own
/// working directory — `None` for a tab that is not running a recognized
/// agent, which is also what hides the "✦" badge.
fn selected_agent_context(
    model: &WorkspaceModel,
    identities: &[AgentIdentity],
) -> Option<SupportKey> {
    let tab = model.session.as_ref()?.selected_tab();
    let binary = agent_identity_for_tab(identities, tab)?;
    let integration = identities
        .iter()
        .find(|identity| identity.binary == binary)
        .map(|identity| identity.integration)?;
    let cwd = pane_in_layout(&tab.layout, tab.focus.pane)?.cwd.clone();
    Some((integration.to_owned(), cwd))
}

/// What the sidebar shows beside one agent tab. These four states are the
/// whole vocabulary, and they are mutually exclusive by the precedence
/// encoded in [`WorkspaceModel::agent_tab_status`] — the single place that
/// decides, so the glyph can never disagree with the model that produced
/// it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AgentTabStatus {
    /// The agent is producing output right now.
    Working,
    /// The agent finished while the user was looking somewhere else, and
    /// the user has not opened the tab since.
    Completed,
    /// The tab the user is on, with nothing in flight.
    Selected,
    /// A quiet agent tab the user is not on.
    Idle,
}

impl AgentTabStatus {
    /// The indicator column, including its trailing space. `tick` only
    /// matters for [`AgentTabStatus::Working`], whose glyph animates.
    pub(super) fn glyph(self, tick: usize) -> String {
        match self {
            AgentTabStatus::Working => format!("{} ", agent_activity_frame(tick)),
            AgentTabStatus::Completed => "\u{2713} ".to_owned(),
            AgentTabStatus::Selected => "\u{25cf} ".to_owned(),
            AgentTabStatus::Idle => "\u{25cb} ".to_owned(),
        }
    }

    /// Idle is the only state drawn faint: the other three all report
    /// something the user asked for or needs to notice.
    pub(super) fn color(self) -> Color {
        match self {
            AgentTabStatus::Idle => crate::ui::TEXT_FAINT,
            _ => crate::ui::ACCENT,
        }
    }
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
    /// Panes known to be running a recognized agent, recorded the first
    /// time the user submits a prompt into one. `apply` has no identity
    /// table of its own, so this is what lets a *later* burst of PTY output
    /// re-arm activity for a pane whose quiet stretch already expired.
    agent_panes: BTreeSet<PaneId>,
    /// When input was last forwarded into a pane, so the echo of the user's
    /// own typing is not mistaken for agent output ([`LOCAL_ECHO_WINDOW`]).
    local_input_at: BTreeMap<PaneId, Instant>,
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
    /// The most recently resolved agent support answer, tagged with the
    /// `(harness, cwd)` it answers — never assumed to apply to a different
    /// selection.
    agent_support: Option<SupportResolution>,
    /// The key a background resolution is currently in flight for, so the
    /// per-frame check cannot queue the same read repeatedly.
    agent_support_pending: Option<SupportKey>,
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
    /// Per-pane reconstruction of the line being typed, flushed on Enter.
    prompt_buffers: BTreeMap<PaneId, PromptBuffer>,
    /// Sink for recorded prompts. `None` leaves the history untouched —
    /// the default, so tests exercise the submission path without writing
    /// to a real UZE home.
    prompt_recorder: Option<mpsc::Sender<(prompt_history::PromptOrigin, String)>>,
}

/// Client-side reconstruction of what the user typed into a pane before
/// Enter.
///
/// UZE forwards keystrokes to a PTY whose line editor it cannot observe, so
/// this models only an ordinary single-line edit: printable characters,
/// backspace/delete, and horizontal cursor movement. Anything that could
/// rewrite the line invisibly — history recall, completion, a kill ring, a
/// control chord this does not encode — marks the buffer untrusted, and an
/// untrusted buffer is discarded at Enter. Recording nothing is always
/// preferable to recording a prompt the user never typed.
struct PromptBuffer {
    characters: Vec<char>,
    cursor: usize,
    trusted: bool,
}

impl Default for PromptBuffer {
    fn default() -> Self {
        Self {
            characters: Vec::new(),
            cursor: 0,
            trusted: true,
        }
    }
}

impl PromptBuffer {
    fn apply(&mut self, key: KeyEvent) {
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            self.trusted = false;
            return;
        }
        match key.code {
            KeyCode::Char(character) => {
                self.characters.insert(self.cursor, character);
                self.cursor += 1;
            }
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.characters.remove(self.cursor);
                }
            }
            KeyCode::Delete => {
                if self.cursor < self.characters.len() {
                    self.characters.remove(self.cursor);
                }
            }
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.characters.len()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.characters.len(),
            _ => self.trusted = false,
        }
    }

    fn paste(&mut self, text: &str) {
        for character in text.chars().map(|c| if c == '\r' { '\n' } else { c }) {
            self.characters.insert(self.cursor, character);
            self.cursor += 1;
        }
    }

    /// The text to record, or `None` when there is nothing trustworthy to
    /// record — including the line-continuation case, where the Enter
    /// inserts a newline instead of submitting and the buffer must survive.
    fn submit(&mut self) -> Option<String> {
        if self.trusted && self.cursor > 0 && self.characters[self.cursor - 1] == '\\' {
            self.characters[self.cursor - 1] = '\n';
            return None;
        }
        let flushed = std::mem::take(self);
        flushed
            .trusted
            .then(|| flushed.characters.into_iter().collect())
    }
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
                self.note_agent_output(damage.pane, !damage.changed.is_empty(), Instant::now());
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
    /// Marks the pane as busy and, when the submission was reconstructed
    /// with confidence, records it. Activity is noted for every Enter in an
    /// agent pane — that signal predates the history and does not depend on
    /// knowing what was typed.
    fn note_agent_prompt_submission(
        &mut self,
        pane: PaneId,
        identities: &[AgentIdentity],
        prompt: Option<&str>,
    ) {
        let origin = self.session.as_ref().and_then(|session| {
            session.workspace.spaces.iter().find_map(|space| {
                space.tabs.iter().find_map(|tab| {
                    if tab.focus.pane != pane {
                        return None;
                    }
                    agent_identity_for_tab(identities, tab).map(|binary| {
                        prompt_history::PromptOrigin {
                            space_label: space.label.clone(),
                            tab_id: tab.id.0,
                            tab_label: tab.label.clone(),
                            agent_binary: binary.to_owned(),
                        }
                    })
                })
            })
        });
        let Some(origin) = origin else {
            return;
        };
        self.agent_panes.insert(pane);
        self.agent_activity.insert(pane, Instant::now());
        self.completed_agent_panes.remove(&pane);
        self.dirty = true;

        if let (Some(prompt), Some(recorder)) = (prompt, self.prompt_recorder.as_ref()) {
            let _ = recorder.send((origin, prompt.to_owned()));
        }
    }

    /// The one place the four sidebar states are decided. Working outranks
    /// Completed (fresh output means the run the check would announce is
    /// not over), and both outrank Selected — a spinner or a check on the
    /// tab you are already on still carries information the plain dot does
    /// not.
    fn agent_tab_status(&self, pane: PaneId, selected: bool) -> AgentTabStatus {
        if self.agent_activity.contains_key(&pane) {
            AgentTabStatus::Working
        } else if self.completed_agent_panes.contains(&pane) {
            AgentTabStatus::Completed
        } else if selected {
            AgentTabStatus::Selected
        } else {
            AgentTabStatus::Idle
        }
    }

    /// Records that input was forwarded into `pane`, arming
    /// [`LOCAL_ECHO_WINDOW`] so the echo that comes straight back does not
    /// read as agent output.
    fn note_local_input(&mut self, pane: PaneId) {
        self.local_input_at.insert(pane, Instant::now());
    }

    /// Output from a known agent pane keeps — or puts — that pane in the
    /// working state. Re-arming matters as much as extending: an agent that
    /// sits silent past [`AGENT_ACTIVITY_IDLE_AFTER`] waiting on a tool or a
    /// network call has already been dropped from `agent_activity`, and
    /// without this the rest of its run would render as finished (or as
    /// nothing at all) while it was still working.
    fn note_agent_output(&mut self, pane: PaneId, changed_cells: bool, now: Instant) {
        if !changed_cells || !self.agent_panes.contains(&pane) {
            return;
        }
        if self
            .local_input_at
            .get(&pane)
            .is_some_and(|at| now.duration_since(*at) < LOCAL_ECHO_WINDOW)
        {
            return;
        }
        self.agent_activity.insert(pane, now);
        self.completed_agent_panes.remove(&pane);
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

// --- Layout --------------------------------------------------------------

// --- Rendering -------------------------------------------------------------

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

/// The label a new agent tab opens with. Agent labels are deliberately
/// independent of the chosen harness: the picker selects what runs, while
/// the tab is numbered by the user's workspace organization.
fn next_agent_label(model: &WorkspaceModel) -> String {
    let count = model.session.as_ref().map_or(0, |session| {
        session
            .selected_space()
            .tabs
            .iter()
            .filter(|tab| is_generated_agent_label(&tab.label))
            .count()
    });
    format!("agent {}", count + 1)
}

fn is_generated_agent_label(label: &str) -> bool {
    label
        .strip_prefix("agent ")
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|number| number > 0)
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
mod tests;
