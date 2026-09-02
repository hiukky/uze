//! Which crate may name which.

use std::{fs, path::PathBuf};

/// One structural rule: a directory that must not name something.
struct Rule {
    /// Printed first on failure — the rule in one line.
    name: &'static str,
    /// Directory scanned, relative to the repository root.
    scope: &'static str,
    /// The token that must not appear in production source.
    forbidden: &'static str,
    /// Printed on failure. Why this rule exists, in terms of what breaks
    /// without it — not a restatement of the rule.
    reason: &'static str,
    /// What to do instead. A rule an agent cannot act on is a rule that
    /// gets worked around.
    remedy: &'static str,
    /// Permanently allowed, with the architectural reason. Never debt:
    /// nothing here should ever be removed to make a number go down.
    sanctioned: &'static [(&'static str, &'static str)],
    /// Debt, frozen at today's count. Exact, not a ceiling: removing a
    /// violation fails until the number is lowered, which puts every
    /// improvement in a diff and stops a budget from over-permitting.
    budget: &'static [(&'static str, usize)],
}

const RULES: &[Rule] = &[
    Rule {
        name: "the CLI and TUI do not reach past the application facade",
        scope: "src",
        forbidden: "uze_core::",
        reason: "presentation consumes read models from uze-application. Naming the \
                 domain directly makes every domain change ripple into the frontend, \
                 and leaves no single surface that could ever be exposed to anything \
                 else — an out-of-process client, a second frontend, an extension.",
        remedy: "add what you need to uze-application (a read model, or a method on \
                 the facade) and call that. If it is genuinely architecture rather \
                 than debt, move the file to `sanctioned` with the reason.",
        // Known blind spot: `src/lib.rs` re-exports `uze_core::*`, so a
        // reach written as `crate::UzeHome` resolves to the domain without
        // naming it here. That facade is itself budgeted above, and its own
        // doc calls it transitional — deleting it is what closes the hole,
        // which is the same work this rule exists to drive.
        sanctioned: &[],
        budget: &[
            // Documented transitional re-export: this crate exists to
            // "preserve the established public imports while application,
            // integrations, and presentation complete their own
            // extraction" (see its module doc). Debt with a stated end,
            // not architecture.
            ("src/lib.rs", 2),
            ("src/main.rs", 1),
            ("src/ui/agent_support.rs", 1),
            ("src/ui/model.rs", 3),
            ("src/ui/orchestrator.rs", 5),
            ("src/ui/orchestrator/render.rs", 2),
            ("src/ui/view/profiles.rs", 1),
            ("src/ui/worker.rs", 4),
        ],
    },
    Rule {
        name: "only the composition root's own consumers name the integrations crate",
        scope: "src",
        forbidden: "uze_integrations",
        reason: "vendor knowledge lives in uze-integrations, reachable through the \
                 registry. The runtime shim and the harness matrix consume that \
                 registry by design; presentation reaching for it directly is how a \
                 hard-coded vendor list starts.",
        remedy: "take the descriptor you need from uze-application, which already \
                 resolves the registry once.",
        sanctioned: &[
            (
                "src/shim.rs",
                "the runtime shim is named in AGENTS.md as a registry consumer: it \
                 resolves the real executable behind a shimmed harness name",
            ),
            (
                "src/bin/uze-harness-matrix.rs",
                "tooling, likewise named in AGENTS.md as a registry consumer",
            ),
        ],
        budget: &[("src/lib.rs", 1), ("src/ui/orchestrator.rs", 1)],
    },
    Rule {
        name: "an extension never touches UZE's own state",
        scope: "crates/uze-extensions/src",
        forbidden: "uze_application",
        reason: "an extension is code UZE runs in its own process (ADR: extension \
                 code is a distinct trust class from plugin bytes). Keeping it a \
                 pure function of what it is handed — no UzeHome, no Store, no \
                 receipts — is what makes it safe to render and what keeps a \
                 capability model tractable if extensions are ever authored \
                 elsewhere.",
        remedy: "the host resolves state and hands the extension the data it needs. \
                 A transport crate (speaking to a foreign binary) is not UZE state \
                 and is allowed.",
        sanctioned: &[],
        budget: &[],
    },
    Rule {
        name: "an extension never names the domain crate",
        scope: "crates/uze-extensions/src",
        forbidden: "uze_core",
        reason: "same trust argument as above, and the same layering one: an \
                 extension that knows the domain cannot be rendered by anything but \
                 this binary.",
        remedy: "take it as data through the extension's own input, or reach for the \
                 Git transport crate, which carries no domain.",
        sanctioned: &[],
        budget: &[],
    },
];

#[test]
fn architecture_rules_hold() {
    let root = repository_root();
    let mut failures = Vec::new();

    for rule in RULES {
        let scope = root.join(rule.scope);
        assert!(
            scope.is_dir(),
            "rule `{}` scans {}, which does not exist",
            rule.name,
            scope.display()
        );
        let sources = production_sources(&scope);
        assert!(
            !sources.is_empty(),
            "rule `{}` found no production source under {}",
            rule.name,
            scope.display()
        );

        let mut seen: Vec<&str> = Vec::new();
        for (path, contents) in &sources {
            let relative = path
                .strip_prefix(&root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");
            if rule
                .sanctioned
                .iter()
                .any(|(allowed, _)| *allowed == relative)
            {
                continue;
            }

            let found = occurrences(contents, rule.forbidden);
            let budget = rule
                .budget
                .iter()
                .find(|(file, _)| *file == relative)
                .map(|(_, count)| *count)
                .unwrap_or(0);

            if found > 0 {
                seen.push(
                    rule.budget
                        .iter()
                        .find(|(file, _)| *file == relative)
                        .map(|(file, _)| *file)
                        .unwrap_or_default(),
                );
            }

            if found > budget {
                failures.push(describe(
                    rule,
                    &relative,
                    &format!(
                        "names `{}` {found} time(s); the budget for this file is {budget}",
                        rule.forbidden
                    ),
                ));
            } else if found < budget {
                failures.push(describe(
                    rule,
                    &relative,
                    &format!(
                        "names `{}` {found} time(s), below its budget of {budget} — lower \
                         the budget to {found}. The number only ever goes down, so that \
                         progress shows up in the diff",
                        rule.forbidden
                    ),
                ));
            }
        }

        for (file, _) in rule.budget {
            if !seen.contains(file) {
                failures.push(describe(
                    rule,
                    file,
                    "carries a budget but no longer violates the rule (or no longer \
                     exists) — delete its budget entry",
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "\n\n{}\n",
        failures.join("\n\n----------------------------------------\n\n")
    );
}

fn describe(rule: &Rule, file: &str, problem: &str) -> String {
    format!(
        "architecture rule violated: {}\n\n  {file} {problem}.\n\n  Why: {}\n\n  Fix: {}",
        rule.name, rule.reason, rule.remedy
    )
}

/// Occurrences of `needle` outside line comments. Test modules are already
/// gone by the time this runs (see [`production_sources`]).
fn occurrences(contents: &str, needle: &str) -> usize {
    contents
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .map(|line| line.matches(needle).count())
        .sum()
}

/// Every `.rs` file under `scope` that is compiled into a release build,
/// with inline `#[cfg(test)] mod … { … }` blocks removed.
///
/// A fixture legitimately builds domain values; forcing it through the
/// facade would be worse code rather than better architecture. So a file
/// some other file declares under `#[cfg(test)]` is skipped outright, and
/// inline test modules are stripped from the ones that remain.
fn production_sources(scope: &std::path::Path) -> Vec<(PathBuf, String)> {
    let mut files = Vec::new();
    collect_rust_files(scope, &mut files);
    files.sort();

    let test_only: Vec<PathBuf> = files
        .iter()
        .flat_map(|path| test_module_declarations(path))
        .collect();

    files
        .into_iter()
        .filter(|path| !test_only.contains(path))
        .map(|path| {
            let contents = fs::read_to_string(&path).unwrap_or_default();
            (path, strip_test_modules(&contents))
        })
        .collect()
}

/// The files `path` declares as `#[cfg(test)] mod <name>;` — resolved
/// against both module layouts (`foo/bar.rs` beside `foo.rs`, and
/// `bar.rs` beside `mod.rs`).
fn test_module_declarations(path: &std::path::Path) -> Vec<PathBuf> {
    let Ok(contents) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let lines: Vec<&str> = contents.lines().collect();
    let mut declared = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if line.trim() != "#[cfg(test)]" {
            continue;
        }
        let Some(next) = lines.get(index + 1) else {
            continue;
        };
        let trimmed = next.trim();
        let Some(name) = trimmed
            .strip_prefix("mod ")
            .and_then(|rest| rest.strip_suffix(';'))
        else {
            continue;
        };
        let Some(directory) = path.parent() else {
            continue;
        };
        let file = format!("{}.rs", name.trim());
        declared.push(directory.join(&file));
        if let Some(stem) = path.file_stem() {
            declared.push(directory.join(stem).join(&file));
        }
    }
    declared
}

fn strip_test_modules(contents: &str) -> String {
    let mut out = Vec::new();
    let mut lines = contents.lines().peekable();
    while let Some(line) = lines.next() {
        if line.trim() == "#[cfg(test)]"
            && let Some(next) = lines.peek()
            && next.trim_start().starts_with("mod ")
            && next.contains('{')
        {
            let opener = lines.next().unwrap_or_default();
            let mut depth = opener.matches('{').count() as i32 - opener.matches('}').count() as i32;
            while depth > 0
                && let Some(body) = lines.next()
            {
                depth += body.matches('{').count() as i32 - body.matches('}').count() as i32;
            }
            continue;
        }
        out.push(line);
    }
    out.join("\n")
}

fn collect_rust_files(directory: &std::path::Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
}

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
