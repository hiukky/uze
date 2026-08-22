//! TUI view — Marketplace route.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph, Wrap},
};

use super::super::hit::Hit;
use super::super::model::TuiModel;
use super::super::{ACCENT, MUTED, SUCCESS, panel_block};

pub(crate) fn render_marketplace(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(46), Constraint::Percentage(54)])
        .split(area);
    let title = if model.marketplace_name.is_empty() {
        " Marketplace ".to_owned()
    } else {
        format!(" Marketplace  ·  {} ", model.marketplace_name)
    };
    let block = panel_block(title);
    let inner = block.inner(columns[0]);
    frame.render_widget(block, columns[0]);
    if model.marketplace_plugins.is_empty() {
        frame.render_widget(
            Paragraph::new("No marketplace plugins available.").wrap(Wrap { trim: true }),
            inner,
        );
    } else {
        let items: Vec<ListItem> = model
            .marketplace_plugins
            .iter()
            .enumerate()
            .map(|(index, plugin)| {
                let selected = index == model.marketplace_selected;
                let marker = if selected { "› " } else { "  " };
                let style = if selected {
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                let status = if plugin.installed {
                    Span::styled("Installed", Style::default().fg(SUCCESS))
                } else {
                    Span::styled("Available", Style::default().fg(MUTED))
                };
                ListItem::new(Line::from(vec![
                    Span::styled(marker, style),
                    Span::styled(&plugin.name, style),
                    Span::raw("  "),
                    status,
                ]))
            })
            .collect();
        frame.render_widget(List::new(items), inner);
        for index in 0..model.marketplace_plugins.len() {
            let row = Rect::new(inner.x, inner.y + index as u16, inner.width, 1);
            if row.y < inner.y + inner.height {
                hits.push((row, Hit::MarketplaceRow(index)));
            }
        }
    }
    render_marketplace_detail(frame, columns[1], model);
}

fn render_marketplace_detail(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
    let Some(plugin) = model.selected_marketplace_plugin() else {
        frame.render_widget(Paragraph::new("").block(panel_block(" Plugin ")), area);
        return;
    };
    let mut lines = vec![
        Line::from(Span::styled(
            &plugin.name,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
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
    if let Some(detail) = &model.marketplace_detail
        && detail.summary.name == plugin.name
    {
        lines.push(Line::from(Span::styled(
            "Capabilities",
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        )));
        for capability in &detail.capabilities {
            lines.push(Line::from(format!(
                "  {:?}  {}",
                capability.kind, capability.name
            )));
        }
        lines.push(Line::from(""));
    }
    lines.push(Line::from(vec![
        Span::styled("Status  ", Style::default().fg(MUTED)),
        if plugin.installed {
            Span::styled("Installed", Style::default().fg(SUCCESS))
        } else {
            Span::styled("Not installed", Style::default().fg(MUTED))
        },
    ]));
    if plugin.is_default {
        lines.push(Line::from(Span::styled(
            "Installed by default on a fresh setup",
            Style::default().fg(MUTED),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" Plugin "))
            .wrap(Wrap { trim: true }),
        area,
    );
}
