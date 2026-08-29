//! CLI presentation primitives.
//!
//! Text reports remain useful in a pipe, but an interactive terminal gets the
//! same restrained hierarchy as the TUI: warm text, sage for healthy state,
//! amber for attention, and red only for failures.

use std::{io::IsTerminal, time::Duration};

use anstyle::{Color, RgbColor, Style};
use indicatif::{ProgressBar, ProgressStyle};

// The single palette definition for the whole CLI. `clap_styles()` below
// hands these exact same `anstyle::Style` values to `clap`'s own Styles
// builder, so `--help`/usage/error text generated *by clap* never drifts
// from the text this module renders by hand.
const BRIGHT: Style = Style::new()
    .bold()
    .fg_color(Some(Color::Rgb(RgbColor(242, 240, 234))));
// No `.dimmed()` here: SGR-faint stacked on an already-muted RGB color
// renders inconsistently across terminals (many blend it further toward
// the background), which is what made every gray line look washed out.
// The TUI (`src/ui.rs`) never combines dim with a color for this reason —
// it only ever reaches for a darker RGB value.
const MUTED: Style = Style::new().fg_color(Some(Color::Rgb(RgbColor(107, 113, 118))));
// Bold gray: section headings need to read as structure, not as another
// muted label, so they get weight instead of a brighter/different hue.
const HEADING: Style = MUTED.bold();
const ACCENT: Style = Style::new().fg_color(Some(Color::Rgb(RgbColor(143, 209, 158))));
const WARNING: Style = Style::new().fg_color(Some(Color::Rgb(RgbColor(224, 181, 103))));
const DANGER: Style = Style::new().fg_color(Some(Color::Rgb(RgbColor(224, 118, 95))));

fn color_enabled() -> bool {
    std::io::stdout().is_terminal()
        && std::env::var_os("NO_COLOR").is_none()
        && std::env::var("TERM").is_ok_and(|term| term != "dumb")
}

fn paint(text: impl AsRef<str>, style: Style) -> String {
    let text = text.as_ref();
    if color_enabled() {
        format!("{style}{text}{style:#}")
    } else {
        text.to_owned()
    }
}

pub fn title(text: impl AsRef<str>) -> String {
    paint(text, BRIGHT)
}
pub fn section(text: impl AsRef<str>) -> String {
    paint(text, HEADING)
}
pub fn label(text: impl AsRef<str>) -> String {
    paint(text, MUTED)
}
pub fn accent(text: impl AsRef<str>) -> String {
    paint(text, ACCENT)
}
pub fn success_text(text: impl AsRef<str>) -> String {
    paint(text, ACCENT)
}
pub fn warning_text(text: impl AsRef<str>) -> String {
    paint(text, WARNING)
}
pub fn error_text(text: impl AsRef<str>) -> String {
    paint(text, DANGER)
}
/// The single most important line in a report (a status headline, a final
/// pass/fail) — bold, matching the TUI's status line (`ui.rs`'s
/// `Status::Success`/`Status::Error`, which are always bold), so the verdict
/// outweighs the incidental success/warning text sprinkled through the body.
pub fn success_heading(text: impl AsRef<str>) -> String {
    paint(text, ACCENT.bold())
}
pub fn warning_heading(text: impl AsRef<str>) -> String {
    paint(text, WARNING.bold())
}

/// The same palette, handed to `clap`'s own Styles builder so a missing
/// argument or an unrecognized subcommand renders in the same voice as
/// every hand-written report in this module, instead of clap's defaults.
pub fn clap_styles() -> clap::builder::Styles {
    clap::builder::Styles::styled()
        .header(HEADING)
        .usage(HEADING)
        .literal(ACCENT)
        .placeholder(MUTED)
        .error(DANGER.bold())
        .valid(ACCENT)
        .invalid(DANGER)
}

/// A borderless, ANSI-aware table: aligned columns without the box-drawing
/// clutter, for the many places a command lists rows of related data
/// (`plugin list`, `market list`, help's command table, …). One
/// construction path means every list in the CLI lines up the same way,
/// instead of each call site hand-computing its own column widths.
fn table() -> comfy_table::Table {
    let mut table = comfy_table::Table::new();
    table
        .load_preset(comfy_table::presets::NOTHING)
        .set_content_arrangement(comfy_table::ContentArrangement::Disabled);
    table
}

/// Renders `rows` as left-aligned, two-space-indented columns, gapped by
/// two spaces. `String` cells may carry this module's ANSI styling —
/// `comfy-table` measures visual width, not byte length, so colored and
/// plain columns still line up.
pub fn aligned_rows(rows: Vec<Vec<String>>) -> String {
    let mut table = table();
    let columns = rows.first().map_or(0, Vec::len);
    for row in rows {
        table.add_row(row);
    }
    for index in 0..columns.saturating_sub(1) {
        if let Some(column) = table.column_mut(index) {
            column.set_padding((0, 2));
        }
    }
    table
        .to_string()
        .lines()
        .map(|line| format!("  {}", line.trim_end()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Renders several row-groups shown under separate headings (e.g. the
/// top-level help's `Project:` and `Machine:` command lists) against one
/// shared column layout. Calling `aligned_rows` once per group has each
/// group compute its own column width in isolation, so two lists of the
/// same kind of content land at different gutters purely because of how
/// they happened to be split into calls — this builds one table across all
/// of them, then hands back each group's already-aligned lines separately
/// so the caller can still print its own heading before each.
pub fn aligned_groups(groups: Vec<Vec<Vec<String>>>) -> Vec<String> {
    let sizes: Vec<usize> = groups.iter().map(Vec::len).collect();
    let rendered = aligned_rows(groups.into_iter().flatten().collect());
    let mut lines = rendered.lines();
    sizes
        .into_iter()
        .map(|size| lines.by_ref().take(size).collect::<Vec<_>>().join("\n"))
        .collect()
}

// Fixed so every section rule in a report is the same length. Sizing the
// rule to its own heading's width (the previous behavior) produced a
// different, arbitrary-looking underline per section instead of a
// consistent divider — comfortably longer than any current heading
// ("Project environment", 20 chars).
const RULE_WIDTH: usize = 24;

/// A report section heading. The heading text stays unchanged outside a TTY,
/// preserving a stable text mode for scripts and saved logs.
pub fn report_section(name: &str) -> String {
    format!("{}\n{}\n", section(name), label("─".repeat(RULE_WIDTH)))
}

pub fn report_title(name: &str, detail: Option<&str>) -> String {
    let mut output = format!("{}\n", title(name));
    if let Some(detail) = detail {
        output.push_str(&format!("{}\n", label(detail)));
    }
    output
}

pub fn key_value(key: &str, value: impl AsRef<str>) -> String {
    format!("  {:<16} {}", label(key), value.as_ref())
}

/// Creates a spinner for long-running operations.
pub fn spinner(message: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_message(message.to_string());
    pb.enable_steady_tick(Duration::from_millis(120));
    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
            .template("{spinner:.green} {msg}")
            .expect("valid progress template"),
    );
    pb
}

pub fn success(msg: &str) {
    println!("{} {}", success_icon(), msg);
}
pub fn warn(msg: &str) {
    eprintln!("{} {}", warning_icon(), msg);
}
pub fn error(msg: &str) {
    eprintln!("{} {}", error_icon(), msg);
}

pub trait Colorize {
    fn green(&self) -> String;
    fn yellow(&self) -> String;
    fn red(&self) -> String;
    fn cyan(&self) -> String;
    fn dim(&self) -> String;
    fn bold(&self) -> String;
}

impl Colorize for str {
    fn green(&self) -> String {
        success_text(self)
    }
    fn yellow(&self) -> String {
        warning_text(self)
    }
    fn red(&self) -> String {
        error_text(self)
    }
    fn cyan(&self) -> String {
        accent(self)
    }
    fn dim(&self) -> String {
        label(self)
    }
    fn bold(&self) -> String {
        title(self)
    }
}

impl Colorize for String {
    fn green(&self) -> String {
        self.as_str().green()
    }
    fn yellow(&self) -> String {
        self.as_str().yellow()
    }
    fn red(&self) -> String {
        self.as_str().red()
    }
    fn cyan(&self) -> String {
        self.as_str().cyan()
    }
    fn dim(&self) -> String {
        self.as_str().dim()
    }
    fn bold(&self) -> String {
        self.as_str().bold()
    }
}

pub fn step_header(step: usize, total: usize, harness: &str) -> String {
    format!(
        "{} {} {}",
        label(format!("[{step}/{total}]")),
        title(harness),
        label("— provisioning…")
    )
}

pub fn success_icon() -> String {
    success_text("✓")
}
pub fn warning_icon() -> String {
    warning_text("!")
}
pub fn error_icon() -> String {
    error_text("×")
}
pub fn log_prefix() -> String {
    label("│")
}
