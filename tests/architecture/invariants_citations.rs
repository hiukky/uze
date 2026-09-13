//! Every citation in `docs/architecture/invariants.md` names a test that exists.
//!
//! AGENTS.md calls that page "the canonical list of 'do not break this'
//! behaviors", each entry "tied to the specific test that proves it". A
//! citation that no longer resolves breaks exactly that tie: the property
//! still reads as guarded, and nothing guards it. Fifty-seven of them had
//! drifted before this scan existed, almost all of them because a refactor
//! moved the file and nothing followed.
//!
//! This is the same discipline the journeys tier already enforces one level
//! up — `journey validate` fails a `proves:` that no longer resolves — and it
//! catches the same class of drift: a page that moved surfaces as a red
//! build, on the change that moved it.
//!
//! What it can check is structural: the file exists and declares a function
//! of that name. It cannot tell you the property drifted away from the test
//! that still bears its name, and nothing can.

use std::{fs, path::PathBuf};

/// A `> `path/to/file.rs::maybe_module::test_name`` line on the page.
#[derive(Debug)]
struct Citation {
    line: usize,
    path: String,
    symbol: String,
}

impl Citation {
    /// The last segment is the function; anything before it is the module
    /// path inside the file, which Rust does not let this scan verify.
    fn function(&self) -> &str {
        self.symbol.rsplit("::").next().unwrap_or(&self.symbol)
    }
}

fn page() -> PathBuf {
    uze_testkit::workspace_root().join("docs/architecture/invariants.md")
}

/// Scans for `` `<path>.rs::<symbol>` `` without a regex crate: the page's
/// citations are always fenced in backticks and always carry `.rs::`.
fn citations(markdown: &str) -> Vec<Citation> {
    let mut found = Vec::new();
    for (index, line) in markdown.lines().enumerate() {
        for fenced in line.split('`').skip(1).step_by(2) {
            let Some((path, symbol)) = fenced.split_once(".rs::") else {
                continue;
            };
            if symbol.is_empty()
                || !symbol
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == ':')
            {
                continue;
            }
            found.push(Citation {
                line: index + 1,
                path: format!("{path}.rs"),
                symbol: symbol.to_owned(),
            });
        }
    }
    found
}

#[test]
fn every_invariant_cites_a_test_that_exists() {
    let page = page();
    let markdown = fs::read_to_string(&page)
        .unwrap_or_else(|error| panic!("could not read {}: {error}", page.display()));
    let citations = citations(&markdown);
    assert!(
        citations.len() > 100,
        "the scan found only {} citations — the page's format changed and this test \
         is no longer reading it",
        citations.len()
    );

    let root = uze_testkit::workspace_root();
    let mut unresolved = Vec::new();
    for citation in &citations {
        let file = root.join(&citation.path);
        let Ok(source) = fs::read_to_string(&file) else {
            unresolved.push(format!(
                "invariants.md:{} cites `{}::{}` — no such file",
                citation.line, citation.path, citation.symbol
            ));
            continue;
        };
        if !declares(&source, citation.function()) {
            unresolved.push(format!(
                "invariants.md:{} cites `{}::{}` — `{}` exists, but declares no `fn {}`",
                citation.line,
                citation.path,
                citation.symbol,
                citation.path,
                citation.function()
            ));
        }
    }

    assert!(
        unresolved.is_empty(),
        "{} of {} citations in docs/architecture/invariants.md no longer resolve. \
         Point each at the test that proves the property now, or delete the entry \
         if nothing does — an invariant nothing proves is not one.\n\n{}",
        unresolved.len(),
        citations.len(),
        unresolved.join("\n")
    );
}

/// Whether `source` declares `fn <name>`, ignoring visibility and `async`.
fn declares(source: &str, name: &str) -> bool {
    source.match_indices("fn ").any(|(at, _)| {
        let rest = &source[at + 3..];
        rest.strip_prefix(name)
            .is_some_and(|tail| !tail.starts_with(|c: char| c.is_alphanumeric() || c == '_'))
    })
}

// The scan's own two halves, proven separately: a citation reader that
// silently stops matching would make the rule above pass on an empty list.

#[test]
fn a_citation_is_read_off_a_quoted_line() {
    let found = citations("> `tests/a/b.rs::outer::the_name`\n");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].path, "tests/a/b.rs");
    assert_eq!(found[0].symbol, "outer::the_name");
    assert_eq!(found[0].function(), "the_name");
}

#[test]
fn ordinary_prose_in_backticks_is_not_a_citation() {
    assert!(citations("the `Store` owns `plugin.json` bytes").is_empty());
}

#[test]
fn a_declaration_is_matched_whole() {
    assert!(declares("    fn the_name() {}", "the_name"));
    assert!(declares("pub async fn the_name(x: u8) {}", "the_name"));
    assert!(!declares("fn the_name_extended() {}", "the_name"));
}
