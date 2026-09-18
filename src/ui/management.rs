//! The management surface — Overview, Plugins, Extensions, Harnesses,
//! Profiles, Keys and Appearance — drawn as a modal over the workspace
//! client (`super::orchestrator`) rather than as a mode beside it. The
//! workspace owns the frame, the event loop and the terminal session; this
//! module owns what the modal keeps between openings
//! ([`ManagementMemory`]), what one opening is ([`TuiModel`]), and how it
//! is drawn into the rectangle the workspace hands it ([`render_modal`]).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Padding, Paragraph},
};

use uze_application::{FirstStepsLayout, ManagementLayout, UzeHome};

use super::hit::Hit;
use super::keys::KeyboardSupport;
use super::model::{self, Overlay, Remembered, Route, Status, TuiModel};
use super::worker::{
    Intent, WorkerResult, dispatch, drain_worker_results, recent_prompts, spawn_refresh,
    spawn_startup,
};
use super::{overlay, scrim, small_caps, small_digits, view};
use crate::ui::theme::{self, Symbol, Token};

/// How long a resolution of the machine stands for before opening the
/// modal re-resolves it. The window exists for one case: the session's
/// own warm-up has just answered and the operator opens the modal right
/// after it, which should show that answer rather than immediately ask
/// the same question again. Past it, opening the modal is a claim about
/// the machine *now* — a `uze add` run in one of the workspace's own panes
/// happened outside anything this client would hear about.
pub(crate) const RESOLUTION_STANDS_FOR: Duration = Duration::from_secs(30);

/// What the modal keeps between openings, owned by `super::run` for the
/// whole session the way the workspace's own
/// [`super::orchestrator::WorkspaceMemory`] is. The modal opens and closes
/// constantly; without this, each opening started from nothing. The
/// screen and the drawers are not here: they outlive the process, in the
/// `ClientLayout` `super::run` owns.
pub(crate) struct ManagementMemory {
    /// The channel every management worker answers on. Session-lived
    /// rather than per-opening, which is what lets the resolution start
    /// before the modal exists and lets an answer outlive the opening
    /// that asked for it, instead of dying with a dropped receiver.
    sender: Sender<WorkerResult>,
    receiver: Receiver<WorkerResult>,
    /// The last opening's resolved machine state and place in it, or
    /// `None` before the first.
    remembered: Option<Remembered>,
    /// Whether a worker still owes this session an answer. Carried across
    /// openings because the channel is: a refresh the operator closed the
    /// modal on still lands, and reopening must not ask a second time.
    in_flight: bool,
    /// Where the modal's own menu column was last dragged to. The modal
    /// has a width of its own, so this is not the workspace sidebar's
    /// value and is not written to the layout file with it.
    menu_width: Option<u16>,
    /// The project the last resolution was about. A remembered answer is
    /// only an answer about the project it was asked for, so opening the
    /// modal over a different one asks again however recent it was.
    resolved_for: Option<PathBuf>,
}

impl ManagementMemory {
    /// A session's management memory, already resolving the machine on a
    /// thread of its own.
    ///
    /// Seeding the default plugins and applying the official snapshot's
    /// pending updates is what *opening uze* does — once, here, rather
    /// than on the first opening of the modal. Started before the
    /// workspace client even attaches, so the answer is normally waiting
    /// by the time anyone asks for it, and the work never sits in front
    /// of the operator as an empty list under a "refreshing" line.
    pub(crate) fn warming(home: &UzeHome) -> Self {
        let memory = Self::unresolved();
        // The launch directory, because the workspace has not attached
        // yet and there is no space to be standing in. What the modal is
        // actually about is decided at its first opening, which asks
        // again when the two differ (see [`Self::open`]).
        let root = context_root();
        spawn_startup(home.clone(), memory.sender.clone(), root.clone());
        Self {
            in_flight: true,
            resolved_for: Some(root),
            ..memory
        }
    }

    /// A memory nothing has asked the machine on behalf of — what a test
    /// starts from, so opening the modal never spawns a real resolution.
    pub(crate) fn unresolved() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            sender,
            receiver,
            remembered: None,
            in_flight: false,
            menu_width: None,
            resolved_for: None,
        }
    }

    pub(crate) fn sender(&self) -> &Sender<WorkerResult> {
        &self.sender
    }

    /// One opening of the modal, picking up where the last one left off.
    ///
    /// `first_steps` is the workspace's, handed in rather than read from
    /// the layout file: the two surfaces share the one list, and the
    /// workspace may have ticked a step off since the file was written.
    pub(crate) fn open(
        &mut self,
        home: &UzeHome,
        root: &Path,
        layout: &ManagementLayout,
        first_steps: &FirstStepsLayout,
        keyboard: KeyboardSupport,
    ) -> TuiModel {
        let mut model = TuiModel {
            context_root: root.to_path_buf(),
            sidebar_width: self.menu_width,
            first_steps_collapsed: first_steps.collapsed,
            first_steps_closed: first_steps.closed,
            steps_taken: first_steps.taken.clone(),
            // Asked of the terminal once, at startup: whether a chord can
            // reach uze at all is a property of the host, and the Keys
            // screen says so rather than letting a binding look alive and
            // do nothing.
            keyboard,
            ..TuiModel::recall(self.remembered.take(), layout)
        };
        // A resolution is an answer about one project, so a different one
        // asks again however recently the last answered — and asks even
        // with one in flight, because that one is about the project this
        // is not.
        let elsewhere = self.resolved_for.as_deref() != Some(root);
        if (opening_re_resolves(model.remembered.resolved_at) || elsewhere)
            && (!self.in_flight || elsewhere)
        {
            // Behind the frame: every list is already on screen, so
            // nothing about this reads as the plugins having gone away.
            spawn_refresh(
                home.clone(),
                self.sender.clone(),
                model.context_root.clone(),
            );
            self.resolved_for = Some(model.context_root.clone());
            self.in_flight = true;
        }
        model.maintenance_in_flight = self.in_flight;
        // Two cases, one answer. Nothing resolved yet is the operator
        // arriving within the first moments of the session; a different
        // project is the operator arriving somewhere the remembered
        // answer is not about. Either way the wait is named rather than
        // drawn as an empty environment, and the queued answer replaces
        // this on the first tick.
        if model.remembered.resolved_at.is_none() || elsewhere {
            model.status = Status::Working("Refreshing environment…".to_owned());
            // Read here rather than waited on from the worker, which
            // reaches it only after seeding plugins, auto-updating and
            // detecting harnesses (see `worker::recent_prompts`): one
            // small file, and the Overview otherwise says "no history
            // yet" — the same words it uses when there genuinely is none.
            model.remembered.prompt_history = recent_prompts(home.clone(), &model.context_root);
        }
        model
    }

    /// Keeps what an opening resolved and arranged for the next one, and
    /// hands back what the workspace owns of it: the shape the layout
    /// file keeps, and the first-steps list the two surfaces share.
    pub(crate) fn close(&mut self, model: TuiModel) -> (ManagementLayout, FirstStepsLayout) {
        self.menu_width = model.sidebar_width;
        self.in_flight = model.maintenance_in_flight;
        let layout = model.management_layout();
        let first_steps = FirstStepsLayout {
            collapsed: model.first_steps_collapsed,
            closed: model.first_steps_closed,
            taken: model.steps_taken.clone(),
        };
        self.remembered = Some(model.remember());
        (layout, first_steps)
    }

    /// One turn of the modal's own clock, taken by the workspace loop
    /// before each frame it draws the modal in: the spinner advances,
    /// transient status expires, every answer that arrived is absorbed,
    /// and whatever the screen now shows that has not been fetched yet is
    /// asked for.
    ///
    /// Answers are drained *before* the frame on purpose: one that
    /// arrived while the modal was closed is already in the channel when
    /// it opens, and draining first is what makes the very first frame
    /// show it.
    pub(crate) fn tick(&mut self, model: &mut TuiModel, home: &UzeHome) {
        model.tick = model.tick.wrapping_add(1);
        model.expire_status();
        model.expire_update_badges();
        if let Some((revision, notice)) = crate::self_update::since(model.release_revision) {
            model.release = notice;
            model.release_revision = revision;
        }
        drain_worker_results(model, &self.receiver);
        for missing in [
            model.drawer_inspect_intent(),
            model.profile_preview_intent(),
            model.appearance_intent(),
        ] {
            if missing != Intent::None {
                dispatch(missing, home, &self.sender, model);
            }
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

// --- Geometry -----------------------------------------------------------

/// Where the modal sits in `frame`: wide, because it holds whole screens
/// with a menu, a list and a drawer side by side, but inset on every side
/// so the workspace is visibly still there behind it. The inset scales
/// down with the terminal rather than being a fixed margin — on a small
/// one the screens need the columns more than the backdrop needs to show.
pub(crate) fn modal_area(frame: Rect) -> Rect {
    let horizontal = (frame.width / 16).min(6);
    let vertical = (frame.height / 12).min(2);
    Rect::new(
        frame.x + horizontal,
        frame.y + vertical,
        frame.width.saturating_sub(2 * horizontal),
        frame.height.saturating_sub(2 * vertical),
    )
}

/// Where the management surface is drawn inside a modal at `area`: inside
/// the border, and one blank row under the title, so the menu's first
/// route never sits against it. The workspace measures a drag inside the
/// modal against the same rectangle.
pub(crate) fn modal_surface(area: Rect) -> Rect {
    Rect::new(
        area.x + 1,
        area.y + 2,
        area.width.saturating_sub(2),
        area.height.saturating_sub(3),
    )
}

/// The rectangles a frame of the modal leaves for the workspace's input
/// handling: the modal itself, and the mark on its title that closes it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ModalChrome {
    pub(crate) area: Rect,
    pub(crate) close: Rect,
}

/// The modal, over a frame the scrim has already pushed back: its border,
/// its title row, and the management surface inside.
pub(crate) fn render_modal(
    frame: &mut ratatui::Frame<'_>,
    frame_area: Rect,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) -> ModalChrome {
    let area = modal_area(frame_area);
    frame.render_widget(Clear, area);
    // The same border every popup of the workspace draws, so the modal
    // reads as one more of its surfaces rather than a different product.
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::fg(Token::BorderDefault))
        .style(theme::on(Token::TextPrimary, Token::SurfaceBackground));
    frame.render_widget(block, area);

    // The title row is drawn by hand rather than through the block's own
    // titles so the close mark has a rect the click can be tested against.
    // Its name is set the way `super::title_row` sets every popup's.
    let title = Rect::new(area.x + 2, area.y, area.width.saturating_sub(4), 1);
    frame.render_widget(
        Paragraph::new(Span::styled(" manage ", theme::fg_bold(Token::TextBright))),
        title,
    );
    // Only the close mark on the right: the key that closes the modal is
    // the one that opened it, and the index lists it for anyone asking.
    let mark = theme::glyph(Symbol::MarkClose);
    let mark_width = theme::width(Symbol::MarkClose);
    let close = Rect::new(
        title.right().saturating_sub(mark_width + 1),
        title.y,
        mark_width + 2,
        1,
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            format!(" {mark} "),
            theme::fg(Token::TextMuted),
        )),
        close,
    );

    render(frame, modal_surface(area), model, hits);
    ModalChrome { area, close }
}

// --- Layout ------------------------------------------------------------

struct Geometry {
    sidebar: Rect,
    content: Rect,
    footer: Rect,
}

/// The one source of truth for management geometry, mirroring
/// `orchestrator::compute_layout`'s shape and reusing its exact
/// `clamp_sidebar_width`/`sidebar_width_for` (see `super`) — the sidebar
/// drag-resize behaves identically in the modal and the workspace because
/// both call the literal same width math, not just similarly-shaped code.
fn compute_layout(frame_area: Rect, sidebar_width_override: Option<u16>) -> Geometry {
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
    Geometry {
        sidebar: columns[0],
        content: content_rows[0],
        footer: content_rows[1],
    }
}

// --- Rendering ----------------------------------------------------------

/// The management surface, filling `area` — the inside of the modal, or a
/// whole test frame.
///
/// Edge to edge within it (no left/right inset — matches the design's
/// `width:100%`) and flush against its top row; one blank row is still
/// kept at the bottom (see `compute_layout`), so the last row doesn't read
/// as clipped the way a top-row title would if it sat with nothing above
/// it. One flat backdrop for the entire area — no panel ever paints its
/// own background; every division is a hairline border or padding, never
/// a filled slab.
pub(crate) fn render(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) {
    frame.render_widget(
        Block::default().style(theme::on(Token::TextPrimary, Token::SurfaceBackground)),
        area,
    );
    // Only two columns span the full height — menu (sidebar) and main
    // container — there is no separate global header/footer row. The help
    // toolbar stays, scoped to the container column.
    let narrow = area.width < 90;
    let layout = compute_layout(area, model.sidebar_width);
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
        Route::Appearance => {
            view::appearance::render_appearance(frame, layout.content, model, hits)
        }
    }

    render_footer(frame, layout.footer, model);

    // Every arm below is a dialog: drawn in the middle of the surface, and
    // the only thing in it that answers until it is dealt with. The scrim
    // is what says so — it goes here rather than inside each arm because
    // what recedes is the screen underneath, which no dialog knows
    // anything about. Only this surface recedes: the workspace behind the
    // modal already has.
    if !matches!(model.overlay, Overlay::None) {
        scrim::render(frame, area);
    }

    match &model.overlay {
        Overlay::None => {}
        Overlay::ActionIndex {
            scopes,
            filter,
            selected,
        } => overlay::render_action_index(frame, area, model, scopes, filter, *selected, hits),
        Overlay::HarnessHelp => overlay::render_harness_help(frame, area),
        Overlay::Confirm { kind, focus } => {
            overlay::render_confirmation(frame, area, kind, *focus, hits)
        }
        Overlay::AddMarketplace(input) => overlay::render_text_prompt(
            frame,
            area,
            "Add marketplace",
            "Local path or https://... source",
            input,
            "add",
        ),
        Overlay::ThemePicker { themes, selected } => {
            overlay::render_theme_picker(frame, area, themes, *selected)
        }
        Overlay::NewProfile(input) => {
            overlay::render_text_prompt(frame, area, "New profile", "Profile name", input, "create")
        }
    }
}

/// What is worth trying once on this side of the product.
///
/// Every one of them works on every screen. That is the rule, not a
/// coincidence: this list is drawn in the same place whatever screen is
/// open, so a step that needs a particular one is a step most readers meet
/// as a row that does nothing when they click it. Asking a row what can be
/// done to it and searching a list were here for exactly that reason and
/// are not any more — both are offered where they apply, by the drawer's
/// buttons and by the search field.
pub(crate) const FIRST_STEPS: [uze_keys::Action; 4] = [
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
        Route::Plugins => Some(model.remembered.marketplaces.len()),
        Route::Extensions => Some(model.extensions.len()),
        Route::Harnesses => Some(
            model
                .remembered
                .doctor
                .as_ref()
                .map_or(0, |d| d.harnesses.len()),
        ),
        Route::Profiles => Some(model.remembered.profiles.len()),
        Route::Keys => None,
        Route::Appearance => None,
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
    // top padding either: the first route must land on the exact row the
    // content column's own header does. No right padding either — mirrors
    // the workspace sidebar's own `Padding::new(1, 0, 0, 0)`, content
    // flush against the divider rather than floating a column away from
    // it. The border itself is the drag handle (see the `Hit::ResizeSidebar`
    // push in `render`), so it picks up the same accent-while-dragging
    // feedback the workspace sidebar uses.
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

    let mut bottom = strip.map_or(inner.bottom(), |rect| rect.y);
    // The release notice sits on the steps rather than under them — the
    // workspace's sidebar says why.
    if let Some(notice) = model.release.as_ref().map(super::ReleaseNotice)
        && let Some(rect) = notice.rect(Rect {
            height: bottom - inner.y,
            ..inner
        })
    {
        let targets = notice.render(frame, rect);
        hits.push((targets.dismiss, Hit::DismissRelease));
        hits.extend(
            targets
                .notes
                .into_iter()
                .map(|rect| (rect, Hit::OpenReleaseNotes)),
        );
        bottom = rect.y;
    }
    let mut rows = super::Rows::over(Rect {
        height: bottom - inner.y,
        ..inner
    });
    for route in model::routes() {
        let rect = if narrow {
            let Some(rect) = rows.next(1) else { break };
            rect
        } else {
            let Some(label) = rows.next(1) else { break };
            let has_subtitle = rows.next(1).is_some();
            rows.gap();
            Rect {
                height: if has_subtitle { 2 } else { 1 },
                ..label
            }
        };
        route_row(
            frame,
            rect,
            route,
            route == model.route,
            route_count(route, model),
        );
        hits.push((rect, Hit::Route(route)));
    }
}

/// One route in the sidebar: a bar at its edge, its name with the count
/// pinned right, and — on a row two tall — its subtitle beneath. The
/// selected route is a raised band the bar lights up on.
fn route_row(
    frame: &mut ratatui::Frame<'_>,
    rect: Rect,
    route: Route,
    selected: bool,
    count: Option<usize>,
) {
    let ground = if selected {
        Token::SurfaceRaised
    } else {
        Token::SurfaceBackground
    };
    if selected {
        frame.render_widget(Block::default().style(theme::bg(ground)), rect);
    }
    let bar_hue = if selected {
        Token::Accent
    } else {
        Token::SurfaceBackground
    };
    for dy in 0..rect.height {
        frame.render_widget(
            Paragraph::new(Span::styled(
                theme::glyph(Symbol::BarMedium),
                theme::on(bar_hue, ground),
            )),
            Rect::new(rect.x, rect.y + dy, 1, 1),
        );
    }

    let text_x = rect.x + 2;
    let text_width = rect.width.saturating_sub(3);
    let raised = |style: Style| {
        if selected {
            style.bg(theme::color(Token::SurfaceRaised))
        } else {
            style
        }
    };
    let label_style = if selected {
        raised(theme::fg_bold(Token::TextBright))
    } else {
        theme::fg(Token::TextInactive)
    };
    let label_rect = Rect::new(text_x, rect.y, text_width, 1);
    let label_rect = match count {
        Some(count) => {
            let count = small_digits(count);
            let [label, count_rect] =
                Layout::horizontal([Constraint::Min(1), Constraint::Length(count.len() as u16)])
                    .areas(label_rect);
            frame.render_widget(
                Paragraph::new(Span::styled(count, raised(theme::fg(Token::Accent))))
                    .alignment(ratatui::layout::Alignment::Right),
                count_rect,
            );
            label
        }
        None => label_rect,
    };
    frame.render_widget(
        Paragraph::new(route_label_line(route, label_style)),
        label_rect,
    );
    if rect.height > 1 {
        frame.render_widget(
            Paragraph::new(Span::styled(
                route.subtitle(),
                raised(theme::fg(Token::TextDim)),
            )),
            Rect::new(text_x, rect.y + 1, text_width, 1),
        );
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
    // in both surfaces, rather than a button here and a chip on the tab
    // strip over there. This row is the hint line and the version.
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(10),
            Constraint::Length(2),
            Constraint::Length(version.len() as u16),
        ])
        .split(inner);
    // One row: a line that does not fit is elided rather than wrapped into
    // a second row the footer does not have.
    let mut line = footer_line(model);
    clip_line(&mut line, columns[0].width as usize);
    frame.render_widget(Paragraph::new(line), columns[0]);
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
    line.spans[i].content =
        std::borrow::Cow::Owned(crate::ui::elide_tail(&line.spans[i].content, max - used));
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

/// What the footer says: how the last thing went while there is anything
/// to say about it, and otherwise what can be done here.
fn footer_line(model: &TuiModel) -> Line<'static> {
    let (hue, text) = match &model.status {
        model::Status::Idle => return hint_line(model),
        model::Status::Working(value) => (
            Token::StateWarning,
            format!(
                "{} {value}",
                theme::frame(theme::Symbol::StatusWorking, model.tick)
            ),
        ),
        model::Status::Success(value) => (Token::StateSuccess, value.clone()),
        model::Status::Error(value) => (Token::StateDanger, value.clone()),
    };
    Line::from(Span::styled(text, theme::fg_bold(hue)))
}
