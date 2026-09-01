//! Everything the workspace client draws.
//!
//! Split out of `orchestrator.rs`, which had grown to 3.5k lines covering
//! three unrelated jobs: driving the session, drawing it, and encoding input
//! for the PTY. Nothing here mutates session state — these take a
//! `&WorkspaceModel` and paint it, which is what makes them one module.

use super::*;

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

pub(super) fn render(
    frame: &mut ratatui::Frame<'_>,
    model: &WorkspaceModel,
    identities: &[AgentIdentity],
    hits: &mut Vec<(Rect, WorkspaceHit)>,
) {
    frame.render_widget(
        Block::default().style(
            Style::default()
                .bg(crate::ui::BASE)
                .fg(crate::ui::TEXT_PRIMARY),
        ),
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
        && let Some(resolution) = &model.agent_support
        && resolution.key == dropdown.key
        && let Some(support) = &resolution.support
    {
        crate::ui::agent_support::render(frame, frame.area(), dropdown.anchor, support);
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
pub(super) fn render_sidebar(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &WorkspaceModel,
    identities: &[AgentIdentity],
    hits: &mut Vec<(Rect, WorkspaceHit)>,
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
    // match the tab strip's height on the other TUI mode — a centered
    // segmented control stands in for the Ctrl+O keybinding.
    if let Some(rect) = row(1) {
        let (_work_rect, manage_rect) = crate::ui::render_mode_toggle(frame, rect, true);
        hits.push((manage_rect, WorkspaceHit::SwitchToManagement));
    }
    if let Some(error) = &model.error
        && let Some(rect) = row(1)
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
    if let Some(rect) = row(1) {
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
    if let Some(rect) = row(1) {
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
                    .map(|pane| crate::ui::display_project_path(&pane.cwd))
                    .unwrap_or_default();
                let mut spans = vec![Span::styled(
                    format!("  {cwd}"),
                    Style::default().fg(crate::ui::TEXT_DIM),
                )];
                if is_active_space {
                    fill_row_bg(&mut spans, cwd_rect.width, crate::ui::SURFACE_OVERLAY);
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
                crate::ui::ACCENT
            } else {
                crate::ui::TEXT_FAINT
            };
            // Bright but not bold — bold is the space header's own marker
            // for "this is the active space" (see `render_space_header`);
            // an agent tab nested under it competing for the same weight
            // read as two different things both shouting "I'm the one".
            // The `●` dot above already says which tab is active.
            let label_style = Style::default().fg(if selected {
                crate::ui::TEXT_BRIGHT
            } else {
                crate::ui::NAV_INACTIVE
            });
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
            let mut spans = vec![
                connector_span,
                Span::styled(indicator, Style::default().fg(indicator_fg)),
                label,
            ];
            if is_active_space {
                fill_row_bg(&mut spans, label_rect.width, crate::ui::SURFACE_OVERLAY);
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
                    .map(|pane| crate::ui::display_project_path(&pane.cwd))
                    .unwrap_or_default();
                let continuation_span =
                    Span::styled(continuation, Style::default().fg(crate::ui::TEXT_FAINT));
                let cwd_span = Span::styled(cwd, Style::default().fg(crate::ui::TEXT_DIM));
                let alias_span = Span::styled(alias, Style::default().fg(crate::ui::TEXT_DIM));
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
                    fill_row_bg(&mut spans, detail_rect.width, crate::ui::SURFACE_OVERLAY);
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
/// [`crate::ui::SURFACE_OVERLAY`] background instead of a left accent bar, so
/// the highlight reads as "this whole block is where you are" rather than
/// a thin per-row marker or an on-brand "selected" tint (deliberately not
/// `SELECTED_BG` — that one borrows the accent hue for a different kind of
/// selection). Its own small function (unlike the tab row, which stays
/// inline in [`render_sidebar`]) purely to keep that function's now-nested
/// loop readable — this has no reuse motivation beyond that.
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
    let mut label_style = Style::default().fg(if selected {
        crate::ui::TEXT_BRIGHT
    } else {
        crate::ui::NAV_INACTIVE
    });
    if selected {
        label_style = label_style.add_modifier(Modifier::BOLD);
    }
    let label = match renaming_this {
        Some(buffer) => Span::styled(
            format!(" {buffer}▏"),
            Style::default()
                .fg(crate::ui::TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ),
        None => Span::styled(format!(" {}", space.label), label_style),
    };
    let mut spans = vec![label];
    if selected {
        fill_row_bg(&mut spans, rect.width, crate::ui::SURFACE_OVERLAY);
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), rect);
    hits.push((rect, WorkspaceHit::SelectSpace(space.id)));
}

pub(super) fn agent_activity_frame(tick: usize) -> &'static str {
    AGENT_ACTIVITY_FRAMES[tick % AGENT_ACTIVITY_FRAMES.len()]
}

/// Stamps `bg` onto every span already in the row, then appends a
/// trailing background-filled run of spaces so the highlight spans the
/// row's full width instead of stopping at the last glyph — same pattern
/// the management views' `render_plugin_row`/`header_line` use for their
/// own selected-row backgrounds.
pub(super) fn fill_row_bg<'a>(spans: &mut Vec<Span<'a>>, width: u16, bg: Color) {
    for span in spans.iter_mut() {
        span.style = span.style.bg(bg);
    }
    let used: usize = spans.iter().map(Span::width).sum();
    let gap = (width as usize).saturating_sub(used);
    spans.push(Span::styled(" ".repeat(gap), Style::default().bg(bg)));
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
                    .fg(crate::ui::TEXT_BRIGHT)
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
