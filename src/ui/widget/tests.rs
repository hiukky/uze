//! What the chrome vocabulary guarantees, checked against a real backend
//! rather than against the builders' own fields: a widget's promise is
//! what lands in the buffer.

use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use uze_theme::Token;

use super::{
    Align, Button, Chip, ChipState, Edge, Field, RowState, Rule, Surface, Toast, ToastKind,
    action_index, button_row, field, mark, row, surface::fill, text, toast,
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
            at = action_index::render(frame, area, rows, rows.len(), "", 0, |_| ())
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

/// The state that decides whether something reads as an input is the one
/// with nothing in it. A field holding text is obvious; a field holding
/// nothing is a caption unless the caret says otherwise.
#[test]
fn an_empty_focused_field_still_shows_where_typing_would_land() {
    let empty = Field::new("", "type to narrow");
    let spans = empty.spans();

    assert_eq!(spans.len(), 2, "the caret, then the hint");
    assert_eq!(spans[0].content, theme::glyph(theme::Symbol::CursorText));
    assert_eq!(spans[1].content, "type to narrow");

    let resting = empty.clone().focused(false).spans();
    assert_eq!(resting.len(), 1, "unfocused: no claim about where keys go");
    assert_eq!(resting[0].content, "type to narrow");
}

/// Typed, the caret follows the text — and only while the field has focus,
/// because two carets on one screen are two claims about where typing goes.
#[test]
fn a_typed_field_carries_its_caret_only_while_focused() {
    let typed = Field::new("harn", "type to narrow");

    let focused = typed.spans();
    assert_eq!(focused.len(), 2);
    assert_eq!(focused[0].content, "harn");
    assert_eq!(focused[1].content, theme::glyph(theme::Symbol::CursorText));

    let resting = typed.focused(false).spans();
    assert_eq!(resting.len(), 1);
    assert_eq!(resting[0].content, "harn");
}

/// The theme names `cursor.text` "the caret in a text input"; `bar.thin`
/// is one of the bars marking the edge of a row.
///
/// The built-in theme spells both `▏`, which is exactly how three inputs
/// came to draw the bar and nobody saw it: the defect is invisible until a
/// glyph set spells them apart, and then it appears in three places at
/// once with nothing tying them together. So this asserts the *symbol*,
/// which is the thing that was wrong, rather than the glyph, which was
/// not.
#[test]
fn the_caret_is_the_carets_own_symbol() {
    assert_eq!(
        field::caret().content,
        theme::glyph(theme::Symbol::CursorText)
    );
}

/// The open index is always focused — it is open, so every key reaches it —
/// and it was the one field that said nothing at all while empty.
#[test]
fn the_open_index_says_it_can_be_typed_into_before_anything_is() {
    let area = Rect::new(0, 0, 80, 24);
    let buffer = drawn(80, 24, |frame| {
        action_index::render(frame, area, &[], 0, "", 0, |_| ());
    });

    let text: String = (0..24)
        .map(|y| (0..80).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect();
    assert!(
        text.contains(&theme::glyph(theme::Symbol::CursorText)),
        "an empty index still shows its caret"
    );
    assert!(text.contains("type to narrow"));
}

/// The rule is what says "field" before anything is typed, and it is the
/// same rule on every input: the list screens' filter and the open index
/// answered this differently until the field owned it.
#[test]
fn a_field_wears_the_same_rule_wherever_it_is_drawn() {
    let area = Rect::new(0, 0, 20, 2);
    let focused = drawn(20, 2, |frame| {
        let inner = Field::new("", "find").render(frame, area);
        assert_eq!(
            inner,
            Rect::new(0, 0, 20, 1),
            "the rule takes the row under"
        );
    });
    let resting = drawn(20, 2, |frame| {
        Field::new("", "find").focused(false).render(frame, area);
    });

    assert_eq!(
        focused[(0, 1)].fg,
        theme::color(Token::Accent),
        "focused: the rule says keys land here"
    );
    assert_eq!(resting[(0, 1)].fg, theme::color(Token::BorderDefault));
}

/// A surface that resized under the typing that narrowed it moved the rows
/// the reader was aiming at, on the keystroke where they could least
/// follow. Its height is what is reachable here, not what is left.
#[test]
fn narrowing_the_index_does_not_resize_it() {
    let area = Rect::new(0, 0, 80, 24);
    let all = uze_keys::active().available(&[uze_keys::Scope::Global]);
    if all.len() < 2 {
        return;
    }
    // The surface's own bottom border is what is being measured, so find
    // the row it lands on rather than trusting the arithmetic above it.
    let bottom_border = |rows: &[action_index::Row]| {
        let buffer = drawn(80, 24, |frame| {
            action_index::render(frame, area, rows, all.len(), "x", 0, |_| ());
        });
        (0..24)
            .rev()
            .find(|&y| (0..80).any(|x| buffer[(x, y)].symbol() == "\u{2518}"))
    };

    assert!(bottom_border(&all).is_some(), "the surface was drawn");
    assert_eq!(
        bottom_border(&all),
        bottom_border(&all[..1]),
        "one match or all of them, the surface is the same size"
    );
}

/// The title names the surface, and the mark that closes it sits on its
/// border where a modal's own controls do.
#[test]
fn the_index_is_titled_help_and_says_how_to_close() {
    let buffer = drawn(80, 24, |frame| {
        action_index::render(frame, Rect::new(0, 0, 80, 24), &[], 4, "", 0, |_| ());
    });
    let text: String = (0..24)
        .map(|y| (0..80).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect();

    assert!(text.contains("Help"), "titled for what it is");
    assert!(!text.contains("Everything you can do"));
    assert!(
        text.contains(&theme::glyph(theme::Symbol::MarkClose)),
        "the mark that closes it is drawn"
    );
    assert!(
        !text.contains("close"),
        "the glyph alone, not a second label"
    );
}

/// The index grows with every action the product gains. Capped, it stays
/// a surface somebody chose the size of, and scrolls past that.
#[test]
fn the_index_is_capped_and_scrolls_rather_than_growing_with_the_product() {
    let area = Rect::new(0, 0, 80, 40);
    let many: Vec<action_index::Row> = uze_keys::active()
        .available(&[uze_keys::Scope::Global])
        .into_iter()
        .cycle()
        .take(40)
        .collect();
    if many.len() < 40 {
        return;
    }

    let buffer = drawn(80, 40, |frame| {
        action_index::render(frame, area, &many, many.len(), "", 0, |_| ());
    });
    let bottom = (0..40)
        .rev()
        .find(|&y| (0..80).any(|x| buffer[(x, y)].symbol() == "\u{2518}"))
        .expect("the surface was drawn");
    let top = (0..40)
        .find(|&y| (0..80).any(|x| buffer[(x, y)].symbol() == "\u{250c}"))
        .expect("the surface was drawn");

    assert!(
        bottom - top < 19,
        "twelve rows plus chrome, not forty: got {}",
        bottom - top + 1
    );
}

/// A cap that hid what the arrows had moved to would be worse than no cap:
/// the window follows the selection, and the bar says where in the list it
/// is.
#[test]
fn the_window_follows_the_selection_past_the_cap() {
    let area = Rect::new(0, 0, 80, 40);
    let many: Vec<action_index::Row> = uze_keys::active()
        .available(&[uze_keys::Scope::Global])
        .into_iter()
        .cycle()
        .take(40)
        .collect();
    if many.len() < 40 {
        return;
    }

    let rows_for = |selected: usize| {
        let mut placed = Vec::new();
        drawn(80, 40, |frame| {
            placed = action_index::render(frame, area, &many, many.len(), "", selected, |p| p);
        });
        placed
    };

    let top = rows_for(0);
    assert!(
        top.iter().any(|(_, position)| *position == 0),
        "the chosen row is drawn"
    );
    assert!(top.len() <= 12, "no more than the cap: {}", top.len());

    let deep = rows_for(35);
    assert!(
        deep.iter().any(|(_, position)| *position == 35),
        "a selection past the window scrolled it into view"
    );
    assert!(
        !deep.iter().any(|(_, position)| *position == 0),
        "and the window actually moved"
    );
}

/// Four kinds, four hues, four marks. The mark carries the class as well
/// as the hue, because a terminal is the one surface where the palette may
/// be the reader's own rather than this design's.
#[test]
fn each_kind_of_outcome_is_told_apart_by_more_than_its_colour() {
    let area = Rect::new(0, 0, 60, 12);
    let marks: Vec<String> = [
        ToastKind::Done,
        ToastKind::Failed,
        ToastKind::Warned,
        ToastKind::Told,
    ]
    .into_iter()
    .map(|kind| {
        let buffer = drawn(60, 12, |frame| {
            toast::stack(
                frame,
                area,
                &[Toast::new(kind, "something happened", "about it")],
            );
        });
        (0..12)
            .map(|y| (0..60).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("")
    })
    .collect();

    for (kind, drawn_text) in [
        (theme::Symbol::MarkOk, &marks[0]),
        (theme::Symbol::MarkFailed, &marks[1]),
        (theme::Symbol::MarkAttention, &marks[2]),
        (theme::Symbol::ChevronRight, &marks[3]),
    ] {
        assert!(
            drawn_text.contains(&theme::glyph(kind)),
            "{kind:?} is the mark its kind carries"
        );
    }
}

/// The clock is drawn. A box that vanishes on a clock nobody can see reads
/// as a glitch; one that says `4s` is one the reader can decide to ignore.
/// One that stays draws no clock — the `✕` already says it can be ended.
#[test]
fn a_toast_says_how_long_it_has_left_or_draws_no_clock_at_all() {
    let area = Rect::new(0, 0, 60, 12);
    let text_of = |toast: Toast| {
        let buffer = drawn(60, 12, |frame| {
            toast::stack(frame, area, &[toast]);
        });
        (0..12)
            .map(|y| (0..60).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("")
    };

    let counting = text_of(Toast::new(ToastKind::Done, "synced", "about it").remaining(Some(4)));
    assert!(counting.contains("4s"), "the seconds left are drawn");

    let staying = text_of(Toast::new(ToastKind::Failed, "rejected", "about it"));
    assert!(!staying.contains("0s"), "one that stays draws no clock");
    assert!(
        staying.contains(&theme::glyph(theme::Symbol::MarkClose)),
        "the close mark is what says it can be ended"
    );
}

/// The stack grows down from where it starts, so a toast arriving does not
/// move the ones already being read.
#[test]
fn an_arriving_toast_does_not_move_the_ones_already_being_read() {
    let area = Rect::new(0, 0, 60, 20);
    let first = Toast::new(ToastKind::Done, "one", "about it");
    let second = Toast::new(ToastKind::Told, "two", "about it");

    let alone = drawn(60, 20, |frame| {
        let placed = toast::stack(frame, area, std::slice::from_ref(&first));
        assert_eq!(placed[0].box_rect.y, area.y);
    });
    let _ = alone;

    drawn(60, 20, |frame| {
        let placed = toast::stack(frame, area, &[first.clone(), second]);
        assert_eq!(placed[0].box_rect.y, area.y, "the first one did not move");
        assert!(
            placed[1].box_rect.y > placed[0].box_rect.y,
            "the new one is below"
        );
        assert_eq!(
            placed[0].box_rect.right(),
            placed[1].box_rect.right(),
            "right-aligned, so the column reads as one"
        );
    });
}

/// An offer is a rect the caller registers; a toast without one answers
/// only by being put away.
#[test]
fn only_a_toast_that_offers_something_answers_with_a_rect_for_it() {
    let area = Rect::new(0, 0, 60, 12);

    drawn(60, 12, |frame| {
        let placed = toast::stack(
            frame,
            area,
            &[Toast::new(ToastKind::Done, "synced", "about it")],
        );
        assert!(placed[0].action.is_none());
    });
    drawn(60, 12, |frame| {
        let placed = toast::stack(
            frame,
            area,
            &[Toast::new(ToastKind::Failed, "rejected", "about it").action("tentar de novo")],
        );
        let action = placed[0].action.expect("the offer has a rect");
        assert!(
            placed[0].box_rect.union(action) == placed[0].box_rect,
            "the offer sits inside its own box"
        );
    });
}

/// Only what fits is drawn, and the answer is as long as what was drawn —
/// so an index into it is an index into what is on screen, which is what
/// makes "dismiss the third one" mean anything.
#[test]
fn a_stack_taller_than_its_room_draws_what_fits_and_says_how_many() {
    let four: Vec<Toast> = (0..4)
        .map(|n| Toast::new(ToastKind::Told, format!("message {n}"), "about it"))
        .collect();

    drawn(60, 20, |frame| {
        let placed = toast::stack(frame, Rect::new(0, 0, 60, 20), &four);
        assert_eq!(placed.len(), 4);
    });
    drawn(60, 5, |frame| {
        let placed = toast::stack(frame, Rect::new(0, 0, 60, 5), &four);
        assert_eq!(placed.len(), 2, "two rows each, one row apart");
        for one in &placed {
            assert!(one.box_rect.bottom() <= 5, "nothing is drawn past the edge");
        }
    });
}

/// The ground is the same whatever the toast is saying; the hue is on the
/// mark and nowhere else.
///
/// Four tinted grounds stacked read as a wall of colour and make the
/// reader parse the surface before the words — and a message is not more
/// urgent for being wider. One surface, four marks.
#[test]
fn a_toast_is_neutral_and_says_what_kind_it_is_with_its_mark_alone() {
    let area = Rect::new(0, 0, 60, 8);
    let ground_of = |kind: ToastKind| {
        let buffer = drawn(60, 8, |frame| {
            toast::stack(
                frame,
                area,
                &[Toast::new(kind, "something happened", "about it")],
            );
        });
        // A cell in the middle of the message, past the mark.
        let row = &[(40, 0), (40, 1)];
        row.map(|(x, y)| buffer[(x, y)].bg)
    };

    let done = ground_of(ToastKind::Done);
    assert_eq!(
        done[0],
        theme::color(Token::SurfaceRaised),
        "one neutral surface"
    );
    assert_eq!(done[1], done[0], "and both its rows carry it");
    for kind in [ToastKind::Failed, ToastKind::Warned, ToastKind::Told] {
        assert_eq!(ground_of(kind), done, "whatever it is saying");
    }
}

/// The message takes the first row almost whole; what is *about* it — the
/// offer and the clock — sits under it on the right. On one row the two
/// competed for the same columns and the message was the one that lost.
#[test]
fn the_message_owns_its_row_and_what_is_about_it_sits_under() {
    let area = Rect::new(0, 0, 60, 8);
    let toast = Toast::new(ToastKind::Failed, "could not sync", "about it")
        .action("try again")
        .remaining(None);

    drawn(60, 8, |frame| {
        let placed = toast::stack(frame, area, std::slice::from_ref(&toast));
        let one = placed[0];
        assert_eq!(one.box_rect.height, 2);
        assert_eq!(
            one.close.y, one.box_rect.y,
            "the close mark is on the message"
        );
        let action = one.action.expect("the offer has a rect");
        assert_eq!(action.y, one.box_rect.y + 1, "the offer is under it");
        assert!(
            action.right() < one.box_rect.right(),
            "and inset from the edge"
        );
    });
}

/// Title on the first row with the toast's own controls; detail on the
/// second, hanging under the title rather than under the mark.
///
/// The two rows answer different questions — what happened, and what the
/// title left out — so nothing on one competes for the other's columns.
#[test]
fn a_toast_reads_as_a_title_with_a_line_under_it() {
    let area = Rect::new(0, 0, 60, 8);
    let toast = Toast::new(
        ToastKind::Done,
        "request #412 synced",
        "3 commits reached main",
    )
    .remaining(Some(4));

    let buffer = drawn(60, 8, |frame| {
        let placed = toast::stack(frame, area, std::slice::from_ref(&toast));
        let one = placed[0];
        assert_eq!(
            one.close.y, one.box_rect.y,
            "the close mark rides the title"
        );
    });
    let row = |y: u16| (0..60).map(|x| buffer[(x, y)].symbol()).collect::<String>();

    let title = row(0);
    let detail = row(1);
    assert!(title.contains("request #412 synced"), "{title}");
    assert!(title.contains("4s"), "the clock rides the title: {title}");
    assert!(
        detail.contains("3 commits reached main"),
        "the detail is under it: {detail}"
    );
    assert!(
        !detail.contains("request #412"),
        "and the title is not repeated: {detail}"
    );

    // The detail hangs under the title: same column, so the mark's own
    // column stays empty on the second row.
    // Columns, not byte offsets: the mark is three bytes and the detail's
    // first character is one, so `str::find` on the two rows compares
    // positions that are not the same unit.
    let column_of = |y: u16, wanted: &str| (0..60).find(|&x| buffer[(x, y)].symbol() == wanted);
    let mark_at = column_of(0, &theme::glyph(theme::Symbol::MarkOk)).unwrap();
    let title_at = column_of(0, "r").unwrap();
    let detail_at = column_of(1, "3").unwrap();
    assert!(mark_at < title_at);
    assert_eq!(detail_at, title_at, "indented to the title, not the mark");
}

/// A toast with nothing more to say still draws its two rows, so a stack
/// of them keeps one rhythm rather than shuffling as each arrives.
#[test]
fn a_toast_with_no_detail_still_keeps_its_shape() {
    let area = Rect::new(0, 0, 60, 8);

    drawn(60, 8, |frame| {
        let placed = toast::stack(
            frame,
            area,
            &[Toast::new(ToastKind::Told, "nothing ready", "about it").remaining(Some(2))],
        );
        assert_eq!(placed[0].box_rect.height, 2);
    });
}

/// A toast is capped however long its words are, and both its lines are
/// elided to fit rather than one of them widening the box.
///
/// The cap is what keeps it a message: a box wide enough to read a
/// paragraph comfortably is a box that has taken the pane, and the pane is
/// why anybody is here. What the summary leaves out is reachable where it
/// is kept — the toast is gone in seconds either way.
#[test]
fn a_toast_is_capped_however_long_its_words_are() {
    let area = Rect::new(0, 0, 200, 8);
    let long = "a".repeat(300);
    let toast = Toast::new(ToastKind::Failed, long.clone(), long).remaining(Some(4));

    drawn(200, 8, |frame| {
        let placed = toast::stack(frame, area, std::slice::from_ref(&toast));
        let width = placed[0].box_rect.width;
        assert!(
            width <= 54,
            "capped whatever it was asked to say: {width} columns"
        );
        assert_eq!(
            placed[0].box_rect.right(),
            area.right(),
            "and still meets the right edge"
        );
    });

    let buffer = drawn(200, 8, |frame| {
        toast::stack(frame, area, std::slice::from_ref(&toast));
    });
    let rows: Vec<String> = (0..2)
        .map(|y| (0..200).map(|x| buffer[(x, y)].symbol()).collect())
        .collect();
    let ellipsis = theme::glyph(theme::Symbol::Ellipsis);
    assert!(rows[0].contains(&ellipsis), "the title says it was cut");
    assert!(rows[1].contains(&ellipsis), "and so does the detail");
}

/// A narrow pane is not a reason to draw a toast that runs off it.
#[test]
fn a_toast_never_draws_wider_than_the_room_it_was_given() {
    let toast = Toast::new(
        ToastKind::Told,
        "a title with several words in it",
        "and a detail with several more",
    );

    for width in [12_u16, 20, 30] {
        drawn(width, 8, |frame| {
            let placed = toast::stack(
                frame,
                Rect::new(0, 0, width, 8),
                std::slice::from_ref(&toast),
            );
            let box_rect = placed[0].box_rect;
            assert!(box_rect.right() <= width, "{width}: {box_rect:?}");
            assert!(box_rect.x < box_rect.right(), "{width}: {box_rect:?}");
        });
    }
}

/// One width for the whole column, whatever each toast has to say.
///
/// Sized to their own words, four boxes have four left edges, and the eye
/// reads four things rather than one column of messages — the raggedness
/// is the first thing it sees and it says nothing, because a message is
/// not longer for being more important.
#[test]
fn every_toast_in_a_stack_is_the_same_width() {
    let area = Rect::new(0, 0, 120, 20);
    let stack = [
        Toast::new(
            ToastKind::Warned,
            "the agent exited with status 1",
            "its pane is still open",
        ),
        Toast::new(
            ToastKind::Told,
            "architect redrew 2 diagrams",
            "the model changed",
        ),
        Toast::new(
            ToastKind::Failed,
            "could not sync",
            "the remote rejected the push",
        )
        .action("try again"),
        Toast::new(ToastKind::Done, "ok", "x").remaining(Some(4)),
    ];

    drawn(120, 20, |frame| {
        let placed = toast::stack(frame, area, &stack);
        assert_eq!(placed.len(), 4);
        let first = placed[0].box_rect;
        for one in &placed {
            assert_eq!(one.box_rect.width, first.width, "one width");
            assert_eq!(one.box_rect.x, first.x, "so one left edge");
            assert_eq!(one.box_rect.right(), area.right(), "and one right edge");
        }
    });
}

/// The width is decided before the first box is drawn, so a toast
/// arriving changes neither where the others sit nor how wide they are.
#[test]
fn an_arriving_toast_resizes_nothing() {
    let area = Rect::new(0, 0, 120, 20);
    let one = Toast::new(ToastKind::Done, "short", "x");
    let long = Toast::new(
        ToastKind::Failed,
        "a much longer title than the first one carries",
        "and a detail to match it",
    )
    .action("try again");

    let mut alone = None;
    drawn(120, 20, |frame| {
        alone = toast::stack(frame, area, std::slice::from_ref(&one))
            .first()
            .map(|placed| placed.box_rect);
    });
    drawn(120, 20, |frame| {
        let placed = toast::stack(frame, area, &[one.clone(), long]);
        assert_eq!(
            Some(placed[0].box_rect),
            alone,
            "the one already on screen did not move or resize"
        );
    });
}
