//! Official default marketplace — a snapshot of this repository's own
//! `marketplace.json` + `plugins/**`, compiled into the binary so a fresh
//! install has something to seed without a network fetch or a running
//! registry.
//!
//! The embedded snapshot represents **a marketplace**, not a plugin: this
//! module extracts the whole snapshot and resolves a plugin name against
//! its `marketplace.json` exactly the way a Git or local marketplace root
//! would (`uze_core::acquisition::marketplace`), so adding a plugin to the
//! marketplace never touches this file. `plugins/uze` is not privileged —
//! it is simply the one entry [`DEFAULT_PLUGIN_IDS`] names as installed by
//! default, which is product policy, not a marketplace fact.
//!
//! Every default plugin goes through the exact same lifecycle a normal
//! `uze add` uses (`UzeApplication::install_materialized`) — Store, Engine,
//! Router and every `IntegrationPort` never learn a plugin's bytes came
//! from the binary rather than disk or Git.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use uze_core::{
    MaterializedPackage, PackageSource, Provenance, ResolvedSource, Result, UzeError,
    acquisition::marketplace,
};

include!(concat!(env!("OUT_DIR"), "/embedded_marketplace.rs"));

/// Ids of the plugins installed by default on a fresh `UZE_HOME`. This is
/// product policy over what the official marketplace *offers*, not the
/// marketplace itself — it names plugins, nothing else. A future
/// marketplace entry the policy doesn't list stays available but
/// uninstalled.
pub const DEFAULT_PLUGIN_IDS: &[&str] = &["uze"];

/// Materializes `plugin_name` from the embedded official marketplace
/// snapshot: extracts the whole snapshot into a fresh scratch directory,
/// reads its `marketplace.json`, and resolves the plugin the same way any
/// marketplace root would. `Err(UnknownPackage)` if the snapshot's manifest
/// does not list `plugin_name`.
pub fn materialize(plugin_name: &str) -> Result<MaterializedPackage> {
    let (root, manifest) = extract_and_parse()?;
    let plugin_root = marketplace::resolve_plugin_source(&manifest, plugin_name, &root)?;

    let provenance = |resolved| Provenance {
        requested: PackageSource::Embedded {
            id: plugin_name.to_owned(),
        },
        resolved,
    };
    let mut materialized = MaterializedPackage::owned(
        root,
        provenance(ResolvedSource::Embedded {
            id: plugin_name.to_owned(),
        }),
    );
    materialized.retarget(
        plugin_root,
        provenance(ResolvedSource::Embedded {
            id: plugin_name.to_owned(),
        }),
    );
    Ok(materialized)
}

/// Whether the embedded marketplace currently carries different content for
/// `plugin_name` than what's installed at `stored_root`. Pure read: the
/// fresh comparison copy lives in a scratch directory cleaned up before
/// this returns. Generic over plugin content — compares the resolved
/// directory tree file-for-file, not a fixed list of known filenames — so
/// it needs no per-plugin knowledge either.
pub fn has_update(plugin_name: &str, stored_root: &Path) -> Result<bool> {
    let current = materialize(plugin_name)?;
    Ok(!trees_match(current.root(), stored_root)?)
}

/// The official embedded marketplace's own declared name (`marketplace.json`'s
/// `name` field) and every plugin entry it lists — a pure, read-only parse of
/// the manifest. This is the one place `uze-application` reads
/// `marketplace.json` structure directly; the Application facade turns this
/// into product-facing read models, and nothing below `uze-core::acquisition`
/// ever sees it.
pub fn entries() -> Result<(String, Vec<marketplace::MarketplacePluginEntry>)> {
    let (root, manifest) = extract_and_parse()?;
    let _ = fs::remove_dir_all(&root);
    Ok((manifest.name, manifest.plugins))
}

/// Extracts a fresh copy of the embedded snapshot and parses its
/// `marketplace.json`. The returned root is the scratch directory the
/// manifest and every plugin subtree live under.
fn extract_and_parse() -> Result<(PathBuf, marketplace::MarketplaceManifest)> {
    let root = extract_embedded_snapshot()?;
    let manifest_path = root.join(uze_core::workspace::MARKETPLACE_MANIFEST_NAME);
    let manifest_bytes = fs::read(&manifest_path).map_err(|source| UzeError::Read {
        path: manifest_path,
        source,
    })?;
    let manifest = marketplace::parse_manifest(&manifest_bytes)?;
    Ok((root, manifest))
}

fn extract_embedded_snapshot() -> Result<PathBuf> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let scratch = std::env::temp_dir().join(format!(
        "uze-embedded-marketplace-{}-{nonce}",
        std::process::id()
    ));
    for (relative, bytes) in EMBEDDED_MARKETPLACE_FILES {
        let destination = scratch.join(relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|source| UzeError::Write {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        fs::write(&destination, bytes).map_err(|source| UzeError::Write {
            path: destination,
            source,
        })?;
    }
    Ok(scratch)
}

/// Whether every file under `a` exists with identical bytes under `b` and
/// vice versa. No knowledge of what the files are — just a directory-tree
/// equality check, so a new plugin file needs no update here.
fn trees_match(a: &Path, b: &Path) -> Result<bool> {
    let a_files = collect_files(a)?;
    let b_files = collect_files(b)?;
    Ok(a_files == b_files)
}

fn collect_files(root: &Path) -> Result<BTreeMap<PathBuf, Vec<u8>>> {
    let mut out = BTreeMap::new();
    collect_files_into(root, root, &mut out)?;
    Ok(out)
}

fn collect_files_into(
    root: &Path,
    current: &Path,
    out: &mut BTreeMap<PathBuf, Vec<u8>>,
) -> Result<()> {
    let entries = fs::read_dir(current).map_err(|source| UzeError::Read {
        path: current.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| UzeError::Read {
            path: current.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_files_into(root, &path, out)?;
        } else {
            let relative = path
                .strip_prefix(root)
                .expect("walked path is under root")
                .to_path_buf();
            let bytes = fs::read(&path).map_err(|source| UzeError::Read {
                path: path.clone(),
                source,
            })?;
            out.insert(relative, bytes);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_official_uze_plugin_resolves_from_the_embedded_snapshot() {
        let materialized = materialize("uze").unwrap();
        assert!(materialized.root().join("plugin.json").is_file());
        assert!(materialized.root().join("skills/init/SKILL.md").is_file());
        assert!(
            materialized
                .root()
                .join("skills/worktree/SKILL.md")
                .is_file()
        );
    }

    #[test]
    fn an_unknown_plugin_name_is_an_error_not_a_silent_empty_result() {
        assert!(materialize("does-not-exist").is_err());
    }

    #[test]
    fn a_fresh_materialization_reports_no_update_against_itself() {
        let materialized = materialize("uze").unwrap();
        assert!(!has_update("uze", materialized.root()).unwrap());
    }

    #[test]
    fn a_stored_copy_with_different_content_reports_an_update() {
        let root = std::env::temp_dir().join(format!(
            "uze-bootstrap-drift-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let materialized = materialize("uze").unwrap();
        copy_tree(materialized.root(), &root);
        fs::write(root.join("plugin.json"), "{}").unwrap();
        assert!(has_update("uze", &root).unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    fn copy_tree(source: &Path, destination: &Path) {
        fs::create_dir_all(destination).unwrap();
        for entry in fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            let target = destination.join(entry.file_name());
            if entry.path().is_dir() {
                copy_tree(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), target).unwrap();
            }
        }
    }
}
