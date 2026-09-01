//! `marketplace.json` — the contract a marketplace root answers "which
//! plugins exist here, and where" with.
//!
//! This module is a pure, deterministic, offline primitive: it reads a
//! manifest and resolves a plugin name against a directory already present
//! on disk. It has no opinion on *how* that directory got there — embedded
//! snapshot, Git checkout, or a local path someone typed are all the same
//! marketplace root to this code, exactly as `PackageSource` keeps the
//! acquisition *mechanism* orthogonal to what gets acquired. Nothing here
//! names a specific plugin; adding one to `marketplace.json` and its
//! directory under the root is the entire integration surface.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{Result, UzeError};

#[derive(Clone, Debug, Deserialize)]
pub struct MarketplaceManifest {
    pub name: String,
    #[serde(default)]
    pub owner: Option<MarketplaceOwner>,
    pub plugins: Vec<MarketplacePluginEntry>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MarketplaceOwner {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MarketplacePluginEntry {
    pub name: String,
    /// A path relative to the marketplace root. Nothing else is supported
    /// yet — a bare Git or registry reference here would need its own
    /// acquisition pass this module deliberately does not perform.
    pub source: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
}

/// Parses and validates a `marketplace.json` payload. Validation is limited
/// to what this module itself must be able to rely on: well-formed JSON
/// matching the shape, and no two plugins claiming the same name. It does
/// not check that any `source` exists — that happens per-lookup in
/// [`resolve_plugin_source`], scoped to the one entry actually requested.
pub fn parse_manifest(bytes: &[u8]) -> Result<MarketplaceManifest> {
    let manifest: MarketplaceManifest =
        serde_json::from_slice(bytes).map_err(|source| UzeError::Json {
            path: PathBuf::from("marketplace.json"),
            source,
        })?;
    let mut seen = std::collections::BTreeSet::new();
    for entry in &manifest.plugins {
        if !seen.insert(entry.name.as_str()) {
            return Err(UzeError::DuplicateMarketplacePlugin(entry.name.clone()));
        }
    }
    Ok(manifest)
}

/// Resolves `plugin_name` against `manifest` to an absolute, canonicalized
/// directory under `marketplace_root`. Rejects a `source` that does not
/// exist and one that escapes `marketplace_root` (a `../` or an absolute
/// path smuggled into the manifest) — the same containment rule package
/// acquisition already holds every source to.
pub fn resolve_plugin_source(
    manifest: &MarketplaceManifest,
    plugin_name: &str,
    marketplace_root: &Path,
) -> Result<PathBuf> {
    let entry = manifest
        .plugins
        .iter()
        .find(|entry| entry.name == plugin_name)
        .ok_or_else(|| UzeError::UnknownPackage(plugin_name.to_owned()))?;
    let canonical_root = marketplace_root
        .canonicalize()
        .map_err(|source| UzeError::Read {
            path: marketplace_root.to_path_buf(),
            source,
        })?;
    let joined = canonical_root.join(&entry.source);
    let canonical_source = joined
        .canonicalize()
        .map_err(|_| UzeError::MissingPath(joined.clone()))?;
    if !canonical_source.starts_with(&canonical_root) {
        return Err(UzeError::UnsafePathReference {
            path: marketplace_root.to_path_buf(),
            reference: entry.source.clone(),
        });
    }
    Ok(canonical_source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_manifest(root: &Path, plugins_json: &str) {
        fs::write(
            root.join("marketplace.json"),
            format!(r#"{{"name":"test","plugins":[{plugins_json}]}}"#),
        )
        .unwrap();
    }

    fn plugin_dir(root: &Path, relative: &str) -> PathBuf {
        let dir = root.join(relative);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("plugin.json"), r#"{"name":"x"}"#).unwrap();
        dir
    }

    #[test]
    fn a_well_formed_manifest_parses() {
        let manifest = parse_manifest(
            br#"{"name":"uze","plugins":[{"name":"uze","source":"./plugins/uze"}]}"#,
        )
        .unwrap();
        assert_eq!(manifest.name, "uze");
        assert_eq!(manifest.plugins.len(), 1);
    }

    #[test]
    fn a_malformed_manifest_is_rejected() {
        assert!(parse_manifest(b"not json").is_err());
        assert!(
            parse_manifest(br#"{"name":"uze"}"#).is_err(),
            "missing plugins field"
        );
    }

    #[test]
    fn a_duplicate_plugin_name_is_rejected() {
        let error = parse_manifest(
            br#"{"name":"uze","plugins":[
                {"name":"foo","source":"./a"},
                {"name":"foo","source":"./b"}
            ]}"#,
        )
        .unwrap_err();
        assert!(matches!(error, UzeError::DuplicateMarketplacePlugin(name) if name == "foo"));
    }

    #[test]
    fn resolves_relative_source_to_an_absolute_canonical_path() {
        let root = uze_testkit::temp::scratch("resolve");
        fs::create_dir_all(&root).unwrap();
        let dir = plugin_dir(&root, "plugins/uze");
        write_manifest(&root, r#"{"name":"uze","source":"./plugins/uze"}"#);
        let manifest = parse_manifest(&fs::read(root.join("marketplace.json")).unwrap()).unwrap();

        let resolved = resolve_plugin_source(&manifest, "uze", &root).unwrap();
        assert_eq!(resolved, dir.canonicalize().unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn two_distinct_plugins_resolve_independently_with_no_special_casing() {
        let root = uze_testkit::temp::scratch("two-plugins");
        fs::create_dir_all(&root).unwrap();
        let first = plugin_dir(&root, "plugins/uze");
        let second = plugin_dir(&root, "plugins/rust-guidelines");
        write_manifest(
            &root,
            r#"{"name":"uze","source":"./plugins/uze"},{"name":"rust-guidelines","source":"./plugins/rust-guidelines"}"#,
        );
        let manifest = parse_manifest(&fs::read(root.join("marketplace.json")).unwrap()).unwrap();

        assert_eq!(
            resolve_plugin_source(&manifest, "uze", &root).unwrap(),
            first.canonicalize().unwrap()
        );
        assert_eq!(
            resolve_plugin_source(&manifest, "rust-guidelines", &root).unwrap(),
            second.canonicalize().unwrap()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_unknown_plugin_name_is_rejected() {
        let root = uze_testkit::temp::scratch("unknown");
        fs::create_dir_all(&root).unwrap();
        plugin_dir(&root, "plugins/uze");
        write_manifest(&root, r#"{"name":"uze","source":"./plugins/uze"}"#);
        let manifest = parse_manifest(&fs::read(root.join("marketplace.json")).unwrap()).unwrap();

        assert!(matches!(
            resolve_plugin_source(&manifest, "does-not-exist", &root),
            Err(UzeError::UnknownPackage(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_missing_source_directory_is_rejected() {
        let root = uze_testkit::temp::scratch("missing-source");
        fs::create_dir_all(&root).unwrap();
        write_manifest(&root, r#"{"name":"uze","source":"./plugins/uze"}"#);
        let manifest = parse_manifest(&fs::read(root.join("marketplace.json")).unwrap()).unwrap();

        assert!(matches!(
            resolve_plugin_source(&manifest, "uze", &root),
            Err(UzeError::MissingPath(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_source_escaping_the_marketplace_root_is_rejected() {
        let root = uze_testkit::temp::scratch("escape");
        fs::create_dir_all(root.join("marketplace")).unwrap();
        plugin_dir(&root, "outside");
        write_manifest(
            &root.join("marketplace"),
            r#"{"name":"evil","source":"../outside"}"#,
        );
        let manifest =
            parse_manifest(&fs::read(root.join("marketplace").join("marketplace.json")).unwrap())
                .unwrap();

        assert!(matches!(
            resolve_plugin_source(&manifest, "evil", &root.join("marketplace")),
            Err(UzeError::UnsafePathReference { .. })
        ));
        fs::remove_dir_all(root).unwrap();
    }
}
