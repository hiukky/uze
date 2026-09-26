//! Everything the workspace client draws.
//!
//! Split out of `orchestrator.rs`, which had grown to 3.5k lines covering
//! three unrelated jobs: driving the session, drawing it, and encoding input
//! for the PTY. Nothing here mutates session state — these take a
//! `&WorkspaceModel` and paint it, which is what makes them one module.

use super::*;
use crate::ui::Rows;
use crate::ui::theme::{self, Symbol, Token};
use crate::ui::widget::{
    self, Chip, ChipState, Edge, POPUP_H_PAD, POPUP_V_PAD, Rule, Surface, TRAILING_PAD,
    action_index, chip, hint, mark, row, text,
};

pub(super) fn blank_pane(pane: PaneId, columns: u16, rows: u16) -> PaneSnapshot {
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

pub(super) fn blank_cell() -> RenderCell {
    RenderCell {
        character: ' ',
        foreground: TerminalColor::DefaultForeground,
        background: TerminalColor::DefaultBackground,
        attributes: CellAttributes::default(),
    }
}

pub(super) struct WorkspaceLayout {
    pub(super) sidebar: Rect,
    pub(super) tab_strip: Rect,
    pub(super) pane: Rect,
}

/// The one source of truth for workspace geometry — both the renderer and
/// the input loop's resize/CreateTab sizing call this, so the PTY dimensions
/// sent to the server always match the rect actually drawn into.
/// `sidebar_width_override` is the user's dragged width, if any (see
/// [`WorkspaceModel::sidebar_width`]); `None` uses the responsive default.
/// Only two areas span the full frame height — menu (sidebar) and main
/// container — there is no separate global header/footer row; the brand
/// and health chrome that used to live in a titlebar now opens the sidebar
/// itself (see [`render_sidebar`]), and this client never shows the help
/// toolbar — that stays exclusive to the management modal.
pub(super) fn compute_layout(
    frame_area: Rect,
    sidebar_width_override: Option<u16>,
) -> WorkspaceLayout {
    // Flush against the top row, not inset by one — the sidebar header is
    // this client's own top edge, and floating it a row down from the real
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
        .map(|width| crate::ui::clamp_sidebar_width(width, area.width))
        .unwrap_or_else(|| crate::ui::sidebar_width_for(area.width));
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

/// What a frame measured that the next event needs back.
///
/// Beside `hits` for the same reason those are: only the render knows how
/// the column came out, and the wheel over the sidebar has to stay inside
/// what it found there.
#[derive(Debug, Default)]
pub(super) struct FrameMetrics {
    /// Rows of the space tree the sidebar could not show — how far the
    /// tree may be scrolled, and zero when it fits.
    pub(super) tree_overflow: u16,
    /// What the code surface's frame left behind: where its navigator
    /// settled, and the scrollbars it drew — see
    /// `extension_view::Rendered`.
    pub(super) code: Option<crate::ui::extension_view::Rendered>,
    /// What the management modal's frame left behind, when it was open.
    pub(super) manage: Option<ManageFrame>,
    /// Whether a caption too long for its room was drawn sliding. The one
    /// thing on an otherwise idle frame that needs the next one: the
    /// clock turns for spinners and for work in flight, and a caption
    /// passing under the pointer is neither.
    pub(super) marquee: bool,
}

/// One frame of the management modal: where it was drawn, and the hit
/// list its own surface produced — in the modal's vocabulary, not this
/// client's, since its clicks are resolved by its own model.
#[derive(Debug)]
pub(super) struct ManageFrame {
    pub(super) chrome: crate::ui::management::ModalChrome,
    pub(super) hits: Vec<(Rect, crate::ui::hit::Hit)>,
}

pub(super) fn render(
    frame: &mut ratatui::Frame<'_>,
    model: &WorkspaceModel,
    identities: &[AgentIdentity],
    hits: &mut Vec<(Rect, WorkspaceHit)>,
    metrics: &mut FrameMetrics,
) {
    widget::root(frame, frame.area());
    let layout = compute_layout(frame.area(), model.sidebar_width);
    render_sidebar(frame, layout.sidebar, model, identities, hits, metrics);
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
    if !render_extension(frame, layout.pane, model, hits, metrics) {
        render_pane(frame, layout.pane, model);
    }
    // Over the pane, under the modals: an outcome is worth covering some
    // output for, and worth nothing at all if it draws over the dialog the
    // reader is answering.
    render_toasts(frame, layout.pane, model, hits);
    // Drawn last so it sits on top of the pane — same ordering the
    // management modal's dialogs use in its own `render`. Anchored to
    // `picker.anchor` (the "✦" button's own rect) rather than centered on
    // the whole frame — a dropdown hanging off the thing you clicked, not a
    // modal interrupting the screen.
    // The two modal surfaces this client has: centred, and the only thing
    // that answers while they are open. Everything below them is a
    // dropdown hanging off the control that opened it, which stays beside
    // a screen that is still live — so the scrim covers these two and
    // nothing else. Same placement as the management modal's: between
    // what was drawn and what is drawn over it.
    if model.preserved.is_some() || model.action_index.is_some() || model.release_notes.is_some() {
        crate::ui::widget::scrim::render(frame, frame.area());
    }
    if let Some(modal) = &model.release_notes {
        let targets = crate::ui::release_notes::render(frame, frame.area(), modal);
        // Prepended: what is underneath must not answer a click meant here.
        hits.splice(
            0..0,
            [
                (targets.close, WorkspaceHit::ReleaseNotesClose),
                (targets.popup, WorkspaceHit::ReleaseNotesBody),
            ],
        );
    }
    if let Some(overlay) = &model.preserved {
        render_preserved(frame, frame.area(), model, overlay);
    }
    if let Some(index) = &model.action_index {
        render_action_index(frame, frame.area(), index, hits);
    }
    if let Some(picker) = &model.agent_picker {
        render_agent_picker(frame, frame.area(), picker.anchor, picker, hits);
    }
    if let Some(dropdown) = &model.support_dropdown
        && let Some(resolution) = &model.remembered.agent_support
        && resolution.key == dropdown.key
        && let Some(support) = &resolution.support
    {
        crate::ui::agent_support::render(frame, frame.area(), dropdown.anchor, support);
    }
    if let Some(anchor) = model.status_catalog {
        render_status_catalog(frame, frame.area(), anchor, model.tick);
    }
    if let Some(popup) = &model.commit_detail {
        render_commit_detail(frame, frame.area(), popup);
    }
    if let Some(menu) = &model.context_menu {
        render_context_menu(frame, frame.area(), menu, hits);
    }
    // Last of all, over everything: the management modal seals the whole
    // client, so the whole frame recedes under it — the popups above
    // included, which is why it is not drawn among them.
    if let Some(manage) = &model.manage {
        crate::ui::widget::scrim::render(frame, frame.area());
        let mut manage_hits = Vec::new();
        let chrome =
            crate::ui::management::render_modal(frame, frame.area(), manage, &mut manage_hits);
        metrics.manage = Some(ManageFrame {
            chrome,
            hits: manage_hits,
        });
        // Prepended: what is underneath must not answer a click meant here.
        hits.splice(
            0..0,
            [
                (chrome.close, WorkspaceHit::CloseManage),
                (chrome.area, WorkspaceHit::ManageSurface),
            ],
        );
    }
}

/// The open extension, drawn where the pane is — a third kind of thing
/// that place shows, beside an agent and a shell. The sidebar and the
/// strip stay live around it, because the surface is about the checkout
/// they are selecting; covering them made reaching another agent a
/// matter of closing the surface first. Whether one was drawn is the
/// answer, since the pane is drawn only when none was.
fn render_extension(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &WorkspaceModel,
    hits: &mut Vec<(Rect, WorkspaceHit)>,
    metrics: &mut FrameMetrics,
) -> bool {
    // The extension answers with content; the host lays it out and
    // therefore is the only side that can say which rectangle a click
    // landed in. The hits come back in the view's own vocabulary and are
    // tagged with the extension they belong to on the way into the shared
    // `hits` vec — the one place that translation happens.
    let mut view_hits = Vec::new();
    let (view, scope, tag): (_, _, fn(ViewHit) -> ExtensionHit) =
        if let Some(architect) = &model.architect {
            (
                uze_extensions::architect::view(
                    architect,
                    crate::ui::extension_view::board_space(area),
                ),
                uze_keys::Scope::Architect,
                ExtensionHit::Architect,
            )
        } else if let Some(code) = &model.code {
            (
                uze_extensions::code::view(
                    code,
                    crate::ui::extension_view::code_space(area, model.code_tree_width, Some(code)),
                ),
                uze_keys::Scope::Code,
                ExtensionHit::Code,
            )
        } else {
            return false;
        };
    metrics.code = Some(crate::ui::extension_view::render(
        frame,
        &view,
        area,
        model.code_tree_width,
        model.code_tree_scroll,
        scope,
        &mut view_hits,
    ));
    hits.extend(
        view_hits
            .into_iter()
            .map(|(rect, hit)| (rect, WorkspaceHit::Extension(tag(hit)))),
    );
    true
}

/// A small popup listing `agent_options`, opened by the selected space
/// header's "✦ new" — a dropdown anchored just below it, creating the
/// picked agent as a new tab in that space. Not built on
/// the management modal's dialog helpers (those are shaped for static
/// text, not a selectable, hit-testable list) — this is self-contained,
/// styled by hand to match the same palette.
pub(super) fn render_agent_picker(
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
        .max(NO_AGENT_SET_UP.len() as u16);
    // Empty, it says so and offers the way out on a row of its own.
    let rows = picker.options.len().max(2) as u16;
    let width = (content_width + 6).min(area.width);
    let height = (rows + 2).min(area.height);
    let popup = Rect::new(
        anchor.x.min((area.x + area.width).saturating_sub(width)),
        (anchor.y + anchor.height).min((area.y + area.height).saturating_sub(height)),
        width,
        height,
    );
    frame.render_widget(Clear, popup);
    // A card, not a floating surface: this menu is anchored to the control
    // that opened it and measures itself — `height` budgets its two border
    // rows and nothing else, and each row leads with its own inset (below).
    // Given the floating inset on top of that, the last option fell
    // outside the box, and with one harness installed there was nothing
    // left to draw at all.
    let inner = Surface::card().title(" new agent ").render(frame, popup);

    if picker.options.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                format!(" {NO_AGENT_SET_UP}"),
                theme::fg(Token::TextMuted),
            )),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );
        if inner.height > 1 {
            let row = Rect::new(inner.x, inner.y + 1, inner.width, 1);
            render_agent_picker_row(frame, row, SET_ONE_UP, true);
            hits.push((row, WorkspaceHit::SetUpAgent));
        }
        return;
    }
    for (index, option) in picker.options.iter().enumerate() {
        if index as u16 >= inner.height {
            break;
        }
        let row = Rect::new(inner.x, inner.y + index as u16, inner.width, 1);
        render_agent_picker_row(frame, row, &option.display_name, index == picker.selected);
        hits.push((row, WorkspaceHit::PickAgent(index)));
    }
}

const NO_AGENT_SET_UP: &str = "no agent set up yet";
const SET_ONE_UP: &str = "set one up";

/// A filled bar for the selected row, not just bold text — a
/// narrowly-scoped exception to this design's usual no-filled-surfaces
/// rule, for one reason: a keyboard-navigable menu needs the affordance.
fn render_agent_picker_row(frame: &mut ratatui::Frame<'_>, row: Rect, label: &str, selected: bool) {
    let (style, text) = if selected {
        let style = Style::default()
            .bg(theme::color(Token::Accent))
            .fg(theme::color(Token::SurfaceBackground))
            .add_modifier(Modifier::BOLD);
        let text = format!(
            " {:<width$}",
            label,
            width = row.width.saturating_sub(1) as usize
        );
        (style, text)
    } else {
        (theme::fg(Token::TextInactive), format!(" {label}"))
    };
    frame.render_widget(Paragraph::new(Span::styled(text, style)), row);
}

/// The right-click action menu — one row per [`MenuAction`] in
/// `menu.items`, keyboard-navigable (Up/Down + Enter) and mouse-clickable,
/// same mechanics and neutral styling as [`render_agent_picker`] (anchored
/// just under the right-clicked row, selected row filled instead of just
/// bold — no action gets a special color of its own, `close` included, so
/// the menu reads as one consistent list rather than singling a row out).
/// See [`ContextMenu`]'s own doc comment for why closing specifically still
/// requires this menu instead of a direct click.
pub(super) fn render_context_menu(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    menu: &ContextMenu,
    hits: &mut Vec<(Rect, WorkspaceHit)>,
) {
    const MIN_WIDTH: u16 = 14;
    let content_width = menu
        .items
        .iter()
        .map(|action| action.label().len())
        .max()
        .unwrap_or(0) as u16;
    let width = (content_width + 2 * POPUP_H_PAD + 2)
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
    let inner = Surface::card().render(frame, popup);

    for (index, action) in menu.items.iter().enumerate() {
        if index as u16 >= inner.height {
            break;
        }
        let row = Rect::new(inner.x, inner.y + index as u16, inner.width, 1);
        let selected = index == menu.selected;
        // A filled bar for the selected row, same affordance
        // `render_agent_picker` uses — always in `theme::color(Token::Accent)`, never a red
        // fill; every row shares the same neutral color otherwise.
        let style = if selected {
            Style::default()
                .bg(theme::color(Token::Accent))
                .fg(theme::color(Token::SurfaceBackground))
                .add_modifier(Modifier::BOLD)
        } else {
            theme::fg(Token::TextInactive)
        };
        let label = format!("{:pad$}{}", "", action.label(), pad = POPUP_H_PAD as usize);
        let text = format!("{label:<width$}", width = inner.width as usize);
        frame.render_widget(Paragraph::new(Span::styled(text, style)), row);
        hits.push((row, WorkspaceHit::ContextMenuAction(index)));
    }
}

/// Whether `cwd` is outside any slot: no repository, no commit to branch
/// from — or an agent that simply has not been isolated, standing in
/// the operator's own directory. An agent there owes its
/// upstream a pull or a push that an agent in a slot never does, which is
/// the one thing its caption says beyond the harness.
fn is_unisolated(cwd: &Path) -> bool {
    !uze_application::is_isolated_checkout(cwd)
}

/// The hue an agent's caption line is drawn in: dim, like every other
/// detail, except under the agent actually receiving keystrokes — whose
/// whole item, both rows of it, is what every command in the footer would
/// act on. Saying so on the caption too spares the operator tracing the
/// bold label back down a row.
fn caption_color(is_current: bool) -> Color {
    if is_current {
        theme::color(Token::StateWarning)
    } else {
        theme::color(Token::TextDim)
    }
}

/// Pins a task's mark to an agent row's right edge — off the divider by
/// the same pad every row's trailing column keeps, so the
/// sidebar's right-hand column is one column and a mark keeps its place
/// however long the label is — and makes that cell a click target opening
/// the status catalog: a glyph nobody can look up is a glyph that reads as
/// decoration.
fn push_trailing_mark(
    spans: &mut Vec<Span<'_>>,
    hits: &mut Vec<(Rect, WorkspaceHit)>,
    label_rect: Rect,
    mark: &str,
    hue: Color,
) {
    let mark = Span::styled(mark.to_owned(), Style::default().fg(hue));
    let mark_width = mark.width() as u16;
    let used =
        spans.iter().map(|span| span.width() as u16).sum::<u16>() + mark_width + TRAILING_PAD;
    spans.push(Span::raw(
        " ".repeat(label_rect.width.saturating_sub(used).max(1) as usize),
    ));
    spans.push(mark);
    spans.push(Span::raw(" ".repeat(TRAILING_PAD as usize)));
    if let Some(mark_x) = label_rect.right().checked_sub(TRAILING_PAD + mark_width) {
        let cell = Rect::new(mark_x, label_rect.y, mark_width, 1);
        hits.push((cell, WorkspaceHit::OpenStatusCatalog(cell)));
    }
}

/// One block per space the user has created (blank-line separated — see
/// the loop below), each expanded (no collapse/accordion) into the agent
/// tabs [`agent_identity_for_tab`] recognizes as running inside it, laid
/// out alike for either kind: a tree whose items carry their status, their
/// task's mark and a caption naming the branch or the directory. A
/// space with no agent tabs shows its current `cwd` alone in place of the
/// tree, so an empty space still reads as "somewhere", not blank. Plain shell
/// tabs (and anything else not recognized as an agent) never appear here;
/// they still exist in the tab strip above the pane (see
/// [`render_tab_strip`]), scoped to whichever space is selected. The
/// underlying workspace/directory this client is attached to (see
/// `Workspace` in `uze-terminal`) is deliberately never shown — it's
/// infrastructure the user never organizes by; spaces are the only unit
/// that matters here.
pub(super) fn render_sidebar(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &WorkspaceModel,
    identities: &[AgentIdentity],
    hits: &mut Vec<(Rect, WorkspaceHit)>,
    metrics: &mut FrameMetrics,
) {
    // No padding at all. Top: the header must land on the exact row the tab
    // strip's own content does (that block has none either), or the two
    // panes' dividers drift out of alignment by one row. Right: the rows
    // keep their own pad off the divider. Left: every row of the column
    // already leads with a column of its own — a space's gutter, a listing's
    // lead — and an inset under those read as a margin the column could not
    // afford.
    let inner = Rule::draggable(Edge::Right, model.dragging_sidebar).render(frame, area);

    let mut rows = Rows::over(inner);

    // The header names the surface and ends in the control that opens the
    // other one — the management modal. No top padding: it lands on the
    // exact row the tab strip's own content does.
    if let Some(rect) = rows.next(1) {
        // One column in, the pad its controls keep at the other end: the
        // header is the column's own chrome, and a name flush against the
        // edge under a right-hand pad reads as a row that slipped.
        frame.render_widget(
            Paragraph::new(Span::styled(
                format!("{}work", " ".repeat(TRAILING_PAD as usize)),
                theme::fg_bold(Token::TextMuted),
            )),
            rect,
        );
        let more = theme::glyph(Symbol::Manage);
        let more_width = theme::width(Symbol::Manage);
        // One column off the divider, the same pad every row of this column
        // keeps at its right edge.
        let more_rect = Rect::new(
            rect.right().saturating_sub(more_width + TRAILING_PAD),
            rect.y,
            more_width,
            1,
        );
        // The way to grow the column sits beside the way out of it, with a
        // rule between them: one names an action this surface takes, the
        // other opens another surface, and side by side without it they read
        // as one pair of controls.
        // Muted while the prompt it opens is open: the word is where that
        // prompt came from, and in the accent beside it, it reads as a
        // second way in rather than as the one already taken. Otherwise the
        // accent is held back until the pointer asks for it, so the header's
        // one coloured word does not outshout the column under it.
        let new = "+ space";
        let new_hue = if model.root_picker.is_some() {
            Token::TextMuted
        } else {
            match chip_state(model, Some(WorkspaceHit::NewSpace)) {
                ChipState::Hovered | ChipState::Pressed => Token::Accent,
                ChipState::Resting | ChipState::Static => Token::AccentMuted,
            }
        };
        let new_width = Span::raw(new).width() as u16;
        let divider = theme::glyph(Symbol::TreeColumnDivider);
        let divider_width = theme::width(Symbol::TreeColumnDivider);
        let new_rect = Rect::new(
            more_rect
                .x
                .saturating_sub(new_width + divider_width + 2 * HEADER_GAP),
            rect.y,
            new_width,
            1,
        );
        frame.render_widget(
            Paragraph::new(Span::styled(new, theme::fg_bold(new_hue))),
            new_rect,
        );
        frame.render_widget(
            Paragraph::new(Span::styled(divider, theme::fg(Token::SurfaceHover))),
            Rect::new(new_rect.right() + HEADER_GAP, rect.y, divider_width, 1),
        );
        hits.push((new_rect, WorkspaceHit::NewSpace));
        // Never a button: only the mark's own colour answers the pointer
        // — muted at rest, beside a label of the same weight, brighter
        // under the hover and brightest while pressed.
        let more_hue = match chip_state(model, Some(WorkspaceHit::OpenManage)) {
            ChipState::Pressed => Token::TextBright,
            ChipState::Hovered => Token::TextPrimary,
            ChipState::Resting | ChipState::Static => Token::TextMuted,
        };
        frame.render_widget(
            Paragraph::new(Span::styled(more, theme::fg_bold(more_hue))),
            more_rect,
        );
        hits.push((more_rect, WorkspaceHit::OpenManage));
    }
    if let Some(error) = &model.error
        && let Some(rect) = rows.next(1)
    {
        frame.render_widget(
            Paragraph::new(Span::styled(
                error.clone(),
                Style::default()
                    .fg(theme::color(Token::StateDanger))
                    .add_modifier(Modifier::BOLD),
            )),
            rect,
        );
    }

    // The hairline under the header lands on the row the tab strip's own
    // bottom border does, so the two columns share one divider line.
    if let Some(rect) = rows.next(1) {
        frame.render_widget(
            Paragraph::new(Span::styled(
                // The full width, up to the divider between the two
                // columns: it is a rule closing the header, not a row of
                // content keeping the column's trailing pad.
                theme::glyph(Symbol::TreeDivider).repeat(rect.width as usize),
                theme::fg(Token::BorderFaint),
            )),
            rect,
        );
    }

    let Some(session) = &model.session else {
        return;
    };

    // A blank row above each space, the first one included: each card
    // gets an edge on both sides, and it is also where a space being
    // dragged is shown landing (see `draw_space_drop`).
    //
    // The fill alone was tried and is not enough. Every card but the one
    // in front is faded, so where two faded cards meet there is no edge
    // to find — the column reads as one surface with headers in it, and
    // the card in front reads as the top or the bottom of whichever
    // neighbour it touches rather than as its own thing. The row over the
    // first card is the same argument against the rule above it.
    //
    // Except between two minimized spaces, the one in front included: a
    // row of nothing between two headers is half the column spent on
    // spaces nobody is looking into, and the fill of the one in front is
    // edge enough on its own. They pack, and the rows open again around
    // whichever one is expanded.
    let rearranging = model.dragging_space.is_some_and(|dragging| dragging.armed);
    let mut gap_above = rows.slot(1).visible();
    // While the root picker is open it owns the column: the listing it
    // draws is a tree of directories, and side by side with the tree of
    // spaces neither would read as the one being chosen from. It stands
    // where the spaces it would join stand. Closing it brings them back.
    if let Some(picker) = &model.root_picker {
        render_root_picker(frame, picker, &mut rows, hits);
        return;
    }

    // The timeline keeps the foot of the column whatever the spaces above
    // come to: a section trailing the last space would sink out of sight
    // under a long tree, and a history that is only there while the tree
    // is short is no place to go looking for one. Its rows are reserved
    // before the spaces are laid out, and handed back to them below.
    let timeline = model
        .remembered
        .git_badge
        .as_ref()
        .and_then(|badge| badge.timeline.as_ref());
    // Two sections stacked at the foot: the steps above the history, each
    // taking its rows before the tree is laid out, so neither is ever
    // drawn over the other. Only one of them is open at a time (see
    // `toggle_timeline`), which is what keeps the pair from eating the
    // column the spaces are for.
    let column_bottom = rows.bottom;
    let steps = model.first_steps();
    let steps_height = steps.height();
    let reserved = timeline.map_or(0, |timeline| {
        timeline_height(
            timeline,
            model.timeline_collapsed,
            model.timeline_rows,
            rows.remaining().saturating_sub(steps_height),
        )
    });
    let strip = steps.rect(Rect::new(
        inner.x,
        inner.y,
        inner.width,
        column_bottom
            .saturating_sub(inner.y)
            .saturating_sub(reserved),
    ));

    // One row of air above the foot, so a tree that grows to meet it still
    // reads as a tree over two sections rather than as one list.
    rows.bottom = strip.map_or(column_bottom - reserved, |rect| rect.y.saturating_sub(1));
    // The release notice sits on whatever holds the foot — the steps, or
    // the history once the steps are put away — rather than under them: it
    // is news, and news below two sections reads as the column's floor.
    if let Some(notice) = model.release.as_ref().map(crate::ui::ReleaseNotice)
        && let Some(rect) = notice.rect(Rect::new(
            inner.x,
            inner.y,
            inner.width,
            rows.bottom.saturating_sub(inner.y),
        ))
    {
        let targets = notice.render(frame, rect);
        hits.push((targets.dismiss, WorkspaceHit::DismissRelease));
        hits.extend(
            targets
                .notes
                .into_iter()
                .map(|rect| (rect, WorkspaceHit::OpenReleaseNotes)),
        );
        rows.bottom = rect.y;
    }

    // What the column cannot show is scrolled to, not lost: the tree grows
    // with the work, and a space that fell off the foot of it — under a
    // long tree above, or under the timeline holding the foot — used to be
    // unreachable rather than merely out of view. The bound is measured
    // here, where the tree's own window is known, and handed back for the
    // wheel to stay inside (see `scroll_tree`).
    let overflow =
        tree_rows(model, session, identities, rearranging).saturating_sub(rows.remaining());
    metrics.tree_overflow = overflow;
    rows.scroll_past(model.remembered.tree_scroll.min(overflow));

    let dropping = model
        .dragging_space
        .filter(|dragging| dragging.armed)
        .and_then(|dragging| dragging.pending);
    for (index, space) in session.workspace.spaces.iter().enumerate() {
        let is_active_space = space.id == session.workspace.selected_space;
        // The row under this space is also the row over the next one. It
        // is wanted beside a space that is open — and always while a
        // space is being carried, because it is the one place a drop can
        // be drawn.
        let margin = rearranging
            || !model.space_folded(space)
            || session.workspace.spaces[index + 1..]
                .first()
                .is_none_or(|next| !model.space_folded(next));
        if dropping == Some(PendingDrop::Before(space.id)) {
            draw_space_drop(frame, gap_above);
        }
        let header = rows.slot(1);
        if header.is_full() {
            break;
        }
        if let Some(header_rect) = header.visible() {
            render_space_header(frame, header_rect, session, space, model, identities, hits);
        }
        // Where a space's work is, under its name. Minimized, only while
        // it is the one in front: the name is what a person reads down a
        // column of spaces, and a path under every one of them is a
        // second column of text to read past to reach the first. Asked
        // for by selecting it, which is when the answer is worth a row.
        //
        // Open with nothing in it, always — there it is not detail under
        // a name, it is the whole of what the block has to show, and
        // without it the space is a header over nothing.
        let folded = model.space_folded(space);
        let empty = agent_tabs_of(space, identities).is_empty();
        if (folded && is_active_space) || (!folded && empty) {
            render_space_caption(frame, &mut rows, hits, model, session, space, identities);
        }
        if folded {
            gap_above = margin.then(|| rows.slot(1).visible()).flatten();
            continue;
        }

        // Every fact a row draws, resolved once per agent, so drawing only
        // decides where each goes.
        let agent_tabs = agents_in_drawing_order(model, space, identities);
        let agents: Vec<SidebarAgent<'_>> = agent_tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                SidebarAgent::resolve(
                    model,
                    identities,
                    space,
                    tab,
                    index + 1 == agent_tabs.len(),
                    is_active_space,
                )
            })
            .collect();

        if !agents.is_empty() {
            let captions: Vec<TreeCaption> = agents
                .iter()
                .map(|agent| TreeCaption::resolve(model, agent))
                .collect();
            draw_tree(frame, &mut rows, hits, is_active_space, &agents, &captions);
        }
        gap_above = margin.then(|| rows.slot(1).visible()).flatten();
    }
    if dropping == Some(PendingDrop::End) {
        draw_space_drop(frame, gap_above);
    }

    if let Some(rect) = strip {
        let mut section_hits = Vec::new();
        crate::ui::extension_view::render_section(
            frame,
            &steps.section(),
            &mut Rows::over(rect),
            false,
            &mut section_hits,
        );
        // The closing mark rides on the header, and an ordinary click
        // resolves against the *first* rect that contains it
        // (`WorkspaceModel::hit_rect_at`) — so the mark goes in ahead of
        // the header it sits on, or the header swallows it and the section
        // folds instead of leaving.
        if let Some(rect) = section_hits.iter().find_map(|(rect, hit)| {
            matches!(hit, ViewHit::ToggleSection)
                .then(|| steps.close_rect(*rect))
                .flatten()
        }) {
            hits.push((rect, WorkspaceHit::CloseFirstSteps));
        }
        for (rect, hit) in section_hits {
            match hit {
                ViewHit::ToggleSection => hits.push((rect, WorkspaceHit::ToggleFirstSteps)),
                ViewHit::SelectItem(index) => {
                    if let Some(action) = FIRST_STEPS.get(index) {
                        hits.push((rect, WorkspaceHit::QuickAction(*action)));
                    }
                }
                _ => {}
            }
        }
    }

    if let Some(timeline) = timeline
        && reserved > 0
    {
        rows.scroll_past(0);
        rows.bottom = column_bottom;
        rows.y = column_bottom - reserved;
        metrics.marquee |= render_timeline(frame, timeline, model, &mut rows, hits);
    }
}

/// The rows the space tree comes to, whether or not the column can show
/// them all: a header per space, the cwd caption a space with no agent
/// shows in place of its tree — and a minimized one shows only while it
/// is the one in front — two rows per agent with the separator row
/// between the two groups, the blank row over each space except between
/// two that come to a single line, and the column's own top margin. A minimized space,
/// or an open one with no agent, has its header and the caption under it. Measured up front rather
/// than counted while drawing, because how far the tree may be scrolled
/// has to be known before its first row is laid out.
fn tree_rows(
    model: &WorkspaceModel,
    session: &Session,
    identities: &[AgentIdentity],
    rearranging: bool,
) -> u16 {
    let spaces = &session.workspace.spaces;
    spaces
        .iter()
        .enumerate()
        .map(|(index, space)| {
            let agents = agent_tabs_of(space, identities).len() as u16;
            // The same conditions `render_sidebar` draws by: a minimized
            // space nobody is in is its header and nothing else, and two
            // minimized spaces in a row have no blank row between them.
            let body = if model.space_folded(space) {
                u16::from(space.id == session.workspace.selected_space)
            } else if agents == 0 {
                1
            } else {
                agent_rows(agents)
            };
            let margin = rearranging
                || !model.space_folded(space)
                || spaces[index + 1..]
                    .first()
                    .is_none_or(|next| !model.space_folded(next));
            1 + body + u16::from(margin)
        })
        .sum::<u16>()
        // The column's own top margin, which is a row above the first
        // space rather than one of the rows between a pair of them.
        + 1
}

/// Where a dragged space would land: an accent hairline across the blank
/// row between two spaces — a line where it goes, rather than a mark on a
/// header, whose leading column already says which space is selected.
fn draw_space_drop(frame: &mut ratatui::Frame<'_>, gap: Option<Rect>) {
    let Some(gap) = gap else {
        return;
    };
    let width = gap.width.saturating_sub(1 + TRAILING_PAD);
    frame.render_widget(
        Paragraph::new(theme::glyph(Symbol::TreeDivider).repeat(width as usize))
            .style(theme::fg(Token::Accent)),
        Rect::new(gap.x + 1, gap.y, width, 1),
    );
}

/// The agent tabs of a space, in the order the sidebar draws them — the
/// order `step_agent` walks too.
pub(super) fn agent_tabs_of<'a>(space: &'a Space, identities: &[AgentIdentity]) -> Vec<&'a Tab> {
    space
        .tabs
        .iter()
        .filter(|tab| agent_identity_for_tab(identities, tab).is_some())
        .collect()
}

/// A space's agents in the order the column draws them: the ones working
/// in its own root, then the isolated ones, each group keeping the order
/// the operator dragged it into.
///
/// The keyboard walks this order rather than the tab order, because an
/// agent that isolates moves between the groups and a walk that skipped
/// past where the row *is* would be reading a column nobody sees.
pub(super) fn agents_in_drawing_order<'a>(
    model: &WorkspaceModel,
    space: &'a Space,
    identities: &[AgentIdentity],
) -> Vec<&'a Tab> {
    let mut tabs = agent_tabs_of(space, identities);
    tabs.sort_by_key(|tab| agent_group(model, tab.id));
    tabs
}

/// Which of a space's two groups the agent on `tab` belongs to — read
/// off the record UZE wrote rather than the shape of the directory the
/// pane happens to sit in.
pub(super) fn agent_group(model: &WorkspaceModel, tab: TabId) -> AgentGroup {
    // The record's own answer, not the branch: every agent is on one now,
    // and an agent in the space's root would have grouped itself with the
    // isolated ones the moment its row learnt what branch that was.
    if let Some(task) = model.tab_task(tab) {
        return if task.isolated {
            AgentGroup::Isolated
        } else {
            AgentGroup::InTheRoot
        };
    }
    // Until there is one — the first frames of an attach, where the panes
    // arrive from the runtime and the records are still being read — the
    // directory the pane stands in is the only fact there is, and it is a
    // fact rather than a guess: `.worktrees/<id>` is a layout UZE owns,
    // and a slot is never a space of its own, so a pane in one was put
    // there by a placement. Drawing every agent in the root's group until
    // a Git pass answers says something untrue about where they are, and
    // says it to an operator who just opened the client.
    model
        .tab(tab)
        .filter(|tab| uze_application::is_isolated_checkout(&tab.pane.cwd))
        .map_or(AgentGroup::InTheRoot, |_| AgentGroup::Isolated)
}

/// The two groups a space's column is drawn in, in the order it draws
/// them: the agents sharing the space's own root, then the ones working
/// in a checkout of their own.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum AgentGroup {
    InTheRoot,
    Isolated,
}

impl AgentGroup {
    pub(super) fn is_isolated(self) -> bool {
        self == Self::Isolated
    }
}

/// The rows a space's agents take under its caption — the header, the
/// caption and the blank row after the space are the caller's. Every kind draws the same
/// two-row item, so one measure serves them all, and the scroll bound,
/// taken before the first row is drawn, can never disagree with the rows
/// that follow.
fn agent_rows(agents: u16) -> u16 {
    (agents * 3).saturating_sub(1)
}

/// One agent of a space, resolved once: what its two rows say and
/// which of its states are on.
struct SidebarAgent<'a> {
    tab: &'a Tab,
    /// Whether this agent works in a checkout of its own. What decides
    /// the group its row sits in, the hue it wears, and whether the tree
    /// branches for it.
    isolated: bool,
    /// Selected *and* in the active space: the one agent receiving
    /// keystrokes.
    is_current: bool,
    status: AgentTabStatus,
    renaming: Option<&'a RenameBuffer>,
    drop_target: bool,
    harness: Option<&'a str>,
    tick: usize,
}

/// What only the tree's two-row item says about an agent: its task's mark
/// and the caption row beneath the label.
struct TreeCaption {
    task_mark: Option<(String, Color)>,
    /// A checkout removed from under the agent whose task the preserved
    /// list still holds — the row offers to resume it.
    resumable: bool,
    /// The harness running the agent, or the words for a checkout that
    /// is gone.
    detail: String,
    detail_color: Color,
}

impl TreeCaption {
    fn resolve(model: &WorkspaceModel, agent: &SidebarAgent<'_>) -> Self {
        let tab = agent.tab;
        let task = model.tab_task(tab.id);
        let task_mark = task.and_then(|task| task_mark(&model.drawn_state(task)));
        // A checkout removed from under the agent is said in words, not as
        // the kernel's `(deleted)` path: the process cannot work there any
        // more, and the task it was running is what the preserved list now
        // holds.
        let lost = model.remembered.lost_checkouts.contains(&tab.pane.id);
        let resumable = lost && model.lost_task(tab.id).is_some();
        // What runs here, named by its id (`claude`, `codex`) — the same
        // word the picker launches and the process reports.
        //
        // Not the branch, which this row used to carry: a task's label is
        // *derived* from its branch (`worktree::label_of`), so the caption
        // repeated the name above it minus the type — and a task nobody
        // named reads `agent/<id>` under a label that is the identifier
        // itself. For an agent in the root it was worse: they all share
        // the operator's branch, so every caption said the same thing.
        // The branch belongs to the space, and the space's header and the
        // timeline are where it is said once.
        //
        // The harness is the one fact that tells two agents of a space
        // apart, and it cost a click to see. A row is only drawn for a
        // recognized harness (`agent_tabs_of`), so the pane's own process
        // is a fallback for a race, not a second answer.
        let detail = if lost {
            "checkout removed".to_owned()
        } else {
            agent
                .harness
                .unwrap_or(tab.pane.process.as_str())
                .to_owned()
        };
        let detail_color = if lost {
            theme::color(Token::StateWarning)
        } else {
            caption_color(agent.is_current)
        };
        Self {
            task_mark,
            resumable,
            detail,
            detail_color,
        }
    }
}

impl<'a> SidebarAgent<'a> {
    fn resolve(
        model: &'a WorkspaceModel,
        identities: &'a [AgentIdentity],
        space: &Space,
        tab: &'a Tab,
        is_last: bool,
        is_active_space: bool,
    ) -> Self {
        // The agent the space is about, not its `selected_tab`: a shell
        // opened beside an agent is part of that agent's own context, and
        // switching into it must not unselect the agent in this tree (see
        // `space_context_agent`). Every space names a context agent,
        // including the ones the user is not in — so `selected` alone put
        // a `●` on one agent per open space, each claiming to be the one
        // receiving keystrokes. Only the active space's selection is that
        // agent.
        let selected = Some(tab.id) == space_context_agent(space, identities);
        let is_current = is_active_space && selected;
        let renaming = model
            .renaming
            .as_ref()
            .filter(|(target, _)| *target == RenameTarget::Tab(tab.id))
            .map(|(_, buffer)| buffer);
        // A tab-reorder drag in this exact space, resolved to drop right
        // before (or, on the last row, at the end after) this one.
        let drop_target = model.dragging_tab.is_some_and(|dragging| {
            dragging.is_pending_drop_row(
                TabDragGroup::Agents(space.id, agent_group(model, tab.id)),
                tab.id,
                is_last,
            )
        });
        Self {
            tab,
            isolated: agent_group(model, tab.id).is_isolated(),
            is_current,
            status: model.agent_tab_status(tab.pane.id, is_current),
            renaming,
            drop_target,
            harness: agent_identity_for_tab(identities, tab),
            tick: model.tick,
        }
    }

    /// The row's label: the rename buffer being typed, or the tab's label
    /// elided to `room`.
    fn label(&self, room: u16) -> Vec<Span<'static>> {
        match self.renaming {
            Some(buffer) => rename_spans(buffer),
            None => {
                // One item in the whole column is bright and bold, and it
                // is whatever is receiving keystrokes — here, or the
                // header of the space in front (see
                // `render_space_header`, which wears the same style under
                // the same predicate).
                //
                // The hue used to follow the space's own context agent
                // while the weight followed `is_current`, and every space
                // names a context agent — the ones nobody is in
                // included. Walking from space A to space B left A's
                // agent in the bright hue and merely unbolded, so two
                // rows claimed to be where the keyboard was.
                //
                // What that hue was worth saying — which agent a space
                // would land on — is now said only once the space is
                // entered, by the same row going bright. A second, dimmer
                // register for it in the spaces nobody is in would be a
                // third level of emphasis in a column that is asking for
                // fewer.
                vec![Span::styled(
                    text::elide(&self.tab.label, room as usize),
                    label_style(self.is_current, false),
                )]
            }
        }
    }
}

/// A label being renamed: bright and bold, since it is where keys land,
/// with the caret where the next one will.
fn rename_spans(buffer: &RenameBuffer) -> Vec<Span<'static>> {
    let style = Style::default()
        .fg(theme::color(Token::TextBright))
        .add_modifier(Modifier::BOLD);
    let (before, after) = buffer.split();
    vec![
        Span::styled(before.to_owned(), style),
        Span::styled(theme::glyph(Symbol::CursorText), style),
        Span::styled(after.to_owned(), style),
    ]
}

/// The row under a minimized or empty space's header: the directory the
/// space's work is in. Lit along with its header, and selecting the space
/// like any caption.
///
/// Minimized, it is drawn only for the space in front. A name is what a
/// person reads down a column of spaces, and the path under every one of
/// them was a second column of text to read past to reach the first —
/// tiring exactly when the column is long, which is when the names
/// matter most. Selecting a space is how it is asked for, and the row it
/// costs is a row the space in front can afford.
///
/// The directory rather than the branch its root is on. A space is a
/// place, and the fold is what asks about it: opened, its agents answer
/// for themselves and say their own branches; minimized, they are gone
/// from the column and the one thing left to say is *where* — which a
/// branch name does not say, and which was the question the header's own
/// `⇄` existed to answer, a control per space for a fact the row could
/// simply carry.
fn render_space_caption(
    frame: &mut ratatui::Frame<'_>,
    rows: &mut Rows,
    hits: &mut Vec<(Rect, WorkspaceHit)>,
    model: &WorkspaceModel,
    session: &Session,
    space: &Space,
    identities: &[AgentIdentity],
) {
    let Some(rect) = rows.slot(1).visible() else {
        return;
    };
    let selected = space.id == session.workspace.selected_space;
    let caption = crate::ui::display_project_path(&space_cwd(space, identities));
    // Pinned to the right edge, the one trailing column every row in the
    // sidebar keeps: the column the header's own name leads stays the
    // space's, and what it is about reads as a caption to it rather than
    // as another row of the tree. Lit along with its header.
    let is_current = header_is_current(space, session, identities, model.space_folded(space));
    let mut spans = vec![space_gutter(selected, is_current)];
    let hue = theme::color(Token::TextDim);
    row::push_trailing(&mut spans, rect.width, caption, hue);
    // A minimized space is its header and this row, and the two are one
    // item: the trace the header wears when it is the row in front runs
    // through this row too, or the item would be lit down half its height.
    let ground = if is_current {
        theme::tinted(Token::Accent, Token::SurfaceRaisedSubtle)
    } else {
        block_ground(selected || !model.space_folded(space))
    };
    row::pad_to(&mut spans, rect.width, ground);
    frame.render_widget(Paragraph::new(Line::from(spans)), rect);
    hits.push((rect, WorkspaceHit::SelectSpace(space.id)));
}

/// The ground under a space's rows: the panel itself where a space is
/// `lifted`, and the same panel faded where it is not. Every space is a
/// block either way — what the fill says is which blocks have something
/// standing on them.
///
/// A space is lifted when it is the one in front *or* when it is open,
/// which is not the same rule as "selected" and used not to be one at
/// all. The fade is a third of a surface that is itself seven percent
/// over the background, so what reaches the screen is about two — enough
/// to find the edge of a minimized card, which is two rows reading as one
/// item, and not enough to be a ground. An open space has agent rows
/// standing on it, and on that fill they stood on the panel instead: the
/// block had a header and then nothing under it, so its rows read as
/// loose in the column rather than as its contents. Which one is in front
/// is the gutter's answer now (see [`space_gutter`]), not the fill's.
fn block_ground(lifted: bool) -> Color {
    if lifted {
        theme::color(Token::SurfaceRaisedSubtle)
    } else {
        theme::faded(Token::SurfaceRaisedSubtle)
    }
}

/// Each agent a two-row item — status and name over the harness running
/// it — one item straight after the next, for either kind, all of it
/// beside the space's gutter (see [`space_gutter`]). Selection in the
/// tree is the item's own fill, a trace of the space's hue over the
/// panel the space sits on, plus the status glyph; the drop indicator
/// during a drag is an accent bar down the item's leading column, on
/// both of its rows so it reads as the whole item.
///
/// An agent in the space's own root stands where its space stands — it
/// works there, so there is no level to draw. The isolated ones hang off
/// a trunk of their own, one column in, because their work does: the
/// group that is somewhere else gets the one indent in the column, and
/// the line down it says those rows are one collection. A group of one
/// is no collection, so it stands where the root's agents stand.
///
/// That trunk is the group's, not the space's, which is why it reads
/// where the old `├─` did not: the space's gutter is a bar flush against
/// its cell and a box-drawing corner cannot meet one, so the connector
/// pointed at a line it could not touch. Given a column of its own there
/// is nothing to meet — it is its own line, and it starts and ends with
/// the group.
///
/// The agent receiving keystrokes is the one stretch of the *space's*
/// gutter in the accent; the trunk stays quiet either way, because it
/// answers which group, not which row.
///
/// Only an open space with agents in it reaches here, so every row this
/// draws stands on a lifted ground whether or not its space is the one
/// in front (see [`block_ground`]).
fn draw_tree(
    frame: &mut ratatui::Frame<'_>,
    rows: &mut Rows,
    hits: &mut Vec<(Rect, WorkspaceHit)>,
    is_active_space: bool,
    agents: &[SidebarAgent<'_>],
    captions: &[TreeCaption],
) {
    // Two groups: the agents working in the space's own root, then the
    // isolated ones. A blank row between them, and only when both have
    // somebody in them — a separator between collections, never between
    // siblings. Isolating an agent moves its row from the first group to
    // the second, which is how the operator sees the action happened.
    // Which of the isolated ones opens the group's trunk and which
    // closes it — the two rows whose glyph is not the one in between.
    let first_isolated = agents.iter().position(|agent| agent.isolated);
    let last_isolated = agents.iter().rposition(|agent| agent.isolated);
    /// Every space `draw_tree` is called for is open.
    const OPEN: bool = true;
    let mut previous: Option<bool> = None;
    for (index, (agent, caption)) in agents.iter().zip(captions).enumerate() {
        let tab = agent.tab;
        let isolated = agent.isolated;
        let branch = Branch::of(
            isolated,
            Some(index) == first_isolated,
            Some(index) == last_isolated,
        );
        if previous.is_some_and(|was| was != isolated)
            && let Some(gap) = rows.slot(1).visible()
        {
            let mut spans = vec![space_gutter(is_active_space, false)];
            row::pad_to(&mut spans, gap.width, block_ground(OPEN));
            frame.render_widget(Paragraph::new(Line::from(spans)), gap);
        }
        previous = Some(isolated);
        // The status glyph and the name land one column inside the
        // header's fold and name, so the column reads space > agent at the
        // cost of one column. The agent receiving keystrokes lights its
        // stretch of the gutter, and so does the row a dragged agent would
        // land before: the same line, in the accent, never a heavier one.
        let lit = agent.is_current || agent.drop_target;
        // The row the keyboard is on carries a trace of its own group's
        // hue over the panel every other row sits on — the item is two
        // rows and the status glyph is one cell, so the block is what
        // reads as "here" at a glance. Nothing outside the active space
        // is tinted: `is_current` is selected *and* receiving keystrokes.
        let surface = if agent.is_current {
            Some(theme::tinted(Token::Accent, Token::SurfaceRaisedSubtle))
        } else {
            Some(block_ground(OPEN))
        };
        // The status glyph in the header's own fold column for an agent
        // in the root, and one further in for one working in a checkout
        // of its own — that column carrying the group's own trunk.
        let lead = || [space_gutter(is_active_space, lit), branch.corner()];
        let label_slot = rows.slot(1);
        if label_slot.is_full() {
            break;
        }
        if let Some(label_rect) = label_slot.visible() {
            let [gutter_span, indent_span] = lead();
            let indicator_span = Span::styled(
                agent.status.glyph(agent.tick),
                Style::default().fg(agent.status.color()),
            );
            // Elided rather than run under the mark pinned to the right
            // edge, the way the branch beneath it is.
            let taken = (gutter_span.width() + indent_span.width() + indicator_span.width()) as u16
                + caption
                    .task_mark
                    .as_ref()
                    .map_or(0, |(mark, _)| 1 + Span::raw(mark.as_str()).width() as u16)
                + TRAILING_PAD;
            let room = label_rect.width.saturating_sub(taken).max(1);
            let label = agent.label(room);
            // The task mark behind the label is the one click target that
            // opens the catalog (see `push_trailing_mark`): the status glyph
            // in front of the name is not, so the row's leading column stays
            // a plain part of selecting the tab. Pushed before the row's
            // own `SelectTab` hit below, since the click search takes the
            // first rect it lands in — a 1-column target inside a row-wide
            // one only ever wins by being found first.
            let mut spans = vec![gutter_span, indent_span, indicator_span];
            spans.extend(label);
            if let Some((mark, hue)) = &caption.task_mark {
                push_trailing_mark(&mut spans, hits, label_rect, mark, *hue);
            }
            if let Some(surface) = surface {
                row::pad_to(&mut spans, label_rect.width, surface);
            }
            frame.render_widget(Paragraph::new(Line::from(spans)), label_rect);
            hits.push((label_rect, WorkspaceHit::SelectTab(tab.id)));
        }

        if let Some(detail_rect) = rows.slot(1).visible() {
            // Under the agent's name, past the indent and the status
            // column, in either kind.
            let mut spans = vec![
                space_gutter(is_active_space, lit),
                branch.stem(),
                Span::raw("  "),
            ];
            // Right-aligned under the task mark, with the same trailing
            // pad off the divider. The way back in, on the row itself:
            // "resume" puts the task this pane was running into a slot of
            // its own, via the same picker a new agent goes through.
            // Offered only while the task is waiting for one (see
            // `lost_task`).
            //
            // The only thing this edge carries. What a pull and a push
            // would move used to stand here too, and it is not the
            // agent's: it is read from the checkout, so every agent
            // sharing one — four in a space's own root is an ordinary
            // day — printed the same two numbers under its own name. It
            // is said once now, on the header of the space whose checkout
            // it is about (see `push_trailing_sync`), which is where the
            // branch went for the same reason.
            const RESUME: &str = "resume";
            let trailing: Vec<Span<'_>> = if caption.resumable {
                vec![Span::styled(RESUME, theme::fg(Token::Accent))]
            } else {
                Vec::new()
            };
            // The branch is elided, never cut: a name longer than the column
            // used to run under the caption and off the right edge, so
            // the one thing the row was pinning there — "resume" — was
            // what disappeared.
            {
                let taken: u16 = spans
                    .iter()
                    .chain(&trailing)
                    .map(|span| span.width() as u16)
                    .sum::<u16>()
                    + TRAILING_PAD;
                let room = detail_rect.width.saturating_sub(taken).max(1);
                spans.push(Span::styled(
                    text::elide(&caption.detail, room as usize),
                    Style::default().fg(caption.detail_color),
                ));
            }
            if !trailing.is_empty() {
                let x = detail_rect
                    .right()
                    .saturating_sub(TRAILING_PAD + RESUME.len() as u16);
                hits.push((
                    Rect::new(x, detail_rect.y, RESUME.len() as u16, 1),
                    WorkspaceHit::ResumeLostCheckout(tab.id),
                ));
                let used: u16 = spans
                    .iter()
                    .chain(&trailing)
                    .map(|span| span.width() as u16)
                    .sum::<u16>()
                    + TRAILING_PAD;
                let gap = detail_rect.width.saturating_sub(used).max(1);
                spans.push(Span::raw(" ".repeat(gap as usize)));
                spans.extend(trailing);
                spans.push(Span::raw(" ".repeat(TRAILING_PAD as usize)));
            }
            if let Some(surface) = surface {
                row::pad_to(&mut spans, detail_rect.width, surface);
            }
            frame.render_widget(Paragraph::new(Line::from(spans)), detail_rect);
            // The label and its dim branch/cwd caption read as one tree
            // item — clicking the caption line must select the tab too, not
            // just the label text above it.
            hits.push((detail_rect, WorkspaceHit::SelectTab(tab.id)));
        }
    }
}

/// What is worth trying once on this side of the product: putting an agent
/// to work, moving between them, seeing what a change actually did, finding
/// the work no live tab is in front of, and the surface that lists the
/// rest. Each is a gesture nobody discovers by staring at a screen, and
/// none of them destroys anything, so a list that invites them costs the
/// reader nothing.
pub(super) const FIRST_STEPS: [Action; 6] = [
    Action::NewAgent,
    Action::NextAgent,
    Action::ToggleChanges,
    Action::ToggleFiles,
    Action::TogglePreservedWork,
    Action::OpenActionIndex,
];

/// Named from the mode rather than from what is open: the key beside a
/// step must not change because an overlay is up.
pub(super) const FIRST_STEP_SCOPES: &[uze_keys::Scope] =
    &[uze_keys::Scope::Global, uze_keys::Scope::Workspace];

/// The rows the tree above the timeline keeps whatever the section is
/// dragged to — a space header, an agent and its caption, and the blank
/// row after them.
const MIN_TREE_ROWS: u16 = 4;

/// The rows the timeline section takes at the foot of the column: its
/// header, and while it is open the divider under it and one row per
/// commit. Left alone, that is within half of what the column has left,
/// since the spaces are what the sidebar is for; dragged (`rows_wanted`),
/// it is what was asked for, within the history there is and what the
/// column can spare past the tree's minimum. Nothing when even the header
/// would not fit.
pub(super) fn timeline_height(
    timeline: &uze_extensions::code::Timeline,
    collapsed: bool,
    rows_wanted: Option<u16>,
    remaining: u16,
) -> u16 {
    let commits = timeline.commits.len() as u16;
    let (chrome, rows, budget) = match rows_wanted {
        _ if collapsed => (1, 0, remaining / 2),
        Some(wanted) => (
            TIMELINE_CHROME,
            wanted.clamp(1, commits),
            remaining.saturating_sub(MIN_TREE_ROWS),
        ),
        None => (TIMELINE_CHROME, commits, remaining / 2),
    };
    if budget < chrome {
        return 0;
    }
    (chrome + rows).min(budget)
}

/// The rows of an open timeline section that are not commits: its header
/// and the divider under it. The drag handler subtracts the same two to
/// turn where the divider was dropped into a count of commit rows.
pub(super) const TIMELINE_CHROME: u16 = 2;

/// The sidebar's commit-timeline section.
///
/// Nothing here knows what a commit is. The extension says what the
/// section holds ([`git::timeline_section`]) and
/// `extension_view::render_section` draws it; this only supplies the host
/// state the extension is not allowed to hold — whether the section is
/// folded, how far it is scrolled, whether its divider is being dragged —
/// and tags the hits that come back with the surface they came from.
fn render_timeline(
    frame: &mut ratatui::Frame<'_>,
    timeline: &uze_extensions::code::Timeline,
    model: &WorkspaceModel,
    rows: &mut Rows,
    hits: &mut Vec<(Rect, WorkspaceHit)>,
) -> bool {
    let section = uze_extensions::code::timeline_section(
        timeline,
        model.timeline_collapsed,
        model.timeline_scroll,
    );
    let mut section_hits = Vec::new();
    let mut column = Rows::over(Rect::new(rows.x, rows.y, rows.width, rows.remaining()));
    // The branch name this header carries is the one caption in the
    // column with no natural length — it is whatever somebody called the
    // work — so it is the one that runs past its room. While the pointer
    // is on the header, it slides instead of stopping at an "…": a
    // branch is read from both ends, and the end is the half an "…" eats.
    let hovered = model.hovered
        == Some(WorkspaceHit::Extension(ExtensionHit::CodeTimeline(
            ViewHit::ToggleSection,
        )));
    let hovered_row = match model.hovered {
        Some(WorkspaceHit::Extension(ExtensionHit::CodeTimeline(ViewHit::SelectItem(index)))) => {
            Some(index)
        }
        _ => None,
    };
    let sliding = crate::ui::extension_view::render_section_with(
        frame,
        &section,
        &mut column,
        model.dragging_timeline,
        hovered.then_some(model.tick),
        hovered_row,
        &mut section_hits,
    );
    hits.extend(section_hits.into_iter().map(|(rect, hit)| {
        (
            rect,
            WorkspaceHit::Extension(ExtensionHit::CodeTimeline(hit)),
        )
    }));
    sliding
}

/// Where the commit popup goes and what it says, resolved once for both
/// drawing it and bounding its scroll.
pub(super) struct CommitDetailLayout {
    pub(super) rect: Rect,
    pub(super) inner: Rect,
    lines: Vec<Line<'static>>,
    /// The rows the text takes once wrapped to `inner`.
    pub(super) content_rows: u16,
}

impl CommitDetailLayout {
    /// The furthest the text can be scrolled and still fill the popup —
    /// what the wheel is held to, so it never scrolls into blank rows.
    pub(super) fn scroll_limit(&self) -> u16 {
        self.content_rows.saturating_sub(self.inner.height)
    }
}

/// One commit's account, beside the timeline row it was opened from and in
/// the pane's own columns — who, when, what it said, how much it touched,
/// and the branches and tags standing at it — the shape the support
/// dropdown already gives a fact sheet. A frame too narrow to fit it
/// beside the sidebar gets it over the pane instead, inset. Never wider
/// or taller than a hover card ought to be: a long message scrolls
/// inside it rather than growing it over the pane.
pub(super) fn commit_detail_layout(area: Rect, popup: &CommitDetailPopup) -> CommitDetailLayout {
    const MAX_WIDTH: u16 = 72;
    const MAX_HEIGHT: u16 = 20;
    const MIN_BESIDE_WIDTH: u16 = 40;
    let detail = &popup.detail;

    let beside = popup.anchor.right() + 1;
    let (x, width) = if area.right().saturating_sub(beside + 1) >= MIN_BESIDE_WIDTH {
        (beside, (area.right() - beside - 1).min(MAX_WIDTH))
    } else {
        let width = area.width.saturating_sub(4).clamp(1, MAX_WIDTH);
        (area.x + (area.width - width) / 2, width)
    };
    let inner_width = usize::from(width.saturating_sub(2 + 2 * POPUP_H_PAD).max(1));

    let mut lines = vec![
        row::title_row("commit", "esc", inner_width),
        Line::default(),
        Line::from(vec![
            Span::styled(
                format!("{} ", theme::glyph(Symbol::MarkToggleOn)),
                theme::fg(Token::StateInfo),
            ),
            Span::styled(detail.author.clone(), theme::fg(Token::TextPrimary)),
            Span::styled(
                {
                    let separator = theme::glyph(Symbol::HintSeparator);
                    format!("{separator}{}{separator}{}", detail.age, detail.date)
                },
                theme::fg(Token::TextSecondary),
            ),
        ]),
        Line::from(Span::styled(
            detail.subject.clone(),
            Style::default()
                .fg(theme::color(Token::TextBright))
                .add_modifier(Modifier::BOLD),
        )),
    ];
    if !detail.body.is_empty() {
        lines.push(Line::default());
        lines.extend(detail.body.lines().map(|line| {
            Line::from(Span::styled(
                line.to_owned(),
                theme::fg(Token::TextSecondary),
            ))
        }));
    }
    lines.push(Line::default());
    lines.push(Line::from(vec![
        Span::styled(
            format!(
                "{} file{} changed",
                detail.files_changed,
                if detail.files_changed == 1 { "" } else { "s" }
            ),
            theme::fg(Token::TextSecondary),
        ),
        Span::styled(
            format!("  +{}", detail.insertions),
            theme::fg(Token::StateSuccess),
        ),
        Span::styled(
            format!("  −{}", detail.deletions),
            theme::fg(Token::StateDanger),
        ),
    ]));
    // The target's label wears the target's gold — the hue the timeline
    // gives what has landed in it — and so does its remote-tracking twin;
    // every other ref at the commit is blue, like a commit still ahead.
    let is_target = |reference: &str| {
        popup.target.as_deref().is_some_and(|target| {
            reference == target
                || reference
                    .strip_suffix(target)
                    .is_some_and(|remote| remote.ends_with('/'))
        })
    };
    let mut footer: Vec<Span<'static>> = Vec::new();
    for reference in &detail.refs {
        if !footer.is_empty() {
            footer.push(Span::raw(" "));
        }
        let hue = if is_target(reference) {
            theme::color(Token::StateWarning)
        } else {
            theme::color(Token::StateInfo)
        };
        footer.push(Span::styled(
            format!(" {reference} "),
            theme::fg(Token::SurfaceBackground).bg(hue),
        ));
    }
    let used: usize = footer.iter().map(Span::width).sum();
    let gap = inner_width.saturating_sub(used + detail.short_hash.chars().count());
    footer.push(Span::raw(" ".repeat(gap.max(1))));
    footer.push(Span::styled(
        detail.short_hash.clone(),
        theme::fg(Token::TextMuted),
    ));
    lines.push(Line::from(footer));

    let content_rows: u16 = lines
        .iter()
        .map(|line| line.width().max(1).div_ceil(inner_width) as u16)
        .sum();
    let height = (content_rows + 2 + 2 * POPUP_V_PAD)
        .min(area.height)
        .clamp(1, MAX_HEIGHT);
    let rect = Rect::new(
        x,
        popup.anchor.y.min(area.bottom().saturating_sub(height)),
        width,
        height,
    );
    let inner = commit_detail_block()
        .padding(Padding::new(
            POPUP_H_PAD,
            POPUP_H_PAD,
            POPUP_V_PAD,
            POPUP_V_PAD,
        ))
        .inner(rect);
    CommitDetailLayout {
        rect,
        inner,
        lines,
        content_rows,
    }
}

fn commit_detail_block() -> Block<'static> {
    Surface::card().into_block()
}

pub(super) fn render_commit_detail(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    popup: &CommitDetailPopup,
) {
    let layout = commit_detail_layout(area, popup);
    let scroll = popup.scroll.min(layout.scroll_limit());
    frame.render_widget(Clear, layout.rect);
    frame.render_widget(commit_detail_block(), layout.rect);
    frame.render_widget(
        Paragraph::new(layout.lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        layout.inner,
    );
}

/// One space's header row in the sidebar tree — its label — dim for every
/// space,
/// active one included: the space is a container, not the thing the
/// operator is looking at, so bold is reserved for the agent tab actually
/// receiving keystrokes (see `render_sidebar`'s agent-row `label_style`).
/// The active space's whole envelope (this header plus every tab/detail/cwd
/// row nested under it — see the `is_active_space` fill in
/// [`render_sidebar`]) gets a neutral background instead of a left accent
/// bar, so the highlight reads as "this whole block is where you are"
/// rather than a thin per-row marker or an on-brand "selected" tint
/// This header row itself stays at the lighter
/// [`theme::color(Token::SurfaceRaised)`] while the rows it anchors go one
/// step darker, [`theme::color(Token::SurfaceRaisedSubtle)`] — the title
/// lifts slightly above the block it names instead of blending into it.
pub(super) fn render_space_header(
    frame: &mut ratatui::Frame<'_>,
    rect: Rect,
    session: &Session,
    space: &Space,
    model: &WorkspaceModel,
    identities: &[AgentIdentity],
    hits: &mut Vec<(Rect, WorkspaceHit)>,
) {
    let selected = space.id == session.workspace.selected_space;
    let collapsed = model.space_folded(space);
    // The header is a target of its own: clicking it lands on the space's
    // own shells, a context no agent row below speaks for. While that is
    // where the operator is — or the space is minimized, so its header is
    // all there is of it — the header wears the bar a selected item does.
    let is_current = header_is_current(space, session, identities, collapsed);
    let renaming_this = model
        .renaming
        .as_ref()
        .filter(|(target, _)| *target == RenameTarget::Space(space.id))
        .map(|(_, buffer)| buffer);
    // Bold while this is the space in front, and bright with it only
    // while the header is also the row receiving keystrokes — which is
    // when the space is minimized or speaks for no agent of its own, and
    // exactly when no agent row of it can be current. So the column
    // holds one bright name and, above it, the weight saying which block
    // that name is in.
    let name_style = label_style(is_current, selected);
    let fold = mark::disclosure(!collapsed);
    let mut spans = vec![
        space_gutter(selected, is_current),
        Span::styled(format!("{fold} "), theme::fg(Token::TextSecondary)),
    ];
    // The fold and the space after it: a target two cells wide, pushed
    // ahead of the row's own `SelectSpace` so it wins the click.
    hits.push((
        Rect::new(rect.x + 1, rect.y, 2, 1),
        WorkspaceHit::ToggleSpaceCollapsed(space.id),
    ));
    match renaming_this {
        Some(buffer) => spans.extend(rename_spans(buffer)),
        None => {
            // The label is what the space is called, and the row says that
            // alone. Where its work lives is the caption under it (see
            // [`render_space_caption`]) once the fold asks — a row that
            // can hold a path of any length, which this one, one line wide
            // and already carrying a name, could not.
            spans.push(Span::styled(space.label.clone(), name_style));
            if collapsed && let Some(status) = folded_status(model, space, identities) {
                spans.push(Span::styled(
                    format!(" {}", status.glyph(model.tick)),
                    Style::default().fg(status.color()),
                ));
            }
            push_trailing_controls(&mut spans, hits, rect, model, space, selected, identities);
        }
    }
    // The space's own row is a target like any other, so when it is the
    // one in front it wears what a selected agent row wears: the same
    // trace of the accent, over its own lighter surface. Everything else
    // about the header is unchanged — the trace says "this row", not
    // "this block", which is what the fill underneath already says.
    let ground = if is_current {
        // The one overlay: exactly what an agent row in front wears, and
        // what the caption under a minimized header wears with it. A
        // second tone for the same meaning would read as two states.
        theme::tinted(Token::Accent, Token::SurfaceRaisedSubtle)
    } else {
        theme::color(Token::SurfaceRaised)
    };
    row::pad_to(
        &mut spans,
        rect.width,
        if selected || !collapsed {
            ground
        } else {
            theme::fade(ground)
        },
    );
    frame.render_widget(Paragraph::new(Line::from(spans)), rect);
    hits.push((rect, WorkspaceHit::SelectSpace(space.id)));
}

/// The line down a space's leading column, from its header to its last
/// row: the space as one block, and only the `selected` one — a rail
/// beside every space said once per space what the block's own fill
/// already says, so the column read as a set of cages the selection had
/// to be found inside. `lit`, in the theme's accent, along what is
/// selected within it.
///
/// A bar rather than a box-drawing vertical, and the faintest tone rather
/// than the muted one: this marks the *edge* of a block, which is what
/// the `bar.*` glyphs are for and why they sit flush left in their cell,
/// where `│` is centred and drawn at whatever weight the font gives a
/// rule. It is the quietest thing in the column on purpose — it says
/// which block, not what is in it.
///
/// One hue for every agent, whichever group it sits in: the groups are
/// already told apart by where they stand in the column, under the
/// separator the two collections are split by, and a second axis saying
/// the same thing costs a colour that then means nothing else.
/// The one column between the space's gutter and an agent's row: empty
/// for an agent in the space's own root, and the isolated group's trunk
/// for one working in a checkout of its own.
///
/// One column, not two. Every step here is paid twice — once by the
/// group and once by the level inside it — so a two-column step put an
/// isolated agent's name four columns past its space's, and the column
/// spent more of itself on saying where a name sits than on the name.
/// Hence the indent *is* the trunk rather than sitting beside one: a
/// single column carrying both the level and the line that says these
/// rows are one collection.
///
/// The trunk is drawn by its ends, not by one glyph repeated: a `├`
/// carries a stem *upward* as well, so at the top of a group it pointed
/// at the blank row above and read as a line broken off rather than a
/// line starting. Each end closes.
///
/// A group of one takes no step at all: there is nothing for a line to
/// join, and an arm to a single row read as a stray mark beside a name
/// standing where every other name stands. The blank row above it still
/// says it is the other collection.
#[derive(Clone, Copy)]
enum Branch {
    /// In the space's own root, or the only isolated one: no level, no
    /// line.
    None,
    /// The first of the isolated ones, opening the trunk.
    Opens,
    /// Isolated, with siblings above and below it.
    Carries,
    /// The last of them: the trunk closes on its name and nothing runs
    /// under its caption.
    Closes,
}

impl Branch {
    fn of(isolated: bool, first: bool, last: bool) -> Self {
        match (isolated, first, last) {
            (false, ..) | (true, true, true) => Self::None,
            (true, true, false) => Self::Opens,
            (true, false, false) => Self::Carries,
            (true, false, true) => Self::Closes,
        }
    }

    /// What stands beside the agent's own name.
    fn corner(self) -> Span<'static> {
        match self {
            Self::None => Span::raw(""),
            Self::Opens => Self::drawn(Symbol::TreeFirst),
            Self::Carries => Self::drawn(Symbol::TreeBranch),
            Self::Closes => Self::drawn(Symbol::TreeLast),
        }
    }

    /// What stands beside the caption under it, which is the same item:
    /// the trunk runs through it, unless the group ended on the name
    /// above.
    fn stem(self) -> Span<'static> {
        match self {
            Self::Opens | Self::Carries => Self::drawn(Symbol::TreeVertical),
            Self::None => Span::raw(""),
            Self::Closes => Span::raw(" "),
        }
    }

    /// One column of a tree glyph. `tree.branch` and `tree.last` are two
    /// cells — a corner and the `─` that reached across to the status
    /// column — and that arm is what made the old connector cost a second
    /// column it did not need.
    fn drawn(symbol: Symbol) -> Span<'static> {
        let glyph = theme::glyph(symbol);
        let corner = glyph.chars().next().map(String::from).unwrap_or_default();
        Span::styled(corner, theme::fg(Token::TextFaint))
    }
}

fn space_gutter(selected: bool, lit: bool) -> Span<'static> {
    if !selected {
        return Span::raw(" ".repeat(theme::width(Symbol::BarThin) as usize));
    }
    Span::styled(theme::glyph(Symbol::BarThin), gutter_style(lit))
}

/// What a name wears: bright *and* bold for the one row receiving
/// keystrokes, bold alone for a name that is in front without being that
/// row, and quiet otherwise.
///
/// Two levels because there are two questions and one of them is not the
/// other's. Exactly one row in the column is `current` — an agent, or
/// the header of a space that is minimized or speaks for no agent — so
/// exactly one name is ever bright, which is what the hue is for. Which
/// *space* is in front is a second fact, and its header carried no mark
/// of it at all: a space of nothing but agents can never be the current
/// row, because there is no tab of its own to land on, so its name had
/// no state to reach however it was clicked. The weight answers that one
/// on its own, and costs the hue nothing.
fn label_style(current: bool, in_front: bool) -> Style {
    let style = Style::default().fg(if current {
        theme::color(Token::TextBright)
    } else {
        theme::color(Token::TextInactive)
    });
    if current || in_front {
        style.add_modifier(Modifier::BOLD)
    } else {
        style
    }
}

fn gutter_style(lit: bool) -> Style {
    if lit {
        theme::fg(Token::Accent)
    } else {
        theme::fg(Token::TextFaint)
    }
}

/// What a minimized space's agents are doing that is worth seeing through
/// the fold: one of them working, or one finished while nobody looked.
/// `None` when every agent is quiet — the header then says nothing more
/// than its name.
fn folded_status(
    model: &WorkspaceModel,
    space: &Space,
    identities: &[AgentIdentity],
) -> Option<AgentTabStatus> {
    let statuses: Vec<AgentTabStatus> = agent_tabs_of(space, identities)
        .iter()
        .map(|tab| model.agent_tab_status(tab.pane.id, false))
        .collect();
    [AgentTabStatus::Working, AgentTabStatus::Completed]
        .into_iter()
        .find(|wanted| statuses.contains(wanted))
}

/// Whether a space's header is the selected item: its space is selected and
/// either no agent of it is — its own shells are — or it is minimized, so
/// the header is all there is of it.
fn header_is_current(
    space: &Space,
    session: &Session,
    identities: &[AgentIdentity],
    folded: bool,
) -> bool {
    space.id == session.workspace.selected_space
        && (folded || space_context_agent(space, identities).is_none())
}

/// The mark a task's state puts after its label, and its hue: agent state
/// (`AgentTabStatus`) owns the column in front of the name, so what the
/// *task* is doing follows it.
///
/// Symbols only, never emoji: an emoji-presentation codepoint (`⚠`, `⏸`,
/// and `✎` in most terminal fonts) is drawn from a different family than
/// everything around it, double-width in some terminals and not others,
/// and immune to the hue this returns — it would ignore the color that
/// carries the meaning. Each state also gets a hue of its own rather than
/// three sharing `theme::color(Token::TextDim)`: color is what tells these apart at a glance,
/// the glyph is what tells them apart once you look. `Ready` deliberately
/// does *not* reuse `✓` — that is `AgentTabStatus::Completed`'s glyph one
/// column to the left, and the same mark in the same accent meaning two
/// different things is what made the second column read as an echo of the
/// first. It wears a mark of its own, `task.ready`, instead.
/// [`render_status_catalog`] is this table's legend and must move with it.
pub(super) fn task_mark(state: &WorkStateView) -> Option<(String, Color)> {
    let (symbol, hue) = match state {
        // Nothing to report, and for the same reason: a task that has not
        // committed yet and one whose agent left with nothing both hold
        // no work. `Closed` in particular must not wear `Integrated`'s
        // arrow — that arrow claims a delivery.
        WorkStateView::Running | WorkStateView::Closed => return None,
        WorkStateView::Uncommitted => (Symbol::PlusMinus, theme::color(Token::StateInfo)),
        WorkStateView::Ready => (Symbol::TaskReady, theme::color(Token::Accent)),
        // The one mark that points away from UZE, because the work does:
        // it is on the forge, and what happens to it next happens there.
        // Muted for the same reason the button is — nothing is being asked
        // of the operator — and deliberately not `Integrated`'s arrow,
        // which claims the work is in the target.
        WorkStateView::Published => (Symbol::ArrowExternal, theme::color(Token::StatePublished)),
        WorkStateView::Integrating => (Symbol::Ellipsis, theme::color(Token::StateInFlight)),
        // Split, where one warning mark used to cover both: a paused rebase
        // wants your hands in the slot, a failed gate wants the code fixed —
        // different work, and the sidebar was the one surface that never
        // said which (the strip's own button already did).
        WorkStateView::Conflicted { .. } => {
            (Symbol::MarkAttention, theme::color(Token::StateWarning))
        }
        WorkStateView::GateFailed => (Symbol::MarkCross, theme::color(Token::StateDanger)),
        WorkStateView::Integrated => (Symbol::ArrowUp, theme::color(Token::StateLanded)),
        WorkStateView::Parked => (Symbol::Menu, theme::color(Token::TextMuted)),
    };
    Some((theme::glyph(symbol), hue))
}

/// The legend for the two status columns an agent row carries, opened by
/// clicking the task mark (see [`WorkspaceHit::OpenStatusCatalog`]).
///
/// Every row is generated from the same tables the sidebar draws with —
/// [`task_mark`] and [`AgentTabStatus::glyph`]/`color` — so a glyph or a
/// hue can never say one thing in the row and another in its own legend.
/// Adding a state to either enum shows up here by itself; only the
/// sentence explaining it is written by hand.
pub(super) fn render_status_catalog(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    anchor: Rect,
    tick: usize,
) {
    // The agent column answers "what is the process doing", the work
    // column "where does the work in its checkout stand" — two questions
    // about the same row, which is exactly why they are two columns and
    // why one legend has to carry both.
    //
    // Both answer for every agent. The work column used to be readable
    // only for an isolated one, so an operator who launched an agent in
    // the project's own root met eight marks that never appeared and
    // reasonably concluded it was broken.
    let agent_rows: Vec<(String, Color, &str, &str)> = [
        (
            AgentTabStatus::Working,
            "working",
            "producing output right now",
        ),
        (
            AgentTabStatus::Completed,
            "completed",
            "finished while you were elsewhere",
        ),
        (
            AgentTabStatus::Selected,
            "here",
            "the tab you are typing into",
        ),
        (AgentTabStatus::Idle, "idle", "quiet, and not where you are"),
    ]
    .into_iter()
    .map(|(status, name, meaning)| {
        (
            status.glyph(tick).trim_end().to_owned(),
            status.color(),
            name,
            meaning,
        )
    })
    .collect();

    // `Running` is absent on purpose: it is the state that draws no mark,
    // because a task with a clean tree and nothing ahead has nothing to
    // report yet — and "the agent is alive" is the other column's answer,
    // which it gives with a spinner. A legend of marks that names a state
    // with no mark leaves a blank glyph and two rows meaning the same
    // thing. The `filter_map` below keeps that true for whatever is added
    // here next.
    let task_rows: Vec<(String, Color, &str, &str)> = [
        (
            WorkStateView::Uncommitted,
            "uncommitted",
            "changes in the checkout, not committed",
        ),
        (
            WorkStateView::Ready,
            "ready",
            "commits ahead on a clean tree — deliverable",
        ),
        (
            WorkStateView::Published,
            "published",
            "on the remote, level with it — with its reviewer",
        ),
        (
            WorkStateView::Integrating,
            "delivering",
            "the rebase, the gate and the push, in flight",
        ),
        (
            WorkStateView::Conflicted { files: Vec::new() },
            "conflict",
            "the rebase stopped; resolve it in the checkout",
        ),
        (
            WorkStateView::GateFailed,
            "checks failed",
            "the gate failed on the rebased commits",
        ),
        (
            WorkStateView::Integrated,
            "delivered",
            "the work is in the target",
        ),
        (
            WorkStateView::Parked,
            "parked",
            "no agent left; the work is still there",
        ),
    ]
    .into_iter()
    .filter_map(|(state, name, meaning)| {
        let (mark, hue) = task_mark(&state)?;
        Some((mark.to_owned(), hue, name, meaning))
    })
    .collect();

    /// Tighter than a popup's own inset: the catalog is a table, and its
    /// columns carry the separation an inset would otherwise provide.
    const CATALOG_H_PAD: u16 = 1;
    const GLYPH_COLUMN: usize = 3;
    let name_column = agent_rows
        .iter()
        .chain(&task_rows)
        .map(|(_, _, name, _)| name.chars().count())
        .max()
        .unwrap_or(0);
    let content_width = agent_rows
        .iter()
        .chain(&task_rows)
        .map(|(_, _, _, meaning)| GLYPH_COLUMN + name_column + 2 + meaning.chars().count())
        .max()
        .unwrap_or(0) as u16;

    let mut lines: Vec<Line<'static>> = Vec::new();
    let section =
        |title: &str, rows: &[(String, Color, &str, &str)], lines: &mut Vec<Line<'static>>| {
            lines.push(Line::from(Span::styled(
                title.to_owned(),
                theme::fg(Token::TextMuted),
            )));
            for (glyph, hue, name, meaning) in rows {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("{glyph:<GLYPH_COLUMN$}"),
                        Style::default().fg(*hue).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{name:<name_column$}  "),
                        theme::fg(Token::TextPrimary),
                    ),
                    Span::styled((*meaning).to_owned(), theme::fg(Token::TextSecondary)),
                ]));
            }
        };
    section("AGENT", &agent_rows, &mut lines);
    lines.push(Line::from(""));
    section("WORK", &task_rows, &mut lines);

    let width = (content_width + 2 * CATALOG_H_PAD + 2).min(area.width);
    let height = (lines.len() as u16 + 2).min(area.height);
    // Anchored to the glyph that was clicked, like every other dropdown
    // here — and pulled back inside the frame when that glyph sits too
    // close to an edge for the popup to fit beside it.
    let popup = Rect::new(
        anchor.x.min((area.x + area.width).saturating_sub(width)),
        (anchor.y + anchor.height).min((area.y + area.height).saturating_sub(height)),
        width,
        height,
    );
    frame.render_widget(Clear, popup);
    // The catalog's own horizontal inset, and no row above: its first
    // line is a heading the title already names.
    let inner = Surface::floating()
        .title(" status ")
        .padding(Padding::new(CATALOG_H_PAD, CATALOG_H_PAD, 0, 0))
        .render(frame, popup);
    frame.render_widget(Paragraph::new(lines), inner);
}

/// Pins what a pull and a push would move to the space header's right
/// edge — the column every row in the sidebar keeps free, the one the
/// agent rows pin their task mark to — followed, on the space in front,
/// by the control that places a new agent in it.
///
/// The control is here rather than in the tab strip because an agent is
/// placed in a space, and this is the row that names it. Only the
/// selected space carries it: the new agent lands in the space in front,
/// and one on every header would be a column of the same word to read
/// past, each promising a space it would not land in.
///
/// The counts are on the header because that is whose fact it is. They
/// are read from the checkout, so they are the same two numbers for every
/// agent standing in one, and the agent rows printed them once each: four
/// agents in a space's own root is an ordinary day, and the column said
/// `⇣₁₁ ⇡₁₉` four times under four different names. Nothing when there is
/// nothing to move, and nothing for a space whose own directory is a slot
/// — that checkout belongs to a task, and what its branch owes upstream
/// is the task's business, not the space's.
fn push_trailing_controls(
    spans: &mut Vec<Span<'_>>,
    hits: &mut Vec<(Rect, WorkspaceHit)>,
    rect: Rect,
    model: &WorkspaceModel,
    space: &Space,
    selected: bool,
    identities: &[AgentIdentity],
) {
    let mut drawn: Vec<Span<'_>> = unisolated_sync_caption(model, &space_cwd(space, identities))
        .into_iter()
        .enumerate()
        .map(|(index, (text, hue))| {
            let gap = if index == 0 { "" } else { " " };
            Span::styled(format!("{gap}{text}"), Style::default().fg(hue))
        })
        .collect();
    let new = selected.then(|| surface_label(Symbol::MarkSparkle, "new"));
    if let Some(label) = &new {
        if !drawn.is_empty() {
            drawn.push(Span::raw(" "));
        }
        // The hue the caption of the agent receiving keystrokes wears
        // (`caption_color`), because this is where the next one lands —
        // held back until the pointer asks for it, so the row's one
        // coloured word does not outshout the name beside it.
        let hue = match chip_state(model, Some(WorkspaceHit::NewAgentMenu)) {
            ChipState::Hovered | ChipState::Pressed => Token::StateWarning,
            ChipState::Resting | ChipState::Static => Token::StateWarningMuted,
        };
        drawn.push(Span::styled(label.clone(), theme::fg_bold(hue)));
    }
    if drawn.is_empty() {
        return;
    }
    let used: u16 = spans
        .iter()
        .chain(&drawn)
        .map(|span| span.width() as u16)
        .sum::<u16>()
        + TRAILING_PAD;
    let Some(gap) = rect.width.checked_sub(used) else {
        return;
    };
    if let Some(label) = &new {
        let width = Span::raw(label.as_str()).width() as u16;
        // Ahead of the row's own `SelectSpace`, so it wins the click.
        hits.push((
            Rect::new(rect.right() - TRAILING_PAD - width, rect.y, width, 1),
            WorkspaceHit::NewAgentMenu,
        ));
    }
    spans.push(Span::raw(" ".repeat(gap as usize)));
    spans.extend(drawn);
    spans.push(Span::raw(" ".repeat(TRAILING_PAD as usize)));
}

/// What a pull and a push would move for a checkout outside any slot,
/// when its branch is the delivery target and something is due either
/// way: `⇣1` in the danger hue for what is to pull, `⇡2` in the success
/// hue for what is to push, each only while its count is non-zero — the
/// shape a shell prompt gives the same fact. No word: the two colours say
/// which is which. Empty inside a slot, on any other branch, without an
/// upstream, or in sync — a caption that says "nothing to do" says it
/// best by saying nothing.
///
/// Ordinary digits, not the small forms. Those exist so a count can ride
/// *inside* a line of text without breaking it, and this one does not
/// ride inside anything — it stands alone at the edge of a row. At that
/// size two small digits beside an arrow read as a smudge on the arrow
/// rather than as a number, which is the one thing a count has to be.
fn unisolated_sync_caption(model: &WorkspaceModel, cwd: &Path) -> Vec<(String, Color)> {
    if !is_unisolated(cwd) {
        return Vec::new();
    }
    sync_caption(model, cwd)
}

/// The pull and push counts for the directory evaluated at `key`, in the
/// shape `unisolated_sync_caption` gives them.
fn sync_caption(model: &WorkspaceModel, key: &Path) -> Vec<(String, Color)> {
    let Some(sync) = model.remembered.upstream_syncs.get(&evaluation_key(key)) else {
        return Vec::new();
    };
    [
        (
            Symbol::SyncBehind,
            sync.pull,
            theme::color(Token::StateDanger),
        ),
        (
            Symbol::SyncAhead,
            sync.push,
            theme::color(Token::StateSuccess),
        ),
    ]
    .into_iter()
    .filter(|(_, count, _)| *count > 0)
    .map(|(arrow, count, hue)| (format!("{}{count}", theme::glyph(arrow)), hue))
    .collect()
}

/// `text` shortened from the left to `width`, keeping its tail — the end
/// of a path is what says where you are; its beginning is what you can
/// afford to lose.
fn elide_head(text: &str, width: usize) -> String {
    let length = text.chars().count();
    if length <= width {
        return text.to_owned();
    }
    let kept = width.saturating_sub(1);
    std::iter::once('…')
        .chain(text.chars().skip(length - kept))
        .collect()
}

/// The "+ space" prompt and the directories it currently matches, drawn as
/// rows of the sidebar itself rather than a floating popup: the prompt is
/// choosing where the next space in this very list goes. It stands where
/// the first space's header stands, with the listing directly under it the
/// way a space's tabs sit under theirs.
///
/// The kinds and the directories they would be created in share the darker
/// of the column's two surfaces — they are the panel — and what is being
/// typed stands on the lighter one, as does whichever directory it has
/// landed on: the two rows that answer to the keyboard are the two that are
/// lifted.
fn render_root_picker(
    frame: &mut ratatui::Frame<'_>,
    picker: &RootPicker,
    rows: &mut Rows,
    hits: &mut Vec<(Rect, WorkspaceHit)>,
) {
    render_query_row(frame, picker, rows);
    if picker.match_count() == 0 {
        if let Some(rect) = rows.next(1) {
            let mut spans = vec![
                picker_lead(),
                Span::styled("no directory matches", theme::fg(Token::TextFaint)),
            ];
            row::pad_to(
                &mut spans,
                rect.width,
                theme::color(Token::SurfaceRaisedSubtle),
            );
            frame.render_widget(Paragraph::new(Line::from(spans)), rect);
        }
        return;
    }
    // The picker has the column to itself, so it offers as many
    // directories as the column has rows — one held back for the tail that
    // says how many more there are.
    let visible = usize::from(rows.remaining()).saturating_sub(1).max(1);
    let start = picker.window_start(visible);
    let needle = picker.input().to_lowercase();
    for (index, candidate) in picker.matches().enumerate().skip(start).take(visible) {
        let Some(rect) = rows.next(1) else { return };
        let selected = picker.selection() == Some(index);
        // A name the query is the head of says so, in the hue of the query
        // itself; a match found further in says nothing, rather than
        // colouring letters that had nothing to do with it.
        let (matched, rest) = split_at_head(&candidate.name, &needle);
        let rest_hue = if selected {
            Token::TextBright
        } else {
            Token::TextInactive
        };
        let mut spans = vec![
            picker_lead(),
            Span::styled(matched, theme::fg(Token::Accent)),
            Span::styled(rest, theme::fg(rest_hue)),
        ];
        row::pad_to(
            &mut spans,
            rect.width,
            theme::color(if selected {
                Token::SurfaceRaised
            } else {
                Token::SurfaceRaisedSubtle
            }),
        );
        frame.render_widget(Paragraph::new(Line::from(spans)), rect);
        hits.push((rect, WorkspaceHit::PickSpaceRoot(index)));
    }
    let hidden = picker.match_count().saturating_sub(start + visible);
    if hidden > 0
        && let Some(rect) = rows.next(1)
    {
        let mut spans = vec![
            picker_lead(),
            Span::styled(format!("+{hidden} more"), theme::fg(Token::TextFaint)),
        ];
        row::pad_to(
            &mut spans,
            rect.width,
            theme::color(Token::SurfaceRaisedSubtle),
        );
        frame.render_widget(Paragraph::new(Line::from(spans)), rect);
    }
}

/// `text` split after the head `needle` names, when it names one at all.
fn split_at_head(text: &str, needle: &str) -> (String, String) {
    if needle.is_empty() || !text.to_lowercase().starts_with(needle) {
        return (String::new(), text.to_owned());
    }
    let cut = text
        .char_indices()
        .nth(needle.chars().count())
        .map_or(text.len(), |(index, _)| index);
    (text[..cut].to_owned(), text[cut..].to_owned())
}

/// The column every row of the picker starts in — the kinds, what is being
/// typed, and each directory offered — with the row's own leading cell left
/// to the mark that says where the keyboard is.
fn picker_lead() -> Span<'static> {
    Span::raw(" ".repeat(PICKER_LEAD))
}

/// The width of that leading cell.
const PICKER_LEAD: usize = 1;

/// The gap on either side of the rule between the header's two controls.
const HEADER_GAP: u16 = 1;

/// The row being typed into: what is being looked for, with the directory
/// it is being looked for in at the row's other end — a prompt that opened
/// with that path already typed into it asked to be deleted before it could
/// be used. The accent down its leading column says this is the row the
/// keyboard is in.
fn render_query_row(frame: &mut ratatui::Frame<'_>, picker: &RootPicker, rows: &mut Rows) {
    let Some(rect) = rows.next(1) else { return };
    let needle = picker.input();
    let mut spans = vec![
        picker_lead(),
        Span::styled(
            format!("{needle}{}", theme::glyph(Symbol::CursorText)),
            Style::default()
                .fg(theme::color(Token::TextBright))
                .add_modifier(Modifier::BOLD),
        ),
    ];
    // Where the typing starts from, and only until there is typing: the
    // line says everything else itself (see `RootPicker`). A path pinned
    // to the right of the line all the way through said where the prompt
    // was in a second place, which is the half that went stale the
    // moment the two could disagree.
    if needle.is_empty() {
        let used: u16 = spans.iter().map(|span| span.width() as u16).sum();
        let room = rect.width.saturating_sub(used + TRAILING_PAD + 1);
        spans.push(Span::styled(
            elide_head(
                &crate::ui::display_project_path(picker.base()),
                room as usize,
            ),
            theme::fg(Token::TextDim),
        ));
    }
    row::pad_to(&mut spans, rect.width, theme::color(Token::SurfaceRaised));
    frame.render_widget(Paragraph::new(Line::from(spans)), rect);
    // The rail the selected space wears, so the row being typed into
    // reads as the space it is about to become.
    frame.render_widget(
        Paragraph::new(theme::glyph(Symbol::BarThin)).style(
            Style::default()
                .fg(theme::color(Token::Accent))
                .bg(theme::color(Token::SurfaceRaised)),
        ),
        Rect::new(rect.x, rect.y, 1, 1),
    );
}

fn chip_state(model: &WorkspaceModel, hit: Option<WorkspaceHit>) -> ChipState {
    let Some(hit) = hit else {
        return ChipState::Static;
    };
    if model.pressed_hit() == Some(hit) {
        ChipState::Pressed
    } else if model.hovered == Some(hit) {
        ChipState::Hovered
    } else {
        ChipState::Resting
    }
}

/// The small notification for a task: what the delivery is about, and
/// where it stands.
///
/// Two parts and no forge's word for either. The subject is the request's
/// own number, or the ending for a completion that is not a request at
/// all (see [`delivery_subject`]); the standing is [`task_mark`]'s own
/// mark, so the strip, the sidebar row and the status legend cannot come
/// to say different things about one state.
///
/// One verb over three completions read the same whether it was about to
/// fast-forward the target under you, open a request against it, or touch
/// nothing outside the branch — which is why the subject is the half that
/// is always there, and the count rides the mark rather than standing
/// alone in front of it.
fn delivery_notification(
    task: &AgentView,
    state: &WorkStateView,
    tick: usize,
) -> Option<(String, Color, bool)> {
    // Which states put a notification in this zone: the ones handing work
    // over passes through. `Uncommitted` and `Parked` carry marks of
    // their own in the sidebar, but they are facts about a checkout
    // rather than steps of a delivery, and this zone is the second.
    // Conditioned, not disabled: a state that cannot be delivered is
    // still reported, and the report simply is not a button.
    let pressable = match state {
        WorkStateView::Ready | WorkStateView::Published | WorkStateView::GateFailed => true,
        WorkStateView::Conflicted { .. }
        | WorkStateView::Integrating
        | WorkStateView::Integrated => false,
        _ => return None,
    };
    let (mark, hue) = task_mark(state)?;
    let standing = match state {
        // The one state `task_mark` cannot lend its glyph to. Its mark is
        // the vocabulary's "there is work to hand over", and the patched
        // set draws that as a create-a-request icon — true for the
        // completion that opens one, a lie in front of a button about to
        // fast-forward the target or to touch nothing outside the branch.
        // What a press sends is commits, whatever the ending, so the
        // count wears the arrow that means exactly that and nothing about
        // a forge.
        //
        // The count is what a press would send, which is not how far the
        // branch is from the target: that distance is the merge's
        // question and stays open until the request lands.
        WorkStateView::Ready => format!(
            "{}{}",
            theme::glyph(Symbol::SyncAhead),
            task.unsynced.unwrap_or(task.ahead)
        ),
        // The one report in this row that is also work in progress, so it
        // is the one that moves: a rebase, a gate and a push take as long
        // as the project's checks do, and a still mark for that many
        // seconds reads as a screen that has stopped. The sidebar's mark
        // keeps `Symbol::Ellipsis` — a single cell has no room to turn.
        WorkStateView::Integrating => agent_activity_frame(tick),
        _ => mark,
    };
    Some((
        format!("{} {standing}", delivery_subject(task)),
        hue,
        pressable,
    ))
}

/// What a delivery is *about*, in the fewest characters that name it: the
/// request's own number once the forge has one, the forge's word for a
/// request before that, and — for the two completions that are not a
/// request at all — where the work is going instead.
///
/// A number needs no word in front of it: `#41` is already unambiguous,
/// and "PR" there would be length spent on nothing. The word earns its
/// place only in the one case that used to leave a bare `#` standing
/// alone — a request that does not exist yet — and only where `origin`
/// said which forge this is. Picking one of the two names blind would
/// take a side the reader may not be on, so an unrecognized remote keeps
/// the `#` both forges write. The branch's published name is not here
/// either: it is the one part of this that has no bound on its length,
/// and a zone that changes width with a branch name moves every button
/// left of it.
fn delivery_subject(task: &AgentView) -> String {
    match task.completion {
        CompletionBehavior::Pr => match task.published_request {
            Some(request) => format!("#{request}"),
            None => task.forge.request_abbreviation().unwrap_or("#").to_owned(),
        },
        CompletionBehavior::Merge => {
            format!("{} {}", theme::glyph(Symbol::ArrowTo), task.target)
        }
        CompletionBehavior::Handoff => "hand off".to_owned(),
    }
}

/// The preserved-work list: every task holding work that no live tab is in
/// front of, with the keys that move it on. Discard asks twice.
/// The reading width this client's centred dialogs keep. A dialog as wide
/// as the terminal is one nobody reads across — the eye loses the line on
/// the way back — and both of these are short lists of short rows. One
/// pair of numbers so the two are the same shape rather than each what its
/// own content happened to come to.
const MIN_POPUP_WIDTH: u16 = 30;
const MAX_POPUP_WIDTH: u16 = 72;

/// Everything that can be done here, each with the key that reaches it.
///
/// The workspace had no such surface at all: two of its most useful
/// gestures were reachable only by someone who had read the source. Every
/// word here comes from the action and every key from the keymap, so it is
/// right by construction and stays right after a rebind.
pub(super) fn render_action_index(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    index: &ActionIndexOverlay,
    hits: &mut Vec<(Rect, WorkspaceHit)>,
) {
    let rows = action_index_rows(&index.scopes, &index.filter);
    let reachable = action_index_rows(&index.scopes, "").len();
    let entries = action_index::render(
        frame,
        area,
        &rows,
        reachable,
        &index.filter,
        index.selected,
        WorkspaceHit::ActionIndexEntry,
    );
    // Prepended: what is underneath must not answer a click meant here.
    hits.splice(0..0, entries);
}

pub(super) fn render_preserved(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &WorkspaceModel,
    overlay: &PreservedOverlay,
) {
    let preserved = model.preserved_tasks();
    let mut selected_line = None;
    let mut lines = vec![Line::from(Span::styled(
        "PRESERVED WORK",
        theme::fg(Token::TextMuted),
    ))];
    if preserved.is_empty() {
        lines.push(Line::from(Span::styled(
            "nothing preserved — every task is either live or delivered",
            theme::fg(Token::TextSecondary),
        )));
    }
    for (index, work) in preserved.iter().enumerate() {
        let selected = index == overlay.selected;
        let (mark, hue) = task_mark(&work.state)
            .unwrap_or_else(|| (theme::glyph(Symbol::MarkDot), theme::color(Token::TextDim)));
        // What the *record* says, which is all this list asks. How far a
        // branch is ahead and what the forge holds are questions about the
        // project you are in, and asking them here would put one Git read
        // per project on the machine behind a keystroke.
        let what = match &work.state {
            WorkStateView::Parked if work.checkout.is_none() => "checkout removed".to_owned(),
            WorkStateView::Parked => "nobody is there".to_owned(),
            WorkStateView::Uncommitted => "uncommitted changes".to_owned(),
            WorkStateView::Conflicted { .. } => "conflict to resolve".to_owned(),
            WorkStateView::GateFailed => "checks failed".to_owned(),
            WorkStateView::Running => "was running".to_owned(),
            WorkStateView::Integrating => "delivering".to_owned(),
            _ => work.branch.clone(),
        };
        // The project, because this list crosses them: two agents carrying
        // a branch of the same name in two repositories are one row twice
        // without it.
        let project = work
            .project
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| work.project.display().to_string());
        let spans = vec![
            Span::styled(
                if selected {
                    format!("{} ", theme::glyph(Symbol::ChevronCollapsed))
                } else {
                    "  ".to_owned()
                },
                theme::fg(Token::Accent),
            ),
            Span::styled(format!("{mark} "), Style::default().fg(hue)),
            Span::styled(
                work.label.clone(),
                Style::default().fg(if selected {
                    theme::color(Token::TextBright)
                } else {
                    theme::color(Token::TextPrimary)
                }),
            ),
            Span::styled(format!("  {project}"), theme::fg(Token::TextMuted)),
            Span::styled(format!("  {what}"), theme::fg(Token::TextSecondary)),
        ];
        if selected {
            // Filled after the popup is measured, not here: a selection
            // that reaches the frame's edge before anything has decided
            // how wide the dialog is *becomes* how wide the dialog is.
            selected_line = Some(lines.len());
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::from(""));
    // Read off the keymap like every other hint. These five keys were
    // written into the string by hand — the last place in the client that
    // still claimed a key nothing had resolved, so a rebinding left it
    // quietly wrong.
    const SCOPES: &[uze_keys::Scope] = &[uze_keys::Scope::Global, uze_keys::Scope::PreservedWork];
    lines.push(if overlay.confirm_discard {
        let mut line = Line::from(Span::styled(
            "discard this task and its branch?  ",
            theme::fg(Token::StateWarning),
        ));
        line.spans
            .extend(hint::line(SCOPES, &[Action::ConfirmDiscard, Action::Dismiss]).spans);
        line
    } else {
        hint::line(
            SCOPES,
            &[
                Action::ResumeTask,
                Action::DeliverTask,
                Action::FinishTask,
                Action::DiscardTask,
                Action::Dismiss,
            ],
        )
    });
    // Measured from the words, then held to the same reading width the
    // index beside it keeps: a dialog as wide as the terminal is a dialog
    // nobody can read across, and this one is a short list of short rows.
    let content = lines.iter().map(Line::width).max().unwrap_or(0) as u16;
    let width = (content + 2 + 2 * POPUP_H_PAD)
        .clamp(MIN_POPUP_WIDTH, MAX_POPUP_WIDTH)
        .min(area.width)
        .max(1);
    let text_width = width.saturating_sub(2 + 2 * POPUP_H_PAD);
    for line in &mut lines {
        text::clip(line, text_width as usize);
    }
    if let Some(index) = selected_line {
        row::pad_to(
            &mut lines[index].spans,
            text_width,
            theme::color(Token::SurfaceSelected),
        );
    }
    let height = (lines.len() as u16 + 2).min(area.height).max(1);
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 3,
        width,
        height,
    );
    frame.render_widget(Clear, popup);
    // No row above: these lines are a list, and the first of them is the
    // one the popup exists to show.
    let inner = Surface::floating()
        .padding(Padding::new(POPUP_H_PAD, POPUP_H_PAD, 0, 0))
        .render(frame, popup);
    frame.render_widget(Paragraph::new(lines), inner);
}

pub(super) fn agent_activity_frame(tick: usize) -> String {
    theme::frame(Symbol::StatusWorking, tick % AGENT_ACTIVITY_FRAMES)
}

/// The horizontal tab strip above the pane: the *selected space's* shell
/// tabs only — agent tabs live exclusively in the sidebar now (see
/// [`render_sidebar`]), so a tab [`agent_identity_for_tab`] recognizes
/// never appears here, the same way a shell tab never appears in the
/// sidebar; other spaces' shell tabs don't appear here either, only the
/// currently selected space's. An active-tab marker in `theme::color(Token::Accent)`/bold-bright
/// text, wrapped in the same neutral [`theme::color(Token::SurfaceRaised)`] chip the
/// sidebar already uses for "this is where you are" (its active space's
/// envelope, its agent tab rows) — this strip used to skip that fill and
/// lean on text weight alone, which read as a lighter kind of "selected"
/// than everywhere else in the TUI. A dim `×` close affordance per tab once
/// more than one exists in the selected space, and a trailing "+" opening
/// another shell in it.
pub(super) fn render_tab_strip(
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
    let inner = Rule::new(Edge::Bottom)
        .padding(Padding::new(0, 1, 0, 0))
        .render(frame, area);

    let Some(session) = &model.session else {
        frame.render_widget(
            Paragraph::new(Span::styled("connecting…", theme::fg(Token::TextMuted))),
            inner,
        );
        return;
    };

    // Scoped to the selected space — switching spaces (sidebar) switches
    // which shells this strip shows, the actual "don't mix projects"
    // payoff of spaces existing at all.
    let space = session.selected_space();
    // …and, within it, to one context: the agent in front of the person
    // followed by the shells opened alongside it, never another agent's.
    // A `None` context is the space's own — its bootstrap shell and
    // anything opened with no agent selected.
    let context = context_agent(model, identities);
    let strip = strip_tabs(space, context, identities);
    // Closability is a per-space rule (the server refuses a removal that
    // would empty a space — see `Session::remove_tab`), so it's judged
    // against every tab in the selected space, not just the ones this
    // strip goes on to show. A close that would take the last of them
    // still goes through: `close_tab_keeping_a_shell` opens the space's
    // replacement first.
    let can_close = space.tabs.len() > 1;

    // The header's right end goes down first — before a tab is measured,
    // let alone drawn — and that order is the whole arrangement. The
    // controls take the columns they need from the right edge inward, and
    // what they leave is the room the tab side then has: neither what the
    // workspace *says* nor how many shells are open can move something
    // the operator is about to *press*, and no tab can be laid over one.
    // A message takes whatever is left after both, ending in a divider
    // that keeps it apart from the controls.
    //
    // Three zones, in one order, reading left to right: what the work is
    // *doing* (the delivery notification), what it has *changed*, and
    // what can be *done* to it. Muted hairlines between them, because
    // they answer three different questions and a reader looking for one
    // of them should not have to sort the row out first. Each hairline
    // belongs to the zone on its left and goes when that zone does, so
    // the strip never draws a divider with nothing on one side of it.
    let mut trailing_right = inner.right();
    // Outside every zone, on the strip's own edge: it is not about this
    // checkout the way the other three are, and the one thing at the end
    // of a row is the one thing nothing else can push around. No fill —
    // it wears the plain backdrop, white at rest and the accent under the
    // pointer, so a single glyph out here never reads as a fourth zone of
    // one button.
    //
    // Its rect is what the dropdown hangs off, so it is measured before
    // it is drawn and the hit carries the same rectangle the glyph is
    // centred in — pad included, since the padding is as much of the
    // target as the glyph is.
    if selected_agent_context(model, identities).is_some() {
        // Air on the leading side only. The strip already insets its own
        // right edge by a column (see `inner`), and a trailing pad on top
        // of that left the one glyph at the end of the row floating two
        // columns off the margin every other row is measured against.
        let sparkle = theme::glyph(Symbol::MarkSparkle);
        let width = Span::raw(&sparkle).width() as u16 + chip::PAD;
        let rect = Rect::new(trailing_right.saturating_sub(width), inner.y, width, 1);
        let hit = WorkspaceHit::OpenAgentSupport(rect);
        let hue = match chip_state(model, Some(hit)) {
            ChipState::Resting => theme::color(Token::TextBright),
            _ => theme::color(Token::Accent),
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw(" "),
                Span::styled(sparkle, Style::default().fg(hue)),
            ])),
            rect,
        );
        hits.push((rect, hit));
        trailing_right = ZoneEdge::Bare.left_of(rect.x);
    }

    // ── actions ────────────────────────────────────────────────────────
    // The two extensions are one group of buttons, not two chips with air
    // between them: each puts its surface where the pane is — the
    // same kind of errand — and one continuous ground says that, where
    // two detached chips read as two unrelated controls.
    //
    // Both are always there — a checkout always has a shape and files —
    // which is what lets them be the fixed pair the eye learns, with
    // every zone that comes and goes sitting to the left of them.
    //
    // No bold: two words side by side at the same weight read as one
    // strip of controls, and bold made each of them claim the row on its
    // own. The pair of glyphs at the tab side's end is the other way
    // round, and says why in its own place.
    {
        // The one standing in the pane wears the strip's one highlight —
        // the pane shows one thing, so the strip lights one thing, and
        // while a surface is up that is its button, not the tab it
        // covers. Said by the ground, not by the word's hue or weight.
        let buttons = [
            (
                WorkspaceHit::OpenArchitect,
                Symbol::Architect,
                "architect",
                model.architect.is_some(),
            ),
            (
                WorkspaceHit::OpenFiles,
                Symbol::Code,
                "code",
                model.code.is_some(),
            ),
        ]
        .map(|(hit, symbol, name, open)| GroupButton {
            hit,
            label: surface_label(symbol, name),
            hue: theme::color(Token::TextSecondary),
            strong: false,
            lit: open,
            switch: true,
        });
        let width = group_width(&buttons);
        let rect = Rect::new(trailing_right.saturating_sub(width), inner.y, width, 1);
        let (spans, group_hits) =
            button_group(model, &buttons, Token::SurfaceRaised, (rect.x, rect.y));
        hits.extend(group_hits);
        frame.render_widget(Paragraph::new(Line::from(spans)), rect);
        trailing_right = ZoneEdge::Filled.left_of(rect.x);
    }

    // ── git changes ────────────────────────────────────────────────────
    // What changed, in the two numbers and nothing else — its own zone
    // and never a ground of its own. There are two extensions, so there
    // are two dedicated buttons; a third filled thing beside them read as
    // a third surface to open rather than as a count of the work sitting
    // next to the ways in, and a plate sliding in under the pointer put
    // the button back the moment anyone went near it.
    //
    // It is still a door, and the hue is what says so: the counts sit at
    // their muted strength and come up to full under the pointer. The
    // colour is the badge's whole message — green is additions, red is
    // deletions — so it cannot be given up at rest the way a grey label
    // could; holding it back and letting the pointer restore it says
    // "this answers you" without anything being drawn.
    //
    // Absent for a clean checkout, hairline and all. It is a badge as
    // much as a door, and a badge with nothing to say says nothing rather
    // than zero — which costs no reachability, because the diff is one
    // mode switch away inside the surface `code` opens.
    if let Some(summary) = model
        .remembered
        .git_badge
        .as_ref()
        .and_then(|badge| badge.summary)
    {
        trailing_right = render_zone_hairline(frame, inner, trailing_right, ZoneEdge::Bare);
        let hit = WorkspaceHit::OpenChanges;
        let (additions, deletions) = match chip_state(model, Some(hit)) {
            ChipState::Resting | ChipState::Static => (
                theme::color(Token::StateSuccessMuted),
                theme::color(Token::StateDangerMuted),
            ),
            ChipState::Hovered | ChipState::Pressed => (
                theme::color(Token::StateSuccess),
                theme::color(Token::StateDanger),
            ),
        };
        let text = format!("+{} -{}", summary.additions, summary.deletions);
        let width = Span::raw(&text).width() as u16 + 2 * chip::PAD;
        let rect = Rect::new(trailing_right.saturating_sub(width), inner.y, width, 1);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    format!("+{}", summary.additions),
                    Style::default().fg(additions),
                ),
                Span::raw(" "),
                Span::styled(
                    format!("-{}", summary.deletions),
                    Style::default().fg(deletions),
                ),
                Span::raw(" "),
            ])),
            rect,
        );
        hits.push((rect, hit));
        trailing_right = ZoneEdge::Bare.left_of(rect.x);
    }

    // ── small notifications ────────────────────────────────────────────
    // Where the delivery stands, as a chip: this one keeps a control's
    // filled shape because in most of its states it *is* one, and a
    // report wearing the same shape sits on the recessed ground that says
    // it is not (see [`ChipState::Static`]).
    //
    // Only what UZE cut. An agent in the project's own root is on the
    // operator's branch, and rebasing it onto the target, running the
    // gate over it and pushing it is theirs to ask for. Its state is
    // still drawn in the sidebar — where the work stands is a fact either
    // way — but this zone is about a delivery UZE performs.
    if let Some(tab) = model.selected_tab()
        && let Some(task) = model.tab_task(tab)
        && task.isolated
        && let Some((text, hue, pressable)) =
            delivery_notification(task, &model.drawn_state(task), model.tick)
    {
        trailing_right = render_zone_hairline(frame, inner, trailing_right, ZoneEdge::Filled);
        let hit = pressable.then_some(WorkspaceHit::Deliver(tab));
        let chip = Chip::new(&text, hue, chip_state(model, hit));
        let rect = chip.rect_ending_at(trailing_right, inner.y);
        chip.render(frame, rect);
        if let Some(hit) = hit {
            hits.push((rect, hit));
        }
        trailing_right = ZoneEdge::Filled.left_of(rect.x);
    }

    // Where the tab side must stop. The controls at the right end are
    // laid out before a single tab is drawn, so the strip's own buttons
    // can never end up under one of them: a strip too narrow for both
    // loses a tab's tail, which the strip can scroll back to, rather than
    // the button that makes the next tab, which nothing else offers.
    let limit = trailing_right;
    let extension_in_front = model.code.is_some() || model.architect.is_some();
    let mut spans = Vec::new();
    let mut x = inner.x;
    let strip_len = strip.len();
    // Where to draw the drag's insertion indicator, if anywhere — captured
    // during the loop below but drawn only after `spans`' one accumulated
    // `Line` covering the whole strip is painted, since that single later
    // render would otherwise cover over a bar drawn mid-loop (unlike the
    // sidebar's per-row renders, every chip here shares that one `Line`).
    let mut drop_indicator: Option<Rect> = None;
    for (strip_index, tab) in strip.into_iter().enumerate() {
        if x >= limit {
            break;
        }
        let is_last = strip_index + 1 == strip_len;
        let is_agent = Some(tab.id) == context;
        // One highlight on the strip: a surface standing in the pane
        // takes it from the tab it covers.
        let selected = tab.id == space.selected_tab && !extension_in_front;
        let marker_fg = if selected {
            theme::color(Token::Accent)
        } else {
            theme::color(Token::TextFaint)
        };
        let label_style = if selected {
            Style::default()
                .fg(theme::color(Token::TextBright))
                .add_modifier(Modifier::BOLD)
        } else {
            theme::fg(Token::TextInactive)
        };
        let marker = Span::styled(
            // The agent leading the strip wears the same mark the button
            // that creates one does, so the first chip reads as the agent
            // this context is about rather than another shell.
            format!(
                "{} ",
                theme::glyph(match (is_agent, selected) {
                    (true, _) => Symbol::MarkSparkle,
                    (false, true) => Symbol::StatusSelected,
                    (false, false) => Symbol::StatusIdle,
                })
            ),
            Style::default().fg(if is_agent && !selected {
                theme::color(Token::TextInactive)
            } else {
                marker_fg
            }),
        );
        let renaming_this = model
            .renaming
            .as_ref()
            .filter(|(target, _)| *target == RenameTarget::Tab(tab.id))
            .map(|(_, buffer)| buffer);
        let tab_label = match renaming_this {
            Some(buffer) => rename_spans(buffer),
            // One name per agent across the whole frame: the tab's own
            // label, which is what the sidebar draws and what renaming
            // edits. A working agent's task carries a label of its own
            // (the prompt's slug, or the bare task identifier when it has
            // no prompt) — showing that here left the same agent reading
            // as "engineer" in the sidebar and "gic3jz" up top.
            None => vec![Span::styled(tab.label.clone(), label_style)],
        };
        // An agent is never closed by a stray click — that stays a
        // right-click and a confirmation in the sidebar (see `ContextMenu`),
        // the same rule that keeps the sidebar's own agent rows unclosable.
        let show_close = renaming_this.is_none() && can_close && !is_agent;
        let content_width = marker.width() as u16
            + tab_label.iter().map(Span::width).sum::<usize>() as u16
            + if show_close { 2 } else { 0 }; // " ×"
        // 1 column of padding on each side, reserved whether or not this
        // tab is selected — only the theme::color(Token::SurfaceRaised) fill toggles with
        // `selected`, never the width. Sizing the chip itself to
        // `selected` used to mean every tab shifted horizontally the
        // moment selection moved past it, reading as the whole strip
        // "resizing" on every tab switch instead of just recoloring.
        let chip_start = x;
        let chip_width = content_width + 2 * chip::PAD;

        let mut chip = vec![Span::raw(" ")];
        chip.push(marker);
        chip.extend(tab_label);
        if show_close {
            chip.push(Span::raw(" "));
            chip.push(Span::styled("×", theme::fg(Token::TextDim)));
            hits.push((
                Rect::new(chip_start + chip::PAD + content_width - 1, inner.y, 1, 1),
                WorkspaceHit::CloseTab(tab.id),
            ));
        }
        chip.push(Span::raw(" "));
        // A tab is a control too, and the pointer says so — one shade
        // under the selected chip's own fill, so hovering an unselected
        // tab never reads as having already switched to it.
        if selected {
            row::pad_to(&mut chip, chip_width, theme::color(Token::SurfaceRaised));
        } else if model.hovered == Some(WorkspaceHit::SelectTab(tab.id)) {
            row::pad_to(
                &mut chip,
                chip_width,
                theme::color(Token::SurfaceRaisedSubtle),
            );
        }
        hits.push((
            Rect::new(chip_start, inner.y, chip_width, 1),
            WorkspaceHit::SelectTab(tab.id),
        ));
        // Same convention as the sidebar's own indicator two functions
        // away: an accent bar on the target chip's own leading column —
        // dropping at the end of the strip lands the bar on the last
        // chip too, not on a slot past it.
        if model.dragging_tab.is_some_and(|dragging| {
            dragging.is_pending_drop_row(TabDragGroup::Strip(space.id, context), tab.id, is_last)
        }) {
            drop_indicator = Some(Rect::new(chip_start, inner.y, 1, 1));
        }
        spans.extend(chip);
        // Just 1 column between chips, not 3 — each chip already reserves
        // its own 1-column pad on both sides (see `PAD` above), so a full
        // 3-column gap on top of that read as too much air once every tab
        // carried that padding, not just the selected one.
        spans.push(Span::raw(" "));
        x += chip_width + 1;
        // A "/" closes the agent leading the strip off from the shells
        // opened beside it, and from the "+" that opens another: two kinds
        // of tab, and without it the gap after the agent read as just
        // another gap between shells. No leading space — the chip's own
        // trailing gap is one — so it sits one column off either side.
        // `TextFaint`, the hue of the zone hairlines on this same backdrop:
        // a separator is not text, and the brighter `TextMuted` made it
        // read as one more word in the strip.
        if is_agent && x < limit {
            spans.push(Span::styled("/", theme::fg(Token::TextFaint)));
            spans.push(Span::raw(" "));
            x += 2;
        }
    }
    // A bold "+" creates a new shell tab directly. It stays neutral, just
    // bolder, being the plain action; a new agent is asked for on its
    // space's header in the sidebar (see `push_trailing_controls`), which
    // is where the space it lands in is named.
    //
    // `SurfaceRaisedBright` backs it where the actions take the plain
    // `SurfaceRaised`: a bare glyph has no word's weight carrying it, and
    // at the plain strength it reads as barely there.
    let buttons = [GroupButton {
        hit: WorkspaceHit::NewTab,
        label: "+".to_owned(),
        hue: theme::color(Token::TextInactive),
        strong: true,
        lit: false,
        switch: false,
    }];
    if x + group_width(&buttons) <= limit {
        let (actions, group_hits) =
            button_group(model, &buttons, Token::SurfaceRaisedBright, (x, inner.y));
        hits.extend(group_hits);
        spans.extend(actions);
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)),
        Rect::new(
            inner.x,
            inner.y,
            limit.saturating_sub(inner.x),
            inner.height,
        ),
    );
    if let Some(rect) = drop_indicator {
        frame.render_widget(
            Paragraph::new(theme::glyph(Symbol::BarThick)).style(theme::fg(Token::Accent)),
            rect,
        );
    }

    render_notice_chip(frame, model, inner, trailing_right);
}

/// A surface chip's label: the word, and the glyph standing for it in a
/// glyph set that actually carries one.
///
/// Two of the three shipped sets deliberately carry none. A surface is an
/// idea — the code of a checkout, the shape of a project, what changed —
/// and plain Unicode has no sign for any of them, so what a set without
/// icons can offer is a box or an arrow that means nothing here. The word
/// already says it; the glyph joins it only where there is a real one.
fn surface_label(symbol: Symbol, name: &str) -> String {
    match theme::glyph(symbol) {
        glyph if glyph.is_empty() => name.to_string(),
        glyph => format!("{glyph} {name}"),
    }
}

/// One member of a button group: what a click on it means, the label it
/// wears, and the hue that label takes.
struct GroupButton {
    hit: WorkspaceHit,
    label: String,
    hue: Color,
    /// Bold, for a member whose label is a bare glyph. A word carries
    /// itself at any weight; a lone "+" on a lifted fill reads as barely
    /// there beside one.
    strong: bool,
    /// Standing in the pane: the strip's one highlight, filled.
    lit: bool,
    /// A switch rather than a push: pressing it lights it or puts it out,
    /// and that change is its answer. The press flash on top of it drew a
    /// third look between the two — lit, then the flash, then unlit.
    switch: bool,
}

/// The columns one member of a group claims: its label, and the air each
/// side of it that is as much of the control as the label is.
fn group_member_width(label: &str) -> u16 {
    Span::raw(label).width() as u16 + 2 * chip::PAD
}

fn group_width(buttons: &[GroupButton]) -> u16 {
    buttons
        .iter()
        .map(|button| group_member_width(&button.label))
        .sum()
}

/// A group of buttons: one continuous ground, the members meeting on
/// their own padding, each answering the pointer alone — so what lifts
/// under it is exactly what a click lands on.
///
/// Nothing is drawn between them. A divider glyph inside a group is a
/// column belonging to no member, and a group like this is only ever one
/// ground or another: the moment one member is hovered, that orphan
/// column keeps the resting fill and the seam reads as a sliver of a
/// third thing wedged between two buttons. With nothing there, the
/// members meet on their padding and the boundary *is* where the fill
/// changes — visible exactly when there is something to see, and nowhere
/// at rest. A drawn divider belongs on the flat backdrop (the zone
/// hairlines, the "/" after the agent tab), where there is no fill for
/// it to disagree with.
///
/// `resting` is the group's own fill, which is the one thing that differs
/// between the two groups on this strip: a pair of bare glyphs needs a
/// brighter one than a pair of words, having no weight or hue of its own
/// otherwise carrying it. Hover and press are the skins every other
/// control in this row wears.
///
/// It answers with spans and rects rather than drawing, because the two
/// callers place a group differently: one lays it at the strip's right
/// end in a rect of its own, the other appends it to the single line the
/// tab side is already building.
///
/// Local to this screen, not in `src/ui/widget/`, by that module's own
/// threshold: the second *file* is what moves a helper into the
/// vocabulary, and both groups are drawn by this function's own caller.
fn button_group(
    model: &WorkspaceModel,
    buttons: &[GroupButton],
    resting: Token,
    origin: (u16, u16),
) -> (Vec<Span<'static>>, Vec<(Rect, WorkspaceHit)>) {
    let (mut x, y) = origin;
    let mut spans = Vec::new();
    let mut hits = Vec::new();
    for button in buttons {
        let (hue, ground) = match chip_state(model, Some(button.hit)) {
            // Filled with the brightest ink and written in the backdrop,
            // the way a strong button is: a lit member is the one thing
            // on the strip that must be found at a glance, and a shade
            // over its neighbour's plate was not enough to find it. It
            // stays lit under the pointer — it is already the answer.
            _ if button.lit => (
                theme::color(Token::SurfaceBackground),
                theme::color(Token::TextBright),
            ),
            ChipState::Resting => (button.hue, theme::color(resting)),
            ChipState::Pressed if button.switch => ChipState::Hovered.skin(button.hue),
            other => other.skin(button.hue),
        };
        let mut label = Style::default().fg(hue).bg(ground);
        if button.strong {
            label = label.add_modifier(Modifier::BOLD);
        }
        let taken = group_member_width(&button.label);
        spans.push(Span::styled(" ", Style::default().bg(ground)));
        spans.push(Span::styled(button.label.clone(), label));
        spans.push(Span::styled(" ", Style::default().bg(ground)));
        hits.push((Rect::new(x, y, taken, 1), button.hit));
        x += taken;
    }
    (spans, hits)
}

/// Whether a zone's edge is a filled surface or bare text on the strip's
/// own backdrop.
///
/// It decides one column, and that column is the whole of the alignment.
/// A filled chip's padding belongs to the control — it is lit, hovered
/// and clicked with the glyphs — so the air beside a hairline has to be a
/// column of its own. Bare text's padding *is* that air already, and
/// adding a second column put the hairline one off centre between two
/// zones: two blank columns against the unfilled side, one against the
/// filled one.
#[derive(Clone, Copy, Eq, PartialEq)]
enum ZoneEdge {
    Filled,
    Bare,
}

impl ZoneEdge {
    /// Where the next thing to the left may end, given this zone starts at
    /// `x`.
    fn left_of(self, x: u16) -> u16 {
        match self {
            Self::Filled => x.saturating_sub(1),
            Self::Bare => x,
        }
    }
}

/// The hairline between two zones of the strip's right end, drawn ending
/// at `right` and answering with the column the zone to its left may end
/// at — which `next` decides, since that zone's own edge is what the air
/// beside the mark is measured against.
///
/// On the plain backdrop, where a drawn divider has no fill to disagree
/// with, and in the faintest text hue there is: it separates two things
/// that are already far apart in meaning, so it has only to be found when
/// looked for, never read. It sits outside every rect a pointer can land
/// on, and a zone draws its own before itself, so a zone with nothing to
/// say takes its divider with it.
fn render_zone_hairline(
    frame: &mut ratatui::Frame<'_>,
    inner: Rect,
    right: u16,
    next: ZoneEdge,
) -> u16 {
    let divider = theme::glyph(Symbol::TreeColumnDivider);
    let width = Span::raw(&divider).width() as u16;
    let rect = Rect::new(right.saturating_sub(width), inner.y, width, 1);
    frame.render_widget(
        Paragraph::new(Span::styled(divider, theme::fg(Token::TextFaint))),
        rect,
    );
    next.left_of(rect.x)
}

/// Everything the workspace has to say, in the one place it says it: the
/// header's own row, left of the actions and divided from them, where the
/// operator's eye already is. Nothing here is clickable and nothing here
/// moves a button — the actions were laid out before this was, and this
/// only takes the room they left.
fn render_notice_chip(
    frame: &mut ratatui::Frame<'_>,
    model: &WorkspaceModel,
    inner: Rect,
    actions_left: u16,
) {
    let Some(chip) = model.notice_chip() else {
        return;
    };
    let spans = vec![
        Span::raw(" "),
        // Work still running says so by moving, which is what buys the
        // words the right to be two: "delivering", not "delivering every
        // ready task…".
        Span::styled(
            match chip.busy {
                true => format!("{} ", agent_activity_frame(model.tick)),
                false => String::new(),
            },
            theme::fg(Token::Accent),
        ),
        Span::styled(
            chip.text,
            Style::default()
                .fg(theme::color(Token::TextBright))
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        // The zone divider, in the same hue and on the same plain backdrop
        // as every other hairline between the header's zones.
        // No filled chip behind any of this: a message is not a control,
        // and the raised surface is what made it read as one.
        Span::styled(
            theme::glyph(Symbol::TreeColumnDivider),
            theme::fg(Token::TextFaint),
        ),
    ];
    // Never past the strip's left edge: what does not fit is this
    // message's own tail, clipped by its rect, not the tabs beside it.
    let Some(room) = actions_left.checked_sub(inner.x).filter(|room| *room > 0) else {
        return;
    };
    let width = (spans.iter().map(Span::width).sum::<usize>() as u16).min(room);
    let rect = Rect::new(actions_left.saturating_sub(width), inner.y, width, 1);
    frame.render_widget(Paragraph::new(Line::from(spans)), rect);
}

pub(super) fn render_pane(frame: &mut ratatui::Frame<'_>, area: Rect, model: &WorkspaceModel) {
    let Some(snapshot) = model.panes.get(&model.focused_pane()) else {
        frame.render_widget(
            Paragraph::new(model.error.as_deref().unwrap_or(" starting shell…"))
                .style(theme::fg(Token::TextMuted)),
            area,
        );
        return;
    };
    let width = area.width.min(snapshot.columns);
    let height = area.height.min(snapshot.rows);
    let selection = model
        .selection
        .filter(|selection| selection.pane == snapshot.pane && selection.is_visible());
    let buffer = frame.buffer_mut();
    let mut encoded = [0u8; 4];
    for row in 0..height {
        for column in 0..width {
            let index = usize::from(row) * usize::from(snapshot.columns) + usize::from(column);
            if let Some(cell) = snapshot.cells.get(index) {
                let mut style = cell_style(cell);
                // Reversed against the cell's own colours rather than
                // tinted with one of ours: a pane's content can be any
                // colour at all, and inversion is the one mark that
                // stays legible over every one of them.
                if selection.is_some_and(|selection| selection.contains(column, row)) {
                    style = if cell.attributes.inverse {
                        style.remove_modifier(Modifier::REVERSED)
                    } else {
                        style.add_modifier(Modifier::REVERSED)
                    };
                }
                buffer[(area.x + column, area.y + row)]
                    .set_symbol(cell.character.encode_utf8(&mut encoded))
                    .set_style(style);
            }
        }
    }
    if snapshot.cursor.row < height && snapshot.cursor.column < width {
        buffer[(
            area.x + snapshot.cursor.column,
            area.y + snapshot.cursor.row,
        )]
            .set_style(
                Style::default()
                    .bg(theme::color(Token::TextBright))
                    .fg(theme::color(Token::SurfaceBackground)),
            );
    }
}

pub(super) fn cell_style(cell: &uze_terminal::RenderCell) -> Style {
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

pub(super) fn color(color: TerminalColor) -> Color {
    match color {
        TerminalColor::DefaultForeground => theme::color(Token::TextPrimary),
        TerminalColor::DefaultBackground => theme::color(Token::SurfaceBackground),
        TerminalColor::Rgb { red, green, blue } => theme::content(red, green, blue),
        // The 16 a program can name by index are the theme's, so a pane
        // cannot contradict the chrome drawn around it. Above 15 are the
        // 240 extended entries no theme defines — passed through as the
        // index they are.
        TerminalColor::Indexed(index) => match uze_theme::active().ansi(index) {
            Some(rgb) => theme::content(rgb.0, rgb.1, rgb.2),
            None => Color::Indexed(index),
        },
    }
}

/// The stack of outcomes, against the top-right of the pane.
///
/// Flush with the pane's own right edge, which is already inset a column
/// from the frame (see `compute_layout`) and is the same column the tab
/// strip's controls end at — so a toast lines up under the chip above it
/// rather than a column short of it. Insetting again here is what put a
/// second margin on that side.
///
/// Flush with the pane's own top row, which already sits a row below the
/// tab strip's text. A row of air was added on top of that one, and two
/// rows is far enough that the message stops reading as an answer to what
/// the strip above it says and starts reading as something floating in the
/// pane — which is the opposite of what a toast anchored to the top-right
/// is for.
fn render_toasts(
    frame: &mut ratatui::Frame<'_>,
    pane: Rect,
    model: &WorkspaceModel,
    hits: &mut Vec<(Rect, WorkspaceHit)>,
) {
    let stack = model.toast_stack();
    if stack.is_empty() || pane.width < 16 || pane.height < 5 {
        return;
    }
    let area = pane;
    let mut targets = Vec::new();
    for (index, placed) in widget::toast::stack(frame, area, &stack)
        .into_iter()
        .enumerate()
    {
        // The offer first: it sits inside the box, and the box answers
        // everything else by putting the message away.
        if let Some(action) = placed.action {
            targets.push((action, WorkspaceHit::ToastAction(index)));
        }
        // The mark first, then the row behind it: both put the message
        // away, and asking the row first would make the mark unreachable
        // rather than merely redundant.
        targets.push((placed.close, WorkspaceHit::DismissToast(index)));
        targets.push((placed.box_rect, WorkspaceHit::DismissToast(index)));
    }
    // Prepended: the pane underneath answers a click anywhere, so a toast
    // asked after it would never be the answer.
    hits.splice(0..0, targets);
}
