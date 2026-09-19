//! What the chrome vocabulary guarantees, checked against a real backend
//! rather than against the builders' own fields: a widget's promise is
//! what lands in the buffer.

use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use uze_theme::Token;

use super::{
    Align, Button, Chip, ChipState, Edge, RowState, Rule, Surface, action_index, button_row, mark,
    row, surface::fill, text,
};
use crate::ui::theme;

fn drawn(
    width: u16,
    height: u16,
    draw: impl FnOnce(&mut ratatui::Frame<'_>),
) -> ratatui::buffer::Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| draw(frame)).unwrap();
    terminal.backend().buffer().clone()
}

/// The inset is the surface's, not the caller's: two floating surfaces
/// asked for nothing in particular leave content the same room, which is
/// the whole reason this module exists.
#[test]
fn every_floating_surface_leaves_the_same_room() {
    let area = Rect::new(0, 0, 20, 8);
    let plain = drawn(20, 8, |frame| {
        assert_eq!(
            Surface::floating().render(frame, area),
            Rect::new(3, 2, 14, 5)
        );
    });
    let titled = drawn(20, 8, |frame| {
        assert_eq!(
            Surface::floating().title(" Theme ").render(frame, area),
            Rect::new(3, 2, 14, 5),
            "a title names the surface; it does not move its content"
        );
    });

    assert_eq!(plain[(0, 0)].symbol(), titled[(0, 0)].symbol());
}

/// A card is the same box without the inset — it is already inside
/// something that gave it room.
#[test]
fn a_card_insets_only_by_its_hairline() {
    drawn(20, 8, |frame| {
        assert_eq!(
            Surface::card().render(frame, Rect::new(0, 0, 20, 8)),
            Rect::new(1, 1, 18, 6)
        );
    });
}

#[test]
fn a_surface_draws_its_border_and_grounds_its_inside() {
    let buffer = drawn(10, 4, |frame| {
        Surface::card().render(frame, Rect::new(0, 0, 10, 4));
    });

    assert_eq!(buffer[(0, 0)].symbol(), "┌");
    assert_eq!(buffer[(9, 3)].symbol(), "┘");
    assert_eq!(
        buffer[(5, 1)].bg,
        theme::color(Token::SurfaceBackground),
        "the inside carries the surface's ground"
    );
}

/// The selectable card says where the keyboard is with its border, and
/// that is the only thing that changes: a card that is not selected still
/// occupies the same cells.
#[test]
fn a_selectable_card_states_selection_in_its_border_alone() {
    let area = Rect::new(0, 0, 12, 4);
    let selected = drawn(12, 4, |frame| {
        let inner = Surface::selectable(true).render(frame, area);
        assert_eq!(inner, Rect::new(1, 1, 10, 2));
    });
    let resting = drawn(12, 4, |frame| {
        let inner = Surface::selectable(false).render(frame, area);
        assert_eq!(inner, Rect::new(1, 1, 10, 2), "selection moves no content");
    });

    assert_eq!(
        selected[(0, 0)].symbol(),
        "╭",
        "rounded: chosen from, not read"
    );
    assert_eq!(selected[(0, 0)].fg, theme::color(Token::Accent));
    assert_eq!(resting[(0, 0)].fg, theme::color(Token::BorderFaint));
}

/// A rule divides; it does not enclose. The rect it answers with is beside
/// the hairline, on the side the content is.
#[test]
fn a_rule_takes_one_column_from_the_edge_it_runs_along() {
    let area = Rect::new(0, 0, 20, 6);

    drawn(20, 6, |frame| {
        assert_eq!(
            Rule::new(Edge::Right).render(frame, area),
            Rect::new(0, 0, 19, 6)
        );
    });
    drawn(20, 6, |frame| {
        assert_eq!(
            Rule::new(Edge::Left).render(frame, area),
            Rect::new(1, 0, 19, 6)
        );
    });
    drawn(20, 6, |frame| {
        assert_eq!(
            Rule::new(Edge::Top).render(frame, area),
            Rect::new(0, 1, 20, 5)
        );
    });
    drawn(20, 6, |frame| {
        assert_eq!(
            Rule::new(Edge::Bottom).render(frame, area),
            Rect::new(0, 0, 20, 5)
        );
    });
}

/// The drag feedback is the divider's own colour — the panel does not move
/// until the mouse does, so this is the only thing saying the grab took.
#[test]
fn a_draggable_rule_goes_accent_only_while_it_is_dragged() {
    let area = Rect::new(0, 0, 6, 2);
    let dragging = drawn(6, 2, |frame| {
        Rule::draggable(Edge::Right, true).render(frame, area);
    });
    let resting = drawn(6, 2, |frame| {
        Rule::draggable(Edge::Right, false).render(frame, area);
    });

    assert_eq!(dragging[(5, 0)].fg, theme::color(Token::Accent));
    assert_eq!(resting[(5, 0)].fg, theme::color(Token::BorderFaint));
}

/// One gap, one label padding, wherever a row of buttons is drawn.
#[test]
fn buttons_in_a_row_are_spaced_the_same_from_either_end() {
    let row = Rect::new(0, 0, 40, 1);
    let buttons = vec![
        (Button::new("Cancel", Token::TextSecondary), 1_u8),
        (Button::new("Delete", Token::StateDanger), 2),
    ];

    let left = drawn(40, 1, |frame| {
        let placed = button_row(frame, row, &buttons, Align::Left);
        assert_eq!(placed[0].0, Rect::new(0, 0, 10, 1));
        assert_eq!(placed[1].0, Rect::new(12, 0, 10, 1), "ten wide, two of gap");
    });
    assert_eq!(&left[(2, 0)].symbol(), &"C");

    drawn(40, 1, |frame| {
        let placed = button_row(frame, row, &buttons, Align::Right);
        assert_eq!(placed[1].0.right(), 40, "the last button meets the edge");
        assert_eq!(placed[0].0, Rect::new(18, 0, 10, 1));
    });
}

/// A right-aligned row cannot be truncated into something readable: the
/// answer that got cut is the one the reader needed to see.
#[test]
fn a_row_too_narrow_to_right_align_draws_nothing() {
    let row = Rect::new(0, 0, 12, 1);
    let buttons = vec![
        (Button::new("Cancel", Token::TextSecondary), 1_u8),
        (Button::new("Delete", Token::StateDanger), 2),
    ];

    drawn(12, 1, |frame| {
        assert!(button_row(frame, row, &buttons, Align::Right).is_empty());
    });
    drawn(12, 1, |frame| {
        assert_eq!(
            button_row(frame, row, &buttons, Align::Left).len(),
            1,
            "left-aligned places what fits and stops"
        );
    });
}

/// `width` is asked before there is a frame, to size the slot the button
/// will go in — so it has to agree with what the button then draws.
#[test]
fn a_buttons_measured_width_is_the_width_it_takes() {
    let button = Button::new("Preview", Token::Accent);

    assert_eq!(button.width(), 11);
    drawn(20, 1, |frame| {
        let placed = button_row(
            frame,
            Rect::new(0, 0, 20, 1),
            &[(button.clone(), ())],
            Align::Left,
        );
        assert_eq!(placed[0].0.width, button.width());
    });
}

#[test]
fn a_fill_paints_the_ground_and_leaves_no_hairline() {
    let buffer = drawn(4, 2, |frame| {
        fill(frame, Rect::new(0, 0, 4, 2), Token::SurfaceSelected);
    });

    for x in 0..4 {
        assert_eq!(buffer[(x, 0)].bg, theme::color(Token::SurfaceSelected));
        assert_eq!(buffer[(x, 0)].symbol(), " ");
    }
}

/// Selection and hover are two questions, and a row under both is where a
/// press would land — so selection is what the ground says.
#[test]
fn a_row_under_both_the_keyboard_and_the_pointer_reads_as_selected() {
    assert_eq!(RowState::of(true, true), RowState::Selected);
    assert_eq!(RowState::of(true, false), RowState::Selected);
    assert_eq!(RowState::of(false, true), RowState::Hovered);
    assert_eq!(RowState::of(false, false), RowState::Resting);
}

/// A resting row is left alone: the list it is in already has a ground,
/// and a row that painted its own would have to know which.
#[test]
fn a_resting_row_is_left_on_whatever_is_behind_it() {
    let mut spans = vec![ratatui::text::Span::raw("plugin")];
    row::fill(&mut spans, 20, RowState::Resting);
    assert_eq!(spans.len(), 1, "nothing is appended and nothing restyled");

    row::fill(&mut spans, 20, RowState::Selected);
    assert_eq!(spans.len(), 2, "a selected row is filled to the width");
    assert_eq!(
        spans[0].style.bg,
        Some(theme::color(Token::SurfaceSelected))
    );
    assert_eq!(
        spans.iter().map(ratatui::text::Span::width).sum::<usize>(),
        20,
        "the ground reaches the edge, or the highlight reads as ragged"
    );
}

/// Pressing is the one state that overrules a label's own colour: the hue
/// becomes the fill and the label drops to the backdrop.
#[test]
fn a_pressed_chip_inverts_and_the_others_keep_their_hue() {
    let hue = theme::color(Token::Accent);

    assert_eq!(ChipState::Resting.skin(hue).0, hue);
    assert_eq!(ChipState::Hovered.skin(hue).0, hue);
    assert_eq!(
        ChipState::Pressed.skin(hue),
        (theme::color(Token::SurfaceBackground), hue)
    );
    assert_eq!(
        ChipState::Static.skin(hue).1,
        theme::color(Token::SurfaceRecessed),
        "not a control: recessed, so the shape never promises a press"
    );
}

/// A chip's padding is part of the control — filled, hovered and clicked
/// like the glyphs are — so the rect it is measured into covers it.
#[test]
fn a_chip_is_measured_with_its_padding_and_sits_against_its_right_edge() {
    let chip = Chip::new("deliver", theme::color(Token::Accent), ChipState::Resting);

    assert_eq!(chip.width(), 9, "seven columns and one of air each side");
    let rect = chip.rect_ending_at(40, 3);
    assert_eq!(rect, Rect::new(31, 3, 9, 1));

    let buffer = drawn(40, 4, |frame| chip.render(frame, rect));
    assert_eq!(
        buffer[(31, 3)].bg,
        theme::color(Token::SurfaceRaised),
        "the padding cell carries the fill, not just the label"
    );
}

/// The mark's own width comes from the theme: a set spelling the ellipsis
/// with three periods takes three columns where `…` takes one, and a cut
/// measured against the wrong one overflows the column it had to fit.
#[test]
fn text_is_cut_to_the_room_there_is_including_the_mark() {
    assert_eq!(text::elide("short", 10), "short");

    let cut = text::elide("a subject line long enough to be cut", 10);
    assert_eq!(cut.chars().count(), 10);
    assert!(cut.ends_with(&theme::glyph(theme::Symbol::Ellipsis)));
}

/// Clipping keeps whole the spans that fit: a line is styled, and cutting
/// the string would lose which part was the key and which the description.
#[test]
fn clipping_a_line_keeps_the_spans_that_fit_and_elides_the_one_that_straddles() {
    let mut line = ratatui::text::Line::from(vec![
        ratatui::text::Span::raw("ctrl+k  "),
        ratatui::text::Span::raw("open everything you can do"),
    ]);
    text::clip(&mut line, 14);

    assert_eq!(line.spans.len(), 2, "the first span fit whole");
    assert_eq!(line.spans[0].content, "ctrl+k  ");
    assert_eq!(line.width(), 14);
}

/// A line shorter than the room is not touched at all.
#[test]
fn a_line_that_fits_is_left_exactly_as_it_was() {
    let mut line = ratatui::text::Line::from(vec![ratatui::text::Span::raw("fits")]);
    text::clip(&mut line, 20);

    assert_eq!(line.spans.len(), 1);
    assert_eq!(line.spans[0].content, "fits");
}

/// The marker takes the positive question, because half its callers hold
/// it that way — and the two that held the negative one wrote the branches
/// in the opposite order.
#[test]
fn the_disclosure_mark_answers_the_open_question() {
    assert_eq!(
        mark::disclosure(true),
        theme::glyph(theme::Symbol::ChevronExpanded)
    );
    assert_eq!(
        mark::disclosure(false),
        theme::glyph(theme::Symbol::ChevronCollapsed)
    );
}

/// The reader is searching for what they want to *do*, and the word for it
/// is as often in the description as in the label.
#[test]
fn the_index_narrows_on_the_description_as_well_as_the_label() {
    let all = uze_keys::active().available(&[uze_keys::Scope::Global]);
    assert!(!all.is_empty(), "the global scope binds something");

    assert_eq!(
        action_index::narrowed(all.clone(), "   ").len(),
        all.len(),
        "nothing typed narrows nothing"
    );

    let Some((action, _)) = all.first().copied() else {
        return;
    };
    let label = action.label();
    let by_label = action_index::narrowed(all.clone(), &label);
    assert!(by_label.iter().any(|(found, _)| *found == action));

    let description = action.description();
    let word = description
        .split_whitespace()
        .find(|word| word.len() > 4)
        .unwrap_or(&label);
    let by_description = action_index::narrowed(all, word);
    assert!(
        by_description.iter().any(|(found, _)| *found == action),
        "a word only the description carries still finds the action"
    );
}

/// An action with no key is a finished design, not a gap: the column it
/// would have filled is dimmed, so the blank never reads as a binding that
/// is there and empty. One of the two copies this replaced left it in the
/// accent.
#[test]
fn an_action_with_no_key_dims_the_column_it_would_have_filled() {
    let bound = uze_keys::active()
        .available(&[uze_keys::Scope::Global])
        .into_iter()
        .find(|(_, chord)| chord.is_some());
    let Some((action, chord)) = bound else {
        return;
    };
    let area = Rect::new(0, 0, 80, 24);
    // The row `render` answers with is where the key column starts, so the
    // cell is found the way a caller finds it rather than by arithmetic
    // this test would have to keep in step.
    let key_cell = |rows: &[action_index::Row]| {
        let mut at = None;
        let buffer = drawn(80, 24, |frame| {
            at = action_index::render(frame, area, rows, "", 0, |_| ())
                .first()
                .map(|(rect, ())| (rect.x, rect.y));
        });
        at.map(|(x, y)| buffer[(x, y)].fg)
    };

    assert_eq!(
        key_cell(&[(action, chord)]),
        Some(theme::color(Token::Accent)),
        "a bound action prints its chord in the accent"
    );
    assert_eq!(
        key_cell(&[(action, None)]),
        Some(theme::color(Token::TextDim)),
        "an unbound one is dimmed, never left reading as an empty binding"
    );
}
