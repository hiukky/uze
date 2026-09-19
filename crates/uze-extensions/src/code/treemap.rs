//! A rectangle divided among weights, so each gets its share of the area
//! and stays as close to square as the others allow.
//!
//! The squarified treemap of Bruls, Huizing and van Wijk: weights largest
//! first, laid in strips along the shorter side, a strip closed when the
//! next weight would make its worst tile worse. Two things are this
//! surface's own. "Shorter" and "square" are judged as *seen* — a cell is
//! about twice as tall as it is wide, and squares measured in cells come
//! out as towers. And every edge is a running total rounded once, so the
//! tiles two edges share can neither overlap nor leave a cell between them.

use crate::shared::canvas::Frame;

/// How many columns a row is worth to the eye.
const ROW: f64 = 2.0;

/// One frame per weight, in the order given, filling `within` exactly.
/// Weights are expected largest first; a weight of nothing gets an empty
/// frame rather than a share.
pub fn squarified(weights: &[u64], within: Frame) -> Vec<Frame> {
    let mut frames = vec![Frame::default(); weights.len()];
    let total: u64 = weights.iter().sum();
    if total == 0 || within.w <= 0 || within.h <= 0 {
        return frames;
    }
    let mut free = within;
    let mut remaining = total as f64;
    let mut first = 0;
    while first < weights.len() && free.w > 0 && free.h > 0 {
        let seen = (f64::from(free.w), f64::from(free.h) * ROW);
        let along_width = seen.0 <= seen.1;
        let side = if along_width { seen.0 } else { seen.1 };
        let scale = seen.0 * seen.1 / remaining;

        let mut last = first + 1;
        let mut strip = weights[first] as f64 * scale;
        while last < weights.len() {
            let with_next = strip + weights[last] as f64 * scale;
            let now = worst(&weights[first..last], strip, side, scale);
            let then = worst(&weights[first..=last], with_next, side, scale);
            if then > now {
                break;
            }
            strip = with_next;
            last += 1;
        }

        let share: f64 = weights[first..last].iter().map(|&w| w as f64).sum();
        let everything_left = last == weights.len();
        let (strip_frame, rest) = cut(free, along_width, share / remaining, everything_left);
        lay_along(
            &weights[first..last],
            strip_frame,
            along_width,
            &mut frames[first..last],
        );
        free = rest;
        remaining -= share;
        first = last;
    }
    frames
}

/// The worst aspect ratio in a strip of `area` laid along `side`.
fn worst(weights: &[u64], area: f64, side: f64, scale: f64) -> f64 {
    let depth = area / side;
    weights
        .iter()
        .map(|&weight| {
            let length = weight as f64 * scale / depth;
            (length / depth).max(depth / length)
        })
        .fold(1.0, f64::max)
}

/// Takes a strip off `free`: across the top when it runs along the width,
/// down the left otherwise. The last strip takes what is left, whatever
/// rounding made of it.
fn cut(free: Frame, along_width: bool, share: f64, everything_left: bool) -> (Frame, Frame) {
    if along_width {
        let depth = match everything_left {
            true => free.h,
            false => ((f64::from(free.h) * share).round() as i32).clamp(1, free.h),
        };
        (
            Frame { h: depth, ..free },
            Frame {
                y: free.y + depth,
                h: free.h - depth,
                ..free
            },
        )
    } else {
        let depth = match everything_left {
            true => free.w,
            false => ((f64::from(free.w) * share).round() as i32).clamp(1, free.w),
        };
        (
            Frame { w: depth, ..free },
            Frame {
                x: free.x + depth,
                w: free.w - depth,
                ..free
            },
        )
    }
}

fn lay_along(weights: &[u64], strip: Frame, along_width: bool, frames: &mut [Frame]) {
    let total: f64 = weights.iter().map(|&w| w as f64).sum();
    let length = if along_width { strip.w } else { strip.h };
    let mut before = 0.0;
    let mut from = 0;
    for (weight, frame) in weights.iter().zip(frames) {
        before += *weight as f64;
        let to = (f64::from(length) * before / total).round() as i32;
        *frame = if along_width {
            Frame {
                x: strip.x + from,
                w: to - from,
                ..strip
            }
        } else {
            Frame {
                y: strip.y + from,
                h: to - from,
                ..strip
            }
        };
        from = to;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WITHIN: Frame = Frame {
        x: 3,
        y: 2,
        w: 80,
        h: 24,
    };

    fn covered(frames: &[Frame], within: Frame) -> Vec<u8> {
        let mut cells = vec![0u8; (within.w * within.h) as usize];
        for frame in frames {
            for y in frame.y..frame.y + frame.h {
                for x in frame.x..frame.x + frame.w {
                    assert!(within.contains(x, y), "{frame:?} leaves {within:?}");
                    cells[((y - within.y) * within.w + (x - within.x)) as usize] += 1;
                }
            }
        }
        cells
    }

    #[test]
    fn every_cell_belongs_to_exactly_one_tile() {
        for weights in [
            vec![6, 6, 4, 3, 2, 2, 1],
            vec![1000, 1, 1, 1],
            vec![5; 37],
            vec![9],
        ] {
            let cells = covered(&squarified(&weights, WITHIN), WITHIN);
            assert!(
                cells.iter().all(|&count| count == 1),
                "{weights:?} left a gap or an overlap"
            );
        }
    }

    #[test]
    fn a_tile_gets_the_share_its_weight_is() {
        let frames = squarified(&[60, 30, 10], WITHIN);
        let cells = f64::from(WITHIN.w * WITHIN.h);
        for (frame, share) in frames.iter().zip([0.6, 0.3, 0.1]) {
            let got = f64::from(frame.w * frame.h) / cells;
            assert!(
                (got - share).abs() < 0.04,
                "{frame:?} is {got}, not {share}"
            );
        }
    }

    #[test]
    fn tiles_are_square_as_seen_rather_than_in_cells() {
        let frames = squarified(
            &[1, 1, 1, 1],
            Frame {
                x: 0,
                y: 0,
                w: 80,
                h: 40,
            },
        );
        for frame in frames {
            assert_eq!(
                (frame.w, frame.h),
                (40, 20),
                "twice as wide as tall, in cells"
            );
        }
    }

    #[test]
    fn equal_weights_never_come_out_as_slivers() {
        for frame in squarified(&[5; 12], WITHIN) {
            let seen = f64::from(frame.w) / (f64::from(frame.h) * ROW);
            assert!(seen.max(1.0 / seen) < 3.0, "{frame:?} is a sliver");
        }
    }

    #[test]
    fn nothing_to_divide_is_not_a_division_by_zero() {
        assert_eq!(squarified(&[], WITHIN), vec![]);
        assert_eq!(squarified(&[0, 0], WITHIN), vec![Frame::default(); 2]);
        assert_eq!(
            squarified(
                &[1],
                Frame {
                    x: 0,
                    y: 0,
                    w: 0,
                    h: 5
                }
            ),
            vec![Frame::default()]
        );
    }
}
