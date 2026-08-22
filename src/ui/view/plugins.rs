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
use super::super::{
    ACCENT, DANGER, MUTED, SELECTED_BG, SUCCESS, WARNING, health_style, route_style, surface_block,
};
use super::{push_capability_table, render_status_card};

pub(crate) fn render_plugins(
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

    let block = surface_block(format!(" Plugins  {} installed ", model.plugins.len()));
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
        // Fixed column widths, mirroring the Marketplace list: every column
        // pads to its own widest value across the panel, so Official/Update
        // available/health line up instead of drifting with each plugin
        // id's length.
        let id_width = model
            .plugins
            .iter()
            .map(|plugin| plugin.id.chars().count())
            .max()
            .unwrap_or(0);
        let items: Vec<ListItem> = model
            .plugins
            .iter()
            .enumerate()
            .map(|(index, plugin)| {
                plugin_row(
                    plugin,
                    index == model.plugins_selected,
                    model,
                    id_width,
                    inner.width,
                )
            })
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

/// Widest badge text in each fixed slot, so a shorter/absent badge still
/// reserves its column's width and the next one doesn't drift.
const OFFICIAL_WIDTH: usize = "Official".len();
const UPDATE_WIDTH: usize = "Update available".len();

fn plugin_row<'a>(
    plugin: &'a PluginSummary,
    selected: bool,
    model: &TuiModel,
    id_width: usize,
    row_width: u16,
) -> ListItem<'a> {
    let marker = if selected { "› " } else { "  " };
    let style = if selected {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let is_official = plugin.source.starts_with("embedded:");
    let id = format!("{:<id_width$}", plugin.id);
    let official = format!(
        "{:<OFFICIAL_WIDTH$}",
        if is_official { "Official" } else { "" }
    );
    let update = format!(
        "{:<UPDATE_WIDTH$}",
        if plugin.update_available == Some(true) {
            "Update available"
        } else {
            ""
        }
    );
    let health = plugin_health(model.doctor.as_ref(), &plugin.id);
    let mut spans = vec![
        Span::styled(marker, style),
        Span::styled(id, style),
        Span::raw("  "),
        Span::styled(official, Style::default().fg(ACCENT)),
        Span::raw("  "),
        Span::styled(update, Style::default().fg(WARNING)),
        Span::raw("  "),
        Span::styled(health, health_style(health)),
    ];
    if selected {
        // The highlight covers the whole row (background included), matching
        // the Marketplace list's selected treatment.
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

fn render_plugin_detail(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
    let Some(plugin) = model.selected_plugin() else {
        frame.render_widget(Paragraph::new("").block(surface_block(" Plugin")), area);
        return;
    };
    // Status card pinned to the bottom, main content scrolling above it —
    // mirrors the Marketplace detail pane's layout.
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .spacing(1)
        .constraints([Constraint::Min(6), Constraint::Length(4)])
        .split(area);

    let is_official = plugin.source.starts_with("embedded:");
    let mut lines = vec![
        Line::from(vec![Span::styled(
            &plugin.id,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
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
            "Resources",
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        )));
        push_capability_table(&mut lines, &inspection.capabilities);
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
            .block(surface_block(" Selected plugin"))
            .wrap(Wrap { trim: true }),
        sections[0],
    );

    let health = plugin_health(model.doctor.as_ref(), &plugin.id);
    let (color, headline, subtitle) = match (health, plugin.update_available) {
        (_, Some(true)) => (
            WARNING,
            "Update available",
            "A newer marketplace revision is ready",
        ),
        ("ready", _) => (SUCCESS, "Installed", "Ready to use in your projects"),
        ("missing", _) => (
            WARNING,
            "Missing",
            "Not attached to any detected harness yet",
        ),
        ("needs attention", _) => (
            DANGER,
            "Needs attention",
            "Drifted, conflicting, or blocked — see Doctor",
        ),
        _ => (MUTED, "Unknown", "No health record for this plugin yet"),
    };
    render_status_card(frame, sections[1], color, headline, subtitle);
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
