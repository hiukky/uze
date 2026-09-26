//! CLI presentation primitives.
//!
//! Text reports remain useful in a pipe, but an interactive terminal gets the
//! same restrained hierarchy as the TUI: warm text, sage for healthy state,
//! amber for attention, and red only for failures.

use std::{io::IsTerminal, sync::Mutex, time::Duration};

use anstyle::{Color, RgbColor, Style};
use indicatif::{ProgressBar, ProgressStyle};
use uze_theme::{Symbol, Token};

// The CLI's half of the design system. Every style below resolves the same
// `uze_theme` token the TUI draws with, so `uze status` and the workspace
// client cannot drift the way two hand-kept palettes always did — and
// `clap_styles()` hands these very values to clap's own Styles builder, so
// even the usage and error text clap generates speaks in one voice.
//
// Functions rather than constants: a theme is resolved at runtime, and the
// active one can change between two invocations of the same process.
fn styled(token: Token) -> Style {
    let rgb = uze_theme::active().color(token);
    Style::new().fg_color(Some(Color::Rgb(RgbColor(rgb.0, rgb.1, rgb.2))))
}

fn bright() -> Style {
    styled(Token::TextBright).bold()
}
// No `.dimmed()` anywhere here: SGR-faint stacked on an already-muted RGB
// color renders inconsistently across terminals (many blend it further
// toward the background), which is what made every gray line look washed
// out. The TUI never combines dim with a color for the same reason — it
// only ever reaches for a darker token.
fn muted() -> Style {
    styled(Token::TextMuted)
}
// Bold muted: section headings need to read as structure, not as another
// muted label, so they get weight instead of a brighter or different hue.
fn heading() -> Style {
    muted().bold()
}
fn accent_style() -> Style {
    styled(Token::Accent)
}
fn warning() -> Style {
    styled(Token::StateWarning)
}
fn danger() -> Style {
    styled(Token::StateDanger)
}

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
    paint(text, bright())
}
pub fn section(text: impl AsRef<str>) -> String {
    paint(text, heading())
}
pub fn label(text: impl AsRef<str>) -> String {
    paint(text, muted())
}
pub fn accent(text: impl AsRef<str>) -> String {
    paint(text, accent_style())
}
pub fn success_text(text: impl AsRef<str>) -> String {
    paint(text, styled(Token::StateSuccess))
}
pub fn warning_text(text: impl AsRef<str>) -> String {
    paint(text, warning())
}
pub fn error_text(text: impl AsRef<str>) -> String {
    paint(text, danger())
}
/// The single most important line in a report (a status headline, a final
/// pass/fail) — bold, matching the TUI's status line (`ui.rs`'s
/// `Status::Success`/`Status::Error`, which are always bold), so the verdict
/// outweighs the incidental success/warning text sprinkled through the body.
pub fn success_heading(text: impl AsRef<str>) -> String {
    paint(text, styled(Token::StateSuccess).bold())
}
pub fn warning_heading(text: impl AsRef<str>) -> String {
    paint(text, warning().bold())
}
pub fn error_heading(text: impl AsRef<str>) -> String {
    paint(text, danger().bold())
}
/// A name the reader acts on — a harness id, a command to run — in the
/// accent, with the weight of a heading.
pub fn accent_heading(text: impl AsRef<str>) -> String {
    paint(text, accent_style().bold())
}

/// The same palette, handed to `clap`'s own Styles builder so a missing
/// argument or an unrecognized subcommand renders in the same voice as
/// every hand-written report in this module, instead of clap's defaults.
pub fn clap_styles() -> clap::builder::Styles {
    clap::builder::Styles::styled()
        .header(heading())
        .usage(heading())
        .literal(accent_style())
        .placeholder(muted())
        .error(danger().bold())
        .valid(accent_style())
        .invalid(danger())
}

/// A borderless, ANSI-aware table: aligned columns without the box-drawing
/// clutter, for the many places a command lists rows of related data
/// (`status -m`, `market list`, help's command table, …). One
/// construction path means every list in the CLI lines up the same way,
/// instead of each call site hand-computing its own column widths.
fn table() -> comfy_table::Table {
    let mut table = comfy_table::Table::new();
    table
        .load_style(comfy_table::presets::NOTHING)
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

/// The spinner currently drawing, if one is. A question asked while a
/// spinner ticks has to get the terminal to itself — see [`uninterrupted`].
static DRAWING: Mutex<Option<ProgressBar>> = Mutex::new(None);

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
    if let Ok(mut drawing) = DRAWING.lock() {
        *drawing = Some(pb.clone());
    }
    pb
}

/// Whether an attempt that failed — HTTPS before the SSH that answered — is
/// printed too. Finished steps always are.
static VERBOSE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The step the spinner is showing, and when it began.
static CURRENT: Mutex<Option<(String, std::time::Instant)>> = Mutex::new(None);

/// Shows what an operation is doing on the spinner drawing now: every step
/// the domain reports takes the spinner's line, and the one it replaces
/// stays above it, checked, with how long it took — the account a
/// thirteen-second command owes the person waiting on it. With `verbose`,
/// an attempt that failed and was followed by another is printed as well.
pub fn follow_steps(verbose: bool) {
    VERBOSE.store(verbose, std::sync::atomic::Ordering::Relaxed);
    uze::steps::listen(on_step);
}

fn on_step(step: &uze::steps::Step) {
    let Some(bar) = DRAWING
        .lock()
        .ok()
        .and_then(|drawing| drawing.clone())
        .filter(|bar| !bar.is_finished())
    else {
        return;
    };
    let verbose = VERBOSE.load(std::sync::atomic::Ordering::Relaxed);
    match describe(step) {
        Some(Described::Now(line)) => {
            // Two receipts at one harness read as one step, not two lines.
            if CURRENT.lock().ok().is_some_and(|current| {
                current
                    .as_ref()
                    .is_some_and(|(showing, _)| *showing == line)
            }) {
                return;
            }
            settle(&bar, true);
            bar.set_message(format!("{line}…"));
            if let Ok(mut current) = CURRENT.lock() {
                *current = Some((line, std::time::Instant::now()));
            }
        }
        Some(Described::Failed(line)) => {
            let began = CURRENT.lock().ok().and_then(|mut current| current.take());
            if verbose {
                let took = began.map(|(_, at)| at.elapsed()).unwrap_or_default();
                say(
                    &bar,
                    &format!("{} {line} {}", error_icon(), label(took_to_say(took))),
                );
            }
        }
        None => {}
    }
}

/// Prints the step still showing as finished — or as where the operation
/// failed.
pub fn settle_steps(bar: &ProgressBar, succeeded: bool) {
    settle(bar, succeeded);
}

fn settle(bar: &ProgressBar, succeeded: bool) {
    let Some((line, at)) = CURRENT.lock().ok().and_then(|mut current| current.take()) else {
        return;
    };
    let icon = if succeeded {
        success_icon()
    } else {
        error_icon()
    };
    say(
        bar,
        &format!("{icon} {line} {}", label(took_to_say(at.elapsed()))),
    );
}

/// A line above the spinner, or on stderr when there is no terminal to
/// draw one on — a CI log still gets the account.
fn say(bar: &ProgressBar, line: &str) {
    if bar.is_hidden() {
        eprintln!("{line}");
    } else {
        bar.println(line);
    }
}

#[cfg(test)]
mod step_tests {
    use super::*;
    use uze::steps::Step;

    #[test]
    fn each_step_reads_as_what_it_does_to_what() {
        let reach = Step::of(&[
            ("step", "reach"),
            ("repository", "github.com/hiukky/ai"),
            ("via", "ssh"),
        ]);
        assert_eq!(
            describe(&reach),
            Some(Described::Now(
                "Reaching github.com/hiukky/ai over SSH".to_owned()
            ))
        );
        let failed = Step::of(&[
            ("step", "reach_failed"),
            ("repository", "github.com/hiukky/ai"),
            ("via", "https (anonymous)"),
            ("reason", "terminal prompts disabled"),
        ]);
        assert_eq!(
            describe(&failed),
            Some(Described::Failed(
                "github.com/hiukky/ai over HTTPS: terminal prompts disabled".to_owned()
            ))
        );
        let download = Step::of(&[("step", "download"), ("files", "5")]);
        assert_eq!(
            describe(&download),
            Some(Described::Now("Downloading 5 files".to_owned()))
        );
        assert_eq!(describe(&Step::of(&[("step", "unheard-of")])), None);
    }

    #[test]
    fn a_fast_step_reads_in_milliseconds_and_a_slow_one_in_seconds() {
        assert_eq!(took_to_say(Duration::from_millis(310)), "310ms");
        assert_eq!(took_to_say(Duration::from_millis(2140)), "2.1s");
    }
}

/// `42s`, `12m`, `3h`: how old a fetched copy is, to the unit that matters.
fn ago(seconds: u64) -> String {
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m", seconds / 60),
        _ => format!("{}h", seconds / 3600),
    }
}

/// `310ms` under a second, `2.1s` from there: a fast step reads as fast
/// rather than as `0.0s`, and a slow one is not a wall of digits.
fn took_to_say(took: Duration) -> String {
    if took < Duration::from_secs(1) {
        format!("{}ms", took.as_millis())
    } else {
        format!("{:.1}s", took.as_secs_f32())
    }
}

#[derive(Debug, Eq, PartialEq)]
enum Described {
    /// What is happening now.
    Now(String),
    /// An attempt that did not work, and why; the operation goes on.
    Failed(String),
}

/// The words for a step. Kept here, beside the spinner that shows them,
/// because the domain reports what it does and never how to say it.
fn describe(step: &uze::steps::Step) -> Option<Described> {
    let field = |name: &str| step.get(name).unwrap_or_default().to_owned();
    let over = |via: &str| match via {
        "https (anonymous)" => "over HTTPS".to_owned(),
        "https (credentials)" => "over HTTPS with your credentials".to_owned(),
        "ssh" => "over SSH".to_owned(),
        "local" => "on this machine".to_owned(),
        other => format!("over {other}"),
    };
    Some(match step.get("step")? {
        "reach" => Described::Now(format!(
            "Reaching {} {}",
            field("repository"),
            over(&field("via"))
        )),
        "reach_failed" => Described::Failed(format!(
            "{} {}: {}",
            field("repository"),
            over(&field("via")),
            field("reason")
        )),
        "catalogue" => Described::Now("Reading the marketplace catalogue".to_owned()),
        "fresh" => Described::Now(format!(
            "Using {} as fetched {} ago (uze update fetches the latest)",
            field("repository"),
            ago(field("age_secs").parse().unwrap_or_default())
        )),
        "download" => Described::Now(format!("Downloading {} files", field("files"))),
        "deliver" => Described::Now(format!("Delivering to {}", field("harness"))),
        "detach" => Described::Now(format!("Removing from {}", field("harness"))),
        "lock" => Described::Now("Recording it in agents.yaml and agents.lock".to_owned()),
        _ => return None,
    })
}

/// Runs `question` with the terminal to itself.
///
/// A spinner redraws its line on stderr every 120 ms, and so does the
/// prompt widget — so a question asked mid-operation was overwritten as
/// fast as it was drawn, which for the trust prompt meant a person
/// answering something they could not read. Held only for as long as the
/// question takes; nothing is suspended when no spinner is drawing.
pub fn uninterrupted<T>(question: impl FnOnce() -> T) -> T {
    // Cloned out rather than held: the question may start a spinner of its
    // own, and a lock held across it would deadlock. A finished bar is
    // skipped rather than suspended — suspending one redraws the line it
    // already closed.
    let drawing = DRAWING
        .lock()
        .ok()
        .and_then(|drawing| drawing.clone())
        .filter(|spinner| !spinner.is_finished());
    match drawing {
        Some(spinner) => spinner.suspend(question),
        None => question(),
    }
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

pub fn step_header(step: usize, total: usize, harness: &str) -> String {
    format!(
        "{} {} {}",
        label(format!("[{step}/{total}]")),
        title(harness),
        label("— provisioning…")
    )
}

pub fn success_icon() -> String {
    success_text(glyph(Symbol::MarkOk))
}
pub fn warning_icon() -> String {
    warning_text(glyph(Symbol::MarkAttention))
}
pub fn error_icon() -> String {
    error_text(glyph(Symbol::MarkCross))
}
pub fn log_prefix() -> String {
    label(glyph(Symbol::TreeColumnDivider))
}

/// The CLI's half of the symbol library. Same set the TUI draws from, for
/// the same reason: a terminal without a Unicode font is not a terminal UZE
/// should be unusable in, and the marks the CLI prints are as much chrome as
/// the ones the TUI does.
pub fn glyph(symbol: Symbol) -> String {
    uze_theme::active().glyph(symbol).to_owned()
}

/// The columns a glyph occupies. A caller laying a column out from a mark
/// — an interactive prompt's cursor gutter, say — must ask rather than
/// assume one: a theme may have replaced it with something wider.
pub fn glyph_width(symbol: Symbol) -> usize {
    uze_theme::active().symbol(symbol).width().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cli_and_the_tui_resolve_a_shared_token_to_the_same_colour() {
        // The point of the whole exercise. These two surfaces used to hold
        // separate copies of the palette, in different style types, and a
        // colour changed in one was silently wrong in the other.
        for token in [
            Token::TextBright,
            Token::TextMuted,
            Token::Accent,
            Token::StateWarning,
            Token::StateDanger,
        ] {
            let cli = styled(token).get_fg_color();
            let tui = uze_theme::active().color(token);
            assert_eq!(
                cli,
                Some(Color::Rgb(RgbColor(tui.0, tui.1, tui.2))),
                "`{token}` differs between the CLI and the TUI"
            );
        }
    }

    #[test]
    fn colour_is_dropped_rather_than_approximated_when_the_terminal_will_not_take_it() {
        // `paint` is the only place that decides, and it decides by asking
        // the terminal — a themed CLI must still pipe cleanly.
        let plain = paint("text", accent_style());
        if color_enabled() {
            assert!(plain.contains("text"));
        } else {
            assert_eq!(plain, "text");
        }
    }
}
