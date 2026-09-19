//! Fitting text to the room there is.
//!
//! A terminal has no reflow and no tooltip: text that does not fit is text
//! the reader loses, so every column in the product cuts, and cutting is
//! one decision — where, and with what mark — not a dozen.
//!
//! The two halves were split by accident rather than by design. [`elide`]
//! sat in `ui.rs` and nine files reached for it; [`clip`] sat in
//! `management.rs`, a *screen*, and four other screens reached across for
//! it as `super::super::management::clip_line`. A primitive a screen
//! exports is a primitive in the wrong place: nothing about cutting a line
//! belongs to the screen that happened to need it first.

use ratatui::text::Line;

use crate::ui::theme::{self, Symbol};

/// `text`, cut to `width` columns with the ellipsis mark when it does not
/// fit.
///
/// The mark's own width comes from the theme: a glyph set that spells the
/// ellipsis with three periods takes three columns where `…` takes one,
/// and a cut measured against the wrong one overflows the column it was
/// meant to fit.
pub(crate) fn elide(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    let Some(kept) = width.checked_sub(theme::width(Symbol::Ellipsis) as usize) else {
        return String::new();
    };
    let mut kept: String = text.chars().take(kept).collect();
    kept.push_str(&theme::glyph(Symbol::Ellipsis));
    kept
}

/// Cuts `line` to `max` columns in place, keeping every span that fits
/// whole and eliding the one that straddles the edge.
///
/// Spans rather than characters because a line is styled: cutting the
/// string would lose which part of it was the key and which the
/// description, and the last visible span has to keep its own style right
/// up to the mark.
pub(crate) fn clip(line: &mut Line<'static>, max: usize) {
    let mut used = 0usize;
    let mut cut = None;
    for (index, span) in line.spans.iter().enumerate() {
        let width = span.width();
        if used + width <= max {
            used += width;
        } else {
            cut = Some(index);
            break;
        }
    }
    let Some(index) = cut else {
        return;
    };
    line.spans[index].content =
        std::borrow::Cow::Owned(elide(&line.spans[index].content, max - used));
    line.spans.truncate(index + 1);
}

/// `text` broken between words into rows of at most `width` columns, with
/// a word wider than a row broken across rows — the one wrapper every
/// surface folds prose with. Empty text is one empty row.
///
/// Folding *before* a paragraph is authored is what keeps a row index a
/// screen row: the plugin drawer anchors its two clickable rows (the
/// marketplace name, the address under it) by counting authored lines, and
/// a description that ratatui's own `Wrap` folded afterwards pushed the
/// drawn rows down and left the targets sitting above them.
pub(crate) fn fold(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_owned()];
    }
    let mut rows: Vec<String> = Vec::new();
    let mut row = String::new();
    for word in text.split_whitespace() {
        // A word wider than the row is broken across rows rather than
        // left to overflow — the paragraph's own wrapper does the same,
        // and a bare URL in a description is exactly that word.
        let mut word = word;
        while word.chars().count() > width {
            if !row.is_empty() {
                rows.push(std::mem::take(&mut row));
            }
            let split = word
                .char_indices()
                .nth(width)
                .map_or(word.len(), |(index, _)| index);
            let (head, tail) = word.split_at(split);
            rows.push(head.to_owned());
            word = tail;
        }
        let projected = row.chars().count() + usize::from(!row.is_empty()) + word.chars().count();
        if projected > width && !row.is_empty() {
            rows.push(std::mem::take(&mut row));
        }
        if !row.is_empty() {
            row.push(' ');
        }
        row.push_str(word);
    }
    if !row.is_empty() || rows.is_empty() {
        rows.push(row);
    }
    rows
}

/// `n` in subscript digits (`12` -> `₁₂`): a count that sits beside a
/// label without competing with it for weight — the route counts in the
/// management sidebar, the pull/push counts under an agent's branch.
pub(crate) fn small_digits(n: usize) -> String {
    n.to_string()
        .chars()
        .map(|c| match c {
            '0' => '₀',
            '1' => '₁',
            '2' => '₂',
            '3' => '₃',
            '4' => '₄',
            '5' => '₅',
            '6' => '₆',
            '7' => '₇',
            '8' => '₈',
            '9' => '₉',
            _ => c,
        })
        .collect()
}

/// Renders a label as Unicode small capitals (`Beta` -> `ʙᴇᴛᴀ`) — the
/// weight of capitals without their full height, for a mark that has to be
/// noticed beside a name without shouting over it. The input is lowercased
/// first so a mixed-case label reads as one even run rather than as a
/// full-height initial followed by small ones. Unicode has no small
/// capital for `q` or `x`, so those stay lowercase — closer in weight to
/// their neighbours than the one full-height letter in the run would be —
/// and anything outside the ASCII alphabet passes through rather than
/// being dropped.
///
/// Every letter maps to exactly one character, and all 24 are East Asian
/// width *neutral*, so a label keeps both its length and its cell count:
/// the padded columns it sits in are unaffected.
///
/// What this depends on is the reader's font, which is why it is spent
/// sparingly — the workspace tab's agent alias and the sidebar's badge for
/// an unsettled route, both short and both read once. The glyphs are
/// scattered across three blocks (IPA Extensions, Phonetic Extensions,
/// and `ꜰ`/`ꜱ` alone in Latin Extended-D) and monospace coverage is thin:
/// measured against the patched Nerd Fonts, Fira Code, JetBrains Mono and
/// Hack carry none of the 24, DejaVu Sans Mono and Meslo 8, Consolas and
/// Liberation Mono 22 — missing exactly `ꜰ` and `ꜱ` — and only Iosevka and
/// Noto Sans Mono all 24. A terminal missing a glyph substitutes a
/// proportional fallback face, which still occupies its one cell but is
/// drawn at another size and optical width, so the run looks unevenly
/// spaced rather than misaligned. That is a font to install rather than a
/// bug to fix here, but it is the reason a status a reader must be able to
/// scan across a list is left in ordinary case.
pub(crate) fn small_caps(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'a' => 'ᴀ',
            'b' => 'ʙ',
            'c' => 'ᴄ',
            'd' => 'ᴅ',
            'e' => 'ᴇ',
            'f' => 'ꜰ',
            'g' => 'ɢ',
            'h' => 'ʜ',
            'i' => 'ɪ',
            'j' => 'ᴊ',
            'k' => 'ᴋ',
            'l' => 'ʟ',
            'm' => 'ᴍ',
            'n' => 'ɴ',
            'o' => 'ᴏ',
            'p' => 'ᴘ',
            'r' => 'ʀ',
            's' => 'ꜱ',
            't' => 'ᴛ',
            'u' => 'ᴜ',
            'v' => 'ᴠ',
            'w' => 'ᴡ',
            'y' => 'ʏ',
            'z' => 'ᴢ',
            other => other,
        })
        .collect()
}
