//! TUI view — Marketplace route.
//!
//! Rendered as a two-level tree: each registered marketplace is a
//! collapsible group header (click to expand/collapse), with its plugins
//! indented underneath. A live filter box narrows both by plugin and by
//! marketplace name. Selection indexes the *visible* sequence
//! (`TuiModel::marketplace_visible_indices`) — headers, spacers, and
//! collapsed/filtered-out plugins are a pure rendering/navigation concern
//! layered on top of the flat, already-grouped `marketplace_plugins` Vec.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, List, ListItem, Paragraph, Wrap},
};

use crate::application::MarketplacePluginSummary;

use super::super::hit::Hit;
use super::super::model::TuiModel;
use super::super::{ACCENT, MUTED, SELECTED_BG, SUCCESS, SURFACE_RAISED, WARNING, surface_block};
use super::{push_capability_table, render_status_card};

/// Both status labels are 9 characters (`Installed`/`Available`), but that's
/// incidental — pad explicitly so alignment holds even if a future status
/// label changes length.
const STATUS_WIDTH: usize = 9;

/// One line of the rendered tree. Only `Plugin` is selectable/clickable;
/// `Header` toggles its group's collapse state; `Spacer` is pure air.
enum Row<'a> {
    Header {
        marketplace: &'a str,
        collapsed: bool,
        all_installed: bool,
        is_official: bool,
    },
    Spacer,
    /// `usize` is a position in the *visible* sequence, not a raw index
    /// into `marketplace_plugins` — see `TuiModel::marketplace_visible_indices`.
    Plugin(usize, &'a MarketplacePluginSummary),
}

/// Groups `model.marketplace_plugins` for display: every marketplace still
/// gets a header (so a fully-collapsed or fully-filtered-out group doesn't
/// just vanish without a trace while filtering is inactive), but a group
/// with zero visible plugins is dropped entirely once the filter is doing
/// real work — an empty header with nothing under it is more confusing
/// than informative once you're actively narrowing the list.
fn build_rows(model: &TuiModel) -> Vec<Row<'_>> {
    let visible = model.marketplace_visible_indices();
    let position_of: std::collections::HashMap<usize, usize> = visible
        .iter()
        .enumerate()
        .map(|(position, &raw)| (raw, position))
        .collect();
    let filtering_active = !model.marketplace_filter.trim().is_empty();
    let mut has_visible: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for (raw, plugin) in model.marketplace_plugins.iter().enumerate() {
        if position_of.contains_key(&raw) {
            has_visible.insert(plugin.marketplace.as_str());
        }
    }

    let mut rows = Vec::with_capacity(model.marketplace_plugins.len() * 2);
    let mut current: Option<&str> = None;
    for (raw, plugin) in model.marketplace_plugins.iter().enumerate() {
        let marketplace = plugin.marketplace.as_str();
        if filtering_active && !has_visible.contains(marketplace) {
            continue;
        }
        if current != Some(marketplace) {
            if current.is_some() {
                rows.push(Row::Spacer);
            }
            let group: Vec<&MarketplacePluginSummary> = model
                .marketplace_plugins
                .iter()
                .filter(|p| p.marketplace == marketplace)
                .collect();
            rows.push(Row::Header {
                marketplace,
                collapsed: model.collapsed_marketplaces.contains(marketplace),
                all_installed: !group.is_empty() && group.iter().all(|p| p.installed),
                is_official: marketplace == "uze-official",
            });
            current = Some(marketplace);
        }
        if let Some(&position) = position_of.get(&raw) {
            rows.push(Row::Plugin(position, plugin));
        }
    }
    rows
}

pub(crate) fn render_marketplace(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .spacing(1)
        .constraints([Constraint::Percentage(46), Constraint::Percentage(54)])
        .split(area);

    let title = if model.marketplace_count == 0 {
        " Marketplace".to_owned()
    } else {
        format!(
            " Marketplace  ·  {} source{}",
            model.marketplace_count,
            if model.marketplace_count == 1 {
                ""
            } else {
                "s"
            }
        )
    };
    let block = surface_block(title);
    let panel_inner = block.inner(columns[0]);
    frame.render_widget(block, columns[0]);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(3)])
        .split(panel_inner);
    render_filter_box(frame, sections[0], model);
    let inner = sections[1];

    if model.marketplace_plugins.is_empty() {
        frame.render_widget(
            Paragraph::new("No marketplace plugins available.").wrap(Wrap { trim: true }),
            inner,
        );
    } else {
        let name_width = model
            .marketplace_plugins
            .iter()
            .map(|plugin| plugin.name.chars().count())
            .max()
            .unwrap_or(0);
        let rows = build_rows(model);
        if rows.is_empty() {
            frame.render_widget(
                Paragraph::new(format!(
                    "No plugins match \"{}\".",
                    model.marketplace_filter.trim()
                ))
                .wrap(Wrap { trim: true }),
                inner,
            );
        } else {
            let mut items: Vec<ListItem> = Vec::with_capacity(rows.len());
            for (render_row, row) in rows.iter().enumerate() {
                match row {
                    Row::Header {
                        marketplace,
                        collapsed,
                        all_installed,
                        is_official,
                    } => {
                        items.push(ListItem::new(header_line(
                            marketplace,
                            *collapsed,
                            *all_installed,
                            *is_official,
                        )));
                        let rect = Rect::new(inner.x, inner.y + render_row as u16, inner.width, 1);
                        if rect.y < inner.y + inner.height {
                            hits.push((
                                rect,
                                Hit::MarketplaceGroupToggle((*marketplace).to_owned()),
                            ));
                        }
                    }
                    Row::Spacer => items.push(ListItem::new(Line::from(""))),
                    Row::Plugin(position, plugin) => {
                        items.push(plugin_row(
                            plugin,
                            *position == model.marketplace_selected,
                            name_width,
                            inner.width,
                        ));
                        let rect = Rect::new(inner.x, inner.y + render_row as u16, inner.width, 1);
                        if rect.y < inner.y + inner.height {
                            hits.push((rect, Hit::MarketplaceRow(*position)));
                        }
                    }
                }
            }
            frame.render_widget(List::new(items), inner);
        }
    }

    render_marketplace_detail(frame, columns[1], model, hits);
}

fn render_filter_box(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
    // The filter is a raised input slab — brighter than the panel surface
    // it sits on — with no border. While filtering, the slab stays raised
    // and the text turns into a live input; otherwise it reads as a hint.
    let slab = if model.filtering {
        SELECTED_BG
    } else {
        SURFACE_RAISED
    };
    let block = Block::default().style(Style::default().bg(slab));
    let text = if model.marketplace_filter.is_empty() {
        Line::from(Span::styled(
            "Filter marketplaces…",
            Style::default().fg(MUTED),
        ))
    } else {
        let mut spans = vec![Span::raw(model.marketplace_filter.clone())];
        if model.filtering {
            spans.push(Span::styled("▏", Style::default().fg(ACCENT)));
        }
        Line::from(spans)
    };
    frame.render_widget(Paragraph::new(text).block(block), area);
}

/// A pill-style chip — solid background, black text, one space of padding
/// each side — for the group header's aggregate badges. Plain colored text
/// (no chip) is used everywhere else; this is deliberately reserved for
/// the header row so the two badge styles stay visually distinct.
fn pill(text: &str, bg: Color) -> Span<'static> {
    Span::styled(
        format!(" {text} "),
        Style::default()
            .fg(Color::Black)
            .bg(bg)
            .add_modifier(Modifier::BOLD),
    )
}

fn header_line<'a>(
    marketplace: &'a str,
    collapsed: bool,
    all_installed: bool,
    is_official: bool,
) -> Line<'a> {
    let chevron = if collapsed { "▸" } else { "▾" };
    let mut spans = vec![
        Span::styled(format!("{chevron} "), Style::default().fg(MUTED)),
        Span::styled(
            marketplace.to_owned(),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
    ];
    // Explicit `Color::Reset` rather than a bare unstyled `Span::raw` — some
    // terminals/multiplexers don't emit a background-reset escape between
    // two adjacent styled spans unless one is actually present, which reads
    // as the pills bleeding into each other with no visible gap.
    let gap = || Span::styled("  ", Style::default().bg(Color::Reset));
    if all_installed {
        spans.push(gap());
        spans.push(pill("Installed", SUCCESS));
    }
    if is_official {
        spans.push(gap());
        spans.push(pill("Default", ACCENT));
    }
    Line::from(spans)
}

/// Widest text each fixed slot after Status ever holds, so a row without
/// that badge still reserves its column and the next one doesn't drift —
/// same recipe as the Plugins route's own fixed columns.
const OFFICIAL_WIDTH: usize = "Official".len();
const UPDATE_WIDTH: usize = "Update available".len();

fn plugin_row<'a>(
    plugin: &'a MarketplacePluginSummary,
    selected: bool,
    name_width: usize,
    row_width: u16,
) -> ListItem<'a> {
    let name_style = if selected {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
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
    let status_style = if plugin.installed {
        Style::default().fg(SUCCESS)
    } else {
        Style::default().fg(MUTED)
    };
    let official = format!(
        "{:<OFFICIAL_WIDTH$}",
        if plugin.marketplace == "uze-official" {
            "Official"
        } else {
            ""
        }
    );
    let update = format!(
        "{:<UPDATE_WIDTH$}",
        if plugin.update_available == Some(true) {
            "Update available"
        } else {
            ""
        }
    );
    let mut spans = vec![
        Span::styled(if selected { "  › " } else { "    " }, name_style),
        Span::styled(name, name_style),
        Span::raw("  "),
        Span::styled(status, status_style),
        Span::raw("  "),
        Span::styled(official, Style::default().fg(MUTED)),
        Span::raw("  "),
        Span::styled(update, Style::default().fg(WARNING)),
    ];

    if selected {
        for span in &mut spans {
            span.style = span.style.bg(SELECTED_BG);
        }
        let used: usize = spans.iter().map(|s| s.width()).sum();
        let gap = (row_width as usize).saturating_sub(used);
        spans.push(Span::styled(
            " ".repeat(gap),
            Style::default().bg(SELECTED_BG),
        ));
    }
    ListItem::new(Line::from(spans))
}

fn render_marketplace_detail(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let Some(plugin) = model.selected_marketplace_plugin() else {
        frame.render_widget(Paragraph::new("").block(surface_block(" Plugin")), area);
        return;
    };

    // Status card pinned to the bottom, main content scrolling above it.
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .spacing(1)
        .constraints([Constraint::Min(6), Constraint::Length(4)])
        .split(area);

    let mut lines = vec![
        Line::from(vec![Span::styled(
            &plugin.name,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(Span::styled(
            plugin.description.clone().unwrap_or_default(),
            Style::default().fg(MUTED),
        )),
    ];
    if !plugin.keywords.is_empty() {
        lines.push(Line::from(Span::styled(
            plugin.keywords.join(", "),
            Style::default().fg(MUTED),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Source",
        Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
    )));
    let source_row_y = area.y + lines.len() as u16 + 1;
    lines.push(Line::from(vec![
        Span::raw(plugin.marketplace.clone()),
        Span::raw("   "),
        Span::styled("↗", Style::default().fg(MUTED)),
    ]));
    // The external-link glyph jumps the Marketplace list to this plugin's
    // group. Registering the whole row (not just the glyph) keeps the
    // click target usable without measuring exact glyph offsets.
    if source_row_y < area.y + area.height {
        hits.push((
            Rect::new(area.x, source_row_y, area.width, 1),
            Hit::JumpToMarketplace(plugin.marketplace.clone()),
        ));
    }
    lines.push(Line::from(""));

    lines.push(Line::from(Span::styled(
        "Resources",
        Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
    )));
    if let Some(detail) = &model.marketplace_detail
        && detail.summary.name == plugin.name
        && detail.summary.marketplace == plugin.marketplace
    {
        push_capability_table(&mut lines, &detail.capabilities);
    } else {
        lines.push(Line::from(Span::styled(
            "  loading…",
            Style::default().fg(MUTED),
        )));
    }

    frame.render_widget(
        Paragraph::new(lines)
            .block(surface_block(" Plugin"))
            .wrap(Wrap { trim: true }),
        sections[0],
    );

    let (color, headline, subtitle) = if plugin.installed {
        if plugin.update_available == Some(true) {
            (
                WARNING,
                "Update available",
                "A newer revision is ready in this marketplace",
            )
        } else {
            (SUCCESS, "Installed", "Ready to use in your projects")
        }
    } else {
        (MUTED, "Not installed", "Press i to install this plugin")
    };
    render_status_card(frame, sections[1], color, headline, subtitle);
}
