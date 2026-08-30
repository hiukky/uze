//! Workspace client for the persistent local terminal runtime (ADR-038).
//!
//! Presentation deliberately reuses the management TUI's palette and layout
//! conventions (`super::BASE`/`ACCENT`/`BORDER`/…, hairline dividers, no
//! filled panels) so switching between the workspace and management
//! contexts with Ctrl+O reads as one product, not two.

use crate::{Result, UzeError};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Padding, Paragraph},
};
use std::{
    collections::BTreeMap,
    io::{self, BufReader},
    path::Path,
    sync::mpsc,
    thread,
    time::Duration,
};
use uze_terminal::{
    CellAttributes, ClientEvent, ClientRequest, Cursor, PROTOCOL_VERSION, PaneId, PaneSnapshot,
    RenderCell, Session, TabId, TerminalColor, attach, read_event, send_request,
};

/// Input/redraw cadence. Unlike the pane content itself — which the server
/// now pushes on PTY output instead of the client polling for it (see
/// ADR-038 follow-up: the previous per-frame `Refresh` request serialized
/// every cell of every pane up to 60x/sec regardless of activity, which is
/// what made typing feel like it hung under any real system load) — this
/// timeout only bounds keyboard/mouse latency.
const POLL: Duration = Duration::from_millis(16);

pub(crate) enum WorkspaceExit {
    Management,
    Quit,
}

pub(crate) fn attach_workspace(
    terminal: &mut super::TerminalSession,
    root: &Path,
) -> Result<WorkspaceExit> {
    // The handshake below must ship the real terminal size: it sizes the PTY
    // used for the session's *already-selected* pane (e.g. a tab restored
    // from a prior attach), and the per-frame resize further down only
    // corrects the size actually visible in that loop's compute_layout call.
    // A placeholder here previously left a stale-selected pane pinned to a
    // wrong fixed size until something happened to trigger a fresh resize.
    let size = terminal.size()?;
    let layout = compute_layout(Rect::new(0, 0, size.width, size.height), None);
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
        ..WorkspaceModel::default()
    };
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
            model.tick = model.tick.wrapping_add(1);
            let mut hits = Vec::new();
            terminal.draw(|frame| render(frame, &model, &mut hits))?;
            model.hits = hits;
            model.dirty = false;
        }
        if event::poll(POLL).map_err(io_error)? {
            match event::read().map_err(io_error)? {
                Event::Key(key) if model.renaming.is_some() => {
                    match key.code {
                        KeyCode::Enter => {
                            if let Some((tab, buffer)) = model.renaming.take() {
                                let trimmed = buffer.trim().to_owned();
                                if !trimmed.is_empty() {
                                    let _ = send_request(
                                        &mut stream,
                                        &ClientRequest::RenameTab {
                                            tab,
                                            label: trimmed,
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
                            label: "shell".into(),
                            columns,
                            rows,
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
                        .and_then(|session| session.workspace.tabs.get(index))
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
                Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) => {
                    let Some(hit) = model
                        .hits
                        .iter()
                        .find(|(rect, _)| {
                            rect.x <= mouse.column
                                && mouse.column < rect.x + rect.width
                                && rect.y <= mouse.row
                                && mouse.row < rect.y + rect.height
                        })
                        .map(|(_, hit)| *hit)
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
                        if let WorkspaceHit::SelectTab(tab) = hit {
                            let label = model
                                .session
                                .as_ref()
                                .and_then(|session| {
                                    session.workspace.tabs.iter().find(|t| t.id == tab)
                                })
                                .map(|t| t.label.clone())
                                .unwrap_or_default();
                            model.renaming = Some((tab, label));
                            model.dirty = true;
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
                                    label: "shell".into(),
                                    columns,
                                    rows,
                                },
                            );
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
                    let new_width = clamp_sidebar_width(
                        mouse.column.saturating_sub(layout.sidebar.x),
                        size.width,
                    );
                    if model.sidebar_width != Some(new_width) {
                        model.sidebar_width = Some(new_width);
                        model.dirty = true;
                    }
                }
                Event::Mouse(mouse) if mouse.kind == MouseEventKind::Up(MouseButton::Left) => {
                    model.dragging_sidebar = false;
                }
                Event::Resize(_, _) | Event::Mouse(_) => {}
                _ => {}
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkspaceHit {
    SelectTab(TabId),
    CloseTab(TabId),
    NewTab,
    SwitchToManagement,
    ResizeSidebar,
}

/// A second `Down(Left)` on the same hit within this window counts as a
/// double-click (enters tab rename); slower than this, it's just another
/// single click.
const DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(400);
/// Narrowest the sidebar can be dragged — tight, but still fits a tab's
/// `●`/`○` marker, a short label, and its indented `cwd · process` caption
/// without wrapping.
const MIN_SIDEBAR_WIDTH: u16 = 14;
/// Widest the sidebar can be dragged, regardless of how wide the terminal
/// is — it's navigation, not the workspace; past this it's just width the
/// pane could otherwise use.
const MAX_SIDEBAR_WIDTH: u16 = 40;
/// Dragging the sidebar border never shrinks the content column (pane +
/// tab strip) below this many columns.
const MIN_CONTENT_WIDTH: u16 = 30;

fn clamp_sidebar_width(width: u16, total_width: u16) -> u16 {
    let max = total_width
        .saturating_sub(MIN_CONTENT_WIDTH)
        .clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
    width.clamp(MIN_SIDEBAR_WIDTH, max)
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
    /// The tab being renamed and its live edit buffer. While set, all
    /// keyboard input edits this instead of reaching the pane, and any
    /// click elsewhere cancels it (same "click outside discards" rule the
    /// management TUI's overlays use).
    renaming: Option<(TabId, String)>,
    last_click: Option<(std::time::Instant, WorkspaceHit)>,
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
            .map(|session| session.workspace.selected_tab)
    }
    fn pane_for_tab(&self, tab: TabId) -> Option<PaneId> {
        self.session.as_ref().and_then(|session| {
            session
                .workspace
                .tabs
                .iter()
                .find(|candidate| candidate.id == tab)
                .map(|candidate| candidate.focus.pane)
        })
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
    let area = Rect::new(
        frame_area.x,
        frame_area.y + 1,
        frame_area.width,
        frame_area.height.saturating_sub(2),
    );
    let sidebar_width = sidebar_width_override
        .map(|width| clamp_sidebar_width(width, area.width))
        .unwrap_or_else(|| sidebar_width_for(area.width));
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(sidebar_width), Constraint::Min(10)])
        .split(area);
    let content_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(columns[1]);
    WorkspaceLayout {
        sidebar: columns[0],
        tab_strip: content_rows[0],
        pane: content_rows[1],
    }
}

fn sidebar_width_for(total_width: u16) -> u16 {
    if total_width < 60 {
        16
    } else if total_width < 90 {
        18
    } else {
        27
    }
}

// --- Rendering -------------------------------------------------------------

fn render(
    frame: &mut ratatui::Frame<'_>,
    model: &WorkspaceModel,
    hits: &mut Vec<(Rect, WorkspaceHit)>,
) {
    frame.render_widget(
        Block::default().style(Style::default().bg(super::BASE).fg(super::TEXT_PRIMARY)),
        frame.area(),
    );
    let layout = compute_layout(frame.area(), model.sidebar_width);
    render_sidebar(frame, layout.sidebar, model, hits);
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
    render_tab_strip(frame, layout.tab_strip, model, hits);
    render_pane(frame, layout.pane, model);
}

/// A workspace/tabs tree: one root row naming the workspace (the project
/// directory), then every tab as a child row — `●`/`○` for
/// selected/unselected plus its label, and a dim caption line underneath
/// with its pane's live `cwd · process` (see
/// [`uze_terminal::PaneRuntime::foreground_status`] on the server side).
/// The sidebar used to just list tabs flatly; this makes explicit that one
/// workspace (one project root, one server) owns many tabs/processes.
fn render_sidebar(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &WorkspaceModel,
    hits: &mut Vec<(Rect, WorkspaceHit)>,
) {
    let border_color = if model.dragging_sidebar {
        super::ACCENT
    } else {
        super::BORDER_FAINT
    };
    // No top padding: the mode toggle must land on the exact row the tab
    // strip's own content does (that block has none either), or the two
    // panes' dividers drift out of alignment by one row.
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(border_color))
        .padding(Padding::new(1, 1, 0, 0));
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

    if let Some(rect) = row(1) {
        frame.render_widget(
            Paragraph::new(Span::styled(
                workspace_name(&session.workspace.root),
                Style::default()
                    .fg(super::TEXT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            )),
            rect,
        );
    }

    let tabs = &session.workspace.tabs;
    let can_close = tabs.len() > 1;
    for (index, tab) in tabs.iter().enumerate() {
        let is_last = index + 1 == tabs.len();
        let connector = if is_last { "└─ " } else { "├─ " };
        let Some(label_rect) = row(1) else { break };

        let selected = tab.id == session.workspace.selected_tab;
        let renaming_this = model
            .renaming
            .as_ref()
            .filter(|(id, _)| *id == tab.id)
            .map(|(_, buffer)| buffer.as_str());
        let dot_fg = if selected {
            super::ACCENT
        } else {
            super::TEXT_FAINT
        };
        let mut label_style = Style::default().fg(if selected {
            super::TEXT_BRIGHT
        } else {
            super::NAV_INACTIVE
        });
        if selected {
            label_style = label_style.add_modifier(Modifier::BOLD);
        }
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
        // `connector`'s box-drawing characters are multi-byte in UTF-8
        // (`"├─ ".len()` is 7, not the 3 columns it actually occupies) —
        // `.len()` here previously threw off the close-button padding,
        // pushing `×` left of where it visually belongs. `Span::width()`
        // measures display columns, matching what `label.width()` already
        // does below.
        let used = connector_span.width() + 2 + label.width();
        let mut spans = vec![
            connector_span,
            Span::styled(
                if selected { "● " } else { "○ " },
                Style::default().fg(dot_fg),
            ),
            label,
        ];
        if renaming_this.is_none() && can_close && inner.width as usize > used + 2 {
            let pad = inner.width as usize - used - 1;
            spans.push(Span::raw(" ".repeat(pad)));
            spans.push(Span::styled("×", Style::default().fg(super::TEXT_DIM)));
            hits.push((
                Rect::new(
                    label_rect.x + inner.width.saturating_sub(1),
                    label_rect.y,
                    1,
                    1,
                ),
                WorkspaceHit::CloseTab(tab.id),
            ));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), label_rect);
        hits.push((label_rect, WorkspaceHit::SelectTab(tab.id)));

        if let Some(detail_rect) = row(1) {
            let continuation = if is_last { "   " } else { "│  " };
            let detail = pane_in_layout(&tab.layout, tab.focus.pane)
                .map(|pane| {
                    format!(
                        "{} · {}",
                        super::display_project_path(&pane.cwd),
                        pane.process
                    )
                })
                .unwrap_or_default();
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(continuation, Style::default().fg(super::TEXT_FAINT)),
                    Span::styled(detail, Style::default().fg(super::TEXT_DIM)),
                ])),
                detail_rect,
            );
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
}

fn workspace_name(root: &std::path::Path) -> String {
    root.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .unwrap_or_else(|| super::display_project_path(root))
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

/// The horizontal tab strip above the pane: an active-tab marker in
/// `ACCENT`/bold-bright text (the same contrast the sidebar uses for
/// selection, not a filled pill background — this design never paints
/// filled surfaces), a dim `×` close affordance per tab once more than one
/// exists, and a trailing `+` to open another.
fn render_tab_strip(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &WorkspaceModel,
    hits: &mut Vec<(Rect, WorkspaceHit)>,
) {
    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(super::BORDER_FAINT))
        .padding(Padding::new(1, 1, 0, 0));
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

    let can_close = session.workspace.tabs.len() > 1;
    let mut spans = Vec::new();
    let mut x = inner.x;
    for tab in &session.workspace.tabs {
        if x >= inner.right() {
            break;
        }
        let selected = tab.id == session.workspace.selected_tab;
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
            .filter(|(id, _)| *id == tab.id)
            .map(|(_, buffer)| buffer.as_str());
        let label = match renaming_this {
            Some(buffer) => Span::styled(
                format!("{buffer}▏"),
                Style::default()
                    .fg(super::TEXT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
            None => Span::styled(tab.label.clone(), label_style),
        };
        let start = x;
        let mut width = marker.width() as u16 + label.width() as u16;
        spans.push(marker);
        spans.push(label);
        if renaming_this.is_none() && can_close {
            spans.push(Span::raw(" "));
            spans.push(Span::styled("×", Style::default().fg(super::TEXT_DIM)));
            hits.push((
                Rect::new(start + width + 1, inner.y, 1, 1),
                WorkspaceHit::CloseTab(tab.id),
            ));
            width += 2;
        }
        hits.push((
            Rect::new(start, inner.y, width, 1),
            WorkspaceHit::SelectTab(tab.id),
        ));
        spans.push(Span::raw("   "));
        x += width + 3;
    }
    if x < inner.right() {
        let new_tab_rect = Rect::new(x, inner.y, "+ new tab".len() as u16, 1);
        spans.push(Span::styled(
            "+ new tab",
            Style::default().fg(super::ACCENT),
        ));
        hits.push((new_tab_rect, WorkspaceHit::NewTab));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
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
