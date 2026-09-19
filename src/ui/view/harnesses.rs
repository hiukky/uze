//! TUI view — Harnesses route.
//!
//! A responsive integration catalog on the left; a detail drawer slides in
//! from the right once a harness is selected,
//! with a draggable left edge to balance the detail against the cards.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

use uze_application::{
    CapabilityKind, HarnessCapabilities,
    application::{ContextMechanism, HarnessContextSupport, HarnessHealth},
};

use super::super::hit::Hit;
use super::super::model::{ResizablePanel, Route, TuiModel};
use super::super::{content_area, render_screen_header};
use super::{DrawerStatus, render_drawer_footer};
use crate::ui::theme::{self, Symbol, Token};

/// Two states, because there are two answers a person can act on: UZE has
/// set this harness up, or it has not. A binary that is not on the machine
/// and one the person installed themselves and never handed to UZE are the
/// same answer to the only question the list asks — can it receive
/// plugins? Which of the two it is, and what to do about it, is the
/// drawer's to say (see [`status_note`]).
///
/// `HarnessHealth` tracks far more: whether an explicit `uze setup` run
/// verified the binary rather than an `uze add` preparing it implicitly,
/// and whether UZE's runtime shim is first on `PATH` for the harnesses
/// that project project-context through it. Every one of those is drawer
/// detail. The last of them used to be a state of its own here, and it
/// put `PATH shadowed` across the card of a harness whose only real news
/// was that nobody had set it up — a sentence about this machine's `PATH`
/// where the person was looking for what to do next. A new state here
/// earns its place only when the list needs them to act differently.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HarnessStatus {
    /// UZE has configured it — ready to receive plugins.
    Configured,
    /// Not on this machine, or on it and never handed to UZE.
    NotConfigured,
}

impl HarnessStatus {
    fn from(harness: &HarnessHealth) -> Self {
        if harness.detection.present && !harness.setup.contains("not configured") {
            Self::Configured
        } else {
            Self::NotConfigured
        }
    }

    /// The mark a card wears, which is nothing at all when a harness is
    /// not configured: a card carrying no mark already says so, and a
    /// second way of saying it is a warning on every harness the person
    /// has not set up — most of them, on most machines, with nothing
    /// wrong.
    fn mark(self) -> Option<String> {
        match self {
            Self::Configured => Some(theme::glyph(Symbol::MarkOk)),
            Self::NotConfigured => None,
        }
    }

    /// The mark with the word for it, for a card wide enough to say it.
    fn badge(self) -> Option<String> {
        self.mark().map(|mark| format!("{mark} {}", self.label()))
    }

    fn label(self) -> &'static str {
        match self {
            Self::Configured => "Configured",
            Self::NotConfigured => "Not configured",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Configured => theme::color(Token::Accent),
            Self::NotConfigured => theme::color(Token::TextMuted),
        }
    }
}

/// The drawer footer's note under the label: the state in the words the
/// person acts on. The distinction the card deliberately does not draw
/// lives here, because this is where doing something about it is — a
/// harness that is not on the machine is installed first, one that is
/// already here is set up.
fn status_note(harness: &HarnessHealth) -> &'static str {
    if !harness.detection.present {
        "Not found on this machine"
    } else if HarnessStatus::from(harness) == HarnessStatus::Configured {
        "Ready to receive plugins"
    } else {
        "Installed — set it up to receive plugins"
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
    let count = model
        .remembered
        .doctor
        .as_ref()
        .map_or(0, |d| d.harnesses.len());
    // The drawer overlays from the right rather than sharing a permanent
    // split, but the header/list still need to lay out *around* it when
    // it's open — otherwise their own right-aligned content runs straight
    // under the drawer and gets clipped mid-word by its Clear. Its initial
    // width is an even split; dragging the divider lets either panel take
    // priority for the task at hand.
    // Shown whenever there is a harness to describe: the drawer is the
    // screen's detail column, not something opened and closed.
    let drawer_shown = model.selected_harness().is_some();
    let drawer_width = if drawer_shown {
        super::drawer_width(ResizablePanel::HarnessDrawer, model, area)
    } else {
        0
    };
    let list_area = Rect::new(
        area.x,
        area.y,
        area.width
            .saturating_sub(drawer_width)
            .saturating_sub(if drawer_shown { 1 } else { 0 }),
        area.height,
    );
    let content = render_screen_header(
        frame,
        list_area,
        Route::Harnesses,
        Some(Span::styled(
            format!("{count} installed"),
            theme::fg(Token::TextMuted),
        )),
    );

    let mut y = content.y;
    let bottom = content.y + content.height;

    if y + 2 <= bottom {
        let filter_area = Rect::new(content.x, y, content.width, 2);
        hits.push((filter_area, Hit::FocusFilter));
        super::filter_box(
            frame,
            filter_area,
            &model.remembered.harness_screen.filter,
            "Filter integrations…",
            model.filtering,
        );
        y += 3;
    }

    match &model.remembered.doctor {
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
                                model.remembered.harness_screen.filter.trim()
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
                    let selected = position == model.remembered.harness_screen.selected;
                    let status = HarnessStatus::from(harness);
                    render_harness_card(frame, rect, harness, status, selected, hits, position);
                }

                let rows = (visible.len() as u16).div_ceil(columns);
                y += rows * (card_height + gap);
            }
            if let Some(status) = &model.remembered.context_status
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

    if let Some(harness) = model.selected_harness() {
        render_harness_drawer(frame, area, model, harness, hits);
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
    // Right-aligned beside the name, and only what fits there with a gap
    // between the two: a card the drawer has narrowed wears the mark
    // alone, because a word clipped mid-way and glued to the name
    // ("Claude Code✓ Configur") says less than the mark on its own does.
    // No mark at all is a harness UZE has not configured.
    let mut title = vec![name];
    let fits = |text: &Option<String>| {
        text.as_deref().is_some_and(|text| {
            title[0].width() + 1 + Span::raw(text).width() <= inner.width as usize
        })
    };
    let badge = status.badge();
    let mark = status.mark();
    let worn = if fits(&badge) {
        badge
    } else if fits(&mark) {
        mark
    } else {
        None
    };
    if let Some(worn) = worn {
        let worn = Span::styled(worn, Style::default().fg(status.color()));
        let gap = inner
            .width
            .saturating_sub((title[0].width() + worn.width()) as u16);
        title.push(Span::raw(" ".repeat(gap as usize)));
        title.push(worn);
    }
    frame.render_widget(
        Paragraph::new(Line::from(title)),
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
    content: Rect,
    model: &TuiModel,
    harness: &HarnessHealth,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let status = HarnessStatus::from(harness);
    let offers = harness.offers();
    let (inner, footer) = super::drawer_body_and_footer(
        super::drawer(frame, content, ResizablePanel::HarnessDrawer, model, hits),
        &offers,
    );
    render_drawer_footer(
        frame,
        footer,
        DrawerStatus {
            color: status.color(),
            headline: status.label(),
            subtitle: status_note(harness),
        },
        &offers,
        model.hovered_offer,
        None,
        hits,
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
            format!("{} PATH shadowed", theme::glyph(Symbol::MarkAttention)),
            theme::fg(Token::StateWarning),
        ),
        ContextMechanism::Unsupported => (
            format!("{} Not supported", theme::glyph(Symbol::MarkUnsupported)),
            theme::fg(Token::StateDanger),
        ),
    }
}

fn capability_status(capabilities: &HarnessCapabilities, kind: CapabilityKind) -> (String, Style) {
    if capabilities.native.contains(&kind) {
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

    /// A shim that is not first on `PATH` is a fact about this machine's
    /// environment, and it used to be the whole of what a card said —
    /// `PATH shadowed`, in warning colour, over a harness that was set up
    /// and working. The card answers what the person can act on; where the
    /// shim actually matters is per resource, and the drawer's own
    /// compatibility rows still say it (see
    /// `a_shadowed_shim_reads_as_an_environment_warning`).
    #[test]
    fn a_shadowed_shim_is_drawer_detail_rather_than_the_cards_state() {
        let harness = configured_harness(false);
        let status = HarnessStatus::from(&harness);

        assert_eq!(status, HarnessStatus::Configured);
        assert_eq!(status.label(), "Configured");
        assert_eq!(status_note(&harness), "Ready to receive plugins");
        assert_eq!(
            status.badge(),
            Some(format!("{} Configured", theme::glyph(Symbol::MarkOk)))
        );
    }

    /// One state, two shapes. A harness nobody installed and one the
    /// person installed themselves and never handed to UZE are the same
    /// answer to what the list asks, and neither wears a mark: a card
    /// carrying nothing is already "not configured", and marking it too
    /// would put a warning on most of the catalog with nothing wrong.
    #[test]
    fn nothing_uze_configured_carries_a_badge_and_both_shapes_are_said_in_the_drawer() {
        let mut absent = configured_harness(true);
        absent.detection.present = false;
        let mut theirs = configured_harness(true);
        theirs.setup = "not configured".to_owned();

        for harness in [&absent, &theirs] {
            let status = HarnessStatus::from(harness);
            assert_eq!(status, HarnessStatus::NotConfigured);
            assert_eq!(status.label(), "Not configured");
            assert_eq!(status.badge(), None, "and the card wears nothing");
            assert_eq!(status.color(), theme::color(Token::TextMuted));
        }

        assert_eq!(status_note(&absent), "Not found on this machine");
        assert_eq!(
            status_note(&theirs),
            "Installed — set it up to receive plugins",
            "the drawer is where doing something about it is"
        );
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
            format!("{} PATH shadowed", theme::glyph(Symbol::MarkAttention))
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
