//! TUI view — Harnesses route.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph, Wrap},
};

use crate::{application::HarnessHealth, capability::CapabilityKind, router::HarnessCapabilities};

use super::super::hit::Hit;
use super::super::model::TuiModel;
use super::super::{ACCENT, DANGER, MUTED, SUCCESS, WARNING, panel_block, setup_style};

pub(crate) fn render_harnesses(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    let block = panel_block(" Harnesses ");
    let inner = block.inner(columns[0]);
    frame.render_widget(block, columns[0]);
    match &model.doctor {
        None => {
            frame.render_widget(Paragraph::new("Loading…").wrap(Wrap { trim: true }), inner);
        }
        Some(doctor) => {
            let items: Vec<ListItem> = doctor
                .harnesses
                .iter()
                .enumerate()
                .map(|(index, harness)| {
                    let selected = index == model.harnesses_selected;
                    let marker = if selected { "› " } else { "  " };
                    let style = if selected {
                        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    };
                    let status = if harness.detection.present {
                        Span::styled("Installed", Style::default().fg(SUCCESS))
                    } else {
                        Span::styled("Not installed", Style::default().fg(MUTED))
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(marker, style),
                        Span::styled(format!("{:<14}", harness.integration), style),
                        status,
                    ]))
                })
                .collect();
            frame.render_widget(List::new(items), inner);
            for index in 0..doctor.harnesses.len() {
                let row = Rect::new(inner.x, inner.y + index as u16, inner.width, 1);
                if row.y < inner.y + inner.height {
                    hits.push((row, Hit::HarnessRow(index)));
                }
            }
        }
    }
    render_harness_detail(frame, columns[1], model);
}

fn render_harness_detail(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
    let Some(harness) = model.selected_harness() else {
        frame.render_widget(Paragraph::new("").block(panel_block(" Harness ")), area);
        return;
    };
    let mut lines = vec![
        Line::from(Span::styled(
            &harness.integration,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("Version   ", Style::default().fg(MUTED)),
            Span::raw(
                harness
                    .detection
                    .version
                    .clone()
                    .unwrap_or_else(|| "unknown".to_owned()),
            ),
        ]),
        Line::from(vec![
            Span::styled("Status    ", Style::default().fg(MUTED)),
            Span::styled(harness.setup.clone(), setup_style(&harness.setup)),
        ]),
        Line::from(vec![
            Span::styled("Delivery  ", Style::default().fg(MUTED)),
            Span::raw(
                harness
                    .strategy
                    .clone()
                    .unwrap_or_else(|| "not configured".to_owned()),
            ),
        ]),
    ];
    if let Some(provisioning) = &harness.provisioning {
        lines.push(Line::from(vec![
            Span::styled("Provisioning  ", Style::default().fg(MUTED)),
            Span::raw(format!(
                "{:?} ({:?})",
                provisioning.status, provisioning.action
            )),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Compatibility",
        Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
    )));
    for (label, status, style) in compatibility_rows(harness) {
        lines.push(Line::from(vec![
            Span::raw(format!("  {label:<10}")),
            Span::styled(status, style),
        ]));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" Harness "))
            .wrap(Wrap { trim: true }),
        area,
    );
}

/// One row per capability UZE knows about, in the order a reader would care
/// about them: what a harness actually delivers today first, what remains
/// unimplemented anywhere last. `AGENTS.md` is listed separately from the
/// `capabilities()`-derived rows below it: instructions are not a
/// `CapabilityKind::Instruction` resource routed through the same
/// `HarnessCapabilities` sets (see `HarnessHealth::native_instructions`'s
/// doc comment) — mixing it into the same lookup would silently mislabel it
/// "not supported" on every harness, since none of them ever populate that
/// capability kind.
fn compatibility_rows(harness: &HarnessHealth) -> Vec<(&'static str, &'static str, Style)> {
    let instructions = if harness.native_instructions {
        ("AGENTS.md", "✓ Native", Style::default().fg(SUCCESS))
    } else {
        ("AGENTS.md", "✓ Bridged", Style::default().fg(SUCCESS))
    };
    let routed = [
        ("Skills", CapabilityKind::AgentSkill),
        ("MCP", CapabilityKind::Mcp),
    ]
    .into_iter()
    .map(|(label, kind)| {
        let (status, style) = capability_status(&harness.capabilities, kind);
        (label, status, style)
    });
    // Recognized on import but not yet routed to any harness — see
    // `uze_core::importers`, which is the only place these kinds appear at
    // all today.
    let unimplemented = [
        ("Agents", "— Not implemented"),
        ("Hooks", "— Not implemented"),
    ]
    .into_iter()
    .map(|(label, status)| (label, status, Style::default().fg(MUTED)));
    std::iter::once(instructions)
        .chain(routed)
        .chain(unimplemented)
        .collect()
}

fn capability_status(
    capabilities: &HarnessCapabilities,
    kind: CapabilityKind,
) -> (&'static str, Style) {
    if capabilities.direct_standard.contains(&kind) || capabilities.native.contains(&kind) {
        ("✓ Native", Style::default().fg(SUCCESS))
    } else if capabilities.adaptable.contains(&kind) {
        ("≈ Adapted", Style::default().fg(WARNING))
    } else if capabilities.degraded.contains(&kind) {
        ("⚠ Degraded", Style::default().fg(WARNING))
    } else {
        ("✗ Not supported", Style::default().fg(DANGER))
    }
}
