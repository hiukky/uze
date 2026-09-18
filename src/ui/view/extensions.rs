//! TUI view — Extensions route.
//!
//! The tool side of the product: official uze extensions that extend the
//! TUI/CLI itself (as opposed to plugins, which are agentic packages
//! delivered *to* harnesses — see `view::plugins`). Rows come straight
//! from `uze_extensions::registry::ExtensionRegistry::builtin`, the one
//! composition root that knows the extension set, so nothing here is
//! hand-maintained. Today every entry is bundled with the binary (there is
//! no loading/enablement surface yet); a responsive catalog of compact cards,
//! and the detail drawer describes the selection the same way
//! Plugins/Harnesses do — its content is static catalog metadata, so there
//! is nothing to fetch.

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

use super::super::hit::Hit;
use super::super::model::{ResizablePanel, Route, TuiModel};
use super::super::{content_area, render_screen_header};
use super::{DrawerStatus, render_drawer_footer};
use crate::ui::theme::{self, Symbol, Token};

pub(crate) fn render_extensions(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let outer = content_area(area);
    // Shown whenever there is an extension to describe: the drawer is the
    // screen's detail column, not something opened and closed.
    let drawer_shown = model.selected_extension().is_some();
    let drawer_width =
        drawer_shown.then(|| super::drawer_width(ResizablePanel::ExtensionDrawer, model, outer));
    let header_width = outer
        .width
        .saturating_sub(drawer_width.unwrap_or(0))
        .saturating_sub(if drawer_shown { 1 } else { 0 });
    let header_area = Rect::new(outer.x, outer.y, header_width, outer.height);
    let content = render_screen_header(
        frame,
        header_area,
        Route::Extensions,
        Some(Span::styled(
            format!("{} bundled", model.extensions.len()),
            theme::fg(Token::TextMuted),
        )),
    );
    let filter_area = Rect::new(content.x, content.y, content.width, 2);
    super::filter_box(
        frame,
        filter_area,
        &model.remembered.extension_screen.filter,
        "Filter extensions…",
        model.filtering,
    );
    hits.push((filter_area, Hit::FocusFilter));
    let catalog_area = Rect::new(
        content.x,
        content.y.saturating_add(3),
        content.width,
        content.height.saturating_sub(3),
    );

    if model.extensions.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "No extensions available.",
                theme::fg(Token::TextMuted),
            )),
            catalog_area,
        );
    } else {
        let visible = model.extension_visible_indices();
        if visible.is_empty() {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    format!(
                        "No extensions match \"{}\".",
                        model.remembered.extension_screen.filter.trim()
                    ),
                    theme::fg(Token::TextMuted),
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
            let selected = position == model.remembered.extension_screen.selected;
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

    if let Some(extension) = model.selected_extension() {
        render_extension_drawer(frame, outer, model, extension, hits);
    }
}

fn render_extension_card(
    frame: &mut ratatui::Frame<'_>,
    rect: Rect,
    extension: &uze_extensions::registry::BuiltinExtension,
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
        extension.name,
        Style::default()
            .fg(if selected {
                theme::color(Token::TextBright)
            } else {
                theme::color(Token::TextPrimary)
            })
            .add_modifier(Modifier::BOLD),
    );
    let badge = Span::styled(
        format!("{} Official", theme::glyph(Symbol::MarkOfficial)),
        theme::fg(Token::StateInfo),
    );
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
            theme::fg(Token::TextSecondary),
        ))
        .wrap(Wrap { trim: true }),
        Rect::new(inner.x, inner.y + 1, inner.width, 2),
    );
    let tags = Line::from(vec![
        Span::styled(extension.surface, theme::fg(Token::TextMuted)),
        Span::raw("  "),
        Span::styled("Built-in", theme::fg(Token::TextMuted)),
    ]);
    frame.render_widget(
        Paragraph::new(tags),
        Rect::new(inner.x, inner.y + 4, inner.width, 1),
    );
    hits.push((rect, Hit::ExtensionRow(index)));
}

fn render_extension_drawer(
    frame: &mut ratatui::Frame<'_>,
    content: Rect,
    model: &TuiModel,
    extension: &uze_extensions::registry::BuiltinExtension,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let inner = super::drawer(frame, content, ResizablePanel::ExtensionDrawer, model, hits);
    let offers = uze_application::application::offers::extension_offers();
    let (body, status) = super::drawer_body_and_footer(inner, &offers);

    let lines = vec![
        Line::from(Span::styled("EXTENSION", theme::fg_bold(Token::TextMuted))),
        Line::from(Span::styled(
            extension.name,
            Style::default()
                .fg(theme::color(Token::TextBright))
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            extension.description,
            theme::fg(Token::TextSecondary),
        )),
        Line::from(""),
        Line::from(Span::styled("SURFACE", theme::fg_bold(Token::TextMuted))),
        Line::from(Span::styled(
            extension.surface,
            theme::fg(Token::TextPrimary),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "HOW TO OPEN",
            theme::fg_bold(Token::TextMuted),
        )),
        Line::from(Span::styled(
            extension.usage,
            theme::fg(Token::TextSecondary),
        )),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), body);

    render_drawer_footer(
        frame,
        status,
        DrawerStatus {
            color: theme::color(Token::Accent),
            headline: "Bundled",
            subtitle: "Ships with uze — always available",
        },
        &offers,
        model.hovered_offer,
        None,
        hits,
    );
}
