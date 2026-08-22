//! TUI view — Plugins route.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph, Wrap},
};

use crate::application::PluginSummary;

use crate::application::DoctorReport;

use super::super::hit::Hit;
use super::super::model::TuiModel;
use super::super::{ACCENT, MUTED, SUCCESS, WARNING, health_style, panel_block, route_style};

pub(crate) fn render_plugins(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(46), Constraint::Percentage(54)])
        .split(area);

    let block = panel_block(format!(" Plugins  {} installed ", model.plugins.len()));
    let inner = block.inner(columns[0]);
    frame.render_widget(block, columns[0]);
    if model.plugins.is_empty() {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    "No plugins installed",
                    Style::default().add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    "Open Marketplace to install one.",
                    Style::default().fg(MUTED),
                )),
            ])
            .wrap(Wrap { trim: true }),
            inner,
        );
    } else {
        let items: Vec<ListItem> = model
            .plugins
            .iter()
            .enumerate()
            .map(|(index, plugin)| plugin_row(plugin, index == model.plugins_selected, model))
            .collect();
        frame.render_widget(List::new(items), inner);
        for index in 0..model.plugins.len() {
            let row = Rect::new(inner.x, inner.y + index as u16, inner.width, 1);
            if row.y < inner.y + inner.height {
                hits.push((row, Hit::PluginRow(index)));
            }
        }
    }
    render_plugin_detail(frame, columns[1], model);
}

fn plugin_row<'a>(plugin: &'a PluginSummary, selected: bool, model: &TuiModel) -> ListItem<'a> {
    let marker = if selected { "› " } else { "  " };
    let style = if selected {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let is_official = plugin.source.starts_with("embedded:");
    let mut spans = vec![Span::styled(marker, style), Span::styled(&plugin.id, style)];
    if is_official {
        spans.push(Span::styled("  Official", Style::default().fg(ACCENT)));
    }
    if plugin.update_available == Some(true) {
        spans.push(Span::styled(
            "  Update available",
            Style::default().fg(WARNING),
        ));
    }
    let health = plugin_health(model.doctor.as_ref(), &plugin.id);
    spans.push(Span::styled(format!("  {health}"), health_style(health)));
    ListItem::new(Line::from(spans))
}

fn render_plugin_detail(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
    let Some(plugin) = model.selected_plugin() else {
        frame.render_widget(Paragraph::new("").block(panel_block(" Plugin ")), area);
        return;
    };
    let is_official = plugin.source.starts_with("embedded:");
    let mut lines = vec![
        Line::from(Span::styled(
            &plugin.id,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            if is_official {
                "Official".to_owned()
            } else {
                format!("Source: {}", plugin.source)
            },
            Style::default().fg(MUTED),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("Capabilities  ", Style::default().fg(MUTED)),
            Span::raw(plugin.capability_count.to_string()),
        ]),
        Line::from(vec![
            Span::styled("Update        ", Style::default().fg(MUTED)),
            match plugin.update_available {
                Some(true) => Span::styled("Available", Style::default().fg(WARNING)),
                Some(false) => Span::styled("Up to date", Style::default().fg(SUCCESS)),
                None => Span::styled("Unknown", Style::default().fg(MUTED)),
            },
        ]),
    ];
    if let Some(inspection) = &model.plugin_detail
        && inspection.plugin.id == plugin.id
    {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Available in",
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        )));
        for delivery in &inspection.deliveries {
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
                    format!("  {:<12}", delivery.integration),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(route, route_style(route)),
            ]));
        }
        let state = &inspection.managed_state;
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Managed  ", Style::default().fg(MUTED)),
            Span::styled(
                format!("{} matched", state.matched),
                Style::default().fg(SUCCESS),
            ),
            Span::styled(
                format!(
                    " · {} missing · {} drifted · {} conflicts · {} blocked",
                    state.missing, state.drifted, state.conflicts, state.blocked
                ),
                Style::default().fg(MUTED),
            ),
        ]));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" Selected plugin "))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn exposure_route_label(plan: &crate::exposure::ExposurePlan) -> &'static str {
    match plan.route {
        crate::router::CompatibilityRoute::Native => "native",
        crate::router::CompatibilityRoute::Adaptable => "adapted",
        crate::router::CompatibilityRoute::Degraded => "degraded",
        crate::router::CompatibilityRoute::Unsupported => "unsupported",
    }
}

fn package_strategy(plan: &crate::exposure::PackageExposurePlan) -> &'static str {
    match plan.route {
        crate::router::CompatibilityRoute::Native => "native",
        crate::router::CompatibilityRoute::Adaptable => "adapted",
        crate::router::CompatibilityRoute::Degraded => "degraded",
        crate::router::CompatibilityRoute::Unsupported => "unsupported",
    }
}

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
