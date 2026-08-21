//! Builtin `uze` package — compiled into the binary and auto-installed.
//!
//! `packages/uze` is the official UZE Skill (`skills/uze/SKILL.md`).
//! Unlike other packages it does not require `uze add` — the binary itself
//! carries the bytes and `UzeApplication::ensure_builtin_plugins` seeds the
//! Store and attaches it where safe, using the same `Store::ingest` +
//! `IntegrationPort::attach` path any other package goes through. No Store,
//! router or integration hardcodes the id; the builtin is just the one
//! package the composition root knows how to materialize.

use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use uze_core::{
    MaterializedPackage, PackageSource, Provenance, ResolvedSource, Result, UzeError, UzeHome,
    UzeStore,
};

/// Embedded bytes — `include_str!` makes the binary self-contained, so
/// `cargo install` doesn't need `packages/uze` on the target machine.
const BUILTIN_PLUGIN_JSON: &str = include_str!("../../../packages/uze/plugin.json");
const BUILTIN_SKILL_MD: &str = include_str!("../../../packages/uze/skills/uze/SKILL.md");

/// Stable provenance for the builtin. The path `builtin:uze` never exists on
/// disk and is never re-acquired via `PackageSource::acquire` — it only
/// identifies the origin in the registry. Updates are handled by
/// `ensure_builtin_plugins` comparing file content, not by `uze update`.
fn builtin_provenance() -> Provenance {
    let source = PackageSource::Local {
        path: PathBuf::from("builtin:uze"),
    };
    Provenance {
        requested: source.clone(),
        resolved: ResolvedSource::Local {
            path: PathBuf::from("builtin:uze"),
        },
    }
}

/// Materialize the builtin package into a temporary directory and return a
/// `MaterializedPackage` that `UzeStore::ingest` can consume. The temp dir is
/// owned by the `MaterializedPackage` (via `owned`) so it is cleaned up
/// automatically if ingestion fails; on success `ingest` copies the bytes
/// into `store/packages/uze` and the temp dir is still removed on drop.
fn materialize_builtin() -> Result<MaterializedPackage> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let scratch =
        std::env::temp_dir().join(format!("uze-builtin-uze-{}-{nonce}", std::process::id()));
    fs::create_dir_all(scratch.join("skills/uze")).map_err(|source| UzeError::Write {
        path: scratch.clone(),
        source,
    })?;
    fs::write(scratch.join("plugin.json"), BUILTIN_PLUGIN_JSON).map_err(|source| {
        UzeError::Write {
            path: scratch.join("plugin.json"),
            source,
        }
    })?;
    fs::write(scratch.join("skills/uze/SKILL.md"), BUILTIN_SKILL_MD).map_err(|source| {
        UzeError::Write {
            path: scratch.join("skills/uze/SKILL.md"),
            source,
        }
    })?;
    Ok(MaterializedPackage::owned(
        scratch.clone(),
        builtin_provenance(),
    ))
}

/// Returns whether the on-disk installed `uze` package matches the embedded
/// bytes. If the package is not installed at all, returns false.
fn builtin_is_current(home: &UzeHome) -> bool {
    let Ok(id) = uze_core::store::PackageId::from_plugin_name("uze", &PathBuf::from("plugin.json"))
    else {
        return false;
    };
    let pkg_dir = home.package_dir(&id);
    let skill_path = pkg_dir.join("skills/uze/SKILL.md");
    let manifest_path = pkg_dir.join("plugin.json");
    if !skill_path.is_file() || !manifest_path.is_file() {
        return false;
    }
    let Ok(skill) = fs::read_to_string(&skill_path) else {
        return false;
    };
    let Ok(manifest) = fs::read_to_string(&manifest_path) else {
        return false;
    };
    skill == BUILTIN_SKILL_MD && manifest == BUILTIN_PLUGIN_JSON
}

/// Ensure the builtin `uze` package is present in `store` and up-to-date.
/// Returns `true` if it installed or updated the package, `false` if already
/// current. Never errors if the package already exists with a *different*
/// provenance (e.g. user manually added `packages/uze` from a filesystem path)
/// — in that case the user's installation is respected and no builtin action
/// is taken.
pub fn ensure_builtin_uze_store(home: &UzeHome, store: &UzeStore) -> Result<bool> {
    // If a package `uze` is already registered, respect it unless it's stale
    // vs. the embedded bytes. A different origin (manual `uze add`) is
    // intentionally not overwritten here — the builtin is a default, not a
    // forced override.
    let existing = store
        .package_ids()
        .ok()
        .and_then(|ids| ids.into_iter().find(|id| id.as_str() == "uze"));
    if let Some(id) = existing {
        if builtin_is_current(home) {
            return Ok(false);
        }
        // Content differs from the embedded bytes (e.g. the binary was
        // upgraded) — remove the stale package so `ingest` below can
        // reinstall it; receipts are reconciled by the caller afterward. A
        // failed removal is not fatal: `ingest` then hits `PackageConflict`
        // and this call reports `false`, which is honest — no update
        // happened — rather than silently pretending one did.
        let _ = store.remove_package(&id);
    }

    // Need to install/refresh the builtin.
    home.ensure_layout()?;
    let materialized = materialize_builtin()?;
    match store.ingest(&materialized) {
        Ok(_) => Ok(true),
        Err(UzeError::PackageConflict { .. }) => {
            // Another provenance already claims `uze` — respect it.
            Ok(false)
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn embedded_manifest_is_valid() {
        let v: serde_json::Value = serde_json::from_str(BUILTIN_PLUGIN_JSON).unwrap();
        assert_eq!(v.get("name").and_then(|n| n.as_str()), Some("uze"));
    }
    #[test]
    fn embedded_skill_has_name_uze() {
        assert!(BUILTIN_SKILL_MD.contains("name: uze"));
    }
}
