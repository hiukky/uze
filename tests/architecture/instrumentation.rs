//! Every application entry point is a span.
//!
//! A command or a key lands on one of `UzeApplication`'s services; if
//! every public method there opens a span, every action has at least one
//! span named for what the person asked, and the mechanisms below it hang
//! under something a reader recognises. Held here rather than remembered:
//! an entry point added without `#[tracing::instrument]` fails by name.

use std::{fs, path::PathBuf};

use crate::layering::strip_test_modules;

/// The service types whose `impl` blocks are scanned. The facade's own
/// constructors and accessors (`impl UzeApplication` in `services.rs`)
/// are not entry points and are not held to this.
const SERVICES: &[&str] = &[
    "Plugins",
    "Marketplace",
    "Health",
    "Context",
    "Project",
    "Profiles",
    "Themes",
    "Workspace",
    "Hooks",
];

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn collect_rust_files(directory: &std::path::Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
}

/// `(file:line, function)` for every `pub fn` inside a service `impl`
/// block that no `#[tracing::instrument]` precedes.
fn uninstrumented_entry_points(path: &std::path::Path, source: &str) -> Vec<String> {
    let mut missing = Vec::new();
    let mut depth: i32 = 0;
    let mut in_service = false;
    let lines: Vec<&str> = source.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        if depth == 0
            && let Some(rest) = line.strip_prefix("impl ")
        {
            let type_name: String = rest
                .chars()
                .take_while(|character| character.is_alphanumeric() || *character == '_')
                .collect();
            in_service = SERVICES.contains(&type_name.as_str());
        }
        if in_service && depth == 1 && line.starts_with("    pub fn ") {
            let instrumented = lines[..index]
                .iter()
                .rev()
                .take_while(|previous| {
                    let code = previous.trim();
                    code.starts_with("#[")
                        || code.starts_with("///")
                        || code.starts_with(")]")
                        || (previous.starts_with("        ") && !code.ends_with('}'))
                })
                .any(|previous| previous.contains("tracing::instrument"));
            if !instrumented {
                let function = line
                    .trim_start()
                    .trim_start_matches("pub fn ")
                    .split('(')
                    .next()
                    .unwrap_or_default();
                missing.push(format!("  {}:{}: {function}", path.display(), index + 1));
            }
        }
        depth += line.matches('{').count() as i32 - line.matches('}').count() as i32;
        if depth == 0 {
            in_service = false;
        }
    }
    missing
}

#[test]
fn every_application_entry_point_is_a_span() {
    let root = repository_root().join("crates/uze-application/src/application");
    let mut files = Vec::new();
    collect_rust_files(&root, &mut files);
    files.sort();
    let mut missing = Vec::new();
    for path in files {
        let source = fs::read_to_string(&path).expect("application source");
        let relative = path.strip_prefix(repository_root()).unwrap_or(&path);
        missing.extend(uninstrumented_entry_points(
            relative,
            &strip_test_modules(&source),
        ));
    }
    assert!(
        missing.is_empty(),
        "every public method of an application service opens a span, so a command or a \
         key has a trace named for what the person asked. Add \
         `#[tracing::instrument(name = \"<service>.<method>\", skip_all, err)]` (with \
         `fields(...)` naming what it acts on) to:\n{}",
        missing.join("\n")
    );
}
