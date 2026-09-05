//! The colour vocabulary.
//!
//! A token names what the coloured thing *is*, never what it looks like.
//! That distinction is the whole point: `state.in-flight` can be cyan in one
//! theme and orange in another and still be right, whereas a token called
//! `cyan` is a lie the moment someone themes it. Two of today's colours
//! exist only because the state hues were already spoken for — an in-flight
//! delivery is not a warning, and delivered work is not a badge — and they
//! arrive here under the meanings they were always serving.

use crate::vocab::vocabulary;

vocabulary! {
    /// A semantic colour. Resolved against the active [`Theme`](crate::Theme)
    /// at the moment of drawing; never carried around as a value.
    pub enum Token {
        // ── surface ────────────────────────────────────────────────────
        /// The backdrop everything else is drawn on, and the colour every
        /// translucent declaration in a theme file is composited over.
        SurfaceBackground = "surface.background",
        /// The selected row's tint — present enough to locate, faint enough
        /// not to become a filled slab.
        SurfaceSelected = "surface.selected",
        /// A block lifted above the backdrop without claiming the accent's
        /// meaning: "this is where you are", not "this is on brand".
        SurfaceRaised = "surface.raised",
        /// One shade below [`SurfaceRaised`](Self::SurfaceRaised), for the
        /// rows *under* a raised header so the header still lifts above what
        /// it names.
        SurfaceRaisedSubtle = "surface.raised-subtle",
        /// Brighter than [`SurfaceRaised`](Self::SurfaceRaised), for plain
        /// decoration that has no weight or hue of its own carrying it.
        SurfaceRaisedBright = "surface.raised-bright",
        /// Recessed rather than lifted: an unselected card or drawer, distinct
        /// from the backdrop but sitting below it.
        SurfaceRecessed = "surface.recessed",

        // ── text ───────────────────────────────────────────────────────
        /// Headings and the active state — the brightest text there is.
        TextBright = "text.bright",
        /// Body default.
        TextPrimary = "text.primary",
        /// Descriptions beneath a heading.
        TextSecondary = "text.secondary",
        /// Key/value content.
        TextTertiary = "text.tertiary",
        /// Labels and eyebrows — the level most of the chrome is written in.
        TextMuted = "text.muted",
        /// A level below [`TextMuted`](Self::TextMuted): supporting detail
        /// *beside* supporting text, where the reader has to tell which is
        /// which.
        TextDim = "text.dim",
        /// The faintest legible level — tree glyphs, an age column, anything
        /// nobody reads unless they went looking.
        TextFaint = "text.faint",
        /// A navigable item that is not the selected one.
        TextInactive = "text.inactive",

        // ── border ─────────────────────────────────────────────────────
        /// Hairline under the titlebar and around the sidebar and inputs.
        BorderDefault = "border.default",
        /// The fainter hairline that separates list rows.
        BorderFaint = "border.faint",

        // ── accent and state ───────────────────────────────────────────
        /// The one signature hue.
        Accent = "accent",
        /// Something worked. Aliases [`Accent`](Self::Accent) by default —
        /// the design's own `levelColor` uses one colour for both.
        StateSuccess = "state.success",
        /// Something needs attention but is not broken.
        StateWarning = "state.warning",
        /// Something failed, or will destroy work.
        StateDanger = "state.danger",
        /// The badge hue — a mark that classifies rather than warns, and so
        /// must not be mistakable for one.
        StateInfo = "state.info",
        /// Work in flight. Its own hue because every state a slot can be in
        /// needs one, and a delivery under way is not a warning.
        StateInFlight = "state.in-flight",
        /// Work that landed. Its own hue for the same reason: delivered work
        /// is not a badge.
        StateLanded = "state.landed",
        /// The wash behind an added line in a diff.
        StateDiffAdded = "state.diff-added",
        /// The wash behind a removed line in a diff.
        StateDiffRemoved = "state.diff-removed",

        // ── the pane's own 16 ──────────────────────────────────────────
        // A program inside a terminal pane emits indexed colours, and until
        // now they resolved to whatever the *outer* terminal happened to use
        // — so a pane could contradict the theme drawn around it.
        Ansi0 = "ansi.0",
        Ansi1 = "ansi.1",
        Ansi2 = "ansi.2",
        Ansi3 = "ansi.3",
        Ansi4 = "ansi.4",
        Ansi5 = "ansi.5",
        Ansi6 = "ansi.6",
        Ansi7 = "ansi.7",
        Ansi8 = "ansi.8",
        Ansi9 = "ansi.9",
        Ansi10 = "ansi.10",
        Ansi11 = "ansi.11",
        Ansi12 = "ansi.12",
        Ansi13 = "ansi.13",
        Ansi14 = "ansi.14",
        Ansi15 = "ansi.15",
    }
}

impl Token {
    /// The 16 indexed colours a terminal pane can emit, in index order.
    pub const ANSI: [Token; 16] = [
        Token::Ansi0,
        Token::Ansi1,
        Token::Ansi2,
        Token::Ansi3,
        Token::Ansi4,
        Token::Ansi5,
        Token::Ansi6,
        Token::Ansi7,
        Token::Ansi8,
        Token::Ansi9,
        Token::Ansi10,
        Token::Ansi11,
        Token::Ansi12,
        Token::Ansi13,
        Token::Ansi14,
        Token::Ansi15,
    ];

    /// Whether this token is drawn *behind* content rather than as content.
    /// The contrast check reads this: measuring a surface against itself
    /// says nothing, and warning that a background is illegible against the
    /// background is noise.
    pub const fn is_surface(self) -> bool {
        matches!(
            self,
            Token::SurfaceBackground
                | Token::SurfaceSelected
                | Token::SurfaceRaised
                | Token::SurfaceRaisedSubtle
                | Token::SurfaceRaisedBright
                | Token::SurfaceRecessed
                | Token::StateDiffAdded
                | Token::StateDiffRemoved
                | Token::BorderDefault
                | Token::BorderFaint
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ansi_block_is_indexed_in_order() {
        for (index, token) in Token::ANSI.iter().enumerate() {
            assert_eq!(token.name(), format!("ansi.{index}"));
        }
    }

    #[test]
    fn no_token_names_a_hue() {
        // The rule the vocabulary exists to hold: a token says what the
        // thing is. A theme that paints `state.landed` orange is still
        // correct; a token called `violet` would have been wrong.
        const HUES: &[&str] = &[
            "red", "green", "blue", "cyan", "violet", "amber", "sage", "grey", "gray", "white",
            "black", "orange", "purple", "yellow",
        ];
        for token in Token::ALL {
            for hue in HUES {
                assert!(
                    !token.name().contains(hue),
                    "token `{token}` names a hue rather than a meaning"
                );
            }
        }
    }
}
