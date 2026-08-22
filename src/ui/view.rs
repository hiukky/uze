//! TUI view — extracted without semantic change.

use std::collections::BTreeMap;

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Padding, Paragraph},
};

use crate::application::PluginCapability;
use uze_core::capability::CapabilityKind;

use super::{MUTED, SURFACE_RAISED};

pub mod context;
pub mod doctor;
pub mod harnesses;
pub mod marketplace;
pub mod overview;
pub mod plugins;

/// A short, human-facing label for a resource kind — used wherever a
/// plugin's capabilities are grouped for display (never for exposure
/// naming, which is `IntegrationPort::exposure_name_candidates`'s own
/// decision).
pub(crate) fn capability_kind_label(kind: CapabilityKind) -> &'static str {
    match kind {
        CapabilityKind::Instruction => "Instructions",
        CapabilityKind::AgentSkill => "Skills",
        CapabilityKind::Mcp => "MCP Servers",
        CapabilityKind::Agent => "Agents",
        CapabilityKind::Action => "Actions",
        CapabilityKind::Hook => "Hooks",
        CapabilityKind::Policy => "Policies",
    }
}

/// The three resource kinds every plugin could plausibly declare, always
/// shown in this fixed order (even empty, as a `–` placeholder) so the
/// Resources card reads as a stable table rather than a list that
/// reshuffles/disappears entirely for a plugin with nothing to show yet.
const CORE_KINDS: [CapabilityKind; 3] = [
    CapabilityKind::AgentSkill,
    CapabilityKind::Agent,
    CapabilityKind::Mcp,
];

/// Appends a grouped-by-kind, indented listing of `capabilities` to `lines`:
/// the three core kinds (Skills/Agents/MCP Servers) always appear, in that
/// order, each as its own label-headed group with a `–` placeholder when
/// empty; any other kind actually present (Instructions/Actions/Hooks/
/// Policies) is appended after, only when non-empty. Groups are separated
/// by blank lines (not drawn dividers), and only the resource's own
/// logical/file name is shown — an MCP server groups as one row here, since
/// the individual tools it exposes are runtime-discovered by the harness
/// that connects to it, not declared anywhere UZE reads.
pub(crate) fn push_capability_table(lines: &mut Vec<Line<'_>>, capabilities: &[PluginCapability]) {
    let mut grouped: BTreeMap<CapabilityKind, Vec<&PluginCapability>> = BTreeMap::new();
    for capability in capabilities {
        grouped.entry(capability.kind).or_default().push(capability);
    }

    let mut ordered_kinds: Vec<CapabilityKind> = CORE_KINDS.to_vec();
    for kind in grouped.keys() {
        if !ordered_kinds.contains(kind) {
            ordered_kinds.push(*kind);
        }
    }

    let mut first = true;
    for kind in ordered_kinds {
        let items = grouped.get(&kind);
        if items.is_none() && !CORE_KINDS.contains(&kind) {
            continue;
        }
        if !first {
            lines.push(Line::from(""));
        }
        first = false;
        lines.push(Line::from(Span::styled(
            capability_kind_label(kind),
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        )));
        match items {
            Some(items) => {
                for item in items {
                    lines.push(Line::from(format!("  › {}", item.name)));
                }
            }
            None => lines.push(Line::from(Span::styled("  –", Style::default().fg(MUTED)))),
        }
    }
}

/// The raised status card pinned under a detail panel's content — same
/// shape for the Marketplace and Plugins routes, each computing its own
/// `(color, headline, subtitle)` from its own domain state (install/update
/// for a catalog entry; health/update for an installed one). A colored dot
/// plus a bold headline on a slightly lighter slab, no border.
pub(crate) fn render_status_card(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    color: Color,
    headline: &str,
    subtitle: &str,
) {
    let block = Block::default()
        .style(Style::default().bg(SURFACE_RAISED))
        .padding(Padding::new(1, 1, 1, 0));
    let lines = vec![
        Line::from(vec![
            Span::styled("● ", Style::default().fg(color)),
            Span::styled(
                headline.to_owned(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(Span::styled(
            subtitle.to_owned(),
            Style::default().fg(MUTED),
        )),
    ];
    frame.render_widget(Paragraph::new(lines).block(block), area);
}
