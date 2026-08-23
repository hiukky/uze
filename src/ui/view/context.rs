//! TUI view — Context route.

use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::application::HarnessContextDelivery;

use super::super::model::TuiModel;
use super::super::{ACCENT, MUTED, TEXT_DIM, TEXT_PRIMARY, WARNING};
use super::super::{content_area, render_divided_row, render_screen_header};
use super::overview::{portability_label, portability_style};

pub(crate) fn render_context(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
    let area = content_area(area);
    let content = render_screen_header(frame, area, "Context", "AGENTS.md bridges", None);

    let Some(status) = &model.context_status else {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "Press a to analyze this project.",
                Style::default().fg(MUTED),
            )),
            Rect::new(content.x, content.y, content.width, 1),
        );
        return;
    };

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Portability  ", Style::default().fg(MUTED)),
            Span::styled(
                portability_label(&status.portability),
                portability_style(Some(status)),
            ),
        ])),
        Rect::new(content.x, content.y, content.width, 1),
    );

    let mut y = content.y + 2;
    let bottom = content.y + content.height;
    for harness in &status.harnesses {
        if y >= bottom {
            break;
        }
        let (label, color, detail) = match &harness.delivery {
            HarnessContextDelivery::Native => (
                "Native",
                ACCENT,
                "Reads AGENTS.md directly, no translation".to_owned(),
            ),
            HarnessContextDelivery::Bridge { needed, state } => {
                use crate::integration::AttachmentState;
                let label = match state {
                    AttachmentState::Matched => "Bridged",
                    AttachmentState::Missing => "Missing",
                    AttachmentState::Drifted => "Drifted",
                    AttachmentState::Conflict => "Conflict",
                    AttachmentState::Blocked => "Blocked",
                };
                let color = match state {
                    AttachmentState::Matched => ACCENT,
                    _ if *needed => WARNING,
                    _ => TEXT_DIM,
                };
                let detail = if *needed {
                    format!("Bridge into its own file — {label:?}", label = state).to_lowercase()
                } else {
                    "Bridge not currently needed".to_owned()
                };
                (label, color, detail)
            }
            HarnessContextDelivery::NotDetected => {
                ("—", TEXT_DIM, "Harness not detected".to_owned())
            }
        };

        let line = Line::from(vec![
            Span::styled(
                format!("{:<16}", harness.display_name),
                Style::default().fg(TEXT_PRIMARY),
            ),
            Span::styled(format!("{detail:<44}"), Style::default().fg(MUTED)),
            Span::styled(format!("{label:>10}"), Style::default().fg(color)),
        ]);
        y = render_divided_row(frame, content, y, line);
    }

    if !status.warnings.is_empty() && y < bottom {
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
