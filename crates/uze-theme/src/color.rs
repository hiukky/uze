//! A colour, and the two operations a theme file needs of one.

use std::fmt;

/// An opaque 24-bit colour. Deliberately not a rendering library's colour
/// type: this crate is the vocabulary, and every consumer adapts it to
/// whatever it draws with.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// Composites `self` at `alpha` (0-255) over `background`.
    ///
    /// This is the operation that used to be done by hand and recorded in a
    /// comment: six of UZE's surfaces are `rgba(255,255,255,α)` pre-blended
    /// over the backdrop, because a terminal has no alpha channel. Doing it
    /// here means a theme author writes the translucent value once instead
    /// of computing the blend — and a light theme gets its own shades from
    /// the same declaration.
    pub fn over(self, background: Rgb, alpha: u8) -> Rgb {
        let blend = |top: u8, bottom: u8| {
            let top = f32::from(top);
            let bottom = f32::from(bottom);
            let a = f32::from(alpha) / 255.0;
            (top * a + bottom * (1.0 - a)).round().clamp(0.0, 255.0) as u8
        };
        Rgb(
            blend(self.0, background.0),
            blend(self.1, background.1),
            blend(self.2, background.2),
        )
    }

    /// WCAG relative luminance, the input to [`contrast_ratio`].
    fn relative_luminance(self) -> f32 {
        let channel = |value: u8| {
            let value = f32::from(value) / 255.0;
            if value <= 0.039_28 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(self.0) + 0.7152 * channel(self.1) + 0.0722 * channel(self.2)
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
}

/// WCAG contrast ratio between two colours, from 1.0 (identical) to 21.0
/// (black on white). Used to *report* an unreadable theme, never to correct
/// one: a colour the author wrote is the author's decision, but a theme
/// they cannot diagnose would be ours.
pub fn contrast_ratio(a: Rgb, b: Rgb) -> f32 {
    let (lighter, darker) = {
        let (a, b) = (a.relative_luminance(), b.relative_luminance());
        if a >= b { (a, b) } else { (b, a) }
    };
    (lighter + 0.05) / (darker + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compositing_reproduces_the_surfaces_the_design_derived_by_alpha() {
        // Most of UZE's surfaces are an `rgba(…)` over the backdrop, blended
        // by hand because a terminal has no alpha channel — their `src/ui.rs`
        // doc comments record the alpha they came from. Reproducing them here
        // is what says a theme author can write the translucent form and get
        // the design's own value back.
        //
        // Two of today's shades (`border.default` and `surface.raised-bright`)
        // were nudged off their stated alpha by hand and are not reproduced by
        // any single blend, which is exactly why the built-in theme declares
        // every colour as a literal rather than deriving it: the default is
        // byte-identical to what shipped, blend or no blend.
        let base = Rgb(10, 12, 13);
        let white = Rgb(255, 255, 255);
        let accent = Rgb(143, 209, 158);

        assert_eq!(white.over(base, 13), Rgb(22, 24, 25)); // border.faint, a≈0.05
        assert_eq!(accent.over(base, 23), Rgb(22, 30, 26)); // surface.selected, a≈0.09
        assert_eq!(white.over(base, 23), Rgb(32, 34, 35)); // surface.raised, a≈0.09
        assert_eq!(white.over(base, 18), Rgb(27, 29, 30)); // surface.raised-subtle, a≈0.07
        assert_eq!(white.over(base, 6), Rgb(16, 18, 19)); // surface.recessed, a≈0.025
    }

    #[test]
    fn compositing_at_the_extremes_is_a_pure_choice() {
        let base = Rgb(10, 12, 13);
        let top = Rgb(200, 100, 50);
        assert_eq!(top.over(base, 255), top);
        assert_eq!(top.over(base, 0), base);
    }

    #[test]
    fn contrast_spans_the_wcag_range() {
        assert!((contrast_ratio(Rgb(0, 0, 0), Rgb(255, 255, 255)) - 21.0).abs() < 0.01);
        assert!((contrast_ratio(Rgb(7, 7, 7), Rgb(7, 7, 7)) - 1.0).abs() < 0.01);
        // Order does not matter — the ratio is between two colours, not
        // from one to the other.
        let (a, b) = (Rgb(10, 12, 13), Rgb(230, 228, 222));
        assert_eq!(contrast_ratio(a, b), contrast_ratio(b, a));
    }
}
