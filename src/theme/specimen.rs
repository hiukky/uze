//! A glyph set drawn so it can be measured: `uze theme specimen`.
//!
//! What a font and a terminal make of UZE's icons cannot be asked of the
//! terminal — only seen. This draws every symbol a set declares in the slot
//! the product draws it in, at fixed positions, beside a calibration row
//! that tells whoever measures the screenshot where the cells are. The
//! Rendering Lab screenshots it in real terminals; an operator can do the
//! same on a terminal the Lab cannot run, and measure that with the same
//! analyzer.
//!
//! Plain text on purpose, with no colour and no escape sequence: the
//! analyzer reads ink against the terminal's own background, and a
//! specimen that painted grounds would be measuring UZE's palette instead
//! of the font.

use serde::Serialize;
use uze_application::{Result, UzeError};
use uze_theme::{Symbol, Theme};

/// Full blocks in the calibration row. Wide enough that the run of them is
/// unmistakable in a screenshot with window chrome around it, and that the
/// cell width averages out the rounding of any one cell.
const CALIBRATION_CELLS: usize = 40;
/// Where the icon sits on its row; the gutter follows, then the reference
/// letter, which is where the layout says text resumes.
const SLOT_COLUMN: usize = 0;
const TEXT_COLUMN: usize = SLOT_COLUMN + 2;
/// The same icon again with nothing near it: what it draws when nothing
/// constrains it, which is what *Whole* compares the slot against.
const FREE_COLUMN: usize = 8;
const LABEL_COLUMN: usize = 16;
/// The letter that stands where text resumes after an icon's slot.
const REFERENCE: &str = "x";

/// The specimen's layout, which `--format json` prints for the analyzer.
/// Rows count from the calibration row, columns from the left edge.
#[derive(Debug, Serialize)]
pub struct Specimen {
    pub set: String,
    pub calibration_row: usize,
    pub calibration_cells: usize,
    /// A row holding only the reference letter at the text column: where
    /// that letter's ink begins when no icon precedes it.
    pub reference_row: usize,
    pub slot_column: usize,
    pub text_column: usize,
    pub free_column: usize,
    pub label_column: usize,
    pub symbols: Vec<SpecimenRow>,
}

#[derive(Debug, Serialize)]
pub struct SpecimenRow {
    pub name: &'static str,
    pub glyph: String,
    /// Whether the glyph is an icon a patched font supplies — the ones the
    /// slot exists for.
    pub icon: bool,
    /// Cells the set declares the glyph occupies.
    pub width: u16,
    pub row: usize,
}

/// The specimen of a bundled glyph set, drawn over the built-in default.
pub fn specimen(set: &str) -> Result<Specimen> {
    let Some(file) = uze_theme::glyph_set_file(set) else {
        return Err(UzeError::UnusableTheme(format!(
            "no glyph set `{set}` — UZE carries {}",
            uze_theme::glyph_sets().join(", ")
        )));
    };
    let identity = uze_theme::Identity::from_file(set, uze_theme::default_file());
    let resolved = uze_theme::resolve_stack(&identity, &[uze_theme::default_file(), file])
        .map_err(|error| UzeError::UnusableTheme(format!("glyph set `{set}`: {error}")))?;
    Ok(layout(set, &resolved.theme, |symbol| {
        file.symbols.contains_key(symbol.name())
    }))
}

fn layout(set: &str, theme: &Theme, declared: impl Fn(Symbol) -> bool) -> Specimen {
    const FIRST_SYMBOL_ROW: usize = 4;
    let symbols = Symbol::ALL
        .iter()
        .copied()
        .filter(|symbol| declared(*symbol))
        .filter(|symbol| !theme.glyph(*symbol).is_empty())
        .enumerate()
        .map(|(index, symbol)| {
            let resolved = theme.symbol(symbol);
            SpecimenRow {
                name: symbol.name(),
                glyph: resolved.glyph().to_owned(),
                icon: resolved.is_icon(),
                width: resolved.width(),
                // A blank row between symbols, so ink that leaves its row
                // is attributable to the symbol it left.
                row: FIRST_SYMBOL_ROW + 2 * index,
            }
        })
        .collect();
    Specimen {
        set: set.to_owned(),
        calibration_row: 0,
        calibration_cells: CALIBRATION_CELLS,
        reference_row: 2,
        slot_column: SLOT_COLUMN,
        text_column: TEXT_COLUMN,
        free_column: FREE_COLUMN,
        label_column: LABEL_COLUMN,
        symbols,
    }
}

impl Specimen {
    /// The specimen as the lines a terminal draws.
    pub fn lines(&self) -> Vec<String> {
        let last_row = self
            .symbols
            .last()
            .map_or(self.reference_row, |row| row.row);
        let mut lines = vec![String::new(); last_row + 1];
        lines[self.calibration_row] = "█".repeat(self.calibration_cells);
        lines[self.reference_row] = format!("{}{REFERENCE}", " ".repeat(self.text_column));
        for symbol in &self.symbols {
            let width = usize::from(symbol.width);
            let mut line = String::new();
            let mut column = 0;
            let mut put = |text: &str, at: usize, cells: usize| {
                line.push_str(&" ".repeat(at.saturating_sub(column)));
                line.push_str(text);
                column = at.max(column) + cells;
            };
            put(&symbol.glyph, self.slot_column, width);
            put(
                REFERENCE,
                self.text_column.max(self.slot_column + width + 1),
                1,
            );
            put(&symbol.glyph, self.free_column, width);
            put(symbol.name, self.label_column, symbol.name.len());
            lines[symbol.row] = line;
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_symbol_a_set_declares_is_drawn_on_a_row_of_its_own() {
        let nerd = specimen("nerd").expect("a bundled set");
        let declared = uze_theme::glyph_set_file("nerd")
            .expect("bundled")
            .symbols
            .len();
        assert_eq!(nerd.symbols.len(), declared);
        let lines = nerd.lines();
        for symbol in &nerd.symbols {
            let line = &lines[symbol.row];
            assert!(
                line.starts_with(&format!("{} {REFERENCE}", symbol.glyph)),
                "{line:?}"
            );
            assert!(line.ends_with(symbol.name), "{line:?}");
        }
        assert!(lines[0].chars().all(|c| c == '█'));
    }

    #[test]
    fn the_free_sample_has_nothing_near_it() {
        let nerd = specimen("nerd").expect("a bundled set");
        let lines = nerd.lines();
        for symbol in &nerd.symbols {
            let cells: Vec<char> = lines[symbol.row].chars().collect();
            let free = nerd.free_column;
            assert_eq!(cells[free].to_string(), symbol.glyph);
            assert!(cells[free + 1..nerd.label_column].iter().all(|c| *c == ' '));
            assert!(cells[nerd.text_column + 1..free].iter().all(|c| *c == ' '));
        }
    }

    #[test]
    fn an_ascii_specimen_is_ascii_but_for_its_calibration_row() {
        let ascii = specimen("ascii").expect("a bundled set");
        for line in &ascii.lines()[1..] {
            assert!(line.is_ascii(), "{line:?}");
        }
    }

    #[test]
    fn an_unknown_set_names_the_ones_there_are() {
        let error = specimen("wingdings").expect_err("not a set").to_string();
        for set in uze_theme::glyph_sets() {
            assert!(error.contains(set), "{error}");
        }
    }
}
