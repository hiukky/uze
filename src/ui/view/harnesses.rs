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

use uze_application::{
    CapabilityKind, HarnessCapabilities,
    application::{ContextMechanism, HarnessContextSupport, HarnessHealth},
};

use super::super::hit::Hit;
use super::super::model::{ResizablePanel, Route, TuiModel};
use super::super::{content_area, render_screen_header, side_panel_area};
use crate::ui::theme::{self, Symbol, Token};

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

    fn glyph(self) -> String {
        match self {
            Self::NotInstalled => theme::glyph(Symbol::MarkClose),
            Self::Installed => theme::glyph(Symbol::StatusSelected),
            Self::Configured => theme::glyph(Symbol::MarkOfficial),
            Self::NeedsPath => theme::glyph(Symbol::MarkAttention),
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
            Self::NotInstalled => theme::color(Token::TextMuted),
            Self::Installed => theme::color(Token::StateWarning),
            Self::Configured => theme::color(Token::Accent),
            Self::NeedsPath => theme::color(Token::StateWarning),
        }
    }
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
            theme::fg(Token::TextMuted),
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
                    theme::color(Token::Accent)
                } else {
                    theme::color(Token::BorderDefault)
                },
            ));
        let inner = block.inner(filter_area);
        frame.render_widget(block, filter_area);
        let text = if model.harnesses_filter.is_empty() {
            Line::from(Span::styled(
                "Filter integrations…",
                theme::fg(Token::TextMuted),
            ))
        } else {
            let mut spans = vec![Span::styled(
                model.harnesses_filter.clone(),
                theme::fg(Token::TextPrimary),
            )];
            if model.filtering && model.route == Route::Harnesses {
                spans.push(Span::styled(
                    theme::glyph(Symbol::BarThin),
                    theme::fg(Token::Accent),
                ));
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
                    Paragraph::new(Span::styled("Loading…", theme::fg(Token::TextMuted))),
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
                            theme::fg(Token::TextMuted),
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
                    render_harness_card(frame, rect, harness, status, selected, hits, position);
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
                            theme::fg(Token::StateWarning),
                        )),
                        Rect::new(content.x, y, content.width, 1),
                    );
                    y += 1;
                }
            }
        }
    }

    if drawer_open && let Some(harness) = model.selected_harness() {
        render_harness_drawer(frame, area, drawer_width, model, harness, hits);
    }
}

fn render_harness_card(
    frame: &mut ratatui::Frame<'_>,
    rect: Rect,
    harness: &HarnessHealth,
    status: HarnessStatus,
    selected: bool,
    hits: &mut Vec<(Rect, Hit)>,
    index: usize,
) {
    let background = if selected {
        theme::color(Token::SurfaceSelected)
    } else {
        theme::color(Token::SurfaceRecessed)
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
                theme::color(Token::TextBright)
            } else {
                theme::color(Token::TextSecondary)
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
            theme::fg(Token::TextDim),
        ))
        .wrap(Wrap { trim: true }),
        Rect::new(inner.x, inner.y + 1, inner.width, 2),
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            harness.integration.clone(),
            theme::fg(Token::TextMuted),
        )),
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
    hits: &mut Vec<(Rect, Hit)>,
) {
    let status = HarnessStatus::from(harness);
    // Receives the exact width already used by `render_harnesses`, so the
    // list and drawer always agree about the draggable boundary.
    let drawer = side_panel_area(area, width);
    frame.render_widget(Clear, drawer);
    frame.render_widget(
        Block::default()
            .borders(ratatui::widgets::Borders::LEFT)
            .border_style(Style::default().fg(
                if model.dragging_panel == Some(ResizablePanel::HarnessDrawer) {
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
        Line::from(Span::styled("HARNESS", theme::fg_bold(Token::TextMuted))),
        Line::from(Span::styled(
            harness.display_name.clone(),
            Style::default()
                .fg(theme::color(Token::TextBright))
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            label_span("Version", theme::fg(Token::TextMuted)),
            Span::styled(
                harness
                    .detection
                    .version
                    .clone()
                    .unwrap_or_else(|| "unknown".to_owned()),
                theme::fg(Token::TextTertiary),
            ),
        ]),
        Line::from(vec![
            label_span("Status", theme::fg(Token::TextMuted)),
            Span::styled(
                format!("{} {}", status.glyph(), status.label()),
                Style::default().fg(status.color()),
            ),
        ]),
        Line::from(vec![
            label_span("Delivery", theme::fg(Token::TextMuted)),
            Span::styled(
                harness
                    .strategy
                    .as_deref()
                    .map(friendly_delivery)
                    .unwrap_or("Not configured yet"),
                theme::fg(Token::TextTertiary),
            ),
        ]),
    ];
    if let Some(provisioning) = &harness.provisioning {
        lines.push(Line::from(vec![
            label_span("Provisioning", theme::fg(Token::TextMuted)),
            Span::styled(
                format!("{:?} ({:?})", provisioning.status, provisioning.action),
                theme::fg(Token::TextTertiary),
            ),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "COMPATIBILITY",
        theme::fg_bold(Token::TextMuted),
    )));
    for (label, status, style) in compatibility_rows(harness) {
        lines.push(Line::from(vec![
            label_span(label, theme::fg(Token::TextSecondary)),
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
///
/// Every row here is machine-scoped: what this harness supports, on this
/// machine, regardless of where `uze` was launched from. Whether one
/// particular project is actually being delivered is the workspace's
/// per-agent support popup's question, answered against that pane's own
/// cwd — asking it here, against the TUI's launch directory, is how the
/// screen used to report "none in project" about `$HOME`.
fn compatibility_rows(harness: &HarnessHealth) -> Vec<(&'static str, String, Style)> {
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
    context_rows(&harness.context_support)
        .into_iter()
        .chain(routed)
        .collect()
}

/// The drawer's project-context rows. Each names the mechanism through
/// which this harness receives that resource, so a harness receiving
/// `AGENTS.md` through the runtime shim reads as supported instead of the
/// old "— Not needed" (which meant only that no installed package had
/// contributed a managed region — a fact about plugins, never about
/// whether the harness could see a project's instructions).
fn context_rows(support: &HarnessContextSupport) -> Vec<(&'static str, String, Style)> {
    let (instructions, instructions_style) = context_row(support.instructions);
    let (agents_directory, agents_directory_style) = context_row(support.agents_directory);
    vec![
        ("AGENTS.md", instructions, instructions_style),
        (".agents", agents_directory, agents_directory_style),
    ]
}

fn context_row(mechanism: ContextMechanism) -> (String, Style) {
    match mechanism {
        ContextMechanism::Native => (
            format!("{} Native", theme::glyph(Symbol::MarkNative)),
            theme::fg(Token::Accent),
        ),
        ContextMechanism::RuntimeShim => (
            format!("{} Runtime shim", theme::glyph(Symbol::MarkNative)),
            theme::fg(Token::Accent),
        ),
        ContextMechanism::Bridge => (
            format!("{} Bridged", theme::glyph(Symbol::MarkNative)),
            theme::fg(Token::Accent),
        ),
        ContextMechanism::ShimShadowed => (
            format!("{} PATH shadowed", theme::glyph(Symbol::MarkWarning)),
            theme::fg(Token::StateWarning),
        ),
        ContextMechanism::Unsupported => (
            format!("{} Not supported", theme::glyph(Symbol::MarkUnsupported)),
            theme::fg(Token::StateDanger),
        ),
    }
}

fn capability_status(capabilities: &HarnessCapabilities, kind: CapabilityKind) -> (String, Style) {
    if capabilities.direct_standard.contains(&kind) || capabilities.native.contains(&kind) {
        (
            format!("{} Native", theme::glyph(Symbol::MarkNative)),
            theme::fg(Token::Accent),
        )
    } else if capabilities.adaptable.contains(&kind) {
        (
            format!("{} Adapted", theme::glyph(Symbol::MarkAdapted)),
            theme::fg(Token::StateWarning),
        )
    } else if capabilities.degraded.contains(&kind) {
        (
            format!("{} Degraded", theme::glyph(Symbol::MarkAdapted)),
            theme::fg(Token::StateWarning),
        )
    } else {
        (
            format!("{} Not supported", theme::glyph(Symbol::MarkUnsupported)),
            theme::fg(Token::StateDanger),
        )
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
            context_support: HarnessContextSupport {
                instructions: ContextMechanism::RuntimeShim,
                agents_directory: ContextMechanism::RuntimeShim,
            },
        }
    }

    #[test]
    fn shadowed_runtime_shim_never_reads_as_configured() {
        let status = HarnessStatus::from(&configured_harness(false));
        assert_eq!(status, HarnessStatus::NeedsPath);
        assert_eq!(status.label(), "PATH shadowed");
        assert_eq!(status.color(), theme::color(Token::StateWarning));
    }

    fn support(
        instructions: ContextMechanism,
        agents_directory: ContextMechanism,
    ) -> HarnessContextSupport {
        HarnessContextSupport {
            instructions,
            agents_directory,
        }
    }

    // The reported regression: Claude Code's AGENTS.md row read
    // "— Not needed" in every project, because the old model asked whether
    // an installed *package* had contributed a managed region rather than
    // whether the harness could receive project instructions at all.
    #[test]
    fn a_runtime_projection_reads_as_supported_not_as_not_needed() {
        let rows = context_rows(&support(
            ContextMechanism::RuntimeShim,
            ContextMechanism::RuntimeShim,
        ));
        assert_eq!(rows[0].0, "AGENTS.md");
        assert_eq!(
            rows[0].1,
            format!("{} Runtime shim", theme::glyph(Symbol::MarkNative))
        );
        assert_eq!(rows[0].2.fg, Some(theme::color(Token::Accent)));
        assert_eq!(rows[1].0, ".agents");
        assert_eq!(
            rows[1].1,
            format!("{} Runtime shim", theme::glyph(Symbol::MarkNative))
        );
    }

    #[test]
    fn each_resource_is_answered_independently() {
        // A harness may discover `.agents/` on its own while still needing
        // a bridge file for `AGENTS.md`.
        let rows = context_rows(&support(ContextMechanism::Bridge, ContextMechanism::Native));
        assert_eq!(
            rows[0].1,
            format!("{} Bridged", theme::glyph(Symbol::MarkNative))
        );
        assert_eq!(
            rows[1].1,
            format!("{} Native", theme::glyph(Symbol::MarkNative))
        );
    }

    #[test]
    fn a_shadowed_shim_reads_as_an_environment_warning() {
        let rows = context_rows(&support(
            ContextMechanism::ShimShadowed,
            ContextMechanism::ShimShadowed,
        ));
        assert_eq!(
            rows[0].1,
            format!("{} PATH shadowed", theme::glyph(Symbol::MarkWarning))
        );
        assert_eq!(rows[0].2.fg, Some(theme::color(Token::StateWarning)));
    }

    #[test]
    fn a_harness_with_no_mechanism_reads_as_unsupported() {
        let rows = context_rows(&support(
            ContextMechanism::Unsupported,
            ContextMechanism::Unsupported,
        ));
        assert_eq!(rows[0].1, "— Not supported");
        assert_eq!(rows[0].2.fg, Some(theme::color(Token::StateDanger)));
    }

    // The drawer is machine-scoped: the same harness reads identically no
    // matter which directory `uze` was launched from, because nothing in
    // the rows is resolved against a project.
    #[test]
    fn compatibility_rows_lead_with_the_two_portable_resources() {
        let harness = configured_harness(true);
        let labels: Vec<_> = compatibility_rows(&harness)
            .into_iter()
            .map(|(label, _, _)| label)
            .collect();
        assert_eq!(
            labels,
            ["AGENTS.md", ".agents", "Skills", "MCP", "Agents", "Hooks"]
        );
    }
}
