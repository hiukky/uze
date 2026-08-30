//! The shared TUI color palette, duplicated from `src/ui.rs` — see the
//! crate root doc comment for why this is a copy rather than a shared
//! import. Values must be kept byte-identical to `src/ui.rs`'s own
//! constants of the same name; an extension rendering a color the rest of
//! the TUI doesn't use reads as a foreign surface, not part of the product.

use ratatui::style::Color;

pub const BASE: Color = Color::Rgb(10, 12, 13); // #0a0c0d
pub const TEXT_BRIGHT: Color = Color::Rgb(242, 240, 234); // #f2f0ea
pub const TEXT_SECONDARY: Color = Color::Rgb(168, 166, 160); // #a8a6a0
pub const MUTED: Color = Color::Rgb(107, 113, 118); // #6b7176
pub const TEXT_DIM: Color = Color::Rgb(91, 96, 101); // #5b6065
pub const TEXT_FAINT: Color = Color::Rgb(61, 66, 71); // #3d4247
pub const ACCENT: Color = Color::Rgb(143, 209, 158); // #8fd19e
pub const SUCCESS: Color = ACCENT;
pub const WARNING: Color = Color::Rgb(224, 181, 103); // #e0b567 (amber)
pub const DANGER: Color = Color::Rgb(224, 118, 95); // #e0765f (red)
pub const BLUE: Color = Color::Rgb(125, 151, 201); // #7d97c9
pub const BORDER_FAINT: Color = Color::Rgb(22, 24, 25);
pub const BORDER: Color = Color::Rgb(30, 31, 32);
pub const SURFACE_OVERLAY: Color = Color::Rgb(32, 34, 35);
/// The inactive nav label color (`#9a9892` in the design).
pub const NAV_INACTIVE: Color = Color::Rgb(154, 152, 146);
