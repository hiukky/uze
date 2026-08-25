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
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
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

pub trait Colorize {
    fn green(&self) -> String;
    fn yellow(&self) -> String;
    fn red(&self) -> String;
    fn cyan(&self) -> String;
    fn dim(&self) -> String;
    fn bold(&self) -> String;
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
    fn cyan(&self) -> String {
        format!("\x1b[36m{}\x1b[0m", self)
    }
    fn dim(&self) -> String {
        format!("\x1b[2m{}\x1b[0m", self)
    }
    fn bold(&self) -> String {
        format!("\x1b[1m{}\x1b[0m", self)
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

/// Formatters for `uze setup` step headers.
pub fn step_header(step: usize, total: usize, harness: &str) -> String {
    format!(
        "{} {} {}",
        format!("[{}/{}]", step, total).cyan().dim(),
        harness.cyan().bold(),
        "— provisioning…".dim()
    )
}

pub fn success_icon() -> String {
    "✓".green()
}
pub fn warning_icon() -> String {
    "⚠".yellow()
}
pub fn error_icon() -> String {
    "✗".red()
}
pub fn log_prefix() -> String {
    "│".dim()
}
