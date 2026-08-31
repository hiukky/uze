//! TUI view — Extensions route.
//!
//! The tool side of the product: official uze extensions that extend the
//! TUI/CLI itself (as opposed to plugins, which are agentic packages
//! delivered *to* harnesses — see `view::plugins`). Rows come straight
//! from `uze_extensions::registry::ExtensionRegistry::builtin`, the one
//! composition root that knows the extension set, so nothing here is
//! hand-maintained. Today every entry is bundled with the binary (there is
//! no loading/enablement surface yet); a responsive catalog of compact cards,
//! and the detail drawer opens on selection the same way Plugins/Harnesses
//! do — its content is static catalog metadata, so there is nothing to fetch.

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use super::super::hit::Hit;
use super::super::model::{ResizablePanel, TuiModel};
use super::super::{
    ACCENT, BASE, BLUE, BORDER, MUTED, SELECTED_BG, SURFACE_OVERLAY, TEXT_BRIGHT, TEXT_PRIMARY,
    TEXT_SECONDARY,
};
use super::super::{content_area, render_screen_header};
use super::render_status_line;

pub(crate) fn render_extensions(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let content = content_area(area);
    let content = render_screen_header(
        frame,
        content,
        "Extensions",
        "official tool extensions",
        Some(Span::styled(
            format!("{} bundled", model.extensions.len()),
            Style::default().fg(MUTED),
        )),
    );
    let drawer_area = area_for_drawer(area);
    let drawer_open = model.extension_drawer_open && model.selected_extension().is_some();
    let drawer_width = drawer_open.then(|| {
        model
            .extension_drawer_width
            .unwrap_or(52)
            .clamp(24, drawer_area.width.saturating_sub(24).max(24))
    });
    let catalog_width = content.width.saturating_sub(drawer_width.unwrap_or(0));
    let filter_area = Rect::new(content.x, content.y, catalog_width, 2);
    render_filter_box(frame, filter_area, model);
    let catalog_area = Rect::new(
        content.x,
        content.y.saturating_add(3),
        catalog_width,
        content.height.saturating_sub(3),
    );

    if model.extensions.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "No extensions available.",
                Style::default().fg(MUTED),
            )),
            catalog_area,
        );
    } else {
        let visible = model.extension_visible_indices();
        if visible.is_empty() {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    format!("No extensions match \"{}\".", model.extension_filter.trim()),
                    Style::default().fg(MUTED),
                )),
                catalog_area,
            );
        }
        let columns = if catalog_area.width >= 110 {
            3
        } else if catalog_area.width >= 72 {
            2
        } else {
            1
        };
        let gap = 1;
        let card_width = (catalog_area.width.saturating_sub(gap * (columns - 1))) / columns;
        let card_height = 7;
        for (position, extension_index) in visible.iter().enumerate() {
            let column = position as u16 % columns;
            let row = position as u16 / columns;
            let rect = Rect::new(
                catalog_area.x + column * (card_width + gap),
                catalog_area.y + row * (card_height + gap),
                card_width,
                card_height,
            );
            if rect.y + rect.height > catalog_area.y + catalog_area.height {
                break;
            }
            let selected = position == model.extensions_selected;
            render_extension_card(
                frame,
                rect,
                &model.extensions[*extension_index],
                selected,
                hits,
                position,
            );
        }
    }

    if drawer_open && let Some(extension) = model.selected_extension() {
        render_extension_drawer(
            frame,
            drawer_area,
            drawer_width.unwrap_or_default(),
            model,
            extension,
            hits,
        );
    }
}

/// The drawer overlays from the *original* (unshrunk) route area, matching
/// how Plugins/Harnesses anchor theirs — `content_area` already insets
/// once for the list; re-deriving here keeps the drawer's own inset
/// independent of how much of that area the header consumed.
fn area_for_drawer(area: Rect) -> Rect {
    content_area(area)
}

fn render_filter_box(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(if model.filtering { ACCENT } else { BORDER }));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let text = if model.extension_filter.is_empty() {
        Line::from(Span::styled(
            "Filter extensions…",
            Style::default().fg(MUTED),
        ))
    } else {
        let mut spans = vec![Span::styled(
            model.extension_filter.clone(),
            Style::default().fg(TEXT_PRIMARY),
        )];
        if model.filtering {
            spans.push(Span::styled("▏", Style::default().fg(ACCENT)));
        }
        Line::from(spans)
    };
    frame.render_widget(Paragraph::new(text), inner);
}

fn render_extension_card(
    frame: &mut ratatui::Frame<'_>,
    rect: Rect,
    extension: &uze_extensions::registry::BuiltinExtension,
    selected: bool,
    hits: &mut Vec<(Rect, Hit)>,
    index: usize,
) {
    let background = if selected { SELECTED_BG } else { BASE };
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
        extension.name,
        Style::default()
            .fg(if selected { TEXT_BRIGHT } else { TEXT_PRIMARY })
            .add_modifier(Modifier::BOLD),
    );
    let badge = Span::styled("✓ Official", Style::default().fg(BLUE));
    let gap = inner
        .width
        .saturating_sub((name.width() + badge.width()) as u16);
    let header = Line::from(vec![name, Span::raw(" ".repeat(gap as usize)), badge]);
    frame.render_widget(
        Paragraph::new(header),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            extension.description,
            Style::default().fg(TEXT_SECONDARY),
        ))
        .wrap(Wrap { trim: true }),
        Rect::new(inner.x, inner.y + 1, inner.width, 2),
    );
    let tags = Line::from(vec![
        Span::styled(
            format!(" {} ", extension.surface),
            Style::default().fg(MUTED),
        ),
        Span::raw(" "),
        Span::styled(" Built-in ", Style::default().fg(MUTED)),
    ]);
    frame.render_widget(
        Paragraph::new(tags),
        Rect::new(inner.x, inner.y + 4, inner.width, 1),
    );
    hits.push((rect, Hit::ExtensionRow(index)));
}

fn render_extension_drawer(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    width: u16,
    model: &TuiModel,
    extension: &uze_extensions::registry::BuiltinExtension,
    hits: &mut Vec<(Rect, Hit)>,
) {
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
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(
                if model.dragging_panel == Some(ResizablePanel::ExtensionDrawer) {
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
            Hit::ResizePanel(ResizablePanel::ExtensionDrawer),
        ),
    );
    // Same sectioning as the Plugins drawer: a body that scrolls/clips
    // naturally and a fixed 3-row status block beneath it, so the two can
    // never overlap regardless of terminal height.
    let sections_x = drawer.x + 2;
    let sections_width = drawer.width.saturating_sub(3);
    let status_height = 3;
    let body = Rect::new(
        sections_x,
        drawer.y + 1,
        sections_width,
        drawer.height.saturating_sub(2 + status_height),
    );
    let status = Rect::new(
        sections_x,
        body.y + body.height,
        sections_width,
        status_height,
    );

    let lines = vec![
        Line::from(Span::styled(
            "EXTENSION",
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            extension.name,
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            extension.description,
            Style::default().fg(TEXT_SECONDARY),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "SURFACE",
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            extension.surface,
            Style::default().fg(TEXT_PRIMARY),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "HOW TO OPEN",
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            extension.usage,
            Style::default().fg(TEXT_SECONDARY),
        )),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), body);

    render_status_line(
        frame,
        status,
        ACCENT,
        "Bundled",
        "Ships with uze — always available",
    );
}
