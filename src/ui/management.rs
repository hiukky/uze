//! Management client for the TUI (routes: Overview, Plugins, Marketplace,
//! Harnesses, Profiles, Doctor) — this mode's counterpart to
//! `super::orchestrator`'s terminal workspace. Presentation deliberately
//! shares the workspace's palette and layout conventions (menu + main
//! container, the Work/Manage toggle, hairline dividers, sidebar
//! drag-resize with the same bounds) so switching between the two with
//! Ctrl+O reads as one product, not two.

use std::sync::mpsc;

use crossterm::event::{self, Event};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Padding, Paragraph, Wrap},
};

use crate::{Result, UzeHome};

use super::hit::Hit;
use super::model::{self, Focus, Overlay, ROUTES, Route, Status, TuiModel};
use super::worker::{Intent, dispatch, drain_worker_results, spawn_startup};
use super::{TerminalSession, overlay, view};

pub(crate) fn run_management(
    terminal: &mut TerminalSession,
    home: UzeHome,
    sidebar_width: &mut Option<u16>,
) -> Result<ManagementExit> {
    let (sender, receiver) = mpsc::channel();
    let mut model = TuiModel {
        status: Status::Working("Refreshing environment…".to_owned()),
        maintenance_in_flight: true,
        // Carries over whatever the user last dragged the sidebar to — in
        // this mode or the workspace's — so switching modes with Ctrl+O
        // never resets it back to the responsive default.
        sidebar_width: *sidebar_width,
        ..TuiModel::default()
    };
    spawn_startup(home.clone(), sender.clone(), model.context_root.clone());
    loop {
        model.tick = model.tick.wrapping_add(1);
        let mut hits = Vec::new();
        terminal.draw(|frame| render(frame, &model, &mut hits))?;
        model.hits = hits;
        drain_worker_results(&mut model, &receiver);
        if event::poll(super::POLL_INTERVAL).map_err(super::io_error)? {
            match event::read().map_err(super::io_error)? {
                Event::Key(key) => {
                    if key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL)
                        && key.code == crossterm::event::KeyCode::Char('o')
                    {
                        return Ok(ManagementExit::Workspace);
                    }
                    let intent = model.apply_key(key);
                    if intent == Intent::Quit {
                        return Ok(ManagementExit::Quit);
                    }
                    dispatch(intent, &home, &sender, &mut model);
                }
                Event::Mouse(mouse) => {
                    // The sidebar-resize clamp needs the terminal's current
                    // total width (its dynamic max shrinks as the terminal
                    // narrows) — the one thing the model can't already know
                    // on its own, unlike everything else `apply_mouse`
                    // decides from `self`.
                    let total_width = terminal.size()?.width;
                    let intent = model.apply_mouse(mouse, total_width);
                    // Written straight through to the shared value (not
                    // just kept on `model`) so a Ctrl+O switch to the
                    // workspace picks up this width immediately, instead of
                    // only on the next drag.
                    *sidebar_width = model.sidebar_width;
                    if intent == Intent::SwitchToWorkspace {
                        return Ok(ManagementExit::Workspace);
                    }
                    dispatch(intent, &home, &sender, &mut model);
                }
                Event::Resize(..) => {}
                _ => {}
            }
        }
    }
}

pub(crate) enum ManagementExit {
    Workspace,
    Quit,
}

// --- Layout ------------------------------------------------------------

struct ManagementLayout {
    sidebar: Rect,
    content: Rect,
    footer: Rect,
}

/// The one source of truth for management geometry, mirroring
/// `orchestrator::compute_layout`'s shape and reusing its exact
/// `clamp_sidebar_width`/`sidebar_width_for` (see `super`) — the sidebar
/// drag-resize behaves identically in both TUIs because both call the
/// literal same width math, not just similarly-shaped code.
fn compute_layout(frame_area: Rect, sidebar_width_override: Option<u16>) -> ManagementLayout {
    // Flush against the top row, not inset by one — see
    // `orchestrator::compute_layout`'s identical change and rationale; kept
    // mirrored here for the same reason the rest of this function is.
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
        .constraints([Constraint::Min(3), Constraint::Length(2)])
        .split(columns[1]);
    ManagementLayout {
        sidebar: columns[0],
        content: content_rows[0],
        footer: content_rows[1],
    }
}

// --- Rendering ----------------------------------------------------------

pub(crate) fn render(
    frame: &mut ratatui::Frame<'_>,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) {
    // Edge to edge horizontally (no left/right inset — matches the design's
    // `width:100%`) and flush against the top row; one blank row is still
    // kept at the bottom (see `compute_layout`), so the last row doesn't
    // read as clipped the way a top-row title would if it sat with nothing
    // above it. One flat backdrop for the entire frame — no panel ever
    // paints its own background; every division is a hairline border or
    // padding, never a filled slab.
    frame.render_widget(
        Block::default().style(Style::default().bg(super::BASE).fg(super::TEXT_PRIMARY)),
        frame.area(),
    );
    // Only two areas span the full frame height — menu (sidebar) and main
    // container — there is no separate global header/footer row. The brand
    // and health chrome that used to live in a titlebar now opens the
    // sidebar instead (see `render_sidebar`); the help toolbar stays,
    // scoped to the container column, since this is the one TUI it belongs
    // in (the workspace/terminal mode never shows it).
    let narrow = frame.area().width < 90;
    let layout = compute_layout(frame.area(), model.sidebar_width);
    render_sidebar(frame, layout.sidebar, model, narrow, hits);
    // The sidebar's own hairline right border doubles as a drag handle —
    // same shape as `orchestrator::render`'s equivalent push, so both
    // sidebars are grabbable in the same place with the same width bounds.
    hits.push((
        Rect::new(
            layout.sidebar.right().saturating_sub(1),
            layout.sidebar.y,
            1,
            layout.sidebar.height,
        ),
        Hit::ResizeSidebar,
    ));

    match model.route {
        Route::Overview => view::overview::render_overview(frame, layout.content, model),
        Route::Plugins => view::plugins::render_plugins(frame, layout.content, model, hits),
        Route::Marketplace => {
            view::marketplace::render_marketplace(frame, layout.content, model, hits)
        }
        Route::Harnesses => view::harnesses::render_harnesses(frame, layout.content, model, hits),
        Route::Profiles => view::profiles::render_profiles(frame, layout.content, model, hits),
        Route::Doctor => view::doctor::render_doctor(frame, layout.content, model),
    }

    render_footer(frame, layout.footer, model);

    match &model.overlay {
        Overlay::None => {}
        Overlay::Help => overlay::render_help(frame, frame.area()),
        Overlay::HarnessHelp => overlay::render_harness_help(frame, frame.area()),
        Overlay::ConfirmRemove { id, focus } => {
            overlay::render_confirm_remove(frame, frame.area(), id, *focus)
        }
        Overlay::ConfirmUpdate(id) => overlay::render_confirm_update(frame, frame.area(), id),
        Overlay::ConfirmInstall { name, marketplace } => {
            overlay::render_confirm_install(frame, frame.area(), name, marketplace)
        }
        Overlay::ConfirmContextApply => overlay::render_confirm_context_apply(frame, frame.area()),
        Overlay::ProtectedPlugin(id) => overlay::render_protected_plugin(frame, frame.area(), id),
        Overlay::AddMarketplace(input) => {
            overlay::render_add_marketplace(frame, frame.area(), input)
        }
        Overlay::NewProfile(input) => overlay::render_new_profile(frame, frame.area(), input),
        Overlay::ConfirmDeleteProfile { id, focus } => {
            overlay::render_confirm_delete_profile(frame, frame.area(), id, *focus)
        }
        Overlay::TrustRequired { plugin, detail, .. } => {
            overlay::render_trust_required(frame, frame.area(), plugin, detail)
        }
    }
}

fn route_subtitle(route: Route) -> &'static str {
    match route {
        Route::Overview => "status & health",
        Route::Marketplace => "browse & install",
        Route::Plugins => "installed plugins",
        Route::Harnesses => "detected agents",
        Route::Profiles => "preferences",
        Route::Doctor => "diagnostics",
    }
}

fn render_sidebar(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    narrow: bool,
    hits: &mut Vec<(Rect, Hit)>,
) {
    // No fill, just a hairline right border — the sidebar sits on the same
    // backdrop as everything else; only a thin divider marks the edge. No
    // top padding either: the mode toggle must land on the exact row the
    // content column's own header does, or the two panes' dividers drift
    // out of alignment by one row. The border itself is the drag handle
    // (see the `Hit::ResizeSidebar` push in `render`), so it picks up the
    // same accent-while-dragging feedback the workspace sidebar uses.
    let border_color = if model.dragging_sidebar {
        super::ACCENT
    } else {
        super::BORDER_FAINT
    };
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
    // health + path/branch) spanning the whole frame; with only menu + main
    // container left, the menu opens with just enough chrome to match the
    // tab strip's height on the other TUI mode — a segmented "work" /
    // "settings" control standing in for the Ctrl+O keybinding (still live,
    // just no longer spelled out as text) instead of the old prose hint.
    if let Some(rect) = row(1) {
        let (work_rect, _manage_rect) = super::render_mode_toggle(frame, rect, false);
        hits.push((work_rect, Hit::SwitchToWorkspace));
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

    for route in ROUTES {
        // Selection reads as a left border accent, not a filled bar — the
        // design never gives the sidebar a background tint at all.
        let selected = route == model.route;
        let border = if selected {
            // A full box-drawing "│", not the thin eighth-block "▏" — the
            // latter renders inconsistently (a sliver, sometimes
            // misaligned) across terminal fonts; "│" is universally
            // supported and reads as a clean solid line.
            Span::styled("│", Style::default().fg(super::ACCENT))
        } else {
            Span::raw(" ")
        };

        if narrow {
            let Some(rect) = row(1) else { break };
            let fg = if selected {
                super::TEXT_BRIGHT
            } else {
                super::NAV_INACTIVE
            };
            let mut style = Style::default().fg(fg);
            if selected {
                style = style.add_modifier(Modifier::BOLD);
            }
            let line = Line::from(vec![border, Span::styled(route.label(), style)]);
            frame.render_widget(Paragraph::new(line), rect);
            hits.push((rect, Hit::Route(route)));
            continue;
        }

        let Some(label_rect) = row(1) else { break };
        let subtitle_rect = row(1);
        row(1); // breathing room between items

        let label_fg = if selected {
            super::TEXT_BRIGHT
        } else {
            super::NAV_INACTIVE
        };
        let mut label_style = Style::default().fg(label_fg);
        if selected {
            label_style = label_style.add_modifier(Modifier::BOLD);
        }
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                border,
                Span::raw(" "),
                Span::styled(route.label(), label_style),
            ])),
            label_rect,
        );
        hits.push((label_rect, Hit::Route(route)));

        if let Some(subtitle_rect) = subtitle_rect {
            let line = Line::from(vec![
                Span::raw("  "),
                Span::styled(route_subtitle(route), Style::default().fg(super::TEXT_DIM)),
            ]);
            frame.render_widget(Paragraph::new(line), subtitle_rect);
            hits.push((subtitle_rect, Hit::Route(route)));
        }
    }
}

fn render_footer(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(super::BORDER_FAINT))
        .padding(Padding::new(1, 1, 0, 0));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let version = format!("v{}", env!("CARGO_PKG_VERSION"));
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(10),
            Constraint::Length(version.len() as u16),
        ])
        .split(inner);
    let mut text = footer(model);
    // Operation messages (install roots, marketplace paths) can exceed the
    // hint column; clip the status line to the column instead of letting it
    // wrap into a second row — the footer is exactly one row tall and the
    // second virtual line would be clipped mid-word, which is worse than an
    // ellipsis.
    if !matches!(model.status, model::Status::Idle)
        && let Some(line) = text.lines.first_mut()
    {
        clip_line(line, columns[0].width as usize);
    }
    frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }), columns[0]);
    frame.render_widget(
        Paragraph::new(Span::styled(version, Style::default().fg(super::TEXT_DIM)))
            .alignment(ratatui::layout::Alignment::Right),
        columns[1],
    );
}

/// Truncates `line` in place to `max` columns, replacing whatever crosses
/// the limit with `…`. Spans are trimmed greedily left-to-right, so the
/// truncation point stays at the text that would have been visible anyway.
pub(crate) fn clip_line(line: &mut Line<'static>, max: usize) {
    let mut used = 0usize;
    let mut cut = None;
    for (i, span) in line.spans.iter().enumerate() {
        let width = span.width();
        if used + width <= max {
            used += width;
        } else {
            cut = Some(i);
            break;
        }
    }
    let Some(i) = cut else {
        return;
    };
    let keep = max.saturating_sub(used).saturating_sub(1); // room for "…"
    let mut truncated: String = line.spans[i].content.chars().take(keep).collect();
    truncated.push('…');
    line.spans[i].content = std::borrow::Cow::Owned(truncated);
    line.spans.truncate(i + 1);
}

fn footer(model: &TuiModel) -> Text<'static> {
    let hint = if model.filtering {
        "type to filter · enter apply · esc clear"
    } else {
        match model.overlay {
            Overlay::None => match model.focus {
                Focus::Sidebar => "↑↓/jk select route · enter/tab open · ? help · q quit",
                _ => route_hint(model),
            },
            Overlay::ConfirmRemove { .. } | Overlay::ConfirmDeleteProfile { .. } => {
                "tab switch · enter confirm · esc cancel · y/n"
            }
            Overlay::ProtectedPlugin(_) => "esc/enter to dismiss",
            Overlay::AddMarketplace(_) => "type path/URL · enter add · esc cancel",
            Overlay::NewProfile(_) => "type name · enter create · esc cancel",
            _ => "enter/y confirm · esc/n cancel",
        }
    };
    match &model.status {
        model::Status::Idle => Text::from(Line::from(super::hint_spans(hint))),
        model::Status::Working(value) => {
            let frame = super::SPINNER_FRAMES[model.tick % super::SPINNER_FRAMES.len()];
            Text::from(vec![
                Line::from(vec![
                    Span::styled(
                        format!("{frame} "),
                        Style::default()
                            .fg(super::WARNING)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        value.clone(),
                        Style::default()
                            .fg(super::WARNING)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(super::hint_spans(hint)),
            ])
        }
        model::Status::Success(value) => Text::from(vec![
            Line::from(Span::styled(
                value.clone(),
                Style::default()
                    .fg(super::SUCCESS)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(super::hint_spans(hint)),
        ]),
        model::Status::Error(value) => Text::from(vec![
            Line::from(Span::styled(
                value.clone(),
                Style::default()
                    .fg(super::DANGER)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(super::hint_spans(hint)),
        ]),
    }
}

fn route_hint(model: &TuiModel) -> &'static str {
    match model.route {
        Route::Overview => {
            if model.overview_install_path().is_some() {
                "i install · r refresh · ? help"
            } else {
                "r refresh · ? help"
            }
        }
        Route::Plugins => "↑↓ select · enter details · u update · r remove",
        Route::Marketplace => {
            "↑↓ select · enter inspect · i install · a add marketplace · / search · esc close"
        }
        Route::Harnesses => "↑↓ select · s setup · a analyze · p apply · ? status · esc close",
        Route::Profiles => match model.profile_panel {
            model::ProfilePanel::List => {
                "↑↓ select · enter edit · n new · d delete · s switch · a apply · tab panel · esc back"
            }
            model::ProfilePanel::Editor => {
                "↑↓ select · ←→/enter change · tab panel · a apply · esc back"
            }
            model::ProfilePanel::Harnesses => {
                "↑↓ select · space toggle · a apply · tab panel · esc back"
            }
        },
        Route::Doctor => "r refresh · ? help",
    }
}
