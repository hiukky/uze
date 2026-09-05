//! Contextual harness support dropdown for an active agent session.

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Padding, Paragraph},
};
use uze_application::{CapabilityKind, HarnessCapabilities};

use uze_application::application::{
    AgentContextStatus, HarnessHealth, ProfileSummary, ResourceDelivery, UndeliveredReason,
};

use super::{ACCENT, BASE, BORDER, MUTED, TEXT_BRIGHT, WARNING};

/// The small, immutable slice of the read model one workspace agent tab
/// needs. Capabilities come from `HarnessHealth` (machine-scoped, the same
/// model the Harnesses screen renders); context delivery comes from
/// `AgentContextStatus`, resolved against *this agent pane's own working
/// directory* rather than the session's attach root — see
/// `uze_application::application::agent_context`.
pub(super) struct AgentSupport {
    display_name: String,
    present: bool,
    capabilities: HarnessCapabilities,
    instructions: State,
    instructions_label: &'static str,
    agents_directory: State,
    agents_directory_label: &'static str,
    profile: String,
}

#[derive(Clone, Copy)]
enum State {
    Ready,
    /// Nothing to deliver and nothing wrong — the project simply does not
    /// carry this resource. Kept distinct from every other state because
    /// collapsing it into an error is precisely how an empty project came
    /// to read as a broken harness.
    Neutral,
    Warning,
    Error,
}

impl AgentSupport {
    pub(super) fn resolve(
        health: HarnessHealth,
        context: &AgentContextStatus,
        profile: Option<&ProfileSummary>,
    ) -> Self {
        let (instructions, instructions_label) = describe(&context.instructions);
        let (agents_directory, agents_directory_label) = describe(&context.agents_directory);
        Self {
            // Identity and presence come from the resolution itself, not
            // from `health`, so the popup can never label one harness's
            // rows with another's name.
            display_name: context.display_name.clone(),
            present: context.present,
            capabilities: health.capabilities,
            instructions,
            instructions_label,
            agents_directory,
            agents_directory_label,
            profile: profile
                .map(|profile| profile.id.clone())
                .unwrap_or_else(|| "default".to_owned()),
        }
    }
}

/// One `ResourceDelivery` as a reader sees it. Every label names the
/// mechanism or the specific reason there is none — never a bare
/// "unavailable", which read as the harness being broken when the real
/// answer was "this project has no AGENTS.md" or "your PATH resolves
/// `claude` to the real binary before UZE's shim".
fn describe(delivery: &ResourceDelivery) -> (State, &'static str) {
    match delivery {
        ResourceDelivery::Native => (State::Ready, "native"),
        ResourceDelivery::Projected => (State::Ready, "loaded (shim)"),
        ResourceDelivery::Bridged => (State::Ready, "loaded (bridge)"),
        ResourceDelivery::AbsentFromProject => (State::Neutral, "none in project"),
        ResourceDelivery::Undelivered(reason) => match reason {
            UndeliveredReason::HarnessAbsent => (State::Error, "harness not installed"),
            UndeliveredReason::ShimShadowed => (State::Warning, "shim not on PATH"),
            UndeliveredReason::Bridge(_) => (State::Warning, "not loaded"),
            UndeliveredReason::Unsupported => (State::Error, "not supported"),
        },
    }
}

pub(super) fn render(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    anchor: Rect,
    support: &AgentSupport,
) {
    // Interior padding beyond the border itself — this used to sit flush
    // against the frame, which read as cramped once the horizontal rules
    // below were removed and had nothing to separate the sections but
    // whitespace.
    const H_PAD: u16 = 2;
    const V_PAD: u16 = 1;

    let width = 40.min(area.width).max(1);
    let inner_width = width.saturating_sub(2 + 2 * H_PAD) as usize;

    // "agent", not "support": what this panel answers is what the agent
    // in front of the operator is running on and what reaches it here —
    // the harness, this checkout's context, the capabilities delivered.
    // "Support" named the read model behind it (`AgentSupport`), which is
    // this codebase's word, not the operator's question.
    let mut lines = vec![
        super::title_row("agent", "esc", inner_width),
        Line::default(),
    ];

    lines.push(section_header("RUNTIME"));
    lines.push(fact_line(
        harness_state(support),
        "Harness",
        &support.display_name,
        inner_width,
    ));
    lines.push(fact_line(
        support.instructions,
        "AGENTS.md",
        support.instructions_label,
        inner_width,
    ));
    lines.push(fact_line(
        support.agents_directory,
        ".agents",
        support.agents_directory_label,
        inner_width,
    ));
    lines.push(fact_line(
        State::Ready,
        "Profile",
        &support.profile,
        inner_width,
    ));

    lines.push(Line::default());
    lines.push(section_header("CAPABILITIES"));

    // Policy is a known `CapabilityKind` but no integration populates it yet
    // (see `crates/uze-integrations`) — showing it here would always read
    // "unavailable" regardless of the harness, which isn't real data.
    for capability in [
        CapabilityKind::AgentSkill,
        CapabilityKind::Mcp,
        CapabilityKind::Hook,
        CapabilityKind::Agent,
    ] {
        let state = capability_state(support, capability);
        lines.push(capability_line(
            state.row_state(),
            capability_label(capability),
            state.label(),
            inner_width,
        ));
        if matches!(state, CapabilityState::Unavailable) {
            lines.push(reason_line(support, capability, inner_width));
        }
    }

    let height = (lines.len() as u16 + 2 + 2 * V_PAD).min(area.height).max(1);
    let popup = Rect::new(
        anchor
            .x
            .saturating_sub(width.saturating_sub(anchor.width))
            .min(area.right().saturating_sub(width)),
        (anchor.y + anchor.height).min(area.bottom().saturating_sub(height)),
        width,
        height,
    );
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(BASE))
        .padding(Padding::new(H_PAD, H_PAD, V_PAD, V_PAD));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    frame.render_widget(Paragraph::new(lines), inner);
}

fn harness_state(support: &AgentSupport) -> State {
    if support.present {
        State::Ready
    } else {
        State::Error
    }
}

fn section_header(label: &'static str) -> Line<'static> {
    Line::from(Span::styled(
        label,
        Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
    ))
}

/// Lays out one `<icon> <label> ... <value>` row, right-aligning `value`
/// within `width` — the shape every row in this popup shares. `fact_line`
/// and `capability_line` only differ in which styles they hand in for
/// `label`/`value`; the icon, clipping, and gap math live here once.
fn styled_row(
    state: State,
    label: &str,
    label_style: Style,
    value: &str,
    value_style: Style,
    width: usize,
) -> Line<'static> {
    let (icon, icon_color) = icon_for(state);
    let value = clip(value, width.saturating_sub(3));
    let gap = width
        .saturating_sub(2 + label.chars().count() + value.chars().count())
        .max(1);
    Line::from(vec![
        Span::styled(format!("{icon} "), Style::default().fg(icon_color)),
        Span::styled(label.to_owned(), label_style),
        Span::raw(" ".repeat(gap)),
        Span::styled(value, value_style),
    ])
}

/// A runtime fact row: label and value both read as plain information, only
/// the leading icon carries state color — used for things like the active
/// profile, never anything the user needs to act on.
fn fact_line(state: State, label: &str, value: &str, width: usize) -> Line<'static> {
    let plain = Style::default().fg(TEXT_BRIGHT);
    styled_row(state, label, plain, value, plain, width)
}

/// A capability status row: the value color itself carries the severity —
/// muted for the unremarkable "supported"/"limited" states, a loud danger
/// color for "unavailable" — and an unavailable capability's own label is
/// struck through to read as switched off.
fn capability_line(state: State, label: &str, value: &str, width: usize) -> Line<'static> {
    let label_style = match state {
        State::Error => Style::default()
            .fg(MUTED)
            .add_modifier(Modifier::CROSSED_OUT),
        _ => Style::default().fg(TEXT_BRIGHT),
    };
    let value_style = match state {
        State::Ready | State::Neutral | State::Warning => Style::default().fg(MUTED),
        State::Error => Style::default()
            .fg(super::DANGER)
            .add_modifier(Modifier::BOLD),
    };
    styled_row(state, label, label_style, value, value_style, width)
}

fn reason_line(support: &AgentSupport, capability: CapabilityKind, width: usize) -> Line<'static> {
    let text = format!(
        "{} does not expose {}",
        support.display_name,
        capability_label(capability).to_lowercase()
    );
    Line::from(Span::styled(
        format!("  {}", clip(&text, width.saturating_sub(2))),
        Style::default().fg(MUTED),
    ))
}

fn icon_for(state: State) -> (&'static str, ratatui::style::Color) {
    match state {
        State::Ready => ("✓", ACCENT),
        State::Neutral => ("·", MUTED),
        State::Warning => ("!", WARNING),
        State::Error => ("✕", super::DANGER),
    }
}

/// A harness capability's support level. Deliberately its own enum rather
/// than a reuse of [`State`]: a capability is a property of the harness
/// alone, so `State::Neutral` — "this project doesn't carry the resource" —
/// has no meaning here and must not be representable.
#[derive(Clone, Copy)]
enum CapabilityState {
    Supported,
    Limited,
    Unavailable,
}

impl CapabilityState {
    fn label(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::Limited => "limited",
            Self::Unavailable => "unavailable",
        }
    }

    fn row_state(self) -> State {
        match self {
            Self::Supported => State::Ready,
            Self::Limited => State::Warning,
            Self::Unavailable => State::Error,
        }
    }
}

fn capability_state(support: &AgentSupport, kind: CapabilityKind) -> CapabilityState {
    let capabilities = &support.capabilities;
    if capabilities.direct_standard.contains(&kind) || capabilities.native.contains(&kind) {
        CapabilityState::Supported
    } else if capabilities.adaptable.contains(&kind) || capabilities.degraded.contains(&kind) {
        CapabilityState::Limited
    } else {
        CapabilityState::Unavailable
    }
}

fn clip(value: &str, max: usize) -> String {
    let mut clipped = value.chars().take(max).collect::<String>();
    if value.chars().count() > max {
        clipped.pop();
        clipped.push('…');
    }
    clipped
}

fn capability_label(kind: CapabilityKind) -> &'static str {
    match kind {
        CapabilityKind::Instruction => "Instructions",
        CapabilityKind::AgentSkill => "Skills",
        CapabilityKind::Mcp => "MCP",
        CapabilityKind::Agent => "Agents",
        CapabilityKind::Hook => "Hooks",
        CapabilityKind::Policy => "Policies",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uze_application::application::{
        AgentContextStatus, ContextMechanism, HarnessContextSupport, HarnessHealth,
    };
    use uze_core::integration::{AttachmentState, HarnessDetection, PublicationStatus};

    fn health(present: bool) -> HarnessHealth {
        HarnessHealth {
            integration: "claude-code".to_owned(),
            display_name: "Claude Code".to_owned(),
            description: String::new(),
            detection: HarnessDetection {
                present,
                version: Some("2.0.0".to_owned()),
            },
            setup: "installed".to_owned(),
            strategy: None,
            provisioning: None,
            publication: PublicationStatus::NotApplicable,
            capabilities: HarnessCapabilities::default(),
            runtime_shim_active: true,
            context_support: HarnessContextSupport {
                instructions: ContextMechanism::RuntimeShim,
                agents_directory: ContextMechanism::RuntimeShim,
            },
        }
    }

    fn support(
        present: bool,
        instructions: ResourceDelivery,
        agents_directory: ResourceDelivery,
    ) -> AgentSupport {
        let context = AgentContextStatus {
            integration: "claude-code".to_owned(),
            display_name: "Claude Code".to_owned(),
            present,
            root: std::path::PathBuf::from("/project"),
            instructions,
            agents_directory,
        };
        AgentSupport::resolve(health(present), &context, None)
    }

    #[test]
    fn a_shim_projection_reads_as_loaded_for_both_resources() {
        let support = support(
            true,
            ResourceDelivery::Projected,
            ResourceDelivery::Projected,
        );
        assert_eq!(support.instructions_label, "loaded (shim)");
        assert!(matches!(support.instructions, State::Ready));
        assert_eq!(support.agents_directory_label, "loaded (shim)");
        assert!(matches!(support.agents_directory, State::Ready));
    }

    #[test]
    fn a_project_without_the_resource_is_neutral_not_an_error() {
        // The regression this popup was reported for: a project with no
        // AGENTS.md rendered a red "not supported" row, which reads as the
        // harness being broken rather than the project simply not carrying
        // one. Each resource is answered on its own, so a project with only
        // `.agents/` still shows that half as delivered.
        let support = support(
            true,
            ResourceDelivery::AbsentFromProject,
            ResourceDelivery::Projected,
        );
        assert_eq!(support.instructions_label, "none in project");
        assert!(matches!(support.instructions, State::Neutral));
        assert_eq!(support.agents_directory_label, "loaded (shim)");
        assert!(matches!(support.agents_directory, State::Ready));
    }

    #[test]
    fn a_shadowed_shim_is_reported_as_a_path_problem() {
        // The most common "why is this amber": the harness is present but
        // this process's PATH resolves its name to the real binary first,
        // so a launch would bypass UZE. The row must say that, not
        // "unavailable"/"not supported".
        let support = support(
            true,
            ResourceDelivery::Undelivered(UndeliveredReason::ShimShadowed),
            ResourceDelivery::Undelivered(UndeliveredReason::ShimShadowed),
        );
        assert_eq!(support.instructions_label, "shim not on PATH");
        assert!(matches!(support.instructions, State::Warning));
        assert_eq!(support.agents_directory_label, "shim not on PATH");
        assert!(matches!(support.agents_directory, State::Warning));
    }

    #[test]
    fn an_absent_harness_names_itself_rather_than_the_project() {
        let support = support(
            false,
            ResourceDelivery::Undelivered(UndeliveredReason::HarnessAbsent),
            ResourceDelivery::Undelivered(UndeliveredReason::HarnessAbsent),
        );
        assert_eq!(support.instructions_label, "harness not installed");
        assert!(matches!(support.instructions, State::Error));
    }

    #[test]
    fn a_native_reader_and_a_matched_bridge_both_read_as_delivered() {
        let native = support(true, ResourceDelivery::Native, ResourceDelivery::Native);
        assert_eq!(native.instructions_label, "native");
        assert!(matches!(native.instructions, State::Ready));

        let bridged = support(
            true,
            ResourceDelivery::Bridged,
            ResourceDelivery::Undelivered(UndeliveredReason::Unsupported),
        );
        assert_eq!(bridged.instructions_label, "loaded (bridge)");
        assert!(matches!(bridged.instructions, State::Ready));
        assert_eq!(bridged.agents_directory_label, "not supported");
    }

    #[test]
    fn a_broken_bridge_is_a_warning_naming_the_missing_delivery() {
        let support = support(
            true,
            ResourceDelivery::Undelivered(UndeliveredReason::Bridge(AttachmentState::Missing)),
            ResourceDelivery::AbsentFromProject,
        );
        assert_eq!(support.instructions_label, "not loaded");
        assert!(matches!(support.instructions, State::Warning));
    }
}
