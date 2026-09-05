//! Enforces that every CLI command is either fast (`Budgeted`, backed by an
//! actual performance-budget test) or explicitly `JustifiedSlow` with a
//! stated, reviewable reason — see `specs/cli-performance/spec.md` and
//! ADR 018 (`docs/adr/018-cache-harness-detection-with-fingerprint-ttl-
//! invalidation.md`). This is the durable guardrail against the exact
//! failure mode that motivated this module: `ensure_default_plugins()`
//! became a hidden, unmeasured bottleneck on nearly every command without
//! anyone deciding that was acceptable, because nothing checked. A command
//! added to `Command`/`ContextAction`/`MarketplaceAction`/`PluginAction`
//! without a matching entry here fails the test suite by name (see
//! `tests::every_cli_command_is_classified`), instead of silently shipping
//! unmeasured.
//!
//! This module is test-only tooling: it changes nothing about what any
//! command does. `CLASSIFICATION` is consulted only by the tests below.

use clap::CommandFactory;

use crate::Cli;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PerformanceClass {
    /// Must complete in low milliseconds once its cache is warm, with no
    /// manual action required — see `specs/cli-performance/spec.md`. Every
    /// command in this class routes through `UzeApplication::detect_cached`
    /// (directly or via `ensure_default_plugins`/`prepare_detected_
    /// integrations`), which is what
    /// `uze_application::application::tests::
    /// cache_warm_detect_cached_meets_the_performance_budget` measures.
    Budgeted,
    /// Exempt from the budget for a stated, reviewable reason — normally a
    /// genuine network or vendor-installer operation UZE does not control
    /// the cost of. The reason exists so an exemption is a deliberate,
    /// diff-visible decision rather than a silent default.
    JustifiedSlow(&'static str),
}

/// One entry per leaf command path, space-separated the way a user types
/// it (`"context inspect"`, not `"context::inspect"` or `"context.
/// inspect"`). Every leaf `clap` resolves for `Cli` must appear here
/// exactly once — see `tests::every_cli_command_is_classified`, which
/// fails by name (missing or stale) rather than silently passing.
pub const CLASSIFICATION: &[(&str, PerformanceClass)] = &[
    // Project scope (root-level).
    ("remove", PerformanceClass::Budgeted),
    ("status", PerformanceClass::Budgeted),
    ("context inspect", PerformanceClass::Budgeted),
    ("context plan", PerformanceClass::Budgeted),
    ("context reconcile", PerformanceClass::Budgeted),
    (
        "install",
        PerformanceClass::JustifiedSlow(
            "reconstructs the project's agent environment from agents.lock, acquiring packages",
        ),
    ),
    // Machine scope: theme. Every one of these is a small JSON read plus a
    // directory listing under `$UZE_HOME` — no harness is probed, and no
    // theme is resolved that is not the one being asked about.
    ("theme list", PerformanceClass::Budgeted),
    ("theme use", PerformanceClass::Budgeted),
    ("theme show", PerformanceClass::Budgeted),
    // Machine scope: market.
    ("market list", PerformanceClass::Budgeted),
    ("market remove", PerformanceClass::Budgeted),
    ("market inspect", PerformanceClass::Budgeted),
    (
        "market add",
        PerformanceClass::JustifiedSlow(
            "adds a marketplace discovery source, which may be a remote URL",
        ),
    ),
    // Machine scope: plugin.
    ("plugin list", PerformanceClass::Budgeted),
    ("plugin inspect", PerformanceClass::Budgeted),
    ("plugin remove", PerformanceClass::Budgeted),
    (
        "plugin install",
        PerformanceClass::JustifiedSlow(
            "installs a plugin via a marketplace, a network operation for a non-local source, or a \
             direct source (local path/Git URL)",
        ),
    ),
    (
        "plugin update",
        PerformanceClass::JustifiedSlow(
            "re-resolves an installed plugin's source, a network operation for a non-local source",
        ),
    ),
    // Diagnostics.
    ("doctor", PerformanceClass::Budgeted),
    (
        "terminal attach",
        PerformanceClass::JustifiedSlow("starts or attaches an interactive local terminal client"),
    ),
    (
        "terminal stop",
        PerformanceClass::JustifiedSlow(
            "terminates the explicitly requested persistent terminal session",
        ),
    ),
    (
        "terminal serve",
        PerformanceClass::JustifiedSlow("internal persistent terminal server process"),
    ),
    (
        "setup",
        PerformanceClass::JustifiedSlow(
            "provisions or updates harness executables through each harness's official installer",
        ),
    ),
    // Internal runtime dispatch (ADR-033): not a user command. `hook-exec`
    // runs package hook commands with a child-process boundary, per-handler
    // timeouts, and bounded reads — inherently outside any CLI latency
    // budget, and only ever invoked by the managed hook configuration UZE
    // itself emits.
    (
        "hook-exec",
        PerformanceClass::JustifiedSlow(
            "internal hook runtime dispatch: spawns each authored handler with bounded stdout/timeout semantics",
        ),
    ),
];

/// Every command in `CLASSIFICATION` marked `Budgeted`, for the
/// complementary check that each one has an actual performance-budget
/// test rather than only a classification (`tests::
/// every_budgeted_command_has_a_named_performance_test`).
///
/// Kept as a plain list here — rather than trying to introspect the test
/// binary from `src/main.rs`, which cannot see `uze-application`'s test
/// module — cross-checked by that module's own test names, so a
/// `Budgeted` entry added here without updating that list is caught by a
/// mismatch, not silently trusted.
pub const BUDGETED_COMMAND_TESTS: &[(&str, &str)] = &[
    (
        "remove",
        "uze_application::application::tests::cache_warm_detect_cached_meets_the_performance_budget",
    ),
    (
        "status",
        "uze_application::application::tests::cache_warm_detect_cached_meets_the_performance_budget",
    ),
    (
        "doctor",
        "uze_application::application::tests::cache_warm_detect_cached_meets_the_performance_budget",
    ),
    (
        "context inspect",
        "uze_application::application::tests::cache_warm_detect_cached_meets_the_performance_budget",
    ),
    (
        "context plan",
        "uze_application::application::tests::cache_warm_detect_cached_meets_the_performance_budget",
    ),
    (
        "context reconcile",
        "uze_application::application::tests::cache_warm_detect_cached_meets_the_performance_budget",
    ),
    (
        "theme list",
        "uze_application::application::theme::tests::theme_selection_meets_the_performance_budget",
    ),
    (
        "theme use",
        "uze_application::application::theme::tests::theme_selection_meets_the_performance_budget",
    ),
    (
        "theme show",
        "uze_application::application::theme::tests::theme_selection_meets_the_performance_budget",
    ),
    (
        "market list",
        "uze_application::application::tests::cache_warm_detect_cached_meets_the_performance_budget",
    ),
    (
        "market remove",
        "uze_application::application::tests::cache_warm_detect_cached_meets_the_performance_budget",
    ),
    (
        "market inspect",
        "uze_application::application::tests::cache_warm_detect_cached_meets_the_performance_budget",
    ),
    (
        "plugin list",
        "uze_application::application::tests::cache_warm_detect_cached_meets_the_performance_budget",
    ),
    (
        "plugin inspect",
        "uze_application::application::tests::cache_warm_detect_cached_meets_the_performance_budget",
    ),
    (
        "plugin remove",
        "uze_application::application::tests::cache_warm_detect_cached_meets_the_performance_budget",
    ),
];

/// Walks `command`'s subcommand tree, collecting the space-joined path of
/// every leaf (a subcommand with no further subcommands of its own) —
/// this is what a user actually types, derived from `clap`'s own model
/// instead of a hand-maintained list next to the `Command`/`*Action`
/// enums, so the two cannot silently drift apart.
fn leaf_command_paths(command: &clap::Command, prefix: &str, out: &mut Vec<String>) {
    let subcommands: Vec<&clap::Command> = command.get_subcommands().collect();
    if subcommands.is_empty() {
        if !prefix.is_empty() {
            out.push(prefix.to_owned());
        }
        return;
    }
    for sub in subcommands {
        let path = if prefix.is_empty() {
            sub.get_name().to_owned()
        } else {
            format!("{prefix} {}", sub.get_name())
        };
        leaf_command_paths(sub, &path, out);
    }
}

/// Every leaf command path `clap` currently resolves for `Cli`, excluding
/// the implicit `help` subcommand `clap::Subcommand` derives by default —
/// it carries no application logic of its own to classify.
pub fn current_leaf_commands() -> Vec<String> {
    let mut out = Vec::new();
    leaf_command_paths(&Cli::command(), "", &mut out);
    out.retain(|path| path != "help");
    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn every_cli_command_is_classified() {
        let actual: BTreeSet<String> = current_leaf_commands().into_iter().collect();
        let classified: BTreeSet<&str> = CLASSIFICATION.iter().map(|(path, _)| *path).collect();

        let unclassified: Vec<&String> = actual
            .iter()
            .filter(|path| !classified.contains(path.as_str()))
            .collect();
        assert!(
            unclassified.is_empty(),
            "command(s) added to the CLI without a PerformanceClass entry in \
             src/command_performance.rs: {unclassified:?}"
        );

        let stale: Vec<&&str> = classified
            .iter()
            .filter(|path| !actual.contains(**path))
            .collect();
        assert!(
            stale.is_empty(),
            "command_performance::CLASSIFICATION has entries for commands that no \
             longer exist in the CLI: {stale:?}"
        );
    }

    #[test]
    fn every_budgeted_command_has_a_named_performance_test() {
        let budgeted: BTreeSet<&str> = CLASSIFICATION
            .iter()
            .filter(|(_, class)| matches!(class, PerformanceClass::Budgeted))
            .map(|(path, _)| *path)
            .collect();
        let tested: BTreeSet<&str> = BUDGETED_COMMAND_TESTS
            .iter()
            .map(|(path, _)| *path)
            .collect();

        let untested: Vec<&&str> = budgeted
            .iter()
            .filter(|path| !tested.contains(**path))
            .collect();
        assert!(
            untested.is_empty(),
            "command(s) classified Budgeted with no entry in BUDGETED_COMMAND_TESTS \
             (a command marked \"should be fast\" must have an actual test checking \
             that): {untested:?}"
        );

        let untracked: Vec<&&str> = tested
            .iter()
            .filter(|path| !budgeted.contains(**path))
            .collect();
        assert!(
            untracked.is_empty(),
            "BUDGETED_COMMAND_TESTS has entries for commands not classified Budgeted \
             in CLASSIFICATION: {untracked:?}"
        );
    }

    #[test]
    fn every_justified_slow_command_states_a_reason() {
        for (path, class) in CLASSIFICATION {
            if let PerformanceClass::JustifiedSlow(reason) = class {
                assert!(
                    !reason.trim().is_empty(),
                    "{path} is JustifiedSlow with an empty reason — state why"
                );
            }
        }
    }
}
