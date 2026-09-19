//! A sequence diagram, painted.
//!
//! The one notation here that needs no layout search: participants are
//! columns, time is rows, and the only decision is how far apart the
//! columns must stand for the longest message between them to fit.

use crate::view::Role;

use crate::shared::canvas::{
    Canvas, Corners, EAST, Frame, Glyphs, NORTH, SOUTH, WEST, arrow_glyph, text_width,
};

use super::model::{Sequence, SequenceStep, Stroke};

const MARGIN: i32 = 2;
const HEAD_HEIGHT: i32 = 3;
const LOOP_WIDTH: i32 = 4;

pub fn paint(sequence: &Sequence, glyphs: Glyphs) -> Canvas {
    let centres = centres(sequence);
    let head = |index: usize| text_width(&sequence.participants[index].title) + 4;
    let reach = sequence
        .steps
        .iter()
        .map(|step| match step {
            SequenceStep::Message { from, to, text, .. } if from == to => {
                centres[*from] + LOOP_WIDTH + 2 + text_width(text)
            }
            SequenceStep::Note { over, text } => centres[*over] + text_width(text) / 2 + 3,
            _ => 0,
        })
        .chain((0..centres.len()).map(|index| centres[index] + head(index) / 2 + 1))
        .max()
        .unwrap_or(0);
    let rows: i32 = sequence.steps.iter().map(step_height).sum();
    let width = reach + MARGIN;
    let mut canvas = Canvas::new(width, HEAD_HEIGHT + rows + 2, glyphs);

    for (index, participant) in sequence.participants.iter().enumerate() {
        let w = head(index);
        let frame = Frame {
            x: centres[index] - w / 2,
            y: 0,
            w,
            h: HEAD_HEIGHT,
        };
        canvas.frame(frame, Corners::Rounded, Role::Muted);
        canvas.text(frame.x + 2, 1, &participant.title, Role::Bright, true);
        for y in HEAD_HEIGHT..canvas.height {
            canvas.line(
                centres[index],
                y,
                NORTH | SOUTH,
                Stroke::Dotted,
                Role::Faint,
            );
        }
    }

    let mut y = HEAD_HEIGHT + 1;
    for step in &sequence.steps {
        match step {
            SequenceStep::Message {
                from,
                to,
                text,
                stroke,
            } if from == to => self_message(&mut canvas, centres[*from], y, text, *stroke),
            SequenceStep::Message {
                from,
                to,
                text,
                stroke,
            } => message(&mut canvas, centres[*from], centres[*to], y, text, *stroke),
            SequenceStep::Note { over, text } => {
                let w = text_width(text) + 4;
                let frame = Frame {
                    x: (centres[*over] - w / 2).max(0),
                    y,
                    w,
                    h: 3,
                };
                canvas.frame(frame, Corners::Square, Role::Faint);
                canvas.text(frame.x + 1, y + 1, &format!(" {text} "), Role::Muted, false);
            }
            SequenceStep::Divider(text) => {
                for x in MARGIN..width - MARGIN {
                    canvas.line(x, y, EAST | WEST, Stroke::Dotted, Role::Faint);
                }
                canvas.text(MARGIN + 2, y, &format!(" {text} "), Role::Secondary, false);
            }
        }
        y += step_height(step);
    }
    canvas
}

fn step_height(step: &SequenceStep) -> i32 {
    match step {
        SequenceStep::Message { from, to, .. } if from == to => 4,
        SequenceStep::Message { .. } => 3,
        SequenceStep::Note { .. } => 4,
        SequenceStep::Divider(_) => 2,
    }
}

/// Where each lifeline stands: packed by the participants' own widths,
/// then pushed apart wherever a message between two of them needs more.
fn centres(sequence: &Sequence) -> Vec<i32> {
    let count = sequence.participants.len();
    let half = |index: usize| (text_width(&sequence.participants[index].title) + 4) / 2;
    let mut gaps: Vec<i32> = (1..count)
        .map(|index| half(index - 1) + half(index) + 4)
        .collect();
    for step in &sequence.steps {
        let SequenceStep::Message { from, to, text, .. } = step else {
            continue;
        };
        let (low, high) = (*from.min(to), *from.max(to));
        let needed = if low == high {
            LOOP_WIDTH + 4 + text_width(text)
        } else {
            text_width(text) + 6
        };
        if low == high {
            if let Some(gap) = gaps.get_mut(low) {
                *gap = (*gap).max(needed);
            }
            continue;
        }
        let spanned: i32 = gaps[low..high].iter().sum();
        if spanned < needed {
            gaps[high - 1] += needed - spanned;
        }
    }
    let mut centre = MARGIN + if count > 0 { half(0) } else { 0 };
    let mut centres = Vec::with_capacity(count);
    for index in 0..count {
        centres.push(centre);
        centre += gaps.get(index).copied().unwrap_or(0);
    }
    centres
}

fn message(canvas: &mut Canvas, from: i32, to: i32, y: i32, text: &str, stroke: Stroke) {
    let (left, right) = (from.min(to), from.max(to));
    // Padded, so a lifeline the label passes over is cleared on both
    // sides of it rather than butting into its first letter.
    let label = left + (right - left - text_width(text)) / 2;
    canvas.text(label, y, &format!(" {text} "), Role::Secondary, false);
    for x in left + 1..right {
        canvas.line(x, y + 1, EAST | WEST, stroke, Role::Dim);
    }
    let (tip, heading, stem) = if to > from {
        (to - 1, EAST, NORTH | SOUTH | EAST)
    } else {
        (to + 1, WEST, NORTH | SOUTH | WEST)
    };
    canvas.line(from, y + 1, stem, Stroke::Solid, Role::Dim);
    canvas.put(
        tip,
        y + 1,
        arrow_glyph(heading, canvas.glyphs),
        Role::Muted,
        false,
    );
}

fn self_message(canvas: &mut Canvas, at: i32, y: i32, text: &str, stroke: Stroke) {
    let far = at + LOOP_WIDTH;
    canvas.line(at, y, NORTH | SOUTH | EAST, Stroke::Solid, Role::Dim);
    for x in at + 1..far {
        canvas.line(x, y, EAST | WEST, stroke, Role::Dim);
        canvas.line(x, y + 2, EAST | WEST, stroke, Role::Dim);
    }
    canvas.line(far, y, SOUTH | WEST, stroke, Role::Dim);
    canvas.line(far, y + 1, NORTH | SOUTH, stroke, Role::Dim);
    canvas.line(far, y + 2, NORTH | WEST, stroke, Role::Dim);
    canvas.put(
        at + 1,
        y + 2,
        arrow_glyph(WEST, canvas.glyphs),
        Role::Muted,
        false,
    );
    canvas.text(far + 2, y + 1, text, Role::Secondary, false);
}
