//! The `uze` binary's library half: the terminal UI and what it shares with
//! the CLI.

pub mod keymap;
pub mod self_update;
/// Where a trace goes: the text log `UZE_LOG` switches on, the OTLP
/// exporter the `telemetry` feature adds, and the `TRACEPARENT` handshake
/// across the one process boundary UZE owns.
pub mod telemetry;
/// Assembling the theme both surfaces draw in. Shared by the TUI and the
/// CLI, so it sits in the library half rather than beside either of them.
pub mod theme;
pub mod ui;
