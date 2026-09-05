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
        sanctioned: &[
            (
                "src/shim.rs",
                "a separate binary entry point, not presentation: the runtime \
                 shim resolves a harness's real executable and must name the \
                 runtime contract to do it",
            ),
            (
                "src/bin/uze-harness-matrix.rs",
                "tooling, and a binary of its own — it reports on the domain \
                 rather than presenting it to a user",
            ),
        ],
        budget: &[],
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
        budget: &[],
    },
    Rule {
        name: "only the two declared owners spawn Git",
        scope: "crates",
        forbidden: "Command::new(\"git\")",
        reason: "how Git is spawned is a contract — the environment it inherits, and \
                 what a non-zero exit means. Two callers with two conventions is what \
                 `uze-git` replaced, and a repository write lock cannot be complete \
                 while a module spawns Git around it.",
        remedy: "use `uze_git::read` or `uze_git::write`. If you need the hardened \
                 profile for untrusted remote content, that belongs beside the \
                 acquisition one, not in a third place.",
        sanctioned: &[
            (
                "crates/uze-git/src/lib.rs",
                "the transport itself — this is the one place the spawn is defined",
            ),
            (
                "crates/uze-core/src/package/acquisition/git.rs",
                "a deliberately different contract, not a second convention: this clones \
             *untrusted remote* repositories, so it strips the environment \
             (`env_clear`, `GIT_CONFIG_NOSYSTEM`, hooks disabled, no credential \
             prompt) — the opposite of `uze-git`, which drives the operator's own \
             checkout and must let their configuration apply",
            ),
        ],
        budget: &[],
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
        name: "an extension holds no machine access of its own",
        scope: "crates/uze-extensions/src",
        forbidden: "std::process",
        reason: "an extension is code UZE runs in its own process, and the only \
                 answer to \"what can it reach\" that survives someone else \
                 authoring one is: whatever it was handed. Spawning a process is \
                 the reach that makes a sandbox impossible later — a `&mut Frame` \
                 could not cross a process boundary, and neither can a fork().",
        remedy: "ask for it through `uze_extensions::Host`, which the workspace \
                 client implements in `src/ui/extension_host.rs`. Widen that trait \
                 if the capability is genuinely new.",
        sanctioned: &[],
        budget: &[],
    },
    Rule {
        name: "an extension does not read the filesystem behind the host's back",
        scope: "crates/uze-extensions/src",
        forbidden: "std::fs",
        reason: "same argument as spawning: a capability the host did not grant is \
                 one it cannot withhold.",
        remedy: "`Host::read_file`. Test fixtures may write to their own scratch \
                 directory — that is test code, which this scan already excludes.",
        sanctioned: &[],
        budget: &[],
    },
    Rule {
        name: "drawing the workspace reaches nothing",
        scope: "src/ui/orchestrator",
        forbidden: "WorkspaceHost",
        reason: "the render and input halves of the workspace client run on the \
                 thread that owns the frame. Holding the extension host there is \
                 what let a `git status` — several processes, unbounded on a large \
                 repository — run inside the `dirty` branch immediately before \
                 `terminal.draw`, which is a stalled UI by construction rather than \
                 by accident.",
        remedy: "read on a thread and answer through a channel, the way \
                 `spawn_git_read`/`spawn_task_evaluation` already do, and give the \
                 renderer the resolved data. If a view needs something a host \
                 resolves, resolve it where the read happens and store it — \
                 `GitView::display_root` is the worked example.",
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
    Rule {
        name: "only a theme adapter names a colour value",
        scope: "src",
        // The construction, not the name: a pane's own `TerminalColor::Rgb`
        // is a *pattern* being read, and matching on what content already
        // carries is not the same act as writing a colour down.
        forbidden: "Color::Rgb(",
        reason: "appearance is data. A colour written at the point of drawing is a \
                 colour nobody can theme, and the four hand-kept copies of one \
                 palette this replaced are what happens next: a value changed in \
                 one of them is silently wrong in the others.",
        remedy: "name what the thing *is* — `theme::fg(Token::TextMuted)`, \
                 `theme::bg(Token::SurfaceSelected)` — and let \
                 `uze_theme` resolve it. A colour that genuinely came from \
                 content rather than from the design system (a pane's own \
                 output, syntax highlighting an extension ships) goes through \
                 `theme::content`.",
        sanctioned: &[
            (
                "src/ui/theme.rs",
                "an adapter: the one place a token becomes a ratatui colour, \
                 for everything the TUI draws",
            ),
            (
                "src/progress.rs",
                "the CLI's adapter, for the same reason and to the same \
                 tokens — anstyle instead of ratatui. Two adapters, one \
                 vocabulary, which is what keeps `uze status` and the \
                 workspace client from drifting",
            ),
        ],
        budget: &[],
    },
];

/// Every reach for the extension host in the workspace client sits inside
/// a `thread::spawn`.
///
/// The rule above keeps the host out of the render and input halves
/// entirely. This one covers the file those halves are driven from, where
/// the host legitimately appears — but only ever as the thing a
/// background read hands to the extension. A `git status` on an ordinary
/// repository outlasts several frames, so where it runs is not a style
/// question: it is the difference between a client that keeps drawing and
/// one that stops.
///
/// Structural rather than exact: the check is that no mention of the host
/// is reachable without passing a `thread::spawn` first, which is what a
/// reviewer would look for.
#[test]
fn the_workspace_client_reaches_for_git_only_from_a_thread() {
    let path = repository_root().join("src/ui/orchestrator.rs");
    let source = fs::read_to_string(&path).expect("the workspace client");
    let source = strip_test_modules(&source);

    let mut depth: i32 = 0;
    let mut spawn_depth: Option<i32> = None;
    let mut escaped = Vec::new();
    for (number, line) in source.lines().enumerate() {
        let code = line.trim_start();
        let is_comment = code.starts_with("//");
        if !is_comment && code.contains("thread::spawn") && spawn_depth.is_none() {
            spawn_depth = Some(depth);
        }
        if !is_comment
            && line.contains("WorkspaceHost")
            && !line.contains("use crate::ui::extension_host")
            && spawn_depth.is_none()
        {
            escaped.push(format!("  src/ui/orchestrator.rs:{}: {}", number + 1, code));
        }
        depth += (line.matches('{').count() as i32) - (line.matches('}').count() as i32);
        if let Some(opened) = spawn_depth
            && depth <= opened
        {
            spawn_depth = None;
        }
    }

    assert!(
        escaped.is_empty(),
        "\n\nthe extension host is reached outside a background thread:\n\n{}\n\n\
         Every Git read this client makes belongs on a thread of its own, answered \
         through a channel — see `spawn_git_read` and `WorkspaceModel::absorb_git_read`. \
         Reading inline is what made a keystroke wait on `git status`.\n",
        escaped.join("\n")
    );
}

/// No chrome glyph is written where it is drawn.
///
/// The companion to the colour rule, and the same argument: a mark typed
/// into a render function is a mark nobody can change, and it is what made a
/// terminal without a Nerd Font — or an operator who simply wants ASCII —
/// something UZE had no answer for. Every one of these is a
/// `uze_theme::Symbol` now, resolved through `src/ui/theme.rs`.
///
/// Deliberately not the whole set of non-ASCII characters. Arrows, the
/// middot and the ellipsis appear in hint lines as *notation* — "↑↓ select"
/// reads as itself in the source and is translated to the active theme's
/// glyphs by `hint_spans` — so what this scans for is the marks, which have
/// no such reading.
#[test]
fn no_chrome_glyph_is_written_where_it_is_drawn() {
    const MARKS: &[char] = &[
        '\u{2726}', // ✦ sparkle
        '\u{25cf}', // ● filled dot
        '\u{25cb}', // ○ hollow dot
        '\u{25c9}', // ◉ target
        '\u{2713}', // ✓ check
        '\u{2715}', // ✕ close
        '\u{221a}', // √ native
        '\u{2248}', // ≈ adapted
        '\u{26a0}', // ⚠ warning
        '\u{258d}', // ▍ thick bar
        '\u{258e}', // ▎ medium bar
        '\u{258f}', // ▏ thin bar
        '\u{251c}', // ├ tree branch
        '\u{2514}', // └ tree last
        '\u{2500}', // ─ divider
        '\u{2502}', // │ column divider
        '\u{25b8}', // ▸ collapsed
        '\u{25be}', // ▾ expanded
        '\u{276f}', // ❯ prompt
        '\u{2261}', // ≡ menu
        '\u{21c4}', // ⇄ swap
        '\u{21e1}', // ⇡ ahead
        '\u{21e3}', // ⇣ behind
        '\u{2197}', // ↗ external
    ];

    let root = repository_root();
    let mut written = Vec::new();
    for (path, contents) in production_sources(&root.join("src/ui")) {
        let relative = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        // The adapter is where a glyph legitimately becomes a string.
        if relative == "src/ui/theme.rs" {
            continue;
        }
        for (number, line) in contents.lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            for mark in MARKS {
                if code.contains(*mark) {
                    written.push(format!("  {relative}:{}: {mark}", number + 1));
                }
            }
        }
    }

    assert!(
        written.is_empty(),
        "\n\nchrome glyphs written inline:\n\n{}\n\n\
         Name the meaning instead — `theme::glyph(Symbol::MarkOfficial)` — and \
         let the active theme decide what it looks like. Add a `Symbol` if none \
         of the existing ones says what this mark means. A column laid out \
         from the glyph needs `theme::width(..)` too: a theme may have \
         replaced it with a wider one.\n",
        written.join("\n")
    );
}

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
