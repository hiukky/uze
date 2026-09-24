//! Enforces that every CLI command is either fast (`Budgeted`, backed by an
//! actual performance-budget test) or explicitly `JustifiedSlow` with a
//! stated, reviewable reason — see `specs/cli-performance/spec.md` and
//! ADR 018 (`docs/adr/018-cache-harness-detection-with-fingerprint-ttl-
//! invalidation.md`). This is the durable guardrail against the exact
//! failure mode that motivated this module: `ensure_default_plugins()`
//! became a hidden, unmeasured bottleneck on nearly every command without
//! anyone deciding that was acceptable, because nothing checked. A command
//! added to `Command`/`ContextAction`/`MarketplaceAction`/`ConfigAction`
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
    /// manual action required — see `specs/cli-performance/spec.md`. Each
    /// command in this class has a test in
    /// `crates/uze-application/tests/performance.rs` that times the
    /// application call it dispatches to, on a fresh application, in a
    /// world where a live probe or a clone would be visible; the mapping
    /// is `BUDGETED_COMMAND_TESTS`.
    Budgeted,
    /// Exempt from the budget for a stated, reviewable reason — normally a
    /// genuine network or vendor-installer operation UZE does not control
    /// the cost of. The reason exists so an exemption is a deliberate,
    /// diff-visible decision rather than a silent default.
    JustifiedSlow(&'static str),
}

/// One entry per leaf command path, space-separated the way a user types
/// it (`"agent context inspect"`, not `"context::inspect"` or `"context.
/// inspect"`). Every leaf `clap` resolves for `Cli` must appear here
/// exactly once — see `tests::every_cli_command_is_classified`, which
/// fails by name (missing or stale) rather than silently passing.
pub const CLASSIFICATION: &[(&str, PerformanceClass)] = &[
    // Project scope (root-level). `remove`, `status`, `install` and
    // `update` each decide their own scope; `inspect` reads one package's
    // delivery on this machine.
    ("remove", PerformanceClass::Budgeted),
    ("status", PerformanceClass::Budgeted),
    ("inspect", PerformanceClass::Budgeted),
    // The agent's own surface. Budgeted for the same reason `status` is:
    // it reads two project files and the task store, validates in memory,
    // and performs one Git ref rename. An agent waiting on this is an
    // agent not working.
    ("agent work name", PerformanceClass::Budgeted),
    // Not budgeted: it walks the declared directory, reads every file in
    // it and lays out and routes every diagram. The cost is the project's
    // own — what it declares and how large those diagrams are — and there
    // is nothing to cache, because the answer is about the bytes on disk
    // right now. It runs where a check runs, not on the way to something
    // else.
    (
        "agent artifacts check",
        PerformanceClass::JustifiedSlow(
            "lays out and routes every diagram the project declares, which is work \
             proportional to what it holds",
        ),
    ),
    // Context reads and the reconcile they lead to sit on the same
    // surface, for the same audience: a person asks `status`, an agent
    // asks these.
    ("agent context inspect", PerformanceClass::Budgeted),
    ("agent context plan", PerformanceClass::Budgeted),
    ("agent context reconcile", PerformanceClass::Budgeted),
    (
        "install",
        PerformanceClass::JustifiedSlow(
            "converges the project's agent environment or installs a package via a marketplace, \
             which reaches its remote",
        ),
    ),
    (
        "update",
        PerformanceClass::JustifiedSlow(
            "re-resolves each declared marketplace ref or installed package's source, which \
             reaches its remote, and reinstalls what moved",
        ),
    ),
    // Machine scope: config. `theme`/`icons` are each a small JSON read plus
    // a directory listing under `$UZE_HOME` — no harness is probed, and no
    // theme is resolved that is not the one being asked about.
    // `notification` is one `config.toml` key read or written.
    ("config theme list", PerformanceClass::Budgeted),
    ("config theme set", PerformanceClass::Budgeted),
    ("config theme show", PerformanceClass::Budgeted),
    ("config icons", PerformanceClass::Budgeted),
    ("config notification", PerformanceClass::Budgeted),
    // The authoring surface: the marketplace scaffold is born with an
    // initial commit, so its cost is the author's own Git identity. The
    // checks read and parse every file the artifact carries — like
    // `agent artifacts check`, the cost is the artifact's own, and there
    // is nothing to cache because the answer is about the bytes right now.
    (
        "agent market create",
        PerformanceClass::JustifiedSlow(
            "initializes a marketplace as a Git repository and makes its first commit",
        ),
    ),
    (
        "agent market check",
        PerformanceClass::JustifiedSlow(
            "reads and parses the marketplace manifest and every plugin it names; the cost is \
             the artifact's own",
        ),
    ),
    ("agent plugin create", PerformanceClass::Budgeted),
    (
        "agent plugin check",
        PerformanceClass::JustifiedSlow(
            "reads and parses every capability file the plugin carries; the cost is the \
             artifact's own",
        ),
    ),
    // Machine scope: market.
    ("market list", PerformanceClass::Budgeted),
    ("market remove", PerformanceClass::Budgeted),
    // Both are a registry write plus dropping a cache entry. `link` also
    // asks Git what repository the checkout is, which is local and answers
    // in one call.
    ("market link", PerformanceClass::Budgeted),
    ("market unlink", PerformanceClass::Budgeted),
    ("market inspect", PerformanceClass::Budgeted),
    ("market host", PerformanceClass::Budgeted),
    (
        "market add",
        PerformanceClass::JustifiedSlow(
            "adds a marketplace discovery source, which may be a remote URL",
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
    (
        "upgrade",
        PerformanceClass::JustifiedSlow(
            "asks for the latest release and downloads it — in the foreground when asked, in a detached process when a command hands off a stale check",
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
/// module — and cross-checked against that module's source by
/// `tests::every_named_performance_test_exists`, so a name that drifts
/// from the test it points at fails by name rather than pointing at
/// nothing.
pub const BUDGETED_COMMAND_TESTS: &[(&str, &str)] = &[
    (
        "remove",
        "crates/uze-application/tests/performance.rs::removals_meet_the_budget",
    ),
    (
        "agent work name",
        "crates/uze-application/tests/performance.rs::the_agent_surface_meets_the_budget",
    ),
    (
        "status",
        "crates/uze-application/tests/performance.rs::status_meets_the_budget",
    ),
    (
        "inspect",
        "crates/uze-application/tests/performance.rs::plugin_list_and_inspect_meet_the_budget",
    ),
    (
        "doctor",
        "crates/uze-application/tests/performance.rs::doctor_meets_the_budget",
    ),
    (
        "agent context inspect",
        "crates/uze-application/tests/performance.rs::context_reads_and_reconcile_meet_the_budget",
    ),
    (
        "agent context plan",
        "crates/uze-application/tests/performance.rs::context_reads_and_reconcile_meet_the_budget",
    ),
    (
        "agent context reconcile",
        "crates/uze-application/tests/performance.rs::context_reads_and_reconcile_meet_the_budget",
    ),
    (
        "config theme list",
        "uze_application::application::theme::tests::theme_selection_meets_the_performance_budget",
    ),
    (
        "config theme set",
        "uze_application::application::theme::tests::theme_selection_meets_the_performance_budget",
    ),
    (
        "config theme show",
        "uze_application::application::theme::tests::theme_selection_meets_the_performance_budget",
    ),
    (
        "config icons",
        "uze_application::application::theme::tests::theme_selection_meets_the_performance_budget",
    ),
    (
        "config notification",
        "uze_application::application::notifications::tests::notification_choice_meets_the_budget",
    ),
    (
        "agent plugin create",
        "crates/uze-core/src/package/authoring/tests.rs::authoring_scaffold_meets_the_budget",
    ),
    (
        "market list",
        "crates/uze-application/tests/performance.rs::market_list_and_inspect_meet_the_budget_without_the_repository",
    ),
    (
        "market remove",
        "crates/uze-application/tests/performance.rs::removals_meet_the_budget",
    ),
    (
        "market link",
        "crates/uze-application/tests/performance.rs::market_link_and_unlink_meet_the_budget",
    ),
    (
        "market unlink",
        "crates/uze-application/tests/performance.rs::market_link_and_unlink_meet_the_budget",
    ),
    (
        "market inspect",
        "crates/uze-application/tests/performance.rs::market_list_and_inspect_meet_the_budget_without_the_repository",
    ),
    (
        "market host",
        "crates/uze-application/tests/performance.rs::market_host_meets_the_budget",
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

    /// A name in `BUDGETED_COMMAND_TESTS` is a claim that a test exists;
    /// this reads the file the name points into and finds the function, so
    /// a renamed, moved or deleted test fails here by name instead of
    /// leaving the classification pointing at nothing.
    ///
    /// Two spellings, because a budget test sits in one of two places: a
    /// module inside `uze-application` (`uze_application::application::…`)
    /// or a test target of its own, named by its path from the workspace
    /// root. The second is where the ones timing a whole command live —
    /// see the header of `crates/uze-application/tests/performance.rs` for
    /// why they cannot share a binary with the crate's other tests.
    #[test]
    fn every_named_performance_test_exists() {
        let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        for (command, test) in BUDGETED_COMMAND_TESTS {
            let (location, function) = test
                .rsplit_once("::")
                .unwrap_or_else(|| panic!("{command}: {test} names no function"));
            let file = if location.ends_with(".rs") {
                workspace.join(location)
            } else {
                let module = location
                    .strip_prefix("uze_application::application::")
                    .unwrap_or_else(|| {
                        panic!(
                            "{command}: {test} is neither a path ending in `.rs` nor under \
                             uze_application::application"
                        )
                    })
                    .split("::")
                    .next()
                    .expect("a module segment");
                workspace
                    .join("crates/uze-application/src/application")
                    .join(format!("{module}.rs"))
            };
            let source = std::fs::read_to_string(&file).unwrap_or_else(|error| {
                panic!("{command}: cannot read {}: {error}", file.display())
            });
            assert!(
                source.contains(&format!("fn {function}(")),
                "{command}: BUDGETED_COMMAND_TESTS names {test}, but {} has no fn {function}",
                file.display()
            );
        }
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
