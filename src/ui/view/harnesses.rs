//! TUI view — Harnesses route.
//!
//! List on the left; a detail drawer slides in from the right once a
//! harness is selected (`TuiModel::harnesses_drawer_open`), covering part
//! of the list rather than sharing a permanent static split — the same
//! interaction the design uses for Marketplace.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
};

use crate::{application::HarnessHealth, capability::CapabilityKind, router::HarnessCapabilities};

use super::super::hit::Hit;
use super::super::model::TuiModel;
use super::super::{
    ACCENT, BASE, BORDER, DANGER, MUTED, SELECTED_BG, TEXT_BRIGHT, TEXT_SECONDARY, TEXT_TERTIARY,
    WARNING,
};
use super::super::{content_area, render_divided_row, render_screen_header};

/// A harness's state collapses onto exactly one of three buckets for this
/// list — `HarnessHealth` itself tracks a finer distinction (whether the
/// last explicit `uze setup` run specifically *verified* the binary, vs.
/// configuration that only ever happened implicitly through `uze add`), but
/// that's an audit-trail detail for the drawer, not something a glance at
/// the list needs: either way the harness is equally ready to receive
/// plugins. New states here should earn their place the same way — only
/// when the list needs to tell the user to act differently, not because the
/// underlying data happens to distinguish something.
#[derive(Clone, Copy, PartialEq, Eq)]
enum HarnessStatus {
    /// The binary isn't on this machine at all.
    NotInstalled,
    /// Detected, but UZE has never configured it (`uze setup` or an
    /// implicit `uze add` preparation).
    Installed,
    /// UZE has configured it — ready to receive plugins.
    Configured,
}

impl HarnessStatus {
    fn from(harness: &HarnessHealth) -> Self {
        if !harness.detection.present {
            Self::NotInstalled
        } else if harness.setup.contains("not configured") {
            Self::Installed
        } else {
            Self::Configured
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            Self::NotInstalled => "✕",
            Self::Installed => "●",
            Self::Configured => "✓",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::NotInstalled => "Not installed",
            Self::Installed => "Installed",
            Self::Configured => "Configured",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::NotInstalled => MUTED,
            Self::Installed => WARNING,
            Self::Configured => ACCENT,
        }
    }
}

/// A row's status text stops this many columns short of the row's right
/// edge — otherwise, with the drawer open, it lands flush against the
/// drawer's own border with no breathing room.
const ROW_RIGHT_PAD: usize = 2;

/// Width of the drawer's label column, shared by the key/value rows (
/// `Version`, `Status`, …) and the COMPATIBILITY rows. The longest label
/// in use is "Provisioning" (12 chars), so 14 guarantees at least a
/// two-space gap — a fixed pad equal to the longest label would glue the
/// value flush against it.
const LABEL_COL: usize = 14;

pub(crate) fn render_harnesses(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let area = content_area(area);
    let count = model.doctor.as_ref().map_or(0, |d| d.harnesses.len());
    // The drawer overlays from the right rather than sharing a permanent
    // split, but the header/list still need to lay out *around* it when
    // it's open — otherwise their own right-aligned content runs straight
    // under the drawer and gets clipped mid-word by its Clear. Split evenly
    // rather than a fixed width: the list only ever needs two short columns
    // (name, status), while the drawer's own content (Delivery strings,
    // COMPATIBILITY rows) is what actually needs the room.
    let drawer_open = model.harnesses_drawer_open && model.selected_harness().is_some();
    let drawer_width = if drawer_open { area.width / 2 } else { 0 };
    let list_area = Rect::new(
        area.x,
        area.y,
        area.width.saturating_sub(drawer_width),
        area.height,
    );
    let content = render_screen_header(
        frame,
        list_area,
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
            // One status column, right-aligned — name / status, each row
            // underlined by a hairline divider.
            let mut y = content.y;
            let bottom = content.y + content.height;
            for (index, harness) in doctor.harnesses.iter().enumerate() {
                if y >= bottom {
                    break;
                }
                let selected = index == model.harnesses_selected;
                let name_fg = if selected { TEXT_BRIGHT } else { TEXT_TERTIARY };
                let status = HarnessStatus::from(harness);

                let name = Span::styled(
                    format!("{:<16}", harness.display_name),
                    Style::default().fg(name_fg),
                );
                let status_span = Span::styled(
                    format!("{} {}", status.glyph(), status.label()),
                    Style::default().fg(status.color()),
                );
                let used = name.width() + status_span.width() + ROW_RIGHT_PAD;
                let gap = (content.width as usize).saturating_sub(used);
                let mut spans = vec![
                    name,
                    Span::raw(" ".repeat(gap)),
                    status_span,
                    Span::raw(" ".repeat(ROW_RIGHT_PAD)),
                ];
                if selected {
                    for span in &mut spans {
                        span.style = span.style.bg(SELECTED_BG);
                    }
                }

                hits.push((
                    Rect::new(content.x, y, content.width, 1),
                    Hit::HarnessRow(index),
                ));
                y = render_divided_row(frame, content, y, Line::from(spans));
            }
        }
    }

    if drawer_open && let Some(harness) = model.selected_harness() {
        render_harness_drawer(frame, area, harness);
    }
}

fn render_harness_drawer(frame: &mut ratatui::Frame<'_>, area: Rect, harness: &HarnessHealth) {
    let status = HarnessStatus::from(harness);
    // Matches `render_harnesses`'s own `drawer_width` — an even split, not a
    // fixed cap, so the list and drawer never disagree about where the
    // boundary sits.
    let width = area.width / 2;
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
            label_span("Version", Style::default().fg(MUTED)),
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
            label_span("Status", Style::default().fg(MUTED)),
            Span::styled(
                format!("{} {}", status.glyph(), status.label()),
                Style::default().fg(status.color()),
            ),
        ]),
        Line::from(vec![
            label_span("Delivery", Style::default().fg(MUTED)),
            Span::styled(
                harness
                    .strategy
                    .as_deref()
                    .map(friendly_delivery)
                    .unwrap_or("Not configured yet"),
                Style::default().fg(TEXT_TERTIARY),
            ),
        ]),
    ];
    if let Some(provisioning) = &harness.provisioning {
        lines.push(Line::from(vec![
            label_span("Provisioning", Style::default().fg(MUTED)),
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
            label_span(label, Style::default().fg(TEXT_SECONDARY)),
            Span::styled(status, style),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

/// A drawer key/value row's label, padded to the shared `LABEL_COL` column
/// so every row's value starts at the same x position.
fn label_span(label: &str, style: Style) -> Span<'static> {
    Span::styled(format!("{label:<width$}", width = LABEL_COL), style)
}

/// `harness.strategy` carries the internal identifier `install()` recorded
/// (see each `IntegrationPort::install` impl) — meant for state/receipts,
/// not a reader. Every identifier currently in use gets a plain-language
/// translation here; an integration adding a new one shows up as the raw
/// identifier rather than silently, so a gap is obvious instead of hidden.
fn friendly_delivery(strategy: &str) -> &str {
    match strategy {
        "managed-user-scope-skills-dir" => "Skills folder (UZE-managed)",
        "native-user-scope-skills-plus-managed-mcp-config" => {
            "Native skills + MCP config (UZE-managed)"
        }
        other => other,
    }
}

/// One row per capability UZE knows about, in the order a reader would care
/// about them: what a harness actually delivers today first, what remains
/// unimplemented anywhere last. `AGENTS.md` is listed separately from the
/// `capabilities()`-derived rows below it: instructions are not a
/// `CapabilityKind::Instruction` resource routed through the same
/// `HarnessCapabilities` sets (see `HarnessHealth::native_instructions`'s
/// doc comment) — mixing it into the same lookup would silently mislabel it
/// "not supported" on every harness, since none of them ever populate that
/// capability kind. Hooks are `CapabilityKind::Hook` and route through the
/// same sets (declared native/adaptable per harness); the hardcoded
/// "Not implemented" stub for them was removed with ADR-033 delivery.
fn compatibility_rows(harness: &HarnessHealth) -> Vec<(&'static str, &'static str, Style)> {
    let instructions = if harness.native_instructions {
        ("AGENTS.md", "√ Native", Style::default().fg(ACCENT))
    } else {
        ("AGENTS.md", "√ Bridged", Style::default().fg(ACCENT))
    };
    let routed = [
        ("Skills", CapabilityKind::AgentSkill),
        ("MCP", CapabilityKind::Mcp),
        ("Agents", CapabilityKind::Agent),
        ("Hooks", CapabilityKind::Hook),
    ]
    .into_iter()
    .map(|(label, kind)| {
        let (status, style) = capability_status(&harness.capabilities, kind);
        (label, status, style)
    });
    std::iter::once(instructions).chain(routed).collect()
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
