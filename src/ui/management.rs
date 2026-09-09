//! Management client for the TUI (routes: Overview, Plugins, Extensions,
//! Harnesses and Profiles) — this mode's counterpart to
//! `super::orchestrator`'s terminal workspace. Presentation deliberately
//! shares the workspace's palette and layout conventions (menu + main
//! container, the Work/Manage toggle, hairline dividers, sidebar
//! drag-resize with the same bounds) so switching between the two with
//! Ctrl+O reads as one product, not two.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Padding, Paragraph, Wrap},
};

use uze_application::{ClientLayout, Result, UzeHome};

use super::hit::Hit;
use super::model::{self, Overlay, ROUTES, Remembered, Route, Status, TuiModel};
use super::worker::{
    Intent, WorkerResult, dispatch, drain_worker_results, recent_prompts, spawn_refresh,
    spawn_startup,
};
use super::{TerminalSession, overlay, scrim, small_caps, small_digits, view};
use crate::ui::theme::{self, Symbol, Token};

/// How long a resolution of the machine stands for before opening this
/// screen re-resolves it. The window exists for one case: the session's
/// own warm-up has just answered and the operator presses Ctrl+O right
/// after it, which should show that answer rather than immediately ask
/// the same question again. Past it, opening the screen is a claim about
/// the machine *now* — a `uze add` run in one of the workspace's own panes
/// happened outside anything this client would hear about.
pub(crate) const RESOLUTION_STANDS_FOR: Duration = Duration::from_secs(30);

/// What the management client keeps between visits to it, owned by
/// `super::run` for the whole session the way the workspace's own
/// [`super::orchestrator::WorkspaceMemory`] is. Ctrl+O leaves this mode
/// and comes back to it constantly; without this, each return started
/// from nothing. The screen and the drawers are not here: they outlive
/// the process, in the `ClientLayout` `super::run` owns.
pub(crate) struct ManagementMemory {
    /// The channel every management worker answers on. Session-lived
    /// rather than per-visit, which is what lets the resolution start
    /// before the screen exists and lets an answer outlive the visit that
    /// asked for it, instead of dying with a dropped receiver.
    sender: Sender<WorkerResult>,
    receiver: Receiver<WorkerResult>,
    /// The last visit's resolved machine state and place in it, or `None`
    /// before the first visit.
    remembered: Option<Remembered>,
    /// Whether a worker still owes this session an answer. Carried across
    /// visits because the channel is: a refresh the operator walked out on
    /// still lands, and re-entering must not ask a second time.
    in_flight: bool,
}

impl ManagementMemory {
    /// A session's management memory, already resolving the machine on a
    /// thread of its own.
    ///
    /// Seeding the default plugins and applying the official snapshot's
    /// pending updates is what *opening uze* does — once, here, rather
    /// than on the first Ctrl+O into this screen. Started before the
    /// workspace client even attaches, so the answer is normally waiting
    /// by the time anyone asks for the screen, and the work never sits in
    /// front of the operator as an empty list under a "refreshing" line.
    pub(crate) fn warming(home: &UzeHome) -> Self {
        let (sender, receiver) = mpsc::channel();
        spawn_startup(home.clone(), sender.clone(), context_root());
        Self {
            sender,
            receiver,
            remembered: None,
            in_flight: true,
        }
    }
}

/// Whether opening the screen asks the machine again, given when it last
/// answered. Nothing resolved yet is not an answer to stand on, so it
/// asks; a resolution inside [`RESOLUTION_STANDS_FOR`] is.
pub(crate) fn opening_re_resolves(resolved_at: Option<Instant>) -> bool {
    resolved_at.is_none_or(|at| at.elapsed() >= RESOLUTION_STANDS_FOR)
}

/// The directory this session speaks about, resolved the same way
/// [`TuiModel`]'s own `context_root` is — the workers started before a
/// model exists must ask the same question it would.
fn context_root() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

pub(crate) fn run_management(
    terminal: &mut TerminalSession,
    home: UzeHome,
    layout: &mut ClientLayout,
    memory: &mut ManagementMemory,
) -> Result<ManagementExit> {
    let sender = memory.sender.clone();
    let mut model = TuiModel {
        // Carries over whatever the user last dragged the sidebar to — in
        // this mode or the workspace's — so switching modes never resets
        // it back to the responsive default.
        sidebar_width: layout.sidebar.width,
        first_steps_collapsed: layout.first_steps.collapsed,
        first_steps_closed: layout.first_steps.closed,
        steps_taken: layout.first_steps.taken.clone(),
        // Asked of the terminal once, at startup: whether a chord can
        // reach uze at all is a property of the host, and the Keys screen
        // says so rather than letting a binding look alive and do nothing.
        keyboard: terminal.keyboard(),
        ..TuiModel::recall(memory.remembered.take(), &layout.management)
    };
    if opening_re_resolves(model.resolved_at) && !memory.in_flight {
        // Behind the frame: every list is already on screen, so nothing
        // about this reads as the plugins having gone away.
        spawn_refresh(home.clone(), sender.clone(), model.context_root.clone());
        memory.in_flight = true;
    }
    model.maintenance_in_flight = memory.in_flight;
    if model.resolved_at.is_none() {
        // The one case where the operator arrives before any answer does:
        // Ctrl+O within the first moments of the session. Nothing to draw
        // yet, so the wait is at least named — and the queued answer, if
        // it landed while the workspace had the screen, replaces this in
        // the same frame (the loop drains before it draws).
        model.status = Status::Working("Refreshing environment…".to_owned());
        // Read here rather than waited on from the startup worker, which
        // reaches it only after seeding plugins and auto-updating (see
        // `worker::recent_prompts`): one small file, and the Overview
        // otherwise says "no history yet" — the same words it uses when
        // there genuinely is none.
        model.prompt_history = recent_prompts(home.clone(), &model.context_root);
    }
    let exit = loop {
        model.tick = model.tick.wrapping_add(1);
        model.expire_status();
        model.expire_update_badges();
        // Before the frame, not after it: an answer that arrived while the
        // workspace had the screen is already in the channel when this
        // mode opens, and draining it first is what makes the very first
        // frame show it.
        drain_worker_results(&mut model, &memory.receiver);
        let mut hits = Vec::new();
        terminal.draw(|frame| render(frame, &model, &mut hits))?;
        model.hits = hits;
        let missing = model.drawer_inspect_intent();
        if missing != Intent::None {
            dispatch(missing, &home, &sender, &mut model);
        }
        if event::poll(super::POLL_INTERVAL).map_err(super::io_error)? {
            match event::read().map_err(super::io_error)? {
                Event::Key(key) => {
                    // Switching modes is an action like any other now: the
                    // keymap resolves it, `leaving_management` recognises
                    // the intent, and this loop no longer holds a key of
                    // its own that the help could not know about.
                    let intent = model.apply_key(key);
                    if intent == Intent::Quit {
                        break ManagementExit::Quit;
                    }
                    if let Some(exit) = leaving_management(&intent) {
                        break exit;
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
                    if let Some(exit) = leaving_management(&intent) {
                        break exit;
                    }
                    dispatch(intent, &home, &sender, &mut model);
                }
                Event::Resize(..) => {}
                _ => {}
            }
        }
    };
    // Handed back to the shared layout rather than kept on `model`, so a
    // Ctrl+O switch to the workspace picks up a drag made here at once,
    // and the next run opens on the screen this visit left.
    layout.sidebar.width = model.sidebar_width;
    layout.first_steps.collapsed = model.first_steps_collapsed;
    layout.first_steps.closed = model.first_steps_closed;
    layout.first_steps.taken = model.steps_taken.clone();
    layout.management = model.management_layout();
    memory.in_flight = model.maintenance_in_flight;
    memory.remembered = Some(model.remember());
    Ok(exit)
}

pub(crate) enum ManagementExit {
    Workspace,
    /// Back to the workspace with this tab selected.
    WorkspaceTab(u64),
    Quit,
}

/// The intents that end this mode rather than being dispatched in it.
fn leaving_management(intent: &Intent) -> Option<ManagementExit> {
    match intent {
        Intent::SwitchToWorkspace => Some(ManagementExit::Workspace),
        Intent::SwitchToWorkspaceTab(tab) => Some(ManagementExit::WorkspaceTab(*tab)),
        _ => None,
    }
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
        Block::default().style(theme::on(Token::TextPrimary, Token::SurfaceBackground)),
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
        Route::Overview => view::overview::render_overview(frame, layout.content, model, hits),
        Route::Plugins => view::plugins::render_plugins(frame, layout.content, model, hits),
        Route::Extensions => {
            view::extensions::render_extensions(frame, layout.content, model, hits)
        }
        Route::Harnesses => view::harnesses::render_harnesses(frame, layout.content, model, hits),
        Route::Profiles => view::profiles::render_profiles(frame, layout.content, model, hits),
        Route::Keys => view::keys::render_keys(frame, layout.content, model, hits),
    }

    if let Some(menu) = &model.row_menu {
        overlay::render_row_menu(frame, frame.area(), menu, hits);
    }

    render_footer(frame, layout.footer, model);

    // Every arm below is a modal: drawn in the middle of the frame, and
    // the only thing on screen that answers until it is dealt with. The
    // scrim is what says so — it goes here rather than inside each arm
    // because what recedes is the screen underneath, which no dialog
    // knows anything about. The row menu above is deliberately not one:
    // it hangs off the row it is about, and the row has to stay readable.
    if !matches!(model.overlay, Overlay::None) {
        scrim::render(frame, frame.area());
    }

    match &model.overlay {
        Overlay::None => {}
        Overlay::ActionIndex {
            scopes,
            filter,
            selected,
        } => overlay::render_action_index(
            frame,
            frame.area(),
            model,
            scopes,
            filter,
            *selected,
            hits,
        ),
        Overlay::HarnessHelp => overlay::render_harness_help(frame, frame.area()),
        Overlay::ConfirmRemove { id, focus } => {
            overlay::render_confirm_remove(frame, frame.area(), id, *focus, hits)
        }
        Overlay::ConfirmUpdate(id) => overlay::render_confirm_update(frame, frame.area(), id, hits),
        Overlay::ConfirmInstall { name, marketplace } => {
            overlay::render_confirm_install(frame, frame.area(), name, marketplace, hits)
        }
        Overlay::ConfirmContextApply => {
            overlay::render_confirm_context_apply(frame, frame.area(), hits)
        }
        Overlay::ConfirmClearPromptHistory => {
            overlay::render_confirm_clear_prompt_history(frame, frame.area(), hits)
        }
        Overlay::ProtectedPlugin(id) => overlay::render_protected_plugin(frame, frame.area(), id),
        Overlay::AddMarketplace(input) => {
            overlay::render_add_marketplace(frame, frame.area(), input)
        }
        Overlay::ThemePicker { themes, selected } => {
            overlay::render_theme_picker(frame, frame.area(), themes, *selected)
        }
        Overlay::NewProfile(input) => overlay::render_new_profile(frame, frame.area(), input),
        Overlay::ConfirmDeleteProfile { id, focus } => {
            overlay::render_confirm_delete_profile(frame, frame.area(), id, *focus, hits)
        }
        Overlay::TrustRequired { plugin, detail, .. } => {
            overlay::render_trust_required(frame, frame.area(), plugin, detail, hits)
        }
    }
}

fn route_subtitle(route: Route) -> &'static str {
    match route {
        Route::Overview => "status & health",
        Route::Plugins => "skills · agents · MCP",
        Route::Extensions => "official tool extensions",
        Route::Harnesses => "detected agents",
        Route::Profiles => "preferences",
        Route::Keys => "what each key does",
    }
}

/// What is worth trying once on this side of the product.
///
/// Every one of them works on every screen. That is the rule, not a
/// coincidence: this list is drawn in the same place whatever screen is
/// open, so a step that needs a particular one is a step most readers meet
/// as a row that does nothing when they click it. Asking a row what can be
/// done to it and searching a list were here for exactly that reason and
/// are not any more — both are offered where they apply, by the row's own
/// `⋯` and by the search field.
pub(crate) const FIRST_STEPS: [uze_keys::Action; 5] = [
    uze_keys::Action::SwitchMode,
    uze_keys::Action::NextScreen,
    uze_keys::Action::OpenThemePicker,
    uze_keys::Action::Refresh,
    uze_keys::Action::OpenActionIndex,
];

/// Named from the mode rather than from what is open: the key beside a
/// step must not change because a dialog is up.
pub(crate) const FIRST_STEP_SCOPES: &[uze_keys::Scope] =
    &[uze_keys::Scope::Global, uze_keys::Scope::Management];

/// The badge beside a nav row: how many of the things that screen is
/// about there are, for the screens that are an inventory of something.
///
/// Two are not, and carry none. Overview is a report rather than a list.
/// Keys is a reference — one row per surface an action can be reached
/// from, so most of them are the same Enter, Esc and arrow keys written
/// out once per dialog, and their total is a fact about the shape of the
/// table rather than about uze. Printed beside "Keys" it reads as how much
/// there is to learn, which is both untrue and the exact impression this
/// screen exists to remove.
fn route_count(route: Route, model: &TuiModel) -> Option<usize> {
    match route {
        Route::Overview => None,
        Route::Plugins => Some(model.marketplaces.len()),
        Route::Extensions => Some(model.extensions.len()),
        Route::Harnesses => Some(model.doctor.as_ref().map_or(0, |d| d.harnesses.len())),
        Route::Profiles => Some(model.profiles.len()),
        Route::Keys => None,
    }
}

/// A nav row's label, plus the badge for a route that carries one. The
/// badge takes the row's own style and overrides only the hue, so it
/// inherits the selected row's background instead of punching a hole in
/// it. Small capitals let a mark sit beside a name without shouting over
/// it; the amber says the screen is not settled without claiming anything
/// is broken.
fn route_label_line(route: Route, style: Style) -> Line<'static> {
    let mut spans = vec![Span::styled(route.label(), style)];
    if let Some(badge) = route.badge() {
        spans.push(Span::styled("  ", style));
        spans.push(Span::styled(
            small_caps(badge),
            style.fg(theme::color(Token::StateWarning)),
        ));
    }
    Line::from(spans)
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
    // out of alignment by one row. No right padding either — mirrors the
    // workspace sidebar's own `Padding::new(1, 0, 0, 0)`, content flush
    // against the divider rather than floating a column away from it. The
    // border itself is the drag handle (see the `Hit::ResizeSidebar` push
    // in `render`), so it picks up the same accent-while-dragging feedback
    // the workspace sidebar uses.
    let border_color = if model.dragging_sidebar {
        theme::color(Token::Accent)
    } else {
        theme::color(Token::BorderFaint)
    };
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(border_color))
        .padding(Padding::new(1, 0, 0, 0));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // The quick strip takes its rows out of the column before anything
    // else is laid out — pinned to the foot means the routes above cannot
    // grow over it.
    let steps = model.first_steps();
    let strip = steps.rect(inner);
    if let Some(rect) = strip {
        let mut section_hits = Vec::new();
        super::extension_view::render_section(
            frame,
            &steps.section(),
            &mut super::Rows::over(rect),
            false,
            &mut section_hits,
        );
        // The closing mark rides on the header, and this client answers a
        // click with the *first* rect that contains it — so the mark goes
        // in ahead of the header it sits on.
        if let Some(rect) = section_hits.iter().find_map(|(rect, hit)| {
            matches!(hit, uze_extensions::view::ViewHit::ToggleSection)
                .then(|| steps.close_rect(*rect))
                .flatten()
        }) {
            hits.push((rect, Hit::CloseFirstSteps));
        }
        for (rect, hit) in section_hits {
            match hit {
                uze_extensions::view::ViewHit::ToggleSection => {
                    hits.push((rect, Hit::ToggleFirstSteps))
                }
                uze_extensions::view::ViewHit::SelectItem(index) => {
                    if let Some(action) = FIRST_STEPS.get(index) {
                        hits.push((rect, Hit::OfferedAction(*action)));
                    }
                }
                _ => {}
            }
        }
    }

    let mut y = inner.y;
    let bottom = strip.map_or(inner.bottom(), |rect| rect.y);
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
    // tab strip's height on the other TUI mode — a centered segmented
    // control stands in for the Ctrl+O keybinding.
    if let Some(rect) = row(1) {
        let (work_rect, _manage_rect) = super::render_mode_toggle(frame, rect, false);
        hits.push((work_rect, Hit::SwitchToWorkspace));
    }
    if let Some(rect) = row(1) {
        frame.render_widget(
            Paragraph::new(Span::styled(
                theme::glyph(Symbol::TreeDivider).repeat(rect.width as usize),
                theme::fg(Token::BorderFaint),
            )),
            rect,
        );
    }

    for route in ROUTES {
        let selected = route == model.route;

        if narrow {
            let Some(rect) = row(1) else { break };
            let fg = if selected {
                theme::color(Token::TextBright)
            } else {
                theme::color(Token::TextInactive)
            };
            let mut style = Style::default().fg(fg);
            if selected {
                style = style
                    .add_modifier(Modifier::BOLD)
                    .bg(theme::color(Token::SurfaceRaised));
            }
            if selected {
                frame.render_widget(
                    Block::default().style(theme::bg(Token::SurfaceRaised)),
                    rect,
                );
            }
            let bar_fg = if selected {
                theme::color(Token::Accent)
            } else {
                theme::color(Token::SurfaceBackground)
            };
            let bar_bg = if selected {
                theme::color(Token::SurfaceRaised)
            } else {
                theme::color(Token::SurfaceBackground)
            };
            let bar = Rect::new(rect.x, rect.y, 1, rect.height);
            for dy in 0..bar.height {
                let cell = Rect::new(bar.x, bar.y + dy, 1, 1);
                frame.render_widget(
                    Paragraph::new(Span::styled(
                        theme::glyph(Symbol::BarMedium),
                        Style::default().fg(bar_fg).bg(bar_bg),
                    )),
                    cell,
                );
            }
            let text_rect = Rect::new(
                rect.x + 2,
                rect.y,
                rect.width.saturating_sub(3),
                rect.height,
            );
            if let Some(count) = route_count(route, model) {
                let count_str = small_digits(count);
                let count_w = count_str.len() as u16;
                let cols = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Min(1), Constraint::Length(count_w)])
                    .split(text_rect);
                let count_style = if selected {
                    Style::default()
                        .fg(theme::color(Token::Accent))
                        .bg(theme::color(Token::SurfaceRaised))
                } else {
                    theme::fg(Token::Accent)
                };
                frame.render_widget(Paragraph::new(route_label_line(route, style)), cols[0]);
                frame.render_widget(
                    Paragraph::new(Span::styled(count_str, count_style))
                        .alignment(ratatui::layout::Alignment::Right),
                    cols[1],
                );
            } else {
                frame.render_widget(Paragraph::new(route_label_line(route, style)), text_rect);
            }
            hits.push((rect, Hit::Route(route)));
            continue;
        }

        let Some(label_rect) = row(1) else { break };
        let subtitle_rect = row(1);
        row(1); // breathing room between items

        let height = if subtitle_rect.is_some() { 2 } else { 1 };
        let block_rect = Rect::new(label_rect.x, label_rect.y, label_rect.width, height);
        if selected {
            frame.render_widget(
                Block::default().style(theme::bg(Token::SurfaceRaised)),
                block_rect,
            );
        }
        let bar_fg = if selected {
            theme::color(Token::Accent)
        } else {
            theme::color(Token::SurfaceBackground)
        };
        let bar_bg = if selected {
            theme::color(Token::SurfaceRaised)
        } else {
            theme::color(Token::SurfaceBackground)
        };
        for dy in 0..height {
            let cell = Rect::new(block_rect.x, block_rect.y + dy, 1, 1);
            frame.render_widget(
                Paragraph::new(Span::styled(
                    theme::glyph(Symbol::BarMedium),
                    Style::default().fg(bar_fg).bg(bar_bg),
                )),
                cell,
            );
        }
        let text_x = block_rect.x + 2;
        let text_w = block_rect.width.saturating_sub(3);
        if selected {
            let label_style = Style::default()
                .fg(theme::color(Token::TextBright))
                .add_modifier(Modifier::BOLD)
                .bg(theme::color(Token::SurfaceRaised));
            let inner_label = Rect::new(text_x, block_rect.y, text_w, 1);
            if let Some(count) = route_count(route, model) {
                let count_str = small_digits(count);
                let count_w = count_str.len() as u16;
                let cols = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Min(1), Constraint::Length(count_w)])
                    .split(inner_label);
                let count_style = Style::default()
                    .fg(theme::color(Token::Accent))
                    .bg(theme::color(Token::SurfaceRaised));
                frame.render_widget(
                    Paragraph::new(route_label_line(route, label_style)),
                    cols[0],
                );
                frame.render_widget(
                    Paragraph::new(Span::styled(count_str, count_style))
                        .alignment(ratatui::layout::Alignment::Right),
                    cols[1],
                );
            } else {
                frame.render_widget(
                    Paragraph::new(route_label_line(route, label_style)),
                    inner_label,
                );
            }
            hits.push((label_rect, Hit::Route(route)));
            if let Some(sub_rect) = subtitle_rect {
                let inner_sub = Rect::new(text_x, block_rect.y + 1, text_w, 1);
                let sub_style = Style::default()
                    .fg(theme::color(Token::TextDim))
                    .bg(theme::color(Token::SurfaceRaised));
                frame.render_widget(
                    Paragraph::new(Span::styled(route_subtitle(route), sub_style))
                        .style(theme::bg(Token::SurfaceRaised)),
                    inner_sub,
                );
                hits.push((sub_rect, Hit::Route(route)));
            }
        } else {
            let label_style = theme::fg(Token::TextInactive);
            let inner_label = Rect::new(text_x, block_rect.y, text_w, 1);
            if let Some(count) = route_count(route, model) {
                let count_str = small_digits(count);
                let count_w = count_str.len() as u16;
                let cols = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Min(1), Constraint::Length(count_w)])
                    .split(inner_label);
                let count_style = theme::fg(Token::Accent);
                frame.render_widget(
                    Paragraph::new(route_label_line(route, label_style)),
                    cols[0],
                );
                frame.render_widget(
                    Paragraph::new(Span::styled(count_str, count_style))
                        .alignment(ratatui::layout::Alignment::Right),
                    cols[1],
                );
            } else {
                frame.render_widget(
                    Paragraph::new(route_label_line(route, label_style)),
                    inner_label,
                );
            }
            hits.push((label_rect, Hit::Route(route)));
            if let Some(sub_rect) = subtitle_rect {
                let inner_sub = Rect::new(text_x, block_rect.y + 1, text_w, 1);
                let line = Line::from(vec![Span::styled(
                    route_subtitle(route),
                    theme::fg(Token::TextDim),
                )]);
                frame.render_widget(Paragraph::new(line), inner_sub);
                hits.push((sub_rect, Hit::Route(route)));
            }
        }
    }
}

fn render_footer(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(theme::fg(Token::BorderFaint))
        .padding(Padding::new(1, 1, 0, 0));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let version = format!("v{}", env!("CARGO_PKG_VERSION"));
    // The way into the index is at the foot of the sidebar now, with the
    // other chrome that belongs to uze rather than to a screen — one place
    // in both modes, rather than a button here and a chip on the tab strip
    // over there. This row is the hint line and the version.
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(10),
            Constraint::Length(2),
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
        Paragraph::new(Span::styled(version, theme::fg(Token::TextDim)))
            .alignment(ratatui::layout::Alignment::Right),
        columns[2],
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
    // Room for however wide this theme's elision marker actually is.
    let keep = max
        .saturating_sub(used)
        .saturating_sub(theme::width(Symbol::Ellipsis) as usize);
    let mut truncated: String = line.spans[i].content.chars().take(keep).collect();
    truncated.push_str(&theme::glyph(Symbol::Ellipsis));
    line.spans[i].content = std::borrow::Cow::Owned(truncated);
    line.spans.truncate(i + 1);
}

/// The hint line: what can be done here, with the keys that do it.
///
/// Every word of it comes from the keymap — the action's own label, and
/// `chord_for` for the key. The five hand-written strings this replaced
/// were the other half of the drift the help overlay had: nothing made
/// them agree with the dispatcher, and nothing could.
fn hint_line(model: &TuiModel) -> Line<'static> {
    let scopes = model.scopes();
    let actions: Vec<uze_keys::Action> = model
        .action_index_rows(&scopes, "")
        .into_iter()
        .filter(|(action, chord)| chord.is_some() && *action != uze_keys::Action::OpenActionIndex)
        .map(|(action, _)| action)
        .take(FOOTER_HINTS)
        .collect();
    // The index is not among them: it has a button of its own at the other
    // end of this row, and the button is the mark that opens it. Naming it
    // twice on one line spends the width of a hint on a repetition.
    crate::ui::hint_for(&scopes, &actions)
}

/// How many of a screen's own actions the footer names before deferring to
/// the index. Enough to be useful on one row, few enough that the row is
/// still read rather than scanned past.
const FOOTER_HINTS: usize = 4;

fn footer(model: &TuiModel) -> Text<'static> {
    let hint = hint_line(model);
    match &model.status {
        model::Status::Idle => Text::from(hint),
        model::Status::Working(value) => {
            let frame = theme::frame(theme::Symbol::StatusWorking, model.tick);
            Text::from(vec![
                Line::from(vec![
                    Span::styled(
                        format!("{frame} "),
                        Style::default()
                            .fg(theme::color(Token::StateWarning))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        value.clone(),
                        Style::default()
                            .fg(theme::color(Token::StateWarning))
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                hint,
            ])
        }
        model::Status::Success(value) => Text::from(vec![
            Line::from(Span::styled(
                value.clone(),
                Style::default()
                    .fg(theme::color(Token::StateSuccess))
                    .add_modifier(Modifier::BOLD),
            )),
            hint,
        ]),
        model::Status::Error(value) => Text::from(vec![
            Line::from(Span::styled(
                value.clone(),
                Style::default()
                    .fg(theme::color(Token::StateDanger))
                    .add_modifier(Modifier::BOLD),
            )),
            hint,
        ]),
    }
}
