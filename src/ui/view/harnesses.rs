//! TUI view — Harnesses route.
//!
//! List on the left; a detail drawer slides in from the right once a
//! harness is selected (`TuiModel::harnesses_drawer_open`), with a draggable
//! left edge to balance the detail against the list.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, Wrap},
};

use crate::integration::AttachmentState;
use crate::{
    application::{HarnessContextDelivery, HarnessHealth, ProjectContextStatus},
    capability::CapabilityKind,
    router::HarnessCapabilities,
};

use super::super::hit::Hit;
use super::super::model::{ResizablePanel, TuiModel};
use super::super::{
    ACCENT, BORDER, DANGER, MUTED, SURFACE_OVERLAY, TEXT_BRIGHT, TEXT_DIM, TEXT_SECONDARY,
    TEXT_TERTIARY, WARNING,
};
use super::super::{content_area, render_divided_row, render_screen_header};
use super::overview::{portability_label, portability_style};

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

/// A row's status text stops this many columns short of the row's right
/// edge — otherwise, with the drawer open, it lands flush against the
/// drawer's own border with no breathing room.
const ROW_RIGHT_PAD: usize = 2;

/// Looks up the one `HarnessContextStatus` (from the same `context_status`
/// the old standalone Context screen read) matching a harness's stable
/// `integration` id — `HarnessHealth` and `HarnessContextStatus` are two
/// separate read models keyed on the same id, not one shared struct.
fn context_delivery_for<'a>(
    status: Option<&'a ProjectContextStatus>,
    integration: &str,
) -> Option<&'a HarnessContextDelivery> {
    status?
        .harnesses
        .iter()
        .find(|harness| harness.integration == integration)
        .map(|harness| &harness.delivery)
}

/// A list row only earns a bridge-health glyph when the AGENTS.md bridge is
/// both needed and not currently `Matched` — an unneeded-but-missing bridge
/// isn't a problem, and a healthy one has nothing to flag. Kept separate
/// from `agents_md_row` (the drawer's fuller label): the list only has room
/// for a glyph, not the label that goes with it.
fn bridge_flag(delivery: Option<&HarnessContextDelivery>) -> Option<(&'static str, Color)> {
    match delivery {
        Some(HarnessContextDelivery::Bridge {
            needed: true,
            state,
        }) if *state != AttachmentState::Matched => {
            let color = match state {
                AttachmentState::Conflict | AttachmentState::Blocked => DANGER,
                _ => WARNING,
            };
            Some(("⚠", color))
        }
        _ => None,
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
            .unwrap_or(area.width / 2)
            .clamp(24, area.width.saturating_sub(24).max(24))
            .min(area.width)
    } else {
        0
    };
    let list_area = Rect::new(
        area.x,
        area.y,
        area.width.saturating_sub(drawer_width),
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
    // Portability comes from the same `context_status` the drawer's own
    // AGENTS.md row reads (see `context_delivery_for`) — one glance at the
    // top of the list before ever selecting a harness.
    if let Some(status) = &model.context_status
        && y < bottom
    {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Portability  ", Style::default().fg(MUTED)),
                Span::styled(
                    portability_label(&status.portability),
                    portability_style(Some(status)),
                ),
            ])),
            Rect::new(content.x, y, content.width, 1),
        );
        y += 2;
    }

    match &model.doctor {
        None => {
            frame.render_widget(
                Paragraph::new(Span::styled("Loading…", Style::default().fg(MUTED))),
                Rect::new(content.x, y, content.width, content.height),
            );
        }
        Some(doctor) => {
            // Two lines per harness — title (name + bridge flag + status,
            // right-aligned) then a muted one-line description — mirroring
            // the sidebar's own selected-row treatment: a left accent bar
            // and a bolder title on selection, never a filled background.
            for (index, harness) in doctor.harnesses.iter().enumerate() {
                if y >= bottom {
                    break;
                }
                let selected = index == model.harnesses_selected;
                let status = HarnessStatus::from(harness);
                let delivery =
                    context_delivery_for(model.context_status.as_ref(), &harness.integration);

                let border = if selected {
                    Span::styled("│", Style::default().fg(ACCENT))
                } else {
                    Span::raw(" ")
                };
                let name_fg = if selected { TEXT_BRIGHT } else { TEXT_TERTIARY };
                let mut name_style = Style::default().fg(name_fg);
                if selected {
                    name_style = name_style.add_modifier(Modifier::BOLD);
                }
                let name = Span::styled(harness.display_name.clone(), name_style);
                let bridge_span = match bridge_flag(delivery) {
                    Some((glyph, color)) => {
                        Span::styled(format!("{glyph:<2}"), Style::default().fg(color))
                    }
                    None => Span::raw("  "),
                };
                let status_span = Span::styled(
                    format!("{} {}", status.glyph(), status.label()),
                    Style::default().fg(status.color()),
                );
                let used = border.width()
                    + 1
                    + name.width()
                    + bridge_span.width()
                    + status_span.width()
                    + ROW_RIGHT_PAD;
                let gap = (content.width as usize).saturating_sub(used);
                let title_line = Line::from(vec![
                    border,
                    Span::raw(" "),
                    name,
                    Span::raw(" ".repeat(gap)),
                    bridge_span,
                    status_span,
                    Span::raw(" ".repeat(ROW_RIGHT_PAD)),
                ]);

                let title_rect = Rect::new(content.x, y, content.width, 1);
                frame.render_widget(Paragraph::new(title_line), title_rect);
                hits.push((title_rect, Hit::HarnessRow(index)));
                y += 1;
                if y >= bottom {
                    break;
                }

                let description_line = Line::from(vec![
                    Span::raw("  "),
                    Span::styled(harness.description.as_str(), Style::default().fg(TEXT_DIM)),
                ]);
                let description_rect = Rect::new(content.x, y, content.width, 1);
                hits.push((description_rect, Hit::HarnessRow(index)));
                y = render_divided_row(frame, content, y, description_line);
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
        let delivery = context_delivery_for(model.context_status.as_ref(), &harness.integration);
        render_harness_drawer(frame, area, drawer_width, model, harness, delivery, hits);
    }
}

fn render_harness_drawer(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    width: u16,
    model: &TuiModel,
    harness: &HarnessHealth,
    delivery: Option<&HarnessContextDelivery>,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let status = HarnessStatus::from(harness);
    // Receives the exact width already used by `render_harnesses`, so the
    // list and drawer always agree about the draggable boundary.
    let width = width.min(area.width);
    let drawer = Rect::new(area.x + area.width - width, area.y, width, area.height);
    frame.render_widget(Clear, drawer);
    frame.render_widget(
        Block::default()
            .borders(ratatui::widgets::Borders::LEFT)
            .border_style(Style::default().fg(
                if model.dragging_panel == Some(ResizablePanel::HarnessDrawer) {
                    ACCENT
                } else {
                    BORDER
                },
            ))
            .style(Style::default().bg(SURFACE_OVERLAY)),
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
    for (label, status, style) in compatibility_rows(harness, delivery) {
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
/// unimplemented anywhere last. `AGENTS.md` is listed separately from the
/// `capabilities()`-derived rows below it: instructions are not a
/// `CapabilityKind::Instruction` resource routed through the same
/// `HarnessCapabilities` sets — mixing it into the same lookup would
/// silently mislabel it "not supported" on every harness, since none of
/// them ever populate that capability kind. Hooks are `CapabilityKind::Hook`
/// and route through the same sets (declared native/adaptable per harness);
/// the hardcoded "Not implemented" stub for them was removed with ADR-033
/// delivery.
fn compatibility_rows(
    harness: &HarnessHealth,
    delivery: Option<&HarnessContextDelivery>,
) -> Vec<(&'static str, &'static str, Style)> {
    let instructions = agents_md_row(delivery);
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

/// The drawer's AGENTS.md compatibility row — the merged replacement for
/// what used to be the standalone Context screen's whole reason to exist.
/// Reads the same `HarnessContextDelivery` that screen read, so a
/// Missing/Drifted/Conflict/Blocked bridge shows up here with the same
/// fidelity it always had, instead of collapsing to a plain "Bridged".
///
/// `state` decides the label first, `needed` only softens it — mirroring
/// the old Context screen exactly. A `Matched` bridge always reads
/// "Bridged", even when `needed` is currently false: `needed` is about
/// whether AGENTS.md *right now* has a matched contribution worth bridging,
/// not whether the on-disk bridge file itself is working — a healthy
/// bridge must never be hidden behind "not needed" just because nothing
/// currently requires writing to it.
fn agents_md_row(delivery: Option<&HarnessContextDelivery>) -> (&'static str, &'static str, Style) {
    match delivery {
        None => ("AGENTS.md", "— Unknown", Style::default().fg(MUTED)),
        Some(HarnessContextDelivery::Native) => {
            ("AGENTS.md", "√ Native", Style::default().fg(ACCENT))
        }
        Some(HarnessContextDelivery::NotDetected) => {
            ("AGENTS.md", "— Not detected", Style::default().fg(MUTED))
        }
        Some(HarnessContextDelivery::Bridge { needed, state }) => {
            // Label always names the bridge's real `state` — `needed` never
            // overrides it, only softens a non-Matched state's color, since
            // an unneeded-but-present problem is still real, just lower
            // priority. `Missing` is the one state that reads as "Not
            // needed" rather than a naked "⚠ Missing" when unneeded: a
            // bridge that doesn't exist and isn't required is the
            // no-op case, not a gap.
            let label = match state {
                AttachmentState::Matched => "√ Bridged",
                AttachmentState::Missing if !needed => "— Not needed",
                AttachmentState::Missing => "⚠ Missing",
                AttachmentState::Drifted => "⚠ Drifted",
                AttachmentState::Conflict => "✕ Conflict",
                AttachmentState::Blocked => "✕ Blocked",
            };
            let color = match state {
                AttachmentState::Matched => ACCENT,
                _ if !needed => TEXT_DIM,
                AttachmentState::Missing | AttachmentState::Drifted => WARNING,
                AttachmentState::Conflict | AttachmentState::Blocked => DANGER,
            };
            ("AGENTS.md", label, Style::default().fg(color))
        }
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
            native_instructions: false,
            runtime_shim_active,
            project_agents_directory_native: false,
        }
    }

    #[test]
    fn shadowed_runtime_shim_never_reads_as_configured() {
        let status = HarnessStatus::from(&configured_harness(false));
        assert_eq!(status, HarnessStatus::NeedsPath);
        assert_eq!(status.label(), "PATH shadowed");
        assert_eq!(status.color(), WARNING);
    }

    // A `Matched` bridge must always read "Bridged", regardless of
    // `needed` — `needed` describes whether AGENTS.md currently has
    // something worth bridging, not whether the bridge file itself is
    // working. Claude Code (the only `Bridge`-type integration) hits
    // `needed: false` any time no installed package's instructions are
    // currently matched in AGENTS.md, which does not make its already
    // -matched `CLAUDE.md` bridge stop existing.
    #[test]
    fn matched_bridge_reads_bridged_even_when_not_currently_needed() {
        let delivery = HarnessContextDelivery::Bridge {
            needed: false,
            state: AttachmentState::Matched,
        };
        let (label, text, _) = agents_md_row(Some(&delivery));
        assert_eq!(label, "AGENTS.md");
        assert_eq!(text, "√ Bridged");
    }

    #[test]
    fn missing_and_unneeded_bridge_reads_not_needed() {
        let delivery = HarnessContextDelivery::Bridge {
            needed: false,
            state: AttachmentState::Missing,
        };
        let (_, text, _) = agents_md_row(Some(&delivery));
        assert_eq!(text, "— Not needed");
    }

    #[test]
    fn missing_and_needed_bridge_is_a_warning() {
        let delivery = HarnessContextDelivery::Bridge {
            needed: true,
            state: AttachmentState::Missing,
        };
        let (_, text, style) = agents_md_row(Some(&delivery));
        assert_eq!(text, "⚠ Missing");
        assert_eq!(style.fg, Some(WARNING));
    }

    #[test]
    fn conflict_while_needed_is_flagged_dangerous_in_list_and_drawer() {
        let delivery = HarnessContextDelivery::Bridge {
            needed: true,
            state: AttachmentState::Conflict,
        };
        let (_, text, style) = agents_md_row(Some(&delivery));
        assert_eq!(text, "✕ Conflict");
        assert_eq!(style.fg, Some(DANGER));

        let (glyph, color) = bridge_flag(Some(&delivery)).expect("needed conflict must flag");
        assert_eq!(glyph, "⚠");
        assert_eq!(color, DANGER);
    }

    #[test]
    fn conflict_while_unneeded_is_named_but_deemphasized() {
        let delivery = HarnessContextDelivery::Bridge {
            needed: false,
            state: AttachmentState::Conflict,
        };
        let (_, text, style) = agents_md_row(Some(&delivery));
        assert_eq!(text, "✕ Conflict");
        assert_eq!(style.fg, Some(TEXT_DIM));

        // An unneeded bridge never earns the list's glyph, no matter how
        // bad its state — the glyph exists to flag bridges the project
        // actually depends on right now.
        assert!(bridge_flag(Some(&delivery)).is_none());
    }
}
