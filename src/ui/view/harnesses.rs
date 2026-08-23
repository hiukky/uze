//! TUI view — Harnesses route.
//!
//! List on the left; a detail drawer slides in from the right once a
//! harness is selected (`TuiModel::harnesses_drawer_open`), covering part
//! of the list rather than sharing a permanent static split — the same
//! interaction the design uses for Marketplace.

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
};

use crate::{application::HarnessHealth, capability::CapabilityKind, router::HarnessCapabilities};

use super::super::hit::Hit;
use super::super::model::TuiModel;
use super::super::{
    ACCENT, BASE, BORDER, DANGER, MUTED, SELECTED_BG, TEXT_BRIGHT, TEXT_SECONDARY, TEXT_TERTIARY,
    WARNING, setup_style,
};
use super::super::{content_area, render_divided_row, render_screen_header};

pub(crate) fn render_harnesses(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let area = content_area(area);
    let count = model.doctor.as_ref().map_or(0, |d| d.harnesses.len());
    let content = render_screen_header(
        frame,
        area,
        "Harnesses",
        "detected agents",
        Some(Span::styled(
            format!("{count} installed"),
            Style::default().fg(MUTED),
        )),
    );

    match &model.doctor {
        None => {
            frame.render_widget(
                Paragraph::new(Span::styled("Loading…", Style::default().fg(MUTED))),
                content,
            );
        }
        Some(doctor) => {
            // Same table-row style as Context: name / detail / right-aligned
            // status, each row underlined by a hairline divider.
            let mut y = content.y;
            let bottom = content.y + content.height;
            for (index, harness) in doctor.harnesses.iter().enumerate() {
                if y >= bottom {
                    break;
                }
                let selected = index == model.harnesses_selected;
                let name_fg = if selected { TEXT_BRIGHT } else { TEXT_TERTIARY };
                let (status_text, status_fg) = if harness.detection.present {
                    ("Installed", ACCENT)
                } else {
                    ("Not detected", MUTED)
                };
                // `harness.setup` is "not configured" / "installed / unverified"
                // / "installed / verified" — a distinct axis from the status
                // column beside it (is the harness *detected on this
                // machine*, vs. has *UZE's own setup* been run and verified
                // for it). The "installed / " prefix on the latter two just
                // repeats the status column's own "Installed" text right
                // next to it, so it's dropped here — the CLI's `uze doctor`
                // output keeps the full phrase since it has no adjacent
                // status column to read it against.
                let detail = if harness.detection.present {
                    harness
                        .setup
                        .strip_prefix("installed / ")
                        .unwrap_or(&harness.setup)
                        .to_owned()
                } else {
                    "Not detected in this environment".to_owned()
                };

                let mut spans = vec![
                    Span::styled(
                        format!("{:<16}", harness.display_name),
                        Style::default().fg(name_fg),
                    ),
                    Span::styled(format!("{detail:<44}"), Style::default().fg(MUTED)),
                    Span::styled(format!("{status_text:>10}"), Style::default().fg(status_fg)),
                ];
                if selected {
                    for span in &mut spans {
                        span.style = span.style.bg(SELECTED_BG);
                    }
                    let used: usize = spans.iter().map(|s| s.width()).sum();
                    let gap = (content.width as usize).saturating_sub(used);
                    spans.push(Span::styled(
                        " ".repeat(gap),
                        Style::default().bg(SELECTED_BG),
                    ));
                }

                hits.push((
                    Rect::new(content.x, y, content.width, 1),
                    Hit::HarnessRow(index),
                ));
                y = render_divided_row(frame, content, y, Line::from(spans));
            }
        }
    }

    if model.harnesses_drawer_open
        && let Some(harness) = model.selected_harness()
    {
        render_harness_drawer(frame, area, harness);
    }
}

fn render_harness_drawer(frame: &mut ratatui::Frame<'_>, area: Rect, harness: &HarnessHealth) {
    let width = 46.min(area.width);
    let drawer = Rect::new(area.x + area.width - width, area.y, width, area.height);
    frame.render_widget(Clear, drawer);
    frame.render_widget(
        Block::default()
            .borders(ratatui::widgets::Borders::LEFT)
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

    let mut lines = vec![
        Line::from(Span::styled(
            "HARNESS",
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            harness.display_name.clone(),
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(format!("{:<12}", "Version"), Style::default().fg(MUTED)),
            Span::styled(
                harness
                    .detection
                    .version
                    .clone()
                    .unwrap_or_else(|| "unknown".to_owned()),
                Style::default().fg(TEXT_TERTIARY),
            ),
        ]),
        Line::from(vec![
            Span::styled(format!("{:<12}", "Status"), Style::default().fg(MUTED)),
            Span::styled(harness.setup.clone(), setup_style(&harness.setup)),
        ]),
        Line::from(vec![
            Span::styled(format!("{:<12}", "Delivery"), Style::default().fg(MUTED)),
            Span::styled(
                harness
                    .strategy
                    .clone()
                    .unwrap_or_else(|| "not configured".to_owned()),
                Style::default().fg(TEXT_TERTIARY),
            ),
        ]),
    ];
    if let Some(provisioning) = &harness.provisioning {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:<12}", "Provisioning"),
                Style::default().fg(MUTED),
            ),
            Span::styled(
                format!("{:?} ({:?})", provisioning.status, provisioning.action),
                Style::default().fg(TEXT_TERTIARY),
            ),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "COMPATIBILITY",
        Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
    )));
    for (label, status, style) in compatibility_rows(harness) {
        lines.push(Line::from(vec![
            Span::styled(format!("{label:<10}"), Style::default().fg(TEXT_SECONDARY)),
            Span::styled(status, style),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), inner);
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
        ("AGENTS.md", "√ Native", Style::default().fg(ACCENT))
    } else {
        ("AGENTS.md", "√ Bridged", Style::default().fg(ACCENT))
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
        ("√ Native", Style::default().fg(ACCENT))
    } else if capabilities.adaptable.contains(&kind) {
        ("≈ Adapted", Style::default().fg(WARNING))
    } else if capabilities.degraded.contains(&kind) {
        ("≈ Degraded", Style::default().fg(WARNING))
    } else {
        ("— Not supported", Style::default().fg(DANGER))
    }
}
