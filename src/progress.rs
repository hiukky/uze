//! Progress and logging utilities for CLI feedback.

use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

/// Creates a spinner for long-running operations.
pub fn spinner(message: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_message(message.to_string());
    pb.enable_steady_tick(Duration::from_millis(120));
    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(&["⠋", "⠙", "", "⠸", "⠼", "⠴", "⠦", "", "⠇", "⠏"])
            .template("{spinner:.cyan} {msg}")
            .unwrap(),
    );
    pb
}

/// Prints a success message.
pub fn success(msg: &str) {
    println!("{} {}", "✓".green(), msg);
}

/// Prints a warning message.
pub fn warn(msg: &str) {
    println!("{} {}", "⚠".yellow(), msg);
}

/// Prints an error message.
pub fn error(msg: &str) {
    eprintln!("{} {}", "✗".red(), msg);
}

trait Colorize {
    fn green(&self) -> String;
    fn yellow(&self) -> String;
    fn red(&self) -> String;
}

impl Colorize for &str {
    fn green(&self) -> String {
        format!("\x1b[32m{}\x1b[0m", self)
    }
    fn yellow(&self) -> String {
        format!("\x1b[33m{}\x1b[0m", self)
    }
    fn red(&self) -> String {
        format!("\x1b[31m{}\x1b[0m", self)
    }
}
