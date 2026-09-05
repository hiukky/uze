//! TUI view — Plugins route.
//!
//! The agentic side of the product: skills, agents, MCP — everything
//! installable from a marketplace. Rendered as a tree: each marketplace is
//! a collapsible group header (click the chevron to expand/collapse), with
//! its plugins indented underneath using `├─`/`└─` prefixes. The embedded
//! `uze-official` snapshot is one group like any other (badged ✓ Official),
//! and ad-hoc installed plugins (`uze add` from a path/Git URL, which no
//! catalog knows about) close the tree as a "local" group — so a direct
//! install never disappears from the TUI. Rows carry their lifecycle
//! status (Installed/Available/Update available) and the full
//! install/update/remove surface: `i` install, `u` update, `r` remove, `a`
//! add marketplace, `/` filter. A live filter box narrows both by plugin
//! and by marketplace name. Selection indexes the *visible* sequence
//! (`TuiModel::marketplace_visible_indices`) — headers, spacers, and
//! collapsed/filtered-out plugins are a pure rendering/navigation concern
//! layered on top of the flat, already-grouped `marketplace_rows` Vec. The
//! detail drawer overlays the list from the right with a draggable left
//! edge — see `TuiModel::marketplace_drawer_open`.

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use uze_application::application::{DoctorReport, MarketplacePluginSummary};

use super::super::hit::Hit;
use super::super::model::{ResizablePanel, TuiModel};
use super::super::route_style;
use super::super::{content_area, render_screen_header, side_panel_area};
use super::{render_status_line, resource_summary};
use crate::ui::theme::{self, Symbol, Token};

/// Both status labels are 9 characters (`Installed`/`Available`), but that's
/// incidental — pad explicitly so alignment holds even if a future status
/// label changes length.
const STATUS_WIDTH: usize = 9;

/// One line of the rendered tree. Only `Plugin` is selectable/clickable;
/// `Header` toggles its group's collapse state; `Spacer` is a blank gap
/// between groups — siblings within one group stay directly adjacent, no
/// gap at all, so the tree reads as tight and continuous.
enum Row {
    Header {
        marketplace: String,
        collapsed: bool,
        all_installed: bool,
        is_official: bool,
    },
    Spacer,
    /// `position` is an index in the *visible* sequence, not a raw index
    /// into `marketplace_rows` — see `TuiModel::marketplace_visible_indices`.
    /// `is_last` picks `└─` vs `├─` among this group's *visible* siblings.
    Plugin {
        position: usize,
        plugin: MarketplacePluginSummary,
        is_last: bool,
    },
}

fn build_rows(model: &TuiModel) -> Vec<Row> {
    let visible = model.marketplace_visible_indices();
    let position_of: std::collections::HashMap<usize, usize> = visible
        .iter()
        .enumerate()
        .map(|(position, &raw)| (raw, position))
        .collect();
    let filtering_active = !model.marketplace_filter.trim().is_empty();

    // Consecutive-run grouping: `marketplace_rows` already emits
    // official-first, then each registered marketplace's plugins, then the
    // local group, so adjacent-match grouping preserves that order without
    // re-sorting.
    let mut groups: Vec<(String, Vec<(usize, MarketplacePluginSummary)>)> = Vec::new();
    for (raw, plugin) in model.marketplace_rows().into_iter().enumerate() {
        match groups.last_mut() {
            Some((name, items)) if *name == plugin.marketplace => {
                items.push((raw, plugin));
            }
            _ => groups.push((plugin.marketplace.clone(), vec![(raw, plugin)])),
        }
    }

    // A blank spacer row follows every plugin row (and a childless header),
    // matching the design's own per-row vertical padding — without it,
    // rows/groups read as glued directly to their badges/siblings with no
    // breathing room at all.
    let mut rows = Vec::new();
    for (marketplace, items) in &groups {
        let visible_items: Vec<&(usize, MarketplacePluginSummary)> = items
            .iter()
            .filter(|(raw, _)| position_of.contains_key(raw))
            .collect();
        if filtering_active && visible_items.is_empty() {
            continue;
        }
        rows.push(Row::Header {
            marketplace: marketplace.clone(),
            collapsed: model.collapsed_marketplaces.contains(marketplace),
            all_installed: !items.is_empty() && items.iter().all(|(_, p)| p.installed),
            is_official: marketplace == "uze-official",
        });
        let count = visible_items.len();
        if count == 0 {
            rows.push(Row::Spacer);
            continue;
        }
        for (i, (raw, plugin)) in visible_items.into_iter().enumerate() {
            let is_last = i + 1 == count;
            rows.push(Row::Plugin {
                position: position_of[raw],
                plugin: plugin.clone(),
                is_last,
            });
            // Siblings stay directly adjacent — no gap, no connector row —
            // so the tree reads as tight and continuous. Only after the
            // group's *last* plugin (whose `└─` already closes the branch)
            // does a blank row separate it from whatever comes next.
            if is_last {
                rows.push(Row::Spacer);
            }
        }
    }
    rows
}

pub(crate) fn render_plugins(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let outer = content_area(area);
    let drawer_open =
        model.marketplace_drawer_open && model.selected_marketplace_plugin().is_some();
    let drawer_width = drawer_open.then(|| {
        model
            .marketplace_drawer_width
            .unwrap_or(super::DRAWER_DEFAULT_WIDTH)
            .clamp(24, outer.width.saturating_sub(24).max(24))
    });
    let list_area_width = outer
        .width
        .saturating_sub(drawer_width.unwrap_or(0))
        .saturating_sub(if drawer_open { 1 } else { 0 });
    let header_area = Rect::new(outer.x, outer.y, list_area_width, outer.height);
    let sources = model.marketplaces.len();
    let trailer = (sources > 0).then(|| {
        Span::styled(
            format!("{sources} source{}", if sources == 1 { "" } else { "s" }),
            theme::fg(Token::TextMuted),
        )
    });
    let content = render_screen_header(
        frame,
        header_area,
        "Plugins",
        "skills · agents · MCP",
        trailer,
    );
    let filter_area = Rect::new(content.x, content.y, content.width, 2);
    render_filter_box(frame, filter_area, model);
    let list_area = Rect::new(
        content.x,
        content.y + 3,
        content.width,
        content.height.saturating_sub(3),
    );

    let marketplace_rows = model.marketplace_rows();
    if marketplace_rows.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "No plugins available.",
                theme::fg(Token::TextMuted),
            )),
            list_area,
        );
    } else {
        let name_width = marketplace_rows
            .iter()
            .map(|plugin| plugin.name.chars().count())
            .max()
            .unwrap_or(0);
        // One shared "label column" width — headers ("{chevron} {name}")
        // and plugin rows ("  {tree-prefix}{name}") pad to the same total
        // so Status lands in the same column for every row, table-style,
        // instead of drifting with each row's own leading text length.
        let header_label_width = marketplace_rows
            .iter()
            .map(|plugin| group_display_name(&plugin.marketplace).chars().count())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .map(|display| 2 + display)
            .max()
            .unwrap_or(0);
        let plugin_label_width = 5 + name_width;
        let label_width = header_label_width.max(plugin_label_width);
        let rows = build_rows(model);
        if rows.is_empty() {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    format!("No plugins match \"{}\".", model.marketplace_filter.trim()),
                    theme::fg(Token::TextMuted),
                )),
                list_area,
            );
        } else {
            for (render_row, row) in rows.iter().enumerate() {
                let y = list_area.y + render_row as u16;
                if y >= list_area.y + list_area.height {
                    break;
                }
                let rect = Rect::new(list_area.x, y, list_area.width, 1);
                match row {
                    Row::Header {
                        marketplace,
                        collapsed,
                        all_installed,
                        is_official,
                    } => {
                        frame.render_widget(
                            Paragraph::new(header_line(
                                marketplace,
                                *collapsed,
                                *all_installed,
                                *is_official,
                                label_width,
                            )),
                            rect,
                        );
                        hits.push((rect, Hit::MarketplaceGroupToggle(marketplace.clone())));
                    }
                    Row::Spacer => {}
                    Row::Plugin {
                        position,
                        plugin,
                        is_last,
                    } => {
                        frame.render_widget(
                            Paragraph::new(plugin_line(
                                plugin,
                                *is_last,
                                *position == model.marketplace_selected,
                                model.was_just_updated(&model.marketplace_plugin_id(plugin)),
                                name_width,
                                label_width,
                                list_area.width,
                            )),
                            rect,
                        );
                        hits.push((rect, Hit::MarketplaceRow(*position)));
                    }
                }
            }
        }
    }

    if drawer_open && let Some(plugin) = model.selected_marketplace_plugin() {
        render_plugin_drawer(
            frame,
            outer,
            drawer_width.unwrap_or_default(),
            model,
            &plugin,
            hits,
        );
    }
}

/// The group name as rendered: "uze-official" reads oddly right above a
/// child plugin that's *also* named "uze" — the badge below already says
/// "Official", so the suffix is pure redundancy — and the synthetic local
/// group gets a capitalized label instead of the bare word.
fn group_display_name(marketplace: &str) -> &str {
    match marketplace {
        "uze-official" => "uze",
        "local" => "Local",
        other => other,
    }
}

fn render_filter_box(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(if model.filtering {
            theme::color(Token::Accent)
        } else {
            theme::color(Token::BorderDefault)
        }));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let text = if model.marketplace_filter.is_empty() {
        Line::from(Span::styled("Filter plugins…", theme::fg(Token::TextMuted)))
    } else {
        let mut spans = vec![Span::styled(
            model.marketplace_filter.clone(),
            theme::fg(Token::TextPrimary),
        )];
        if model.filtering {
            spans.push(Span::styled(
                theme::glyph(Symbol::BarThin),
                theme::fg(Token::Accent),
            ));
        }
        Line::from(spans)
    };
    frame.render_widget(Paragraph::new(text), inner);
}

fn header_line(
    marketplace: &str,
    collapsed: bool,
    all_installed: bool,
    is_official: bool,
    label_width: usize,
) -> Line<'static> {
    let chevron = theme::glyph(if collapsed {
        Symbol::ChevronCollapsed
    } else {
        Symbol::ChevronExpanded
    });
    // See `group_display_name` — the header shows the display name, while
    // the underlying value (used for toggling, hit-testing, filtering) is
    // untouched; this only affects what's drawn.
    let display_name = group_display_name(marketplace);
    // The name is padded to `label_width` (minus the 2-wide "{chevron} "
    // that precedes it) so "Installed" lands in the same column as every
    // plugin row's own Status, table-style, rather than trailing right
    // after however long this particular name happens to be.
    let mut spans = vec![
        Span::styled(format!("{chevron} "), theme::fg(Token::TextDim)),
        Span::styled(
            format!(
                "{:<width$}",
                display_name,
                width = label_width.saturating_sub(2)
            ),
            Style::default()
                .fg(theme::color(Token::TextBright))
                .add_modifier(Modifier::BOLD),
        ),
    ];
    if all_installed {
        spans.push(Span::raw("  "));
        spans.push(Span::styled("Installed", theme::fg(Token::Accent)));
    }
    if is_official {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("{} Official", theme::glyph(Symbol::MarkOfficial)),
            theme::fg(Token::StateInfo),
        ));
    }
    Line::from(spans)
}

/// Widest text each fixed slot after Status ever holds, so a row without
/// that badge still reserves its column and the next one doesn't drift.
const UPDATE_WIDTH: usize = "Update available".len();

fn plugin_line<'a>(
    plugin: &'a MarketplacePluginSummary,
    is_last: bool,
    selected: bool,
    just_updated: bool,
    name_width: usize,
    label_width: usize,
    row_width: u16,
) -> Line<'a> {
    let prefix = format!(
        "{} ",
        theme::glyph(if is_last {
            Symbol::TreeLast
        } else {
            Symbol::TreeBranch
        })
    );
    // This row's own leading width (border + " " + prefix + name) is
    // `5 + name_width`; when some header's name is longer, `label_width`
    // exceeds that — pad the extra into the gap before Status so it still
    // lands in the same column as every header's own badge.
    let extra_pad = label_width.saturating_sub(5 + name_width);
    let name_style = if selected {
        theme::fg(Token::TextBright)
    } else {
        theme::fg(Token::TextSecondary)
    };
    let status_style = if plugin.installed {
        theme::fg(Token::Accent)
    } else {
        theme::fg(Token::TextDim)
    };
    let name = format!("{:<name_width$}", plugin.name);
    let status = format!(
        "{:<STATUS_WIDTH$}",
        if plugin.installed {
            "Installed"
        } else {
            "Available"
        }
    );
    // "Updated" and "Update available" are the same slot and mutually
    // exclusive by construction: the badge is only ever raised by an update
    // that just landed, which is exactly what clears `update_available`.
    // The remaining "Update available" therefore always means one uze
    // declined to apply on its own — press `u`.
    let (update, update_style) = if just_updated {
        ("Updated", theme::fg(Token::Accent))
    } else if plugin.update_available == Some(true) {
        ("Update available", theme::fg(Token::StateWarning))
    } else {
        ("", Style::default())
    };
    let update = format!("{update:<UPDATE_WIDTH$}");
    let mut spans = vec![
        Span::styled(
            if selected {
                theme::glyph(Symbol::TreeColumnDivider)
            } else {
                " ".to_owned()
            },
            theme::fg(Token::Accent),
        ),
        Span::styled(format!(" {prefix}"), theme::fg(Token::TextFaint)),
        Span::styled(name, name_style),
        Span::raw(" ".repeat(2 + extra_pad)),
        Span::styled(status, status_style),
        Span::raw("  "),
        Span::styled(update, update_style),
    ];
    if selected {
        for span in &mut spans {
            span.style = span.style.bg(theme::color(Token::SurfaceSelected));
        }
        let used: usize = spans.iter().map(Span::width).sum();
        let gap = (row_width as usize).saturating_sub(used);
        spans.push(Span::styled(
            " ".repeat(gap),
            theme::bg(Token::SurfaceSelected),
        ));
    }
    Line::from(spans)
}

fn render_plugin_drawer(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    width: u16,
    model: &TuiModel,
    plugin: &MarketplacePluginSummary,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let drawer = side_panel_area(area, width);
    frame.render_widget(Clear, drawer);
    frame.render_widget(
        Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(
                if model.dragging_panel == Some(ResizablePanel::MarketplaceDrawer) {
                    theme::color(Token::Accent)
                } else {
                    theme::color(Token::SurfaceRecessed)
                },
            ))
            .style(theme::bg(Token::SurfaceRecessed)),
        drawer,
    );
    hits.insert(
        0,
        (
            Rect::new(drawer.x, drawer.y, 1, drawer.height),
            Hit::ResizePanel(ResizablePanel::MarketplaceDrawer),
        ),
    );

    let sections_x = drawer.x + 2;
    let sections_width = drawer.width.saturating_sub(3);
    let status_height = 3;
    let body = Rect::new(
        sections_x,
        drawer.y + 1,
        sections_width,
        drawer.height.saturating_sub(2 + status_height),
    );
    let status_area = Rect::new(
        sections_x,
        body.y + body.height,
        sections_width,
        status_height,
    );

    let room = body.width as usize;
    let mut lines = vec![Line::from(Span::styled(
        "PLUGIN",
        theme::fg_bold(Token::TextMuted),
    ))];
    lines.extend(fold(&plugin.name, room).into_iter().map(|row| {
        Line::from(Span::styled(
            row,
            Style::default()
                .fg(theme::color(Token::TextBright))
                .add_modifier(Modifier::BOLD),
        ))
    }));
    lines.push(Line::from(""));
    lines.extend(
        fold(plugin.description.as_deref().unwrap_or_default(), room)
            .into_iter()
            .map(|row| Line::from(Span::styled(row, theme::fg(Token::TextSecondary)))),
    );
    if !plugin.keywords.is_empty() {
        lines.extend(
            fold(&plugin.keywords.join(", "), room)
                .into_iter()
                .map(|row| Line::from(Span::styled(row, theme::fg(Token::TextDim)))),
        );
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "SOURCE",
        theme::fg_bold(Token::TextMuted),
    )));
    let source_row_y = body.y + lines.len() as u16;
    let name = group_display_name(&plugin.marketplace);
    let homepage = model
        .marketplaces
        .iter()
        .find(|entry| entry.name == plugin.marketplace)
        .and_then(|entry| entry.homepage.clone());
    // Just the name: one glyph on the card, and it belongs to the address
    // below, which is the row that leaves the application. Marking this
    // row too made the pair read as two links to the same place.
    lines.push(Line::from(Span::styled(
        name.to_owned(),
        theme::fg(Token::TextPrimary),
    )));
    if source_row_y < body.y + body.height {
        hits.push((
            Rect::new(body.x, source_row_y, body.width, 1),
            Hit::JumpToMarketplace(plugin.marketplace.clone()),
        ));
    }
    // The address itself, on a row of its own and clickable along its
    // whole length. It was a one-column "↗" beside the name to begin
    // with, which is a target you miss by moving the mouse one cell —
    // and missing it landed on the jump underneath, which re-selects the
    // group already selected and so reads as nothing happening at all.
    // Writing the address out also gives the reader something to check
    // before trusting it, and something to copy when the terminal this
    // drawer draws into has no browser to hand it to.
    if let Some(url) = homepage.as_deref() {
        let url_row_y = body.y + lines.len() as u16;
        let address_room = body.width.saturating_sub(2) as usize;
        // Muted until the pointer is on it, the way the rest of this
        // drawer's secondary text reads: an address that wore the accent
        // at rest competed with the row the reader had actually selected.
        // The underline is what says "link" while it sits quiet; the
        // accent is what answers the pointer.
        let link = if model.source_link_hovered {
            Style::default()
                .fg(theme::color(Token::Accent))
                .add_modifier(Modifier::UNDERLINED)
        } else {
            Style::default()
                .fg(theme::color(Token::TextMuted))
                .add_modifier(Modifier::UNDERLINED)
        };
        lines.push(Line::from(vec![
            // Underlined, which is what a link looks like everywhere else
            // a person reads one. The accent alone said "interactive" in
            // this palette's own vocabulary and nothing at all in anyone
            // else's — the row was a target the whole time and still read
            // as a caption.
            Span::styled(crate::ui::elide_tail(url, address_room), link),
            Span::raw(" "),
            Span::styled(theme::glyph(Symbol::ArrowExternal), link),
        ]));
        if url_row_y < body.y + body.height {
            hits.push((
                Rect::new(body.x, url_row_y, body.width, 1),
                Hit::OpenLink(plugin.marketplace.clone()),
            ));
        }
    }
    lines.push(Line::from(""));

    // Installed rows read their resources/deliveries from the installed
    // package inspection (`InspectPlugin`), available rows from the
    // catalog detail (`InspectMarketplacePlugin`) — each fetch lands in a
    // different cache, so the drawer consults whichever matches this row.
    let installed_inspection = plugin.installed.then(|| {
        let id = model.marketplace_plugin_id(plugin);
        model
            .plugin_detail
            .as_ref()
            .filter(|detail| detail.plugin.id == id)
    });
    let catalog_detail = (!plugin.installed).then(|| {
        model.marketplace_detail.as_ref().filter(|detail| {
            detail.summary.name == plugin.name && detail.summary.marketplace == plugin.marketplace
        })
    });

    lines.push(Line::from(Span::styled(
        "RESOURCES",
        theme::fg_bold(Token::TextMuted),
    )));
    let resources = installed_inspection
        .flatten()
        .map(|detail| resource_summary(&detail.capabilities))
        .or_else(|| {
            catalog_detail
                .flatten()
                .map(|detail| resource_summary(&detail.capabilities))
        })
        .unwrap_or_else(|| "loading…".to_owned());
    lines.push(Line::from(Span::styled(
        resources,
        theme::fg(Token::TextPrimary),
    )));

    if let Some(detail) = installed_inspection.flatten() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "AVAILABLE IN",
            theme::fg_bold(Token::TextMuted),
        )));
        for delivery in &detail.deliveries {
            let route = delivery
                .package_plan
                .as_ref()
                .map(package_strategy)
                .unwrap_or_else(|| {
                    delivery
                        .capabilities
                        .first()
                        .and_then(|c| c.plan.as_ref())
                        .map(exposure_route_label)
                        .unwrap_or("unsupported")
                });
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {:<12}", delivery.display_name),
                    theme::fg(Token::TextSecondary),
                ),
                Span::styled(route, route_style(route)),
            ]));
        }
        let state = &detail.managed_state;
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Managed  ", theme::fg(Token::TextMuted)),
            Span::styled(
                format!("{} matched", state.matched),
                theme::fg(Token::Accent),
            ),
            Span::styled(
                format!(
                    " · {} missing · {} drifted · {} conflicts · {} blocked",
                    state.missing, state.drifted, state.conflicts, state.blocked
                ),
                theme::fg(Token::TextMuted),
            ),
        ]));
    }

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), body);

    if plugin.installed {
        let qualified_id = model.marketplace_plugin_id(plugin);
        let health = plugin_health(model.doctor.as_ref(), &qualified_id);
        let subtitle = match health {
            "ready" => "Ready to use in your projects",
            "missing" => "Installation is missing artifacts",
            "needs attention" => "Managed state needs attention",
            _ => "Health unknown",
        };
        if model.was_just_updated(&qualified_id) {
            render_status_line(
                frame,
                status_area,
                theme::color(Token::Accent),
                "Updated",
                "Brought up to date automatically when uze started",
            );
        } else if plugin.update_available == Some(true) {
            render_status_line(
                frame,
                status_area,
                theme::color(Token::StateWarning),
                "Update available",
                "Needs your confirmation — press u to apply it",
            );
        } else {
            render_status_line(
                frame,
                status_area,
                theme::color(Token::Accent),
                "Installed",
                subtitle,
            );
        }
    } else {
        render_status_line(
            frame,
            status_area,
            theme::color(Token::TextMuted),
            "Not installed",
            "Press i to install this plugin",
        );
    }
}

/// Folds `text` to `width` the way the drawer's paragraph would, but
/// *before* it is authored — so every line the drawer pushes is one drawn
/// row, and a row index is a screen row.
///
/// The drawer renders through `Wrap`, and its two clickable rows (the
/// marketplace name, the address under it) were anchored by counting
/// authored lines. A description long enough to fold pushed the drawn rows
/// down and left the targets sitting above them: the address read as a
/// link and answered nothing. Wrapping the free text here keeps the two
/// counts the same number by construction, with no measurement of what
/// ratatui did afterwards.
fn fold(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_owned()];
    }
    let mut rows: Vec<String> = Vec::new();
    let mut row = String::new();
    for word in text.split_whitespace() {
        // A word wider than the row is broken across rows rather than
        // left to overflow — the paragraph's own wrapper does the same,
        // and a bare URL in a description is exactly that word.
        let mut word = word;
        while word.chars().count() > width {
            if !row.is_empty() {
                rows.push(std::mem::take(&mut row));
            }
            let split = word
                .char_indices()
                .nth(width)
                .map_or(word.len(), |(index, _)| index);
            let (head, tail) = word.split_at(split);
            rows.push(head.to_owned());
            word = tail;
        }
        let projected = row.chars().count() + usize::from(!row.is_empty()) + word.chars().count();
        if projected > width && !row.is_empty() {
            rows.push(std::mem::take(&mut row));
        }
        if !row.is_empty() {
            row.push(' ');
        }
        row.push_str(word);
    }
    if !row.is_empty() || rows.is_empty() {
        rows.push(row);
    }
    rows
}

fn exposure_route_label(plan: &uze_application::ExposurePlan) -> &'static str {
    match plan.route {
        uze_application::CompatibilityRoute::Native => "native",
        uze_application::CompatibilityRoute::Adaptable => "adapted",
        uze_application::CompatibilityRoute::Degraded => "degraded",
        uze_application::CompatibilityRoute::Unsupported => "unsupported",
    }
}

fn package_strategy(plan: &uze_application::PackageExposurePlan) -> &'static str {
    match plan.route {
        uze_application::CompatibilityRoute::Native => "native",
        uze_application::CompatibilityRoute::Adaptable => "adapted",
        uze_application::CompatibilityRoute::Degraded => "degraded",
        uze_application::CompatibilityRoute::Unsupported => "unsupported",
    }
}

/// Attachment health for one plugin, derived from the doctor report every
/// refresh carries — never fetched per row, so the status line always has
/// a real answer instead of a masked placeholder.
fn plugin_health(doctor: Option<&DoctorReport>, plugin: &str) -> &'static str {
    let Some(state) = doctor
        .and_then(|doctor| doctor.attachments.iter().find(|item| item.plugin == plugin))
        .map(|item| &item.state)
    else {
        return "unknown";
    };
    if state.drifted + state.conflicts + state.blocked > 0 {
        "needs attention"
    } else if state.missing > 0 {
        "missing"
    } else {
        "ready"
    }
}
