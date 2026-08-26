//! Generates a static table embedding the official UZE marketplace snapshot
//! (`marketplace.json` + everything under `plugins/`) into the binary.
//!
//! Deliberately generic: this walks whatever files exist at build time and
//! emits one `include_bytes!` per file, keyed by its path relative to the
//! repository root. Adding a plugin to the marketplace means adding files
//! and a `marketplace.json` entry — nothing here names a specific plugin,
//! so nothing here needs to change.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

fn main() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("crates/uze-application has two parent directories under the repo root")
        .to_path_buf();

    let mut entries = Vec::new();
    let marketplace_manifest = repo_root.join("marketplace.json");
    collect_file(&repo_root, &marketplace_manifest, &mut entries);
    collect_dir(&repo_root, &repo_root.join("plugins"), &mut entries);
    entries.sort();

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR is set by cargo"));
    let generated_path = out_dir.join("embedded_marketplace.rs");
    let mut generated = fs::File::create(&generated_path).expect("create generated file");
    writeln!(
        generated,
        "pub static EMBEDDED_MARKETPLACE_FILES: &[(&str, &[u8])] = &["
    )
    .unwrap();
    for (relative, absolute) in &entries {
        writeln!(
            generated,
            "    ({relative:?}, include_bytes!({absolute:?})),",
            relative = relative,
            absolute = absolute.display().to_string()
        )
        .unwrap();
    }
    writeln!(generated, "];").unwrap();

    println!("cargo:rerun-if-changed={}", marketplace_manifest.display());
    println!(
        "cargo:rerun-if-changed={}",
        repo_root.join("plugins").display()
    );
}

/// Records one file, `relative` to `repo_root`, as `(relative, absolute)`.
fn collect_file(repo_root: &Path, absolute: &Path, out: &mut Vec<(String, PathBuf)>) {
    let relative = absolute
        .strip_prefix(repo_root)
        .expect("collected path is under the repo root")
        .to_string_lossy()
        .replace('\\', "/");
    out.push((relative, absolute.to_path_buf()));
}

fn collect_dir(repo_root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let entry = entry.expect("readable directory entry");
        let path = entry.path();
        if path.is_dir() {
            collect_dir(repo_root, &path, out);
        } else {
            collect_file(repo_root, &path, out);
        }
    }
}
