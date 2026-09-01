//! TUI view — Harnesses route.
//!
//! A responsive integration catalog on the left; a detail drawer slides in
//! from the right once a harness is selected (`TuiModel::harnesses_drawer_open`),
//! with a draggable left edge to balance the detail against the cards.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::integration::AttachmentState;
use crate::{
    application::{AgentContextStatus, HarnessHealth, ResourceDelivery, UndeliveredReason},
    capability::CapabilityKind,
    router::HarnessCapabilities,
};

use super::super::hit::Hit;
use super::super::model::{ResizablePanel, Route, TuiModel};
use super::super::{
    ACCENT, BORDER, DANGER, MUTED, SELECTED_BG, SURFACE_SUBTLE, TEXT_BRIGHT, TEXT_DIM,
    TEXT_PRIMARY, TEXT_SECONDARY, TEXT_TERTIARY, WARNING,
};
use super::super::{content_area, render_screen_header};

/// A harness's state collapses onto exactly one of three buckets for this
/// list — `HarnessHealth` itself tracks a finer distinction (whether the
/// last explicit `uze setup` run specifically *verified* the binary, vs.
/// configuration that only ever happened implicitly through `uze add`), but
/// that's an audit-trail detail for the drawer, not something a glance at
/// the list needs: either way the harness is equally ready to receive
/// plugins. New states here should earn their place the same way — only
/// when the list needs to tell the user to act differently, not because the
/// underlying data happens to distinguish something.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HarnessStatus {
    /// The binary isn't on this machine at all.
    NotInstalled,
    /// Detected, but UZE has never configured it (`uze setup` or an
    /// implicit `uze add` preparation).
    Installed,
    /// UZE has configured it — ready to receive plugins.
    Configured,
    /// A real harness binary shadows UZE's runtime shim on PATH.
    NeedsPath,
}

impl HarnessStatus {
    fn from(harness: &HarnessHealth) -> Self {
        if !harness.detection.present {
            Self::NotInstalled
        } else if !harness.runtime_shim_active {
            Self::NeedsPath
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
            Self::NeedsPath => "!",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::NotInstalled => "Not installed",
            Self::Installed => "Installed",
            Self::Configured => "Configured",
            Self::NeedsPath => "PATH shadowed",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::NotInstalled => MUTED,
            Self::Installed => WARNING,
            Self::Configured => ACCENT,
            Self::NeedsPath => WARNING,
        }
    }
}

/// Looks up the one `AgentContextStatus` matching a harness's stable
/// `integration` id — `HarnessHealth` (machine-scoped) and
/// `AgentContextStatus` (this project's delivery) are two separate read
/// models keyed on the same id, not one shared struct.
fn agent_context_for<'a>(
    context: &'a [AgentContextStatus],
    integration: &str,
) -> Option<&'a AgentContextStatus> {
    context
        .iter()
        .find(|harness| harness.integration == integration)
}

/// A list row earns a glyph only for a real gap: the project carries a
/// portable resource this harness is not receiving. A project that simply
/// has no `AGENTS.md` (or no `.agents/`) is not a gap and never flags —
/// that conflation is what made every Claude Code row read as a problem,
/// or as "not needed", regardless of what was actually being delivered.
/// Kept separate from `context_rows` (the drawer's fuller labels): the list
/// only has room for a glyph, not the label that goes with it.
fn context_gap_flag(context: Option<&AgentContextStatus>) -> Option<(&'static str, Color)> {
    let context = context?;
    let worst = [&context.instructions, &context.agents_directory]
        .into_iter()
        .find(|delivery| delivery.is_gap())?;
    let color = match worst {
        ResourceDelivery::Undelivered(UndeliveredReason::Bridge(
            AttachmentState::Conflict | AttachmentState::Blocked,
        ))
        | ResourceDelivery::Undelivered(UndeliveredReason::Unsupported)
        | ResourceDelivery::Undelivered(UndeliveredReason::HarnessAbsent) => DANGER,
        _ => WARNING,
    };
    Some(("⚠", color))
}

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
    // under the drawer and gets clipped mid-word by its Clear. Its initial
    // width is an even split; dragging the divider lets either panel take
    // priority for the task at hand.
    let drawer_open = model.harnesses_drawer_open && model.selected_harness().is_some();
    let drawer_width = if drawer_open {
        model
            .harness_drawer_width
            .unwrap_or(super::DRAWER_DEFAULT_WIDTH)
            .clamp(24, area.width.saturating_sub(24).max(24))
    } else {
        0
    };
    let list_area = Rect::new(
        area.x,
        area.y,
        area.width
            .saturating_sub(drawer_width)
            .saturating_sub(if drawer_open { 1 } else { 0 }),
        area.height,
    );
    let content = render_screen_header(
        frame,
        list_area,
        "Integrations",
        "detected agents",
        Some(Span::styled(
            format!("{count} installed"),
            Style::default().fg(MUTED),
        )),
    );

    let mut y = content.y;
    let bottom = content.y + content.height;

    if y + 2 <= bottom {
        let filter_area = Rect::new(content.x, y, content.width, 2);
        let block = Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(
                if model.filtering && model.route == Route::Harnesses {
                    ACCENT
                } else {
                    BORDER
                },
            ));
        let inner = block.inner(filter_area);
        frame.render_widget(block, filter_area);
        let text = if model.harnesses_filter.is_empty() {
            Line::from(Span::styled(
                "Filter integrations…",
                Style::default().fg(MUTED),
            ))
        } else {
            let mut spans = vec![Span::styled(
                model.harnesses_filter.clone(),
                Style::default().fg(TEXT_PRIMARY),
            )];
            if model.filtering && model.route == Route::Harnesses {
                spans.push(Span::styled("▏", Style::default().fg(ACCENT)));
            }
            Line::from(spans)
        };
        frame.render_widget(Paragraph::new(text), inner);
        y += 3;
    }

    match &model.doctor {
        None => {
            if y < bottom {
                frame.render_widget(
                    Paragraph::new(Span::styled("Loading…", Style::default().fg(MUTED))),
                    Rect::new(content.x, y, content.width, bottom.saturating_sub(y)),
                );
            }
        }
        Some(doctor) => {
            let visible = model.harness_visible_indices();
            if visible.is_empty() {
                if y < bottom {
                    frame.render_widget(
                        Paragraph::new(Span::styled(
                            format!(
                                "No integrations match \"{}\".",
                                model.harnesses_filter.trim()
                            ),
                            Style::default().fg(MUTED),
                        )),
                        Rect::new(content.x, y, content.width, 1),
                    );
                }
            } else {
                let columns = if content.width >= 110 { 3 } else { 2 };
                let gap = 1;
                let card_width = (content.width.saturating_sub(gap * (columns - 1))) / columns;
                let card_height = 7;
                for (position, &raw_index) in visible.iter().enumerate() {
                    let harness = &doctor.harnesses[raw_index];
                    let column = position as u16 % columns;
                    let row = position as u16 / columns;
                    let rect = Rect::new(
                        content.x + column * (card_width + gap),
                        y + row * (card_height + gap),
                        card_width,
                        card_height,
                    );
                    if rect.y + rect.height > bottom {
                        break;
                    }
                    let selected = position == model.harnesses_selected;
                    let status = HarnessStatus::from(harness);
                    let context = agent_context_for(&model.agent_context, &harness.integration);
                    render_harness_card(
                        frame, rect, harness, status, context, selected, hits, position,
                    );
                }

                let rows = (visible.len() as u16).div_ceil(columns);
                y += rows * (card_height + gap);
            }
            if let Some(status) = &model.context_status
                && !status.warnings.is_empty()
                && y < bottom
            {
                y += 1;
                for warning in &status.warnings {
                    if y >= bottom {
                        break;
                    }
                    frame.render_widget(
                        Paragraph::new(Span::styled(
                            format!("! {warning}"),
                            Style::default().fg(WARNING),
                        )),
                        Rect::new(content.x, y, content.width, 1),
                    );
                    y += 1;
                }
            }
        }
    }

    if drawer_open && let Some(harness) = model.selected_harness() {
        let context = agent_context_for(&model.agent_context, &harness.integration);
        render_harness_drawer(frame, area, drawer_width, model, harness, context, hits);
    }
}

#[allow(clippy::too_many_arguments)]
fn render_harness_card(
    frame: &mut ratatui::Frame<'_>,
    rect: Rect,
    harness: &HarnessHealth,
    status: HarnessStatus,
    context: Option<&AgentContextStatus>,
    selected: bool,
    hits: &mut Vec<(Rect, Hit)>,
    index: usize,
) {
    let background = if selected {
        SELECTED_BG
    } else {
        SURFACE_SUBTLE
    };
    frame.render_widget(
        Paragraph::new("").style(Style::default().bg(background)),
        rect,
    );
    let inner = Rect::new(
        rect.x.saturating_add(2),
        rect.y.saturating_add(1),
        rect.width.saturating_sub(4),
        rect.height.saturating_sub(2),
    );
    let name = Span::styled(
        harness.display_name.clone(),
        Style::default()
            .fg(if selected {
                TEXT_BRIGHT
            } else {
                TEXT_SECONDARY
            })
            .add_modifier(Modifier::BOLD),
    );
    let status_badge = Span::styled(
        format!("{} {}", status.glyph(), status.label()),
        Style::default().fg(status.color()),
    );
    let title_gap = inner
        .width
        .saturating_sub((name.width() + status_badge.width()) as u16);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            name,
            Span::raw(" ".repeat(title_gap as usize)),
            status_badge,
        ])),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            harness.description.clone(),
            Style::default().fg(TEXT_DIM),
        ))
        .wrap(Wrap { trim: true }),
        Rect::new(inner.x, inner.y + 1, inner.width, 2),
    );
    let mut tags = vec![Span::styled(
        harness.integration.clone(),
        Style::default().fg(MUTED),
    )];
    if let Some((glyph, color)) = context_gap_flag(context) {
        tags.push(Span::raw("  "));
        tags.push(Span::styled(
            format!("{glyph} context"),
            Style::default().fg(color),
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(tags)),
        Rect::new(inner.x, inner.y + 4, inner.width, 1),
    );
    hits.push((rect, Hit::HarnessRow(index)));
}

fn render_harness_drawer(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    width: u16,
    model: &TuiModel,
    harness: &HarnessHealth,
    context: Option<&AgentContextStatus>,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let status = HarnessStatus::from(harness);
    // Receives the exact width already used by `render_harnesses`, so the
    // list and drawer always agree about the draggable boundary.
    let width = width.min(area.width);
    let drawer = Rect::new(
        area.x + area.width - width,
        area.y - 1,
        width,
        area.height + 1,
    );
    frame.render_widget(Clear, drawer);
    frame.render_widget(
        Block::default()
            .borders(ratatui::widgets::Borders::LEFT)
            .border_style(Style::default().fg(
                if model.dragging_panel == Some(ResizablePanel::HarnessDrawer) {
                    ACCENT
                } else {
                    SURFACE_SUBTLE
                },
            ))
            .style(Style::default().bg(SURFACE_SUBTLE)),
        drawer,
    );
    hits.insert(
        0,
        (
            Rect::new(drawer.x, drawer.y, 1, drawer.height),
            Hit::ResizePanel(ResizablePanel::HarnessDrawer),
        ),
    );
    let inner = Rect::new(
        drawer.x + 2,
        drawer.y + 1,
        drawer.width - 3,
        drawer.height - 2,
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
    for (label, status, style) in compatibility_rows(harness, context) {
        lines.push(Line::from(vec![
            label_span(label, Style::default().fg(TEXT_SECONDARY)),
            Span::styled(status, style),
        ]));
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
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
/// unimplemented anywhere last. The two portable *project* resources
/// (`AGENTS.md` and `.agents/`) are listed separately from the
/// `capabilities()`-derived rows below them, and read from a different
/// model: they are not `CapabilityKind` resources routed through
/// `HarnessCapabilities` — mixing them into the same lookup would silently
/// mislabel them "not supported" on every harness, since none of them ever
/// populate `CapabilityKind::Instruction`.
fn compatibility_rows(
    harness: &HarnessHealth,
    context: Option<&AgentContextStatus>,
) -> Vec<(&'static str, &'static str, Style)> {
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
    context_rows(context).into_iter().chain(routed).collect()
}

/// The drawer's project-context rows. Each names the mechanism actually
/// carrying that resource into this harness, so a harness receiving
/// `AGENTS.md` through the runtime shim reads as delivered instead of the
/// old "— Not needed" (which meant only that no installed package had
/// contributed a managed region — a fact about plugins, never about
/// whether the harness could see the project's own instructions).
fn context_rows(context: Option<&AgentContextStatus>) -> Vec<(&'static str, &'static str, Style)> {
    let Some(context) = context else {
        let unknown = ("— Unknown", Style::default().fg(MUTED));
        return vec![
            ("AGENTS.md", unknown.0, unknown.1),
            (".agents", unknown.0, unknown.1),
        ];
    };
    let (instructions, instructions_style) = context_row(&context.instructions);
    let (agents_directory, agents_directory_style) = context_row(&context.agents_directory);
    vec![
        ("AGENTS.md", instructions, instructions_style),
        (".agents", agents_directory, agents_directory_style),
    ]
}

fn context_row(delivery: &ResourceDelivery) -> (&'static str, Style) {
    match delivery {
        ResourceDelivery::Native => ("√ Native", Style::default().fg(ACCENT)),
        ResourceDelivery::Projected => ("√ Runtime shim", Style::default().fg(ACCENT)),
        ResourceDelivery::Bridged => ("√ Bridged", Style::default().fg(ACCENT)),
        // Not a gap and not a success: the project carries nothing to
        // deliver. Dimmed so it never competes with a row that needs
        // attention.
        ResourceDelivery::AbsentFromProject => ("— None in project", Style::default().fg(TEXT_DIM)),
        ResourceDelivery::Undelivered(reason) => match reason {
            UndeliveredReason::HarnessAbsent => ("— Not detected", Style::default().fg(MUTED)),
            UndeliveredReason::ShimShadowed => ("⚠ PATH shadowed", Style::default().fg(WARNING)),
            UndeliveredReason::Bridge(AttachmentState::Missing) => {
                ("⚠ Missing", Style::default().fg(WARNING))
            }
            UndeliveredReason::Bridge(AttachmentState::Drifted) => {
                ("⚠ Drifted", Style::default().fg(WARNING))
            }
            UndeliveredReason::Bridge(AttachmentState::Conflict) => {
                ("✕ Conflict", Style::default().fg(DANGER))
            }
            UndeliveredReason::Bridge(AttachmentState::Blocked) => {
                ("✕ Blocked", Style::default().fg(DANGER))
            }
            // `Matched` is `Bridged` above, never a reason for a gap.
            UndeliveredReason::Bridge(AttachmentState::Matched) => {
                ("√ Bridged", Style::default().fg(ACCENT))
            }
            UndeliveredReason::Unsupported => ("✕ Not supported", Style::default().fg(DANGER)),
        },
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn configured_harness(runtime_shim_active: bool) -> HarnessHealth {
        HarnessHealth {
            integration: "claude-code".to_owned(),
            display_name: "Claude Code".to_owned(),
            description: "test harness".to_owned(),
            detection: uze_core::integration::HarnessDetection {
                present: true,
                version: Some("1.0.0".to_owned()),
            },
            setup: "configured".to_owned(),
            strategy: None,
            provisioning: None,
            publication: uze_core::integration::PublicationStatus::NotApplicable,
            capabilities: HarnessCapabilities::default(),
            runtime_shim_active,
        }
    }

    #[test]
    fn shadowed_runtime_shim_never_reads_as_configured() {
        let status = HarnessStatus::from(&configured_harness(false));
        assert_eq!(status, HarnessStatus::NeedsPath);
        assert_eq!(status.label(), "PATH shadowed");
        assert_eq!(status.color(), WARNING);
    }

    fn context(
        instructions: ResourceDelivery,
        agents_directory: ResourceDelivery,
    ) -> AgentContextStatus {
        AgentContextStatus {
            integration: "claude-code".to_owned(),
            display_name: "Claude Code".to_owned(),
            present: true,
            root: std::path::PathBuf::from("/project"),
            instructions,
            agents_directory,
        }
    }

    // The reported regression: Claude Code's AGENTS.md row read
    // "— Not needed" in every project, because the old model asked whether
    // an installed *package* had contributed a managed region rather than
    // whether the harness was receiving the project's instructions at all.
    #[test]
    fn a_runtime_projection_reads_as_delivered_not_as_not_needed() {
        let context = context(ResourceDelivery::Projected, ResourceDelivery::Projected);
        let rows = context_rows(Some(&context));
        assert_eq!(rows[0].0, "AGENTS.md");
        assert_eq!(rows[0].1, "√ Runtime shim");
        assert_eq!(rows[0].2.fg, Some(ACCENT));
        assert_eq!(rows[1].0, ".agents");
        assert_eq!(rows[1].1, "√ Runtime shim");
        assert!(context_gap_flag(Some(&context)).is_none());
    }

    #[test]
    fn a_project_carrying_nothing_is_dimmed_and_never_flagged() {
        let context = context(
            ResourceDelivery::AbsentFromProject,
            ResourceDelivery::AbsentFromProject,
        );
        let rows = context_rows(Some(&context));
        assert_eq!(rows[0].1, "— None in project");
        assert_eq!(rows[0].2.fg, Some(TEXT_DIM));
        assert!(context_gap_flag(Some(&context)).is_none());
    }

    #[test]
    fn each_resource_is_answered_independently() {
        // A project with only `.agents/` still shows its Skills delivered
        // — the old model gated the whole projection on AGENTS.md, so this
        // rendered as "not supported".
        let context = context(
            ResourceDelivery::AbsentFromProject,
            ResourceDelivery::Projected,
        );
        let rows = context_rows(Some(&context));
        assert_eq!(rows[0].1, "— None in project");
        assert_eq!(rows[1].1, "√ Runtime shim");
        assert!(context_gap_flag(Some(&context)).is_none());
    }

    #[test]
    fn a_matched_bridge_still_reads_as_delivered() {
        let context = context(ResourceDelivery::Bridged, ResourceDelivery::Native);
        let rows = context_rows(Some(&context));
        assert_eq!(rows[0].1, "√ Bridged");
        assert_eq!(rows[1].1, "√ Native");
    }

    #[test]
    fn a_real_gap_is_named_and_flagged_in_the_list() {
        let context = context(
            ResourceDelivery::Undelivered(UndeliveredReason::Bridge(AttachmentState::Conflict)),
            ResourceDelivery::Native,
        );
        let rows = context_rows(Some(&context));
        assert_eq!(rows[0].1, "✕ Conflict");
        assert_eq!(rows[0].2.fg, Some(DANGER));
        let (glyph, color) = context_gap_flag(Some(&context)).expect("a conflict must flag");
        assert_eq!(glyph, "⚠");
        assert_eq!(color, DANGER);
    }

    #[test]
    fn a_shadowed_shim_flags_as_an_environment_warning() {
        let context = context(
            ResourceDelivery::Undelivered(UndeliveredReason::ShimShadowed),
            ResourceDelivery::Undelivered(UndeliveredReason::ShimShadowed),
        );
        let rows = context_rows(Some(&context));
        assert_eq!(rows[0].1, "⚠ PATH shadowed");
        let (_, color) = context_gap_flag(Some(&context)).expect("a shadowed shim must flag");
        assert_eq!(color, WARNING);
    }
}
