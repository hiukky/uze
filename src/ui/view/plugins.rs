//! TUI view — Plugins route.
//!
//! A flat, two-line-per-row list (name + source/health on the first line,
//! description on the second) — the design shows no split/drawer here at
//! all. `Enter` still opens a detail drawer with the deliveries/managed
//! state UZE already computes; the design just never depicts that state,
//! not that the app shouldn't offer it.

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::application::{DoctorReport, PluginSummary};

use super::super::hit::Hit;
use super::super::model::TuiModel;
use super::super::{
    ACCENT, BASE, BLUE, BORDER, MUTED, SELECTED_BG, TEXT_BRIGHT, TEXT_DIM, TEXT_SECONDARY, WARNING,
    health_style, route_style,
};
use super::super::{content_area, render_screen_header};
use super::resource_summary;

pub(crate) fn render_plugins(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let content = content_area(area);
    let content = render_screen_header(
        frame,
        content,
        "Plugins",
        "installed plugins",
        Some(Span::styled(
            format!("{} installed", model.plugins.len()),
            Style::default().fg(MUTED),
        )),
    );

    if model.plugins.is_empty() {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    "No plugins installed",
                    Style::default()
                        .fg(TEXT_BRIGHT)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    "Open Marketplace to install one.",
                    Style::default().fg(MUTED),
                )),
            ]),
            content,
        );
    } else {
        let mut y = content.y;
        for (index, plugin) in model.plugins.iter().enumerate() {
            if y + 1 >= content.y + content.height {
                break;
            }
            let selected = index == model.plugins_selected;
            let rect = Rect::new(content.x, y, content.width, 2);
            render_plugin_row(frame, rect, plugin, selected, model, hits, index);
            // A blank row between blocks — otherwise one plugin's
            // description sits directly against the next plugin's name
            // with no breathing room at all.
            y += 3;
        }
    }

    if model.plugin_drawer_open
        && let Some(plugin) = model.selected_plugin()
    {
        render_plugin_drawer(frame, area_for_drawer(area), model, plugin);
    }
}

/// The drawer overlays from the *original* (unshrunk) route area, matching
/// how Marketplace/Harnesses anchor theirs — `content_area` already insets
/// once for the list; re-deriving here keeps the drawer's own inset
/// independent of how much of that area the header consumed.
fn area_for_drawer(area: Rect) -> Rect {
    content_area(area)
}

fn render_plugin_row(
    frame: &mut ratatui::Frame<'_>,
    rect: Rect,
    plugin: &PluginSummary,
    selected: bool,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
    index: usize,
) {
    let health = plugin_health(model.doctor.as_ref(), &plugin.id);
    let is_official = plugin.source.starts_with("embedded:");
    // The marketplace catalog already knows this plugin's *marketplace*
    // name ("ai", "uze-official") — resolving through it beats showing
    // `plugin.source`'s raw value, which for a local/git install is a full
    // filesystem path or URL, far too noisy for a single list row.
    let catalog_marketplace = model
        .marketplace_plugins
        .iter()
        .find(|m| format!("{}@{}", m.name, m.marketplace) == plugin.id)
        .map(|m| m.marketplace.clone());
    // Official gets the same "✓ Official" badge the Marketplace tree shows
    // for its group header, not the literal "uze-official" string — the
    // badge is the point, the raw marketplace name is an implementation
    // detail nobody needs staring back at them from every plugin row.
    let (source_label, source_color) = if is_official {
        ("✓ Official".to_owned(), BLUE)
    } else if let Some(marketplace) = catalog_marketplace {
        (marketplace, TEXT_DIM)
    } else {
        // Not from any known marketplace (an ad-hoc git/local `uze add`) —
        // fall back to the source string's last path segment rather than
        // the full path.
        let label = plugin
            .source
            .rsplit(['/', '\\'])
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or(&plugin.source)
            .to_owned();
        (label, TEXT_DIM)
    };

    let name_fg = if selected {
        TEXT_BRIGHT
    } else {
        TEXT_SECONDARY
    };
    let left = vec![Span::styled(
        plugin.active_name.clone(),
        Style::default().fg(name_fg),
    )];
    let right = vec![
        Span::styled(source_label, Style::default().fg(source_color)),
        Span::raw("  "),
        Span::styled(health, health_style(health)),
    ];
    let used: usize = left.iter().chain(right.iter()).map(Span::width).sum();
    let gap = (rect.width as usize).saturating_sub(used);
    let mut name_spans = left;
    name_spans.push(Span::raw(" ".repeat(gap.max(2))));
    name_spans.extend(right);

    // Best-effort description: cross-referenced from the marketplace catalog
    // by name — `PluginSummary` itself carries none, and nothing here
    // invents one for a plugin the catalog doesn't recognize (an ad-hoc
    // git/local install, most often).
    let description = model
        .marketplace_plugins
        .iter()
        .find(|m| format!("{}@{}", m.name, m.marketplace) == plugin.id)
        .and_then(|m| m.description.clone())
        .unwrap_or_default();
    let mut desc_spans = vec![Span::styled(description, Style::default().fg(MUTED))];

    if selected {
        for span in name_spans.iter_mut().chain(desc_spans.iter_mut()) {
            span.style = span.style.bg(SELECTED_BG);
        }
        for spans in [&mut name_spans, &mut desc_spans] {
            let used: usize = spans.iter().map(Span::width).sum();
            let gap = (rect.width as usize).saturating_sub(used);
            spans.push(Span::styled(
                " ".repeat(gap),
                Style::default().bg(SELECTED_BG),
            ));
        }
    }

    let top = Rect::new(rect.x, rect.y, rect.width, 1);
    let bottom = Rect::new(rect.x, rect.y + 1, rect.width, 1);
    frame.render_widget(Paragraph::new(Line::from(name_spans)), top);
    frame.render_widget(Paragraph::new(Line::from(desc_spans)), bottom);
    hits.push((rect, Hit::PluginRow(index)));
}

fn render_plugin_drawer(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    plugin: &PluginSummary,
) {
    let width = 52.min(area.width);
    let drawer = Rect::new(
        area.x + area.width - width,
        area.y - 1,
        width,
        area.height + 1,
    );
    frame.render_widget(Clear, drawer);
    frame.render_widget(
        Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(BORDER))
            .style(Style::default().bg(BASE)),
        drawer,
    );
    let inner = Rect::new(
        drawer.x + 2,
        drawer.y + 1,
        drawer.width - 3,
        drawer.height - 1,
    );

    let is_official = plugin.source.starts_with("embedded:");
    let mut lines = vec![
        Line::from(Span::styled(
            "PLUGIN",
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            &plugin.active_name,
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        // The active name is what every harness invokes (`/<active_name>:*`);
        // `id` is the real, marketplace-qualified origin — shown separately
        // whenever they differ, i.e. whenever an install-time `alias`
        // resolved a name collision (ADR-038), so a person removing or
        // inspecting this row is never left guessing which physical package
        // it actually is.
        Line::from(if plugin.active_name == plugin.id {
            Span::styled(format!("Origin: {}", plugin.id), Style::default().fg(MUTED))
        } else {
            Span::styled(
                format!("Origin: {} (aliased)", plugin.id),
                Style::default().fg(WARNING),
            )
        }),
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
            Span::styled(
                plugin.capability_count.to_string(),
                Style::default().fg(TEXT_SECONDARY),
            ),
        ]),
        Line::from(vec![
            Span::styled("Update        ", Style::default().fg(MUTED)),
            match plugin.update_available {
                Some(true) => Span::styled("Available", Style::default().fg(WARNING)),
                Some(false) => Span::styled("Up to date", Style::default().fg(ACCENT)),
                None => Span::styled("Unknown", Style::default().fg(MUTED)),
            },
        ]),
    ];
    if let Some(inspection) = &model.plugin_detail
        && inspection.plugin.id == plugin.id
    {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "RESOURCES",
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            resource_summary(&inspection.capabilities),
            Style::default().fg(TEXT_SECONDARY),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "AVAILABLE IN",
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
                    format!("  {:<12}", delivery.display_name),
                    Style::default().fg(TEXT_SECONDARY),
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
                Style::default().fg(ACCENT),
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
    frame.render_widget(Paragraph::new(lines), inner);
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
