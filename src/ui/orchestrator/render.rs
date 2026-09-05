//! Everything the workspace client draws.
//!
//! Split out of `orchestrator.rs`, which had grown to 3.5k lines covering
//! three unrelated jobs: driving the session, drawing it, and encoding input
//! for the PTY. Nothing here mutates session state — these take a
//! `&WorkspaceModel` and paint it, which is what makes them one module.

use super::*;
use crate::ui::{Rows, fill_row_bg};

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
/// itself (see [`render_sidebar`]), and this mode never shows the help
/// toolbar (see [`ui::render_footer`](crate::ui::render_footer) — that
/// stays exclusive to the management TUI).
pub(super) fn compute_layout(
    frame_area: Rect,
    sidebar_width_override: Option<u16>,
) -> WorkspaceLayout {
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
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct FrameMetrics {
    /// Rows of the space tree the sidebar could not show — how far the
    /// tree may be scrolled, and zero when it fits.
    pub(super) tree_overflow: u16,
}

pub(super) fn render(
    frame: &mut ratatui::Frame<'_>,
    model: &WorkspaceModel,
    identities: &[AgentIdentity],
    hits: &mut Vec<(Rect, WorkspaceHit)>,
    metrics: &mut FrameMetrics,
) {
    frame.render_widget(
        Block::default().style(
            Style::default()
                .bg(crate::ui::BASE)
                .fg(crate::ui::TEXT_PRIMARY),
        ),
        frame.area(),
    );
    // The Git changes overlay covers the entire frame when open (see
    // `git::view`) — everything below would just be drawn and
    // immediately hidden underneath it, so skip it outright rather than
    // paying for a sidebar/tab-strip/pane render this frame will never
    // show.
    if let Some(git) = &model.git_view {
        // The extension answers with content; the host lays it out and
        // therefore is the only side that can say which rectangle a click
        // landed in. The hits come back in the view's own vocabulary and
        // are tagged with the extension they belong to on the way into the
        // shared `hits` vec — the one place that translation happens.
        let mut view_hits = Vec::new();
        let area = frame.area();
        let view = git::view(
            git,
            crate::ui::extension_view::content_space(area, model.git_tree_width),
        );
        crate::ui::extension_view::render(frame, &view, area, model.git_tree_width, &mut view_hits);
        hits.extend(
            view_hits
                .into_iter()
                .map(|(rect, hit)| (rect, WorkspaceHit::Extension(ExtensionHit::Git(hit)))),
        );
        return;
    }
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
    render_pane(frame, layout.pane, model);
    // Drawn last so it sits on top of the pane — same ordering the
    // management TUI's overlays use in its own `render`. Anchored to
    // `picker.anchor` (the "✦" button's own rect) rather than centered on
    // the whole frame — a dropdown hanging off the thing you clicked, not a
    // modal interrupting the screen.
    // A notice about the tab already on screen said its piece next to the
    // deliver button in the header (`render_tab_strip`) — this is only the
    // fallback for a workspace-wide message, or one about a task that is
    // not what the operator is currently looking at.
    if let Some(text) = model.notice_for_footer() {
        render_notice(frame, layout.pane, &text);
    }
    if let Some(overlay) = &model.preserved {
        render_preserved(frame, frame.area(), model, overlay);
    }
    if let Some(picker) = &model.agent_picker {
        render_agent_picker(frame, frame.area(), picker.anchor, picker, hits);
    }
    if let Some(dropdown) = &model.support_dropdown
        && let Some(resolution) = &model.agent_support
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
}

/// A small popup listing `agent_options`, opened by the tab strip's "✦"
/// button — a dropdown anchored just below it, creating the
/// picked agent as a new tab in the currently selected space. Not built on
/// the management TUI's `render_modal`/`modal_block` (those are shaped for
/// static text, not a selectable, hit-testable list) — this is
/// self-contained, styled by hand to match the same palette.
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
                .fg(crate::ui::ACCENT)
                .add_modifier(Modifier::BOLD),
        )
        .borders(Borders::ALL)
        .border_style(Style::default().fg(crate::ui::BORDER))
        .style(Style::default().bg(crate::ui::BASE));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    if picker.options.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "no harnesses found",
                Style::default().fg(crate::ui::MUTED),
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
                .bg(crate::ui::ACCENT)
                .fg(crate::ui::BASE)
                .add_modifier(Modifier::BOLD);
            let text = format!(
                " {:<width$}",
                option.display_name,
                width = inner.width.saturating_sub(1) as usize
            );
            (style, text)
        } else {
            let style = Style::default().fg(crate::ui::NAV_INACTIVE);
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
pub(super) fn render_context_menu(
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
        .border_style(Style::default().fg(crate::ui::BORDER))
        .style(Style::default().bg(crate::ui::BASE));
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
                .bg(crate::ui::ACCENT)
                .fg(crate::ui::BASE)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(crate::ui::NAV_INACTIVE)
        };
        let label = format!("{:pad$}{}", "", action.label(), pad = H_PAD as usize);
        let text = format!("{label:<width$}", width = inner.width as usize);
        frame.render_widget(Paragraph::new(Span::styled(text, style)), row);
        hits.push((row, WorkspaceHit::ContextMenuAction(index)));
    }
}

/// How a caption row reads a pane's directory: a slot shows as the primary
/// it hangs off rather than as its own `.worktrees/<id>` path — that tail
/// is two more segments in a column only 28-40 wide (see
/// `crate::ui::MIN_SIDEBAR_WIDTH`), and the primary is where the operator
/// is; the slot is where the agent is, which every agent has and none
/// needs announced.
fn caption_path(cwd: &Path) -> String {
    match uze_application::isolated_checkout(cwd) {
        Some(checkout) => crate::ui::display_project_path(checkout.primary),
        None => crate::ui::display_project_path(cwd),
    }
}

/// Renders lowercase ASCII letters as their Unicode small-capital form
/// (`claude` -> `ᴄʟᴀᴜᴅᴇ`) — a quieter way to give a badge-like label some
/// visual weight without full-height capitals. Unicode has no small-cap
/// `q` or `x`, so those (and anything already non-lowercase-ASCII) pass
/// through unchanged rather than being dropped or capitalized.
fn small_caps(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'a' => 'ᴀ',
            'b' => 'ʙ',
            'c' => 'ᴄ',
            'd' => 'ᴅ',
            'e' => 'ᴇ',
            'f' => 'ꜰ',
            'g' => 'ɢ',
            'h' => 'ʜ',
            'i' => 'ɪ',
            'j' => 'ᴊ',
            'k' => 'ᴋ',
            'l' => 'ʟ',
            'm' => 'ᴍ',
            'n' => 'ɴ',
            'o' => 'ᴏ',
            'p' => 'ᴘ',
            'r' => 'ʀ',
            's' => 'ꜱ',
            't' => 'ᴛ',
            'u' => 'ᴜ',
            'v' => 'ᴠ',
            'w' => 'ᴡ',
            'y' => 'ʏ',
            'z' => 'ᴢ',
            other => other,
        })
        .collect()
}

/// Whether `cwd` is outside any slot — the fallback every agent tab
/// otherwise never needs: no repository, no commit to branch from, Git
/// absent or refusing. An agent in the operator's own tree is the one
/// thing the operator has to know about, so its caption — the branch it
/// is on — is the one drawn in the warning hue rather than dim (see
/// [`caption_color`]). Not a mark on the row, and not a status in the
/// catalog: the branch is already there, and its colour says it.
fn is_unisolated(cwd: &Path) -> bool {
    !uze_application::is_isolated_checkout(cwd)
}

/// The hue an agent's caption line is drawn in: dim, like every other
/// detail, except for an agent working in the operator's own tree.
fn caption_color(cwd: &Path) -> Color {
    if is_unisolated(cwd) {
        crate::ui::WARNING
    } else {
        crate::ui::TEXT_DIM
    }
}

/// Appends a one-cell mark behind a space to an agent row, and makes that
/// cell a click target opening the status catalog: a glyph nobody can
/// look up is a glyph that reads as decoration.
fn push_trailing_mark(
    spans: &mut Vec<Span<'_>>,
    hits: &mut Vec<(Rect, WorkspaceHit)>,
    label_rect: Rect,
    mark: &'static str,
    hue: Color,
) {
    let mark_x = label_rect.x + spans.iter().map(|span| span.width() as u16).sum::<u16>() + 1; // the space this mark is drawn behind
    spans.push(Span::styled(format!(" {mark}"), Style::default().fg(hue)));
    if mark_x < label_rect.right() {
        let cell = Rect::new(mark_x, label_rect.y, 1, 1);
        hits.push((cell, WorkspaceHit::OpenStatusCatalog(cell)));
    }
}

/// A two-level tree, one block per space the user has created (blank-line
/// separated — see the loop below), each expanded (no collapse/accordion)
/// into the agent tabs [`agent_identity_for_tab`] recognizes as running
/// inside it — `●`/`○` for the space's context agent (see
/// `space_context_agent`) vs. the rest, plus its label and, right-
/// aligned on that same row, the harness alias in place of the raw process
/// name (see [`agent_identity_for_tab`]). A dim caption line
/// underneath names the task's own working branch, falling back to its
/// pane's live cwd (as [`caption_path`] renders it, so an agent in a slot
/// reads as its primary checkout rather than as a `.worktrees/<id>` path
/// too long for the column) for the moment before that task association
/// resolves. A space with no agent tabs shows its current `cwd` alone in place of the tree,
/// so an empty space still reads as "somewhere", not blank. Plain shell
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
    let border_color = if model.dragging_sidebar {
        crate::ui::ACCENT
    } else {
        crate::ui::BORDER_FAINT
    };
    // No top padding: the header must land on the exact row the tab strip's
    // own content does (that block has none either), or the two panes'
    // dividers drift out of alignment by one row. No right padding either:
    // the header action sits flush against the divider, with only the left
    // side keeping its 1-column inset.
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(border_color))
        .padding(Padding::new(1, 0, 0, 0));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut rows = Rows::over(inner);

    // Mode toggle, one line: this used to be a global titlebar (brand +
    // status + Ctrl+O hint + path) spanning the whole frame; with only menu
    // + main container left, the menu opens with just enough chrome to
    // match the tab strip's height on the other TUI mode — a centered
    // segmented control stands in for the Ctrl+O keybinding.
    if let Some(rect) = rows.next(1) {
        let (_work_rect, manage_rect) = crate::ui::render_mode_toggle(frame, rect, true);
        hits.push((manage_rect, WorkspaceHit::SwitchToManagement));
    }
    if let Some(error) = &model.error
        && let Some(rect) = rows.next(1)
    {
        frame.render_widget(
            Paragraph::new(Span::styled(
                error.clone(),
                Style::default()
                    .fg(crate::ui::DANGER)
                    .add_modifier(Modifier::BOLD),
            )),
            rect,
        );
    }
    if let Some(rect) = rows.next(1) {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "─".repeat(rect.width as usize),
                Style::default().fg(crate::ui::BORDER_FAINT),
            )),
            rect,
        );
    }

    let Some(session) = &model.session else {
        return;
    };

    // The summary and creation action share the row directly below the
    // header divider: the quiet count gives the workspace scope, while the
    // right-aligned action remains the primary affordance.
    if let Some(rect) = rows.next(1) {
        let agent_count = session
            .workspace
            .spaces
            .iter()
            .flat_map(|space| space.tabs.iter())
            .filter(|tab| agent_identity_for_tab(identities, tab).is_some())
            .count();
        let count_label = format!(
            "{agent_count} agent{}",
            if agent_count == 1 { "" } else { "s" }
        );
        frame.render_widget(
            Paragraph::new(Span::styled(
                count_label,
                Style::default().fg(crate::ui::TEXT_DIM),
            )),
            rect,
        );
        let label = "+ new";
        let label_x = rect.x + rect.width.saturating_sub(label.len() as u16);
        frame.render_widget(
            Paragraph::new(Span::styled(label, Style::default().fg(crate::ui::ACCENT)))
                .alignment(Alignment::Right),
            rect,
        );
        hits.push((
            Rect::new(label_x, rect.y, label.len() as u16, 1),
            WorkspaceHit::NewSpace,
        ));
    }
    rows.gap();
    // While the root picker is open it owns the column: the listing it
    // draws is a tree of directories, and side by side with the tree of
    // spaces neither would read as the one being chosen from. Closing it
    // brings the spaces straight back.
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
        .git_badge
        .as_ref()
        .and_then(|badge| badge.timeline.as_ref());
    let reserved = timeline.map_or(0, |timeline| {
        timeline_height(
            timeline,
            model.timeline_collapsed,
            model.timeline_rows,
            rows.remaining(),
        )
    });
    let column_bottom = rows.bottom;
    rows.bottom -= reserved;

    // What the column cannot show is scrolled to, not lost: the tree grows
    // with the work, and a space that fell off the foot of it — under a
    // long tree above, or under the timeline holding the foot — used to be
    // unreachable rather than merely out of view. The bound is measured
    // here, where the tree's own window is known, and handed back for the
    // wheel to stay inside (see `scroll_tree`).
    let overflow = tree_rows(session, identities).saturating_sub(rows.remaining());
    metrics.tree_overflow = overflow;
    rows.scroll_past(model.tree_scroll.min(overflow));

    for space in &session.workspace.spaces {
        let is_active_space = space.id == session.workspace.selected_space;
        let header = rows.slot(1);
        if header.is_full() {
            break;
        }
        if let Some(header_rect) = header.visible() {
            render_space_header(frame, header_rect, session, space, model, hits);
        }

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
            if let Some(cwd_rect) = rows.slot(1).visible() {
                let cwd = space
                    .tabs
                    .iter()
                    .find(|tab| tab.id == space.selected_tab)
                    .and_then(|tab| pane_in_layout(&tab.layout, tab.focus.pane))
                    .map(|pane| crate::ui::display_project_path(&pane.cwd))
                    .unwrap_or_default();
                let mut spans = vec![Span::styled(
                    format!("  {cwd}"),
                    Style::default().fg(crate::ui::TEXT_DIM),
                )];
                if is_active_space {
                    fill_row_bg(&mut spans, cwd_rect.width, crate::ui::ACTIVE_SPACE_OVERLAY);
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
            let label_slot = rows.slot(1);
            if label_slot.is_full() {
                break;
            }

            let cwd = pane_in_layout(&tab.layout, tab.focus.pane)
                .map(|pane| pane.cwd.clone())
                .unwrap_or_default();
            // A tab-reorder drag in this exact space, resolved to drop
            // right before (or, on the last row, at the end after) this
            // one: an accent bar down the row's own leading column, same
            // "which row is affected" language the sidebar's other row
            // affordances (status glyphs, the active-space tint) already
            // use, in place of a separate line between rows that would
            // need its own row out of an already tightly budgeted list.
            // Drawn on both the label row and the detail row below — the
            // tab is a two-row item, not just its label, so the bar has to
            // run the item's full height to read as "this whole item"
            // rather than something clipped to its top row alone.
            let show_drop_indicator = model.dragging_tab.is_some_and(|dragging| {
                dragging.is_pending_drop_row(TabDragGroup::Agents(space.id), tab.id, is_last)
            });

            if let Some(label_rect) = label_slot.visible() {
                // The agent the space is about, not its `selected_tab`: a
                // shell opened beside an agent is part of that agent's own
                // context, and switching into it must not unselect the agent
                // in this tree (see `space_context_agent`).
                let selected = Some(tab.id) == space_context_agent(space, identities);
                // Every space names a context agent, including the ones the
                // user is not in — so `selected` alone put a `●` on one agent
                // per open space, each claiming to be the one receiving
                // keystrokes. Only the active space's selection is that agent.
                let status = model.agent_tab_status(tab.focus.pane, is_active_space && selected);
                let renaming_this = model
                    .renaming
                    .as_ref()
                    .filter(|(target, _)| *target == RenameTarget::Tab(tab.id))
                    .map(|(_, buffer)| buffer.as_str());
                let indicator = status.glyph(model.tick);
                let indicator_fg = status.color();
                // Bold belongs to the agent, not the space it runs in (see
                // `render_space_header`, which never bolds its own label) — the
                // tab actually receiving keystrokes is the thing worth shouting
                // about, not the container it happens to sit in. Reserved for
                // the one tab that is both selected and in the active space
                // (see the `status` comment above `is_active_space && selected`
                // for why `selected` alone isn't enough).
                let mut label_style = Style::default().fg(if selected {
                    crate::ui::TEXT_BRIGHT
                } else {
                    crate::ui::NAV_INACTIVE
                });
                if is_active_space && selected {
                    label_style = label_style.add_modifier(Modifier::BOLD);
                }
                let connector_span =
                    Span::styled(connector, Style::default().fg(crate::ui::TEXT_FAINT));
                let label = match renaming_this {
                    Some(buffer) => Span::styled(
                        format!("{buffer}▏"),
                        Style::default()
                            .fg(crate::ui::TEXT_BRIGHT)
                            .add_modifier(Modifier::BOLD),
                    ),
                    None => Span::styled(tab.label.clone(), label_style),
                };
                // The task mark behind the label is the one click target that
                // opens the catalog (see `push_trailing_mark`): the status glyph
                // in front of the name is not, so the row's leading column
                // stays a plain part of selecting the tab. Pushed before the
                // row's own `SelectTab` hit below, since the click search takes
                // the first rect it lands in — a 1-column target inside a
                // row-wide one only ever wins by being found first.
                let mut spans = vec![
                    connector_span,
                    Span::styled(indicator, Style::default().fg(indicator_fg)),
                    label,
                ];
                if let Some((mark, hue)) = model
                    .tab_task(tab.id)
                    .and_then(|task| task_mark(&task.state))
                {
                    push_trailing_mark(&mut spans, hits, label_rect, mark, hue);
                }
                // The alias in place of the raw process name — this list only
                // ever holds tabs `agent_identity_for_tab` already resolved, so
                // it never falls back to showing something like a bare version
                // string (see that function's doc comment). Right-aligned, not
                // tacked onto the label behind a "·" — pinning it to the row's
                // own right edge keeps its column stable as different labels
                // vary in length. A 1-column trailing pad keeps it off the
                // sidebar's own flush-right divider (see `render_sidebar`'s
                // `Padding::new(1, 0, 0, 0)`) — that padding drop suits a
                // button glued to the edge, not a plain text label.
                let alias = agent_identity_for_tab(identities, tab).unwrap_or_default();
                let alias_span =
                    Span::styled(small_caps(alias), Style::default().fg(crate::ui::TEXT_DIM));
                const TRAILING_PAD: u16 = 1;
                let used: u16 = spans.iter().map(|span| span.width() as u16).sum::<u16>()
                    + alias_span.width() as u16
                    + TRAILING_PAD;
                let gap = label_rect.width.saturating_sub(used);
                spans.push(Span::raw(" ".repeat(gap as usize)));
                spans.push(alias_span);
                spans.push(Span::raw(" ".repeat(TRAILING_PAD as usize)));
                if is_active_space {
                    fill_row_bg(
                        &mut spans,
                        label_rect.width,
                        crate::ui::ACTIVE_SPACE_OVERLAY,
                    );
                }
                frame.render_widget(Paragraph::new(Line::from(spans)), label_rect);
                hits.push((label_rect, WorkspaceHit::SelectTab(tab.id)));
                if show_drop_indicator {
                    frame.render_widget(
                        Paragraph::new("▍").style(Style::default().fg(crate::ui::ACCENT)),
                        Rect::new(label_rect.x, label_rect.y, 1, 1),
                    );
                }
            }

            if let Some(detail_rect) = rows.slot(1).visible() {
                let continuation = if is_last { "     " } else { "  │  " };
                // The task's own working branch in place of the cwd path —
                // what this agent will deliver from. An agent outside any
                // slot has no task, so its branch is the one its
                // evaluation read at the directory itself. Either falls
                // back to the cwd (via `caption_path`) for the moment
                // right after tab creation, before the async evaluation
                // resolves and a branch exists to show.
                // A checkout removed from under the agent is said in
                // words, not as the kernel's `(deleted)` path: the process
                // cannot work there any more, and the task it was running
                // is what the preserved list now holds.
                let lost = model.lost_checkouts.contains(&tab.focus.pane);
                let detail = if lost {
                    "checkout removed".to_owned()
                } else {
                    model
                        .tab_task(tab.id)
                        .map(|task| task.branch.clone())
                        .or_else(|| unisolated_branch(model, &cwd))
                        .unwrap_or_else(|| caption_path(&cwd))
                };
                let continuation_span =
                    Span::styled(continuation, Style::default().fg(crate::ui::TEXT_FAINT));
                let detail_color = if lost {
                    crate::ui::WARNING
                } else {
                    caption_color(&cwd)
                };
                let mut spans = vec![continuation_span];
                // Right-aligned under the alias, with the same trailing
                // pad off the divider: a count pinned to the row's edge
                // keeps its column as branches vary in length.
                // The way back in, on the row itself: "resume" puts the
                // task this pane was running into a slot of its own, via
                // the same picker a new agent goes through. Offered only
                // while the task is waiting for one (see `lost_task`).
                const RESUME: &str = "resume";
                let resumable = lost && model.lost_task(tab.focus.pane).is_some();
                let sync: Vec<Span<'_>> = if resumable {
                    vec![Span::styled(RESUME, Style::default().fg(crate::ui::ACCENT))]
                } else {
                    unisolated_sync_caption(model, &cwd)
                        .into_iter()
                        .enumerate()
                        .map(|(index, (text, hue))| {
                            let gap = if index == 0 { "" } else { " " };
                            Span::styled(format!("{gap}{text}"), Style::default().fg(hue))
                        })
                        .collect()
                };
                // The branch is elided, never cut: a name longer than the
                // column used to run under the sync caption and off the
                // right edge, so the one thing the row was pinning there —
                // "3 ahead", "resume" — was what disappeared. What it says
                // is now sized to what is left after that caption, and the
                // "…" says a name was shortened rather than leaving the
                // reader to wonder whether the branch really ends there.
                {
                    let taken: u16 = spans
                        .iter()
                        .chain(&sync)
                        .map(|span| span.width() as u16)
                        .sum::<u16>()
                        + crate::ui::TRAILING_PAD;
                    let room = detail_rect.width.saturating_sub(taken).max(1);
                    spans.push(Span::styled(
                        crate::ui::elide_tail(&detail, room as usize),
                        Style::default().fg(detail_color),
                    ));
                }
                if !sync.is_empty() {
                    const TRAILING_PAD: u16 = 1;
                    if resumable {
                        let x = detail_rect
                            .right()
                            .saturating_sub(TRAILING_PAD + RESUME.len() as u16);
                        hits.push((
                            Rect::new(x, detail_rect.y, RESUME.len() as u16, 1),
                            WorkspaceHit::ResumeLostCheckout(tab.id),
                        ));
                    }
                    let used: u16 = spans
                        .iter()
                        .chain(&sync)
                        .map(|span| span.width() as u16)
                        .sum::<u16>()
                        + TRAILING_PAD;
                    let gap = detail_rect.width.saturating_sub(used).max(1);
                    spans.push(Span::raw(" ".repeat(gap as usize)));
                    spans.extend(sync);
                    spans.push(Span::raw(" ".repeat(TRAILING_PAD as usize)));
                }
                if is_active_space {
                    fill_row_bg(
                        &mut spans,
                        detail_rect.width,
                        crate::ui::ACTIVE_SPACE_OVERLAY,
                    );
                }
                frame.render_widget(Paragraph::new(Line::from(spans)), detail_rect);
                // The label and its dim branch/cwd caption read as one tree
                // item — clicking the caption line must select the tab too,
                // not just the label text above it.
                hits.push((detail_rect, WorkspaceHit::SelectTab(tab.id)));
                if show_drop_indicator {
                    frame.render_widget(
                        Paragraph::new("▍").style(Style::default().fg(crate::ui::ACCENT)),
                        Rect::new(detail_rect.x, detail_rect.y, 1, 1),
                    );
                }
            }
            // A light gap between sibling tabs. A bare blank row was tried
            // and discarded — it broke the tree's own "│" connector into
            // two disconnected stubs. Continuing that connector through the
            // gap row keeps the tree intact while still giving each 2-row
            // item a little room to breathe. Skipped for the last tab: it
            // has no sibling below to connect to, and the space loop's own
            // blank row already separates it from whatever comes next.
            if !is_last && let Some(gap_rect) = rows.slot(1).visible() {
                let mut spans = vec![Span::styled(
                    "  │  ",
                    Style::default().fg(crate::ui::TEXT_FAINT),
                )];
                if is_active_space {
                    fill_row_bg(&mut spans, gap_rect.width, crate::ui::ACTIVE_SPACE_OVERLAY);
                }
                frame.render_widget(Paragraph::new(Line::from(spans)), gap_rect);
            }
        }
        // One blank row *between* spaces (not between a tab and its own
        // detail line, which stays tight per the comment above) — each
        // space is its own block, and needs the breathing room a flat
        // tab list didn't.
        rows.gap();
    }

    if let Some(timeline) = timeline
        && reserved > 0
    {
        rows.scroll_past(0);
        rows.bottom = column_bottom;
        rows.y = column_bottom - reserved;
        render_timeline(frame, timeline, model, &mut rows, hits);
    }
}

/// The rows the space tree comes to, whether or not the column can show
/// them all: a header per space, the cwd caption a space with no agent
/// shows in place of its tree, two rows per agent with the connector row
/// between siblings, and the blank row closing each space. Measured up
/// front rather than counted while drawing, because how far the tree may
/// be scrolled has to be known before its first row is laid out.
fn tree_rows(session: &Session, identities: &[AgentIdentity]) -> u16 {
    session
        .workspace
        .spaces
        .iter()
        .map(|space| {
            let agents = space
                .tabs
                .iter()
                .filter(|tab| agent_identity_for_tab(identities, tab).is_some())
                .count() as u16;
            let body = if agents == 0 { 1 } else { agents * 3 - 1 };
            1 + body + 1
        })
        .sum()
}

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
    timeline: &git::Timeline,
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
    timeline: &git::Timeline,
    model: &WorkspaceModel,
    rows: &mut Rows,
    hits: &mut Vec<(Rect, WorkspaceHit)>,
) {
    let section = git::timeline_section(timeline, model.timeline_collapsed, model.timeline_scroll);
    let mut section_hits = Vec::new();
    crate::ui::extension_view::render_section(
        frame,
        &section,
        rows,
        model.dragging_timeline,
        &mut section_hits,
    );
    hits.extend(section_hits.into_iter().map(|(rect, hit)| {
        (
            rect,
            WorkspaceHit::Extension(ExtensionHit::GitTimeline(hit)),
        )
    }));
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
    const H_PAD: u16 = 2;
    const V_PAD: u16 = 1;
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
    let inner_width = usize::from(width.saturating_sub(2 + 2 * H_PAD).max(1));

    let mut lines = vec![
        crate::ui::title_row("commit", "esc", inner_width),
        Line::default(),
        Line::from(vec![
            Span::styled("◉ ", Style::default().fg(crate::ui::BLUE)),
            Span::styled(
                detail.author.clone(),
                Style::default().fg(crate::ui::TEXT_PRIMARY),
            ),
            Span::styled(
                format!(" · {} · {}", detail.age, detail.date),
                Style::default().fg(crate::ui::TEXT_SECONDARY),
            ),
        ]),
        Line::from(Span::styled(
            detail.subject.clone(),
            Style::default()
                .fg(crate::ui::TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        )),
    ];
    if !detail.body.is_empty() {
        lines.push(Line::default());
        lines.extend(detail.body.lines().map(|line| {
            Line::from(Span::styled(
                line.to_owned(),
                Style::default().fg(crate::ui::TEXT_SECONDARY),
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
            Style::default().fg(crate::ui::TEXT_SECONDARY),
        ),
        Span::styled(
            format!("  +{}", detail.insertions),
            Style::default().fg(crate::ui::SUCCESS),
        ),
        Span::styled(
            format!("  −{}", detail.deletions),
            Style::default().fg(crate::ui::DANGER),
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
            crate::ui::WARNING
        } else {
            crate::ui::BLUE
        };
        footer.push(Span::styled(
            format!(" {reference} "),
            Style::default().fg(crate::ui::BASE).bg(hue),
        ));
    }
    let used: usize = footer.iter().map(Span::width).sum();
    let gap = inner_width.saturating_sub(used + detail.short_hash.chars().count());
    footer.push(Span::raw(" ".repeat(gap.max(1))));
    footer.push(Span::styled(
        detail.short_hash.clone(),
        Style::default().fg(crate::ui::MUTED),
    ));
    lines.push(Line::from(footer));

    let content_rows: u16 = lines
        .iter()
        .map(|line| line.width().max(1).div_ceil(inner_width) as u16)
        .sum();
    let height = (content_rows + 2 + 2 * V_PAD)
        .min(area.height)
        .clamp(1, MAX_HEIGHT);
    let rect = Rect::new(
        x,
        popup.anchor.y.min(area.bottom().saturating_sub(height)),
        width,
        height,
    );
    let inner = commit_detail_block()
        .padding(Padding::new(H_PAD, H_PAD, V_PAD, V_PAD))
        .inner(rect);
    CommitDetailLayout {
        rect,
        inner,
        lines,
        content_rows,
    }
}

fn commit_detail_block() -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(crate::ui::BORDER))
        .style(Style::default().bg(crate::ui::BASE))
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

/// The one column every right-pinned label in the sidebar keeps off the
/// divider (see `render_sidebar`'s `Padding::new(1, 0, 0, 0)`).
/// One space's header row in the sidebar tree — its label, or its root once
/// the `⇄` behind it is clicked (never both: see
/// `WorkspaceModel::roots_shown`) — dim for every space,
/// active one included: the space is a container, not the thing the
/// operator is looking at, so bold is reserved for the agent tab actually
/// receiving keystrokes (see `render_sidebar`'s agent-row `label_style`).
/// The active space's whole envelope (this header plus every tab/detail/cwd
/// row nested under it — see the `is_active_space` fill in
/// [`render_sidebar`]) gets a neutral background instead of a left accent
/// bar, so the highlight reads as "this whole block is where you are"
/// rather than a thin per-row marker or an on-brand "selected" tint
/// (deliberately not `SELECTED_BG` — that one borrows the accent hue for a
/// different kind of selection). This header row itself stays at the
/// lighter [`crate::ui::SURFACE_OVERLAY`] while the rows it anchors go one
/// step darker, [`crate::ui::ACTIVE_SPACE_OVERLAY`] — the title lifts
/// slightly above the block it names instead of blending into it. Its own
/// small function (unlike the tab row, which stays inline in
/// [`render_sidebar`]) purely to keep that function's now-nested loop
/// readable — this has no reuse motivation beyond that.
pub(super) fn render_space_header(
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
    // Never bright, never bold, selected or not — the background fill
    // below already carries "this is where you are"; the label itself
    // stays out of the way of the agent name bolded underneath it.
    let label_style = Style::default().fg(crate::ui::NAV_INACTIVE);
    let mut spans = vec![Span::raw(" ")];
    match renaming_this {
        Some(buffer) => spans.push(Span::styled(
            format!("{buffer}▏"),
            Style::default()
                .fg(crate::ui::TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        )),
        None => {
            // The label is what the space is called; the root is where its
            // work lives. One of them at a time — the row is one line wide
            // and a path is the one thing on it that can be any length —
            // with the toggle behind the text as the way to the other. The
            // root in the dimmest text, no brackets: it only says where.
            // Not while renaming: the buffer being typed is the only thing
            // that row should say.
            if model.roots_shown.contains(&space.id) {
                spans.push(Span::styled(
                    crate::ui::display_project_path(&space.root),
                    Style::default().fg(crate::ui::TEXT_DIM),
                ));
            } else {
                spans.push(Span::styled(space.label.clone(), label_style));
            }
            push_root_toggle(&mut spans, hits, rect, space.id);
        }
    }
    if selected {
        fill_row_bg(&mut spans, rect.width, crate::ui::SURFACE_OVERLAY);
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), rect);
    hits.push((rect, WorkspaceHit::SelectSpace(space.id)));
}

/// Appends the `⇄` to a space header, pinned to the row's right edge — the
/// same column the agent rows below pin their harness alias to (see
/// `render_sidebar`'s `TRAILING_PAD`), so the sidebar's right-hand column
/// stays one column — and makes that one cell the click target flipping
/// the row between label and root. Readable, not faint: it is a control,
/// not a tree-prefix glyph. Pushed before the row's own `SelectSpace` hit,
/// since the click search takes the first rect it lands in (same rule as
/// [`push_trailing_mark`]).
fn push_root_toggle(
    spans: &mut Vec<Span<'_>>,
    hits: &mut Vec<(Rect, WorkspaceHit)>,
    rect: Rect,
    space: SpaceId,
) {
    const TRAILING_PAD: u16 = 1;
    let used: u16 = spans.iter().map(|span| span.width() as u16).sum::<u16>() + 1 + TRAILING_PAD;
    let Some(gap) = rect.width.checked_sub(used) else {
        return;
    };
    let toggle_x = rect.right() - 1 - TRAILING_PAD;
    spans.push(Span::raw(" ".repeat(gap as usize)));
    spans.push(Span::styled(
        "⇄",
        Style::default().fg(crate::ui::TEXT_SECONDARY),
    ));
    spans.push(Span::raw(" ".repeat(TRAILING_PAD as usize)));
    hits.push((
        Rect::new(toggle_x, rect.y, 1, 1),
        WorkspaceHit::ToggleSpaceRoot(space),
    ));
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
/// three sharing `TEXT_DIM`: color is what tells these apart at a glance,
/// the glyph is what tells them apart once you look. `Ready` deliberately
/// does *not* reuse `✓` — that is `AgentTabStatus::Completed`'s glyph one
/// column to the left, and the same mark in the same accent meaning two
/// different things is what made the second column read as an echo of the
/// first. It wears the `⇧` of the delivery button it enables instead.
/// [`render_status_catalog`] is this table's legend and must move with it.
pub(super) fn task_mark(state: &TaskStateView) -> Option<(&'static str, Color)> {
    match state {
        // Nothing to report, and for the same reason: a task that has not
        // committed yet and one whose agent left with nothing both hold
        // no work. `Closed` in particular must not wear `Integrated`'s
        // arrow — that arrow claims a delivery.
        TaskStateView::Running | TaskStateView::Closed => None,
        TaskStateView::Uncommitted => Some(("±", crate::ui::BLUE)),
        TaskStateView::Ready => Some(("⇧", crate::ui::ACCENT)),
        TaskStateView::Integrating => Some(("…", crate::ui::CYAN)),
        // Split, where one `⚠` used to cover both: a paused rebase wants
        // your hands in the slot, a failed gate wants the code fixed —
        // different work, and the sidebar was the one surface that never
        // said which (the strip's own button already did).
        TaskStateView::Conflicted { .. } => Some(("!", crate::ui::WARNING)),
        TaskStateView::GateFailed => Some(("×", crate::ui::DANGER)),
        TaskStateView::Integrated => Some(("↑", crate::ui::VIOLET)),
        TaskStateView::Parked => Some(("≡", crate::ui::MUTED)),
    }
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
    // The agent column answers "what is the process doing", the task
    // column "what does its branch hold" — two questions about the same
    // row, which is exactly why they are two columns and why one legend
    // has to carry both.
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
            TaskStateView::Uncommitted,
            "uncommitted",
            "changes in the slot, not committed",
        ),
        (
            TaskStateView::Ready,
            "ready",
            "commits ahead on a clean tree — deliverable",
        ),
        (
            TaskStateView::Integrating,
            "delivering",
            "delivery in progress",
        ),
        (
            TaskStateView::Conflicted { files: Vec::new() },
            "conflict",
            "the rebase stopped; resolve it in the slot",
        ),
        (
            TaskStateView::GateFailed,
            "checks failed",
            "the gate failed on the rebased commits",
        ),
        (
            TaskStateView::Integrated,
            "delivered",
            "the work is in the target",
        ),
        (
            TaskStateView::Parked,
            "parked",
            "no agent left; the slot still holds work",
        ),
    ]
    .into_iter()
    .filter_map(|(state, name, meaning)| {
        let (mark, hue) = task_mark(&state)?;
        Some((mark.to_owned(), hue, name, meaning))
    })
    .collect();

    const H_PAD: u16 = 1;
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
                Style::default().fg(crate::ui::MUTED),
            )));
            for (glyph, hue, name, meaning) in rows {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("{glyph:<GLYPH_COLUMN$}"),
                        Style::default().fg(*hue).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{name:<name_column$}  "),
                        Style::default().fg(crate::ui::TEXT_PRIMARY),
                    ),
                    Span::styled(
                        (*meaning).to_owned(),
                        Style::default().fg(crate::ui::TEXT_SECONDARY),
                    ),
                ]));
            }
        };
    section("AGENT", &agent_rows, &mut lines);
    lines.push(Line::from(""));
    section("TASK", &task_rows, &mut lines);

    let width = (content_width + 2 * H_PAD + 2).min(area.width);
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
    let block = Block::default()
        .title(" status ")
        .title_style(
            Style::default()
                .fg(crate::ui::ACCENT)
                .add_modifier(Modifier::BOLD),
        )
        .borders(Borders::ALL)
        .border_style(Style::default().fg(crate::ui::BORDER))
        .padding(Padding::new(H_PAD, H_PAD, 0, 0))
        .style(Style::default().bg(crate::ui::BASE));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    frame.render_widget(Paragraph::new(lines), inner);
}

/// The branch an agent outside any slot is on, as its evaluation last
/// read it. `None` inside a slot: the branch there belongs to the task,
/// and the key an unslotted directory is evaluated under is its own path.
fn unisolated_branch(model: &WorkspaceModel, cwd: &Path) -> Option<String> {
    if !is_unisolated(cwd) {
        return None;
    }
    model.branches.get(&evaluation_key(cwd)).cloned()
}

/// What a pull and a push would move for an agent outside any slot, when
/// its branch is the delivery target and something is due either way:
/// `⇣₁` in the danger hue for what is to pull, `⇡₂` in the success hue
/// for what is to push, each only while its count is non-zero — the
/// shape a shell prompt gives the same fact, the count in subscript so
/// the arrow leads. No word: the two colours say which is which. Empty
/// inside a slot, on any other branch, without an upstream, or in sync —
/// a caption that says "nothing to do" says it best by saying nothing.
fn unisolated_sync_caption(model: &WorkspaceModel, cwd: &Path) -> Vec<(String, Color)> {
    if !is_unisolated(cwd) {
        return Vec::new();
    }
    let Some(sync) = model.upstream_syncs.get(&evaluation_key(cwd)) else {
        return Vec::new();
    };
    [
        ('\u{21e3}', sync.pull, crate::ui::DANGER),
        ('\u{21e1}', sync.push, crate::ui::SUCCESS),
    ]
    .into_iter()
    .filter(|(_, count, _)| *count > 0)
    .map(|(arrow, count, hue)| (format!("{arrow}{}", crate::ui::small_digits(count)), hue))
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

/// The "+ new" prompt and the directories it currently matches, drawn as
/// rows of the sidebar itself rather than a floating popup: the prompt is
/// choosing where the next space in this very list goes. It stands where
/// the first space's header stands, with the listing directly under it the
/// way a space's tabs sit under theirs.
fn render_root_picker(
    frame: &mut ratatui::Frame<'_>,
    picker: &RootPicker,
    rows: &mut Rows,
    hits: &mut Vec<(Rect, WorkspaceHit)>,
) {
    if let Some(rect) = rows.next(1) {
        // The typed segment is what the listing below is matching on, so
        // it reads as the query it is — bright against the dim directory
        // it is searching.
        let (directory, needle) = picker
            .input()
            .rfind('/')
            .map_or(("", picker.input()), |separator| {
                picker.input().split_at(separator + 1)
            });
        let mut spans = vec![
            Span::styled(" at ", Style::default().fg(crate::ui::MUTED)),
            Span::styled(
                // What is being typed must stay visible in a column this
                // narrow, so the directory in front of it is the part that
                // gives way.
                elide_head(
                    directory,
                    (rect.width as usize).saturating_sub(" at ".len() + needle.chars().count() + 1),
                ),
                Style::default().fg(crate::ui::TEXT_DIM),
            ),
            Span::styled(
                format!("{needle}▏"),
                Style::default()
                    .fg(crate::ui::TEXT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
        ];
        fill_row_bg(&mut spans, rect.width, crate::ui::SURFACE_OVERLAY);
        frame.render_widget(Paragraph::new(Line::from(spans)), rect);
    }
    if picker.match_count() == 0 {
        if let Some(rect) = rows.next(1) {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    "    no directory matches",
                    Style::default().fg(crate::ui::TEXT_FAINT),
                )),
                rect,
            );
        }
        return;
    }
    // The picker has the column to itself, so it offers as many
    // directories as the column has rows — one held back for the tail that
    // says how many more there are.
    let visible = usize::from(rows.remaining()).saturating_sub(1).max(1);
    let start = picker.window_start(visible);
    for (index, candidate) in picker.matches().enumerate().skip(start).take(visible) {
        let Some(rect) = rows.next(1) else { return };
        let selected = index == picker.selected();
        let mut spans = vec![
            Span::styled(
                if selected { "  › " } else { "    " },
                Style::default().fg(crate::ui::ACCENT),
            ),
            Span::styled(
                candidate.name.clone(),
                Style::default().fg(if selected {
                    crate::ui::TEXT_BRIGHT
                } else {
                    crate::ui::NAV_INACTIVE
                }),
            ),
        ];
        if selected {
            fill_row_bg(&mut spans, rect.width, crate::ui::SURFACE_OVERLAY);
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), rect);
        hits.push((rect, WorkspaceHit::PickSpaceRoot(index)));
    }
    let hidden = picker.match_count().saturating_sub(start + visible);
    if hidden > 0
        && let Some(rect) = rows.next(1)
    {
        frame.render_widget(
            Paragraph::new(Span::styled(
                format!("    +{hidden} more"),
                Style::default().fg(crate::ui::TEXT_FAINT),
            )),
            rect,
        );
    }
}

/// The header's delivery button for a task: text, hue, and whether it is a
/// button at all rather than a state the header only reports.
fn deliver_button(task: &TaskView) -> Option<(String, Color, bool)> {
    match &task.state {
        TaskStateView::Ready => Some((format!("⇧{}", task.ahead), crate::ui::ACCENT, true)),
        // The hue is the state's own (see `task_mark`), not the button's
        // mood: one meaning, one color, wherever the state is drawn.
        TaskStateView::GateFailed => Some(("⇧ retry".to_owned(), crate::ui::DANGER, true)),
        TaskStateView::Conflicted { .. } => {
            Some(("! conflict".to_owned(), crate::ui::WARNING, false))
        }
        TaskStateView::Integrating => Some(("… delivering".to_owned(), crate::ui::CYAN, false)),
        _ => None,
    }
}

/// One line over the pane's bottom row — the fallback for a notice that
/// cannot be pinned to the selected tab's own header (see
/// `WorkspaceModel::notice_for_footer`): a workspace-wide message, or one
/// about a task that is not what is currently on screen.
fn render_notice(frame: &mut ratatui::Frame<'_>, pane: Rect, text: &str) {
    if pane.height == 0 {
        return;
    }
    let row = Rect::new(pane.x, pane.bottom().saturating_sub(1), pane.width, 1);
    let mut spans = vec![Span::styled(
        format!(" {text}"),
        Style::default().fg(crate::ui::TEXT_BRIGHT),
    )];
    fill_row_bg(&mut spans, row.width, crate::ui::SURFACE_OVERLAY_BRIGHT);
    frame.render_widget(Clear, row);
    frame.render_widget(Paragraph::new(Line::from(spans)), row);
}

/// The preserved-work list: every task holding work that no live tab is in
/// front of, with the keys that move it on. Discard asks twice.
pub(super) fn render_preserved(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &WorkspaceModel,
    overlay: &PreservedOverlay,
) {
    const H_PAD: u16 = 2;
    let preserved = model.preserved_tasks();
    let mut lines = vec![Line::from(Span::styled(
        "PRESERVED WORK",
        Style::default().fg(crate::ui::MUTED),
    ))];
    if preserved.is_empty() {
        lines.push(Line::from(Span::styled(
            "nothing preserved — every task is either live or delivered",
            Style::default().fg(crate::ui::TEXT_SECONDARY),
        )));
    }
    for (index, (_, task)) in preserved.iter().enumerate() {
        let selected = index == overlay.selected;
        let (mark, hue) = task_mark(&task.state).unwrap_or(("·", crate::ui::TEXT_DIM));
        let what = match &task.state {
            TaskStateView::Ready => format!(
                "{} commit{}, not delivered",
                task.ahead,
                if task.ahead == 1 { "" } else { "s" }
            ),
            TaskStateView::Parked if task.checkout.is_none() => format!(
                "checkout removed, {} commit{} kept",
                task.ahead,
                if task.ahead == 1 { "" } else { "s" }
            ),
            TaskStateView::Uncommitted | TaskStateView::Parked => "uncommitted changes".to_owned(),
            TaskStateView::Conflicted { files } => format!(
                "conflict in {} file{}",
                files.len(),
                if files.len() == 1 { "" } else { "s" }
            ),
            TaskStateView::GateFailed => "checks failed".to_owned(),
            TaskStateView::Running => "no commits yet".to_owned(),
            TaskStateView::Integrating => "delivering".to_owned(),
            TaskStateView::Integrated => "delivered".to_owned(),
            TaskStateView::Closed => "nothing to deliver".to_owned(),
        };
        let mut spans = vec![
            Span::styled(
                if selected { "▸ " } else { "  " },
                Style::default().fg(crate::ui::ACCENT),
            ),
            Span::styled(format!("{mark} "), Style::default().fg(hue)),
            Span::styled(
                task.label.clone(),
                Style::default().fg(if selected {
                    crate::ui::TEXT_BRIGHT
                } else {
                    crate::ui::TEXT_PRIMARY
                }),
            ),
            Span::styled(
                format!("  {what}"),
                Style::default().fg(crate::ui::TEXT_SECONDARY),
            ),
        ];
        if selected {
            fill_row_bg(&mut spans, area.width, crate::ui::SELECTED_BG);
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        if overlay.confirm_discard {
            "discard this task and its branch?  [y] yes   [any other key] no"
        } else {
            "[r] resume   [i] deliver   [f] mark done   [d] discard   [esc] close"
        },
        Style::default().fg(if overlay.confirm_discard {
            crate::ui::WARNING
        } else {
            crate::ui::MUTED
        }),
    )));
    let content = lines.iter().map(Line::width).max().unwrap_or(0) as u16;
    let width = (content + 2 + 2 * H_PAD).min(area.width).max(1);
    let height = (lines.len() as u16 + 2).min(area.height).max(1);
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 3,
        width,
        height,
    );
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(crate::ui::BORDER))
        .style(Style::default().bg(crate::ui::BASE))
        .padding(Padding::new(H_PAD, H_PAD, 0, 0));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    frame.render_widget(Paragraph::new(lines), inner);
}

pub(super) fn agent_activity_frame(tick: usize) -> &'static str {
    AGENT_ACTIVITY_FRAMES[tick % AGENT_ACTIVITY_FRAMES.len()]
}

/// The horizontal tab strip above the pane: the *selected space's* shell
/// tabs only — agent tabs live exclusively in the sidebar now (see
/// [`render_sidebar`]), so a tab [`agent_identity_for_tab`] recognizes
/// never appears here, the same way a shell tab never appears in the
/// sidebar; other spaces' shell tabs don't appear here either, only the
/// currently selected space's. An active-tab marker in `ACCENT`/bold-bright
/// text, wrapped in the same neutral [`crate::ui::SURFACE_OVERLAY`] chip the
/// sidebar already uses for "this is where you are" (its active space's
/// envelope, its agent tab rows) — this strip used to skip that fill and
/// lean on text weight alone, which read as a lighter kind of "selected"
/// than everywhere else in the TUI. A dim `×` close affordance per tab once
/// more than one exists in the selected space, and trailing "+"/"✦" actions
/// to open another of either kind (both land in the selected space).
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
    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(crate::ui::BORDER_FAINT))
        .padding(Padding::new(0, 1, 0, 0));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(session) = &model.session else {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "connecting…",
                Style::default().fg(crate::ui::MUTED),
            )),
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
    let strip: Vec<&Tab> = context
        .and_then(|agent| space.tabs.iter().find(|tab| tab.id == agent))
        .into_iter()
        .chain(space.tabs.iter().filter(|tab| {
            agent_identity_for_tab(identities, tab).is_none() && tab.agent == context
        }))
        .collect();
    // Closability is a per-space rule (the server refuses to remove a
    // space's only tab — see `Session::remove_tab`), so it's judged
    // against every tab in the selected space, not just the ones this
    // strip goes on to show.
    let can_close = space.tabs.len() > 1;
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
        if x >= inner.right() {
            break;
        }
        let is_last = strip_index + 1 == strip_len;
        let is_agent = Some(tab.id) == context;
        let selected = tab.id == space.selected_tab;
        let marker_fg = if selected {
            crate::ui::ACCENT
        } else {
            crate::ui::TEXT_FAINT
        };
        let label_style = if selected {
            Style::default()
                .fg(crate::ui::TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(crate::ui::NAV_INACTIVE)
        };
        let marker = Span::styled(
            // The agent leading the strip wears the same "✦" the button
            // that creates one does, so the first chip reads as the agent
            // this context is about rather than another shell.
            match (is_agent, selected) {
                (true, _) => "✦ ",
                (false, true) => "● ",
                (false, false) => "○ ",
            },
            Style::default().fg(if is_agent && !selected {
                crate::ui::NAV_INACTIVE
            } else {
                marker_fg
            }),
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
                    .fg(crate::ui::TEXT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
            // One name per agent across the whole frame: the tab's own
            // label, which is what the sidebar draws and what renaming
            // edits. A working agent's task carries a label of its own
            // (the prompt's slug, or the bare task identifier when it has
            // no prompt) — showing that here left the same agent reading
            // as "engineer" in the sidebar and "gic3jz" up top.
            None => Span::styled(tab.label.clone(), label_style),
        };
        // An agent is never closed by a stray click — that stays a
        // right-click and a confirmation in the sidebar (see `ContextMenu`),
        // the same rule that keeps the sidebar's own agent rows unclosable.
        let show_close = renaming_this.is_none() && can_close && !is_agent;
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
            chip.push(Span::styled("×", Style::default().fg(crate::ui::TEXT_DIM)));
            hits.push((
                Rect::new(chip_start + PAD + content_width - 1, inner.y, 1, 1),
                WorkspaceHit::CloseTab(tab.id),
            ));
        }
        chip.push(Span::raw(" "));
        if selected {
            fill_row_bg(&mut chip, chip_width, crate::ui::SURFACE_OVERLAY);
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
        spans.push(Span::styled("/", Style::default().fg(crate::ui::MUTED)));
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
                    .fg(crate::ui::NAV_INACTIVE)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled("│", Style::default().fg(crate::ui::BORDER_FAINT)),
            Span::raw(" "),
            Span::styled("✦", Style::default().fg(crate::ui::ACCENT)),
            Span::raw(" "),
        ];
        hits.push((Rect::new(action_start, inner.y, 3, 1), WorkspaceHit::NewTab));
        hits.push((
            Rect::new(action_start + 4, inner.y, 3, 1),
            WorkspaceHit::NewAgentMenu,
        ));
        fill_row_bg(
            &mut actions,
            button_width,
            crate::ui::SURFACE_OVERLAY_BRIGHT,
        );
        spans.extend(actions);
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
    if let Some(rect) = drop_indicator {
        frame.render_widget(
            Paragraph::new("▍").style(Style::default().fg(crate::ui::ACCENT)),
            rect,
        );
    }

    // The status badge belongs to the active agent/shell tab's `cwd`, not
    // the workspace root. It is intentionally absent for a clean directory
    // or one outside Git; when it is present, it remains the entry point to
    // the full changes overlay. Unlike the "+"/"✦" pair above, this button
    // is a bare icon with no filled chip behind it — it sits directly on
    // the plain backdrop.
    let mut trailing_right = inner.right();
    if selected_agent_context(model, identities).is_some() {
        let button = vec![Span::styled(
            "✦",
            Style::default()
                .fg(crate::ui::ACCENT)
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
    // What last happened to a delivery sits right where its trigger does:
    // the tab already says whose agent this is, so nothing here repeats
    // the label the footer needs when the task in question is off screen
    // (see `WorkspaceModel::notice_for_tab`/`notice_for_footer`). A fresh
    // notice takes this spot over the button itself — the outcome is more
    // worth the operator's eye than a button an evaluation tick hasn't
    // caught up to retiring yet — and gives it back once the notice ages
    // out or a new one replaces it.
    if let Some(tab) = model.selected_tab()
        && let Some(detail) = model.notice_for_tab(tab)
    {
        let mut chip = vec![
            Span::raw(" "),
            Span::styled(
                detail.to_owned(),
                Style::default()
                    .fg(crate::ui::TEXT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ];
        let chip_width = chip.iter().map(Span::width).sum::<usize>() as u16;
        fill_row_bg(&mut chip, chip_width, crate::ui::SURFACE_OVERLAY_BRIGHT);
        let chip_rect = Rect::new(
            trailing_right.saturating_sub(chip_width),
            inner.y,
            chip_width,
            1,
        );
        frame.render_widget(Paragraph::new(Line::from(chip)), chip_rect);
        trailing_right = chip_rect.x.saturating_sub(1);
    }
    // Otherwise, one verb — deliver — whose ending is the project's
    // completion, not a choice made here. Conditioned, not disabled: when
    // the task cannot be delivered the button is absent, and the sidebar
    // mark says why.
    else if let Some(tab) = model.selected_tab()
        && let Some(task) = model.tab_task(tab)
        && let Some((text, hue, clickable)) = deliver_button(task)
    {
        let button = vec![Span::styled(
            text,
            Style::default().fg(hue).add_modifier(Modifier::BOLD),
        )];
        let button_width = button.iter().map(Span::width).sum::<usize>() as u16;
        let button_rect = Rect::new(
            trailing_right.saturating_sub(button_width),
            inner.y,
            button_width,
            1,
        );
        frame.render_widget(Paragraph::new(Line::from(button)), button_rect);
        if clickable {
            hits.push((button_rect, WorkspaceHit::Deliver(tab)));
        }
        trailing_right = button_rect.x.saturating_sub(1);
    }
    if let Some(summary) = model.git_badge.as_ref().and_then(|badge| badge.summary) {
        let mut badge = vec![
            Span::raw(" "),
            Span::styled(
                format!("+{}", summary.additions),
                Style::default().fg(crate::ui::SUCCESS),
            ),
            Span::raw(" "),
            Span::styled(
                format!("-{}", summary.deletions),
                Style::default().fg(crate::ui::DANGER),
            ),
            Span::raw(" "),
        ];
        let badge_width = badge.iter().map(Span::width).sum::<usize>() as u16;
        fill_row_bg(&mut badge, badge_width, crate::ui::SURFACE_OVERLAY);
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

pub(super) fn render_pane(frame: &mut ratatui::Frame<'_>, area: Rect, model: &WorkspaceModel) {
    let Some(snapshot) = model.panes.get(&model.focused_pane()) else {
        frame.render_widget(
            Paragraph::new(model.error.as_deref().unwrap_or(" starting shell…"))
                .style(Style::default().fg(crate::ui::MUTED)),
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
            .set_style(
                Style::default()
                    .bg(crate::ui::TEXT_BRIGHT)
                    .fg(crate::ui::BASE),
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
        TerminalColor::DefaultForeground => crate::ui::TEXT_PRIMARY,
        TerminalColor::DefaultBackground => crate::ui::BASE,
        TerminalColor::Rgb { red, green, blue } => Color::Rgb(red, green, blue),
        TerminalColor::Indexed(index) => Color::Indexed(index),
    }
}
