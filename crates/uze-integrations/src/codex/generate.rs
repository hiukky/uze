//! Codex's GENERATED native plugin envelope: for a canonical UZE package
//! that ships no explicit `.codex-plugin/plugin.json`, this module
//! deterministically synthesizes one into a UZE-owned derived directory —
//! never into the Store — so the package can still install as one native
//! Codex plugin instead of decomposing into per-capability shims.
//!
//! Generated Native Package sits between Explicit Native Package and Native
//! Capability in the delivery hierarchy (ADR-020/ADR-021, refining ADR-013
//! §2), exactly as it does for Claude
//! (`crate::claude::generate`) — this module mirrors that one's shape and
//! discipline, adapted to Codex's own manifest format: `skills` names one
//! directory (not an inline list), and `mcpServers` names one external file
//! rather than embedding servers inline.

use std::{collections::BTreeSet, fs, path::Path, path::PathBuf};

use uze_core::{
    Result, UzeError,
    home::UzeHome,
    integration::{AttachmentReceipt, ManagedArtifact},
    project::Resource,
    store::StoredPackage,
};

/// The second, UZE-owned marketplace this module publishes into —
/// deliberately distinct from `MARKETPLACE_NAME` ("uze-local", reserved for
/// explicit envelopes and never touched here) so a generated envelope can
/// never be confused with, or silently override, an author-provided one.
pub(super) const GENERATED_MARKETPLACE_NAME: &str = "uze-local-generated";

/// The `kind` this module stamps on its own receipts. Distinct from
/// `marketplace-plugin` (the explicit-envelope kind) so a receipt's own
/// shape already announces which lifecycle owns it before
/// `detail["origin"]` is even read.
pub(super) const GENERATED_PLUGIN_KIND: &str = "marketplace-plugin-generated";

/// Root of every package's generated envelope directory, AND the catalogue
/// root Codex is pointed at for the generated marketplace. Codex resolves a
/// catalogue entry's `source.path` relative to the marketplace root and
/// rejects both absolute and escaping relative paths (confirmed empirically
/// against Codex 0.148.0, see `CodexIntegration::catalogue_root`'s doc
/// comment) — that constraint is why generated package directories live
/// directly under this root rather than under a Store-relative path. Lives
/// under `$UZE_HOME/state/attachments/codex/generated/`, the same
/// convention Claude's generated envelope uses, never under the Store.
pub(super) fn generated_root(uze_home: &UzeHome) -> PathBuf {
    uze_home
        .state_dir()
        .join("attachments")
        .join("codex")
        .join("generated")
}

fn generated_package_dir_for_id(uze_home: &UzeHome, package_id: &str) -> PathBuf {
    generated_root(uze_home).join(package_id)
}

/// Whether this package has anything UZE can safely represent as a
/// generated native envelope: no explicit envelope of its own, and at
/// least one of the two structural surfaces UZE synthesizes from (a
/// conventional `skills/` directory, or a root `mcp.json`) — identical
/// eligibility rule to Claude's, applied to Codex's own explicit-envelope
/// marker file.
pub(super) fn generatable(package: &StoredPackage) -> bool {
    !package.root.join(".codex-plugin/plugin.json").is_file()
        && (package.root.join("skills").is_dir() || package.root.join("mcp.json").is_file())
}

/// The intersection ADR-013 §2 requires (`provided = discovered ∩
/// declared`), computed against the STRUCTURAL surface a generated manifest
/// declares — not by re-parsing a manifest this same module just wrote, so
/// generation and coverage agree by construction. Identical rule to
/// Claude's `generated_exact_coverage`: a Skill is covered iff it lives
/// under the package's conventional `skills/` directory; an MCP server is
/// covered iff its name appears in the package's own `mcp.json`.
pub(super) fn generated_exact_coverage(
    package: &StoredPackage,
    resources: &[&Resource],
) -> BTreeSet<String> {
    let declared_mcp: BTreeSet<String> = fs::read(package.root.join("mcp.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|value| {
            value
                .get("mcpServers")
                .and_then(serde_json::Value::as_object)
                .map(|servers| servers.keys().cloned().collect())
        })
        .unwrap_or_default();

    let mut provided = BTreeSet::new();
    for resource in resources {
        match resource.capability.kind {
            uze_core::capability::CapabilityKind::AgentSkill => {
                let Some(relative) = resource.capability.path.strip_prefix(&package.root).ok()
                else {
                    continue;
                };
                let Some(parent) = relative.parent() else {
                    continue;
                };
                if parent.starts_with("skills") {
                    provided.insert(resource.identity());
                }
            }
            uze_core::capability::CapabilityKind::Mcp => {
                if let Some(name) = &resource.resource_name
                    && declared_mcp.contains(name)
                {
                    provided.insert(resource.identity());
                }
            }
            _ => {}
        }
    }
    provided
}

/// The generated `.codex-plugin/plugin.json` document. Name/version/
/// description come from the package's own canonical `plugin.json`
/// (`package.manifest`), never invented. `skills` is declared as the fixed
/// `"./skills/"` convention (mirroring the real explicit-envelope fixture
/// shape, `e2e/fixtures/plugin-first-conformance/.codex-plugin/plugin.json`);
/// `mcpServers` names the generated `.mcp.json` sibling file, written only
/// when the package actually has a root `mcp.json` to project.
fn generated_manifest_document(package: &StoredPackage) -> serde_json::Value {
    let (description, version) = read_name_fields(package);

    let mut document = serde_json::json!({
        "name": package.id.as_str(),
        "version": version,
        "description": description,
    });

    if package.root.join("skills").is_dir() {
        document["skills"] = serde_json::json!("./skills/");
    }
    if package.root.join("mcp.json").is_file() {
        document["mcpServers"] = serde_json::json!("./.mcp.json");
    }

    document
}

fn read_name_fields(package: &StoredPackage) -> (String, String) {
    fs::read(&package.manifest)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .map(|value| {
            let description = value
                .get("description")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("UZE-managed Codex plugin, generated from a vendor-neutral package.")
                .to_owned();
            let version = value
                .get("version")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("0.1.0")
                .to_owned();
            (description, version)
        })
        .unwrap_or_else(|| {
            (
                "UZE-managed Codex plugin, generated from a vendor-neutral package.".to_owned(),
                "0.1.0".to_owned(),
            )
        })
}

/// Materializes (or refreshes) one package's generated envelope directory.
/// Idempotent and deterministic: recreated wholesale from the Store package
/// on every call, never incrementally patched — the directory is entirely
/// UZE-owned and non-authoritative (ADR-013 §4). `skills/` and `.mcp.json`
/// are symlinked to the Store's own bytes, never copied, so they can never
/// drift from the Store and a `.mcp.json` never duplicates content.
pub(super) fn materialize_generated_package(
    uze_home: &UzeHome,
    package: &StoredPackage,
) -> Result<PathBuf> {
    let dir = generated_package_dir_for_id(uze_home, package.id.as_str());
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|source| UzeError::Write {
            path: dir.clone(),
            source,
        })?;
    }
    let plugin_dir = dir.join(".codex-plugin");
    fs::create_dir_all(&plugin_dir).map_err(|source| UzeError::Write {
        path: plugin_dir.clone(),
        source,
    })?;
    let manifest = generated_manifest_document(package);
    fs::write(
        plugin_dir.join("plugin.json"),
        serde_json::to_vec_pretty(&manifest).expect("generated manifest is serializable"),
    )
    .map_err(|source| UzeError::Write {
        path: plugin_dir.join("plugin.json"),
        source,
    })?;

    let skills_source = package.root.join("skills");
    if skills_source.is_dir() {
        symlink(&skills_source, &dir.join("skills"))?;
    }
    let mcp_source = package.root.join("mcp.json");
    if mcp_source.is_file() {
        symlink(&mcp_source, &dir.join(".mcp.json"))?;
    }
    Ok(dir)
}

/// Removes one package's generated envelope directory by id alone — used at
/// detach time, when only the receipt's `package_id` (not a full
/// `StoredPackage`) is available. Safe unconditionally: this directory is
/// never anything but a Derived Artifact (ADR-013 §4).
pub(super) fn remove_generated_package_by_id(uze_home: &UzeHome, package_id: &str) -> Result<()> {
    let dir = generated_package_dir_for_id(uze_home, package_id);
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|source| UzeError::Write { path: dir, source })?;
    }
    Ok(())
}

/// Packages eligible for the generated marketplace's catalogue: no explicit
/// envelope of their own, and something structurally safe to generate from.
/// Mirrors `plugin::publishable`'s role for the explicit marketplace.
fn generated_publishable(packages: &[StoredPackage]) -> Vec<&StoredPackage> {
    packages
        .iter()
        .filter(|package| generatable(package))
        .collect()
}

/// The catalogue document, in the same shape as `plugin::catalogue_document`
/// (Codex requires `policy`/`category` on every entry), but with
/// `source.path` relative to the generated marketplace's own root — each
/// generated package directory lives directly under it, exactly like
/// Claude's `"./<id>"` convention.
fn generated_catalogue_document(packages: &[StoredPackage]) -> serde_json::Value {
    let plugins: Vec<serde_json::Value> = generated_publishable(packages)
        .into_iter()
        .map(|package| {
            serde_json::json!({
                "name": package.id.as_str(),
                "source": { "source": "local", "path": format!("./{}", package.id.as_str()) },
                "policy": { "installation": "AVAILABLE", "authentication": "ON_INSTALL" },
                "category": "Developer tools"
            })
        })
        .collect();
    serde_json::json!({
        "name": GENERATED_MARKETPLACE_NAME,
        "interface": { "displayName": "UZE Local (generated)" },
        "plugins": plugins,
    })
}

/// Rebuilds every generatable package's derived directory and the generated
/// marketplace catalogue referencing them. Safe to call repeatedly: each
/// package directory is rebuilt wholesale, and a package that stopped being
/// `generatable` (e.g. it gained an explicit envelope) simply drops out of
/// the catalogue — its now-orphaned directory is cleaned up at detach time
/// by `remove_generated_package_by_id`, not by this function.
pub(super) fn write_generated_catalogue(
    uze_home: &UzeHome,
    packages: &[StoredPackage],
) -> Result<()> {
    let root = generated_root(uze_home);
    fs::create_dir_all(&root).map_err(|source| UzeError::Write {
        path: root.clone(),
        source,
    })?;
    for package in generated_publishable(packages) {
        materialize_generated_package(uze_home, package)?;
    }
    let catalogue_path = root.join(".agents/plugins/marketplace.json");
    let parent = catalogue_path
        .parent()
        .expect("catalogue path has a parent");
    fs::create_dir_all(parent).map_err(|source| UzeError::Write {
        path: parent.to_path_buf(),
        source,
    })?;
    uze_core::persistence::write_atomic(
        &catalogue_path,
        &serde_json::to_vec_pretty(&generated_catalogue_document(packages))
            .expect("catalogue is serializable"),
    )
}

pub(super) fn generated_catalogue_matches(uze_home: &UzeHome, packages: &[StoredPackage]) -> bool {
    let catalogue_path = generated_root(uze_home).join(".agents/plugins/marketplace.json");
    let expected = generated_catalogue_document(packages);
    match fs::read(&catalogue_path) {
        Ok(bytes) => serde_json::from_slice::<serde_json::Value>(&bytes)
            .is_ok_and(|actual| actual == expected),
        Err(_) => generated_publishable(packages).is_empty(),
    }
}

pub(super) fn generated_package_receipt(
    integration_id: &str,
    package: &StoredPackage,
    marketplace_root: &Path,
    generated_dir: &Path,
    selector: &str,
) -> AttachmentReceipt {
    AttachmentReceipt {
        package_id: package.id.as_str().to_owned(),
        resource_identity: None,
        integration: integration_id.to_owned(),
        strategy: "native-plugin-marketplace-generated".to_owned(),
        artifact: ManagedArtifact::IntegrationOwned {
            kind: GENERATED_PLUGIN_KIND.to_owned(),
            selector: selector.to_owned(),
            detail: [
                (
                    "marketplace_root".to_owned(),
                    serde_json::json!(marketplace_root),
                ),
                // The path Codex actually reports back as the installed
                // plugin's `source.path` is the GENERATED directory it was
                // catalogued from, not the canonical Store package root —
                // unlike the explicit-envelope path, where those two are
                // the same directory. Recording the Store root here would
                // make `inspect_codex_plugin`'s source comparison see
                // permanent drift against Codex's own truthful report.
                ("package_root".to_owned(), serde_json::json!(generated_dir)),
                ("origin".to_owned(), serde_json::json!("generated")),
            ]
            .into_iter()
            .collect(),
        },
    }
}

#[cfg(unix)]
fn symlink(source: &Path, target: &Path) -> Result<()> {
    std::os::unix::fs::symlink(source, target).map_err(|source_error| UzeError::Write {
        path: target.to_path_buf(),
        source: source_error,
    })
}

#[cfg(not(unix))]
fn symlink(_source: &Path, target: &Path) -> Result<()> {
    Err(UzeError::UnsupportedRuntimeProjection(target.to_path_buf()))
}

#[cfg(test)]
mod generated_native_tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::PathBuf;

    use uze_core::capability::{Capability, CapabilityKind, Representation};
    use uze_core::home::UzeHome;
    use uze_core::integration::IntegrationPort;
    use uze_core::project::Resource;

    use super::super::CodexIntegration;
    use super::*;

    fn temp_root(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "uze-codex-generated-{label}-{nonce}-{}",
            std::process::id()
        ))
    }

    /// Builds a canonical package with NO vendor envelope of any kind —
    /// exactly the North Star `flow` fixture shape.
    fn make_plain_package(label: &str, with_mcp: bool) -> (PathBuf, StoredPackage) {
        let root = temp_root(label);
        let pkg_root = root.join("pkg");
        fs::create_dir_all(pkg_root.join("skills/commit")).unwrap();
        fs::write(
            pkg_root.join("skills/commit/SKILL.md"),
            "---\nname: commit\n---\n",
        )
        .unwrap();
        fs::write(
            pkg_root.join("plugin.json"),
            r#"{"name":"flow","version":"1.2.0","description":"Vendor-neutral flow package"}"#,
        )
        .unwrap();
        if with_mcp {
            fs::write(
                pkg_root.join("mcp.json"),
                r#"{"mcpServers":{"mcp-a":{"command":"a"}}}"#,
            )
            .unwrap();
        }
        let id =
            uze_core::store::PackageId::from_plugin_name("flow", &pkg_root.join("plugin.json"))
                .unwrap();
        let pkg = StoredPackage {
            id,
            root: pkg_root.clone(),
            manifest: pkg_root.join("plugin.json"),
            provenance: uze_core::acquisition::Provenance {
                requested: uze_core::acquisition::PackageSource::Local {
                    path: PathBuf::from("/tmp/fake"),
                },
                resolved: uze_core::acquisition::ResolvedSource::Local {
                    path: PathBuf::from("/tmp/fake"),
                },
            },
        };
        (root, pkg)
    }

    fn skill_resource(pkg: &StoredPackage) -> Resource {
        let path = pkg.root.join("skills/commit/SKILL.md");
        Resource::from_package(
            pkg.id.clone(),
            pkg.root.clone(),
            Capability {
                kind: CapabilityKind::AgentSkill,
                representation: Representation::Standard,
                path,
                payload: Vec::new(),
            },
        )
    }

    fn mcp_resource(pkg: &StoredPackage, name: &str) -> Resource {
        let path = pkg.root.join("mcp.json");
        Resource::from_package_named(
            pkg.id.clone(),
            pkg.root.clone(),
            Capability {
                kind: CapabilityKind::Mcp,
                representation: Representation::Standard,
                path,
                payload: Vec::new(),
            },
            name.to_owned(),
        )
    }

    #[test]
    fn plain_package_is_generatable_and_explicit_envelope_is_not() {
        let (_root, pkg) = make_plain_package("generatable", false);
        assert!(generatable(&pkg));
        fs::create_dir_all(pkg.root.join(".codex-plugin")).unwrap();
        fs::write(pkg.root.join(".codex-plugin/plugin.json"), "{}").unwrap();
        assert!(!generatable(&pkg));
        let _ = fs::remove_dir_all(_root);
    }

    #[test]
    fn package_exposure_plan_falls_back_to_generated_route_without_an_envelope() {
        let (_root, pkg) = make_plain_package("plan", false);
        let r_a = skill_resource(&pkg);
        let resources = vec![&r_a];
        let integration =
            CodexIntegration::new(_root.join("agents"), UzeHome::at(_root.join("uze")));
        let plan = integration
            .package_exposure_plan(&pkg, &resources)
            .expect("generated route should apply");
        assert_eq!(plan.route, uze_core::router::CompatibilityRoute::Native);
        assert_eq!(
            plan.provided_resource_identities,
            BTreeSet::from([r_a.identity()])
        );
        let _ = fs::remove_dir_all(_root);
    }

    #[test]
    fn explicit_envelope_still_takes_precedence_over_generation() {
        let (_root, pkg) = make_plain_package("precedence", false);
        fs::create_dir_all(pkg.root.join(".codex-plugin")).unwrap();
        fs::write(
            pkg.root.join(".codex-plugin/plugin.json"),
            r#"{"name":"flow","version":"9.9.9","skills":"./skills/"}"#,
        )
        .unwrap();
        let r_a = skill_resource(&pkg);
        let resources = vec![&r_a];
        let uze_home = UzeHome::at(_root.join("uze"));
        let integration = CodexIntegration::new(_root.join("agents"), uze_home.clone());
        let plan = integration
            .package_exposure_plan(&pkg, &resources)
            .expect("explicit route should apply");
        assert_eq!(
            plan.provided_resource_identities,
            BTreeSet::from([r_a.identity()])
        );
        assert!(
            !generated_root(&uze_home).join(pkg.id.as_str()).exists(),
            "generation must never be attempted when an explicit envelope is present"
        );
        let _ = fs::remove_dir_all(_root);
    }

    #[test]
    fn malformed_explicit_envelope_does_not_fall_through_to_generation() {
        let (_root, pkg) = make_plain_package("malformed-explicit", false);
        fs::create_dir_all(pkg.root.join(".codex-plugin")).unwrap();
        fs::write(pkg.root.join(".codex-plugin/plugin.json"), "{not json").unwrap();
        let r_a = skill_resource(&pkg);
        let uze_home = UzeHome::at(_root.join("uze"));
        let integration = CodexIntegration::new(_root.join("agents"), uze_home.clone());
        let plan = integration
            .package_exposure_plan(&pkg, &[&r_a])
            .expect("a present (even malformed) explicit envelope still takes the explicit route");
        assert!(
            plan.provided_resource_identities.is_empty(),
            "malformed explicit manifests yield empty coverage, not the generated route's coverage"
        );
        assert!(
            !generated_root(&uze_home).join(pkg.id.as_str()).exists(),
            "generation must never be attempted when an explicit envelope file is present"
        );
        let _ = fs::remove_dir_all(_root);
    }

    #[test]
    fn package_exposure_plan_never_writes_to_disk() {
        let (_root, pkg) = make_plain_package("read-only", true);
        let r_a = skill_resource(&pkg);
        let r_m = mcp_resource(&pkg, "mcp-a");
        let resources = vec![&r_a, &r_m];
        let uze_home = UzeHome::at(_root.join("uze"));
        let integration = CodexIntegration::new(_root.join("agents"), uze_home.clone());
        let _plan = integration.package_exposure_plan(&pkg, &resources);
        assert!(
            !generated_root(&uze_home).exists(),
            "computing a plan must never materialize the generated directory"
        );
        let _ = fs::remove_dir_all(_root);
    }

    #[test]
    fn materialize_generated_package_never_writes_into_the_store_package() {
        let (_root, pkg) = make_plain_package("no-store-mutation", true);
        let uze_home = UzeHome::at(_root.join("uze"));
        let before: BTreeSet<PathBuf> = walk(&pkg.root);
        let dir = materialize_generated_package(&uze_home, &pkg).unwrap();
        let after: BTreeSet<PathBuf> = walk(&pkg.root);
        assert_eq!(
            before, after,
            "Store package tree must be byte-for-byte unchanged"
        );
        assert!(dir.starts_with(uze_home.state_dir()));
        assert!(dir.join(".codex-plugin/plugin.json").is_file());
        assert!(dir.join("skills").is_symlink());
        assert_eq!(
            fs::read_link(dir.join("skills")).unwrap(),
            pkg.root.join("skills")
        );
        assert!(dir.join(".mcp.json").is_symlink());
        assert_eq!(
            fs::read_link(dir.join(".mcp.json")).unwrap(),
            pkg.root.join("mcp.json")
        );
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join(".codex-plugin/plugin.json")).unwrap())
                .unwrap();
        assert_eq!(manifest["skills"], "./skills/");
        assert_eq!(manifest["mcpServers"], "./.mcp.json");
        let _ = fs::remove_dir_all(_root);
    }

    #[test]
    fn materialize_generated_package_is_deterministic_across_rebuilds() {
        let (_root, pkg) = make_plain_package("deterministic", true);
        let uze_home = UzeHome::at(_root.join("uze"));
        materialize_generated_package(&uze_home, &pkg).unwrap();
        let first = fs::read(
            generated_root(&uze_home)
                .join(pkg.id.as_str())
                .join(".codex-plugin/plugin.json"),
        )
        .unwrap();
        materialize_generated_package(&uze_home, &pkg).unwrap();
        let second = fs::read(
            generated_root(&uze_home)
                .join(pkg.id.as_str())
                .join(".codex-plugin/plugin.json"),
        )
        .unwrap();
        assert_eq!(first, second);
        let _ = fs::remove_dir_all(_root);
    }

    #[test]
    fn remove_generated_package_deletes_only_the_derived_directory() {
        let (_root, pkg) = make_plain_package("removal", false);
        let uze_home = UzeHome::at(_root.join("uze"));
        materialize_generated_package(&uze_home, &pkg).unwrap();
        assert!(generated_root(&uze_home).join(pkg.id.as_str()).exists());
        remove_generated_package_by_id(&uze_home, pkg.id.as_str()).unwrap();
        assert!(!generated_root(&uze_home).join(pkg.id.as_str()).exists());
        assert!(
            pkg.root.join("skills/commit/SKILL.md").is_file(),
            "Store bytes untouched"
        );
        let _ = fs::remove_dir_all(_root);
    }

    #[test]
    fn uncovered_skill_outside_the_conventional_directory_falls_back() {
        let (_root, pkg) = make_plain_package("partial", false);
        fs::create_dir_all(pkg.root.join("extra")).unwrap();
        fs::write(pkg.root.join("extra/SKILL.md"), "---\nname: extra\n---\n").unwrap();
        let r_in = skill_resource(&pkg);
        let r_out = Resource::from_package(
            pkg.id.clone(),
            pkg.root.clone(),
            Capability {
                kind: CapabilityKind::AgentSkill,
                representation: Representation::Standard,
                path: pkg.root.join("extra/SKILL.md"),
                payload: Vec::new(),
            },
        );
        let resources = vec![&r_in, &r_out];
        let covered = generated_exact_coverage(&pkg, &resources);
        assert_eq!(covered, BTreeSet::from([r_in.identity()]));
        assert!(!covered.contains(&r_out.identity()));

        let uze_home = UzeHome::at(_root.join("uze"));
        let integration = CodexIntegration::new(_root.join("agents"), uze_home);
        let fallback = integration.exposure_plan(&r_out);
        assert!(!matches!(
            fallback.mechanism,
            uze_core::exposure::ExposureMechanism::Unsupported { .. }
        ));
        let _ = fs::remove_dir_all(_root);
    }

    // --- Generation-eligibility matrix, mirroring Claude's ----------------

    /// A. 1 Skill only → generated native package.
    #[test]
    fn matrix_single_skill_only_generates() {
        let (_root, pkg) = make_plain_package("matrix-skill-only", false);
        let r_a = skill_resource(&pkg);
        let resources = vec![&r_a];
        let integration =
            CodexIntegration::new(_root.join("agents"), UzeHome::at(_root.join("uze")));
        let plan = integration
            .package_exposure_plan(&pkg, &resources)
            .expect("a single Skill alone must qualify for generation");
        assert_eq!(plan.route, uze_core::router::CompatibilityRoute::Native);
        assert_eq!(
            plan.provided_resource_identities,
            BTreeSet::from([r_a.identity()])
        );
        let _ = fs::remove_dir_all(_root);
    }

    /// B. 1 MCP only → generated native package.
    #[test]
    fn matrix_single_mcp_only_generates() {
        let (_root, pkg) = make_plain_package("matrix-mcp-only", true);
        fs::remove_dir_all(pkg.root.join("skills")).unwrap();
        let r_m = mcp_resource(&pkg, "mcp-a");
        let resources = vec![&r_m];
        let integration =
            CodexIntegration::new(_root.join("agents"), UzeHome::at(_root.join("uze")));
        let plan = integration
            .package_exposure_plan(&pkg, &resources)
            .expect("a single MCP server alone must qualify for generation");
        assert_eq!(
            plan.provided_resource_identities,
            BTreeSet::from([r_m.identity()])
        );
        let _ = fs::remove_dir_all(_root);
    }

    /// C. 1 Skill + 1 MCP → generated native package, both covered.
    #[test]
    fn matrix_skill_plus_mcp_generates_with_full_coverage() {
        let (_root, pkg) = make_plain_package("matrix-skill-mcp", true);
        let r_a = skill_resource(&pkg);
        let r_m = mcp_resource(&pkg, "mcp-a");
        let resources = vec![&r_a, &r_m];
        let integration =
            CodexIntegration::new(_root.join("agents"), UzeHome::at(_root.join("uze")));
        let plan = integration
            .package_exposure_plan(&pkg, &resources)
            .expect("skill + MCP together must qualify for generation");
        assert_eq!(
            plan.provided_resource_identities,
            BTreeSet::from([r_a.identity(), r_m.identity()])
        );
        let _ = fs::remove_dir_all(_root);
    }

    /// D. Multiple Skills → generated native package, all covered.
    #[test]
    fn matrix_multiple_skills_all_covered() {
        let (_root, pkg) = make_plain_package("matrix-multi-skill", false);
        fs::create_dir_all(pkg.root.join("skills/deploy")).unwrap();
        fs::write(
            pkg.root.join("skills/deploy/SKILL.md"),
            "---\nname: deploy\n---\n",
        )
        .unwrap();
        let r_commit = skill_resource(&pkg);
        let r_deploy = Resource::from_package(
            pkg.id.clone(),
            pkg.root.clone(),
            Capability {
                kind: CapabilityKind::AgentSkill,
                representation: Representation::Standard,
                path: pkg.root.join("skills/deploy/SKILL.md"),
                payload: Vec::new(),
            },
        );
        let resources = vec![&r_commit, &r_deploy];
        let integration =
            CodexIntegration::new(_root.join("agents"), UzeHome::at(_root.join("uze")));
        let plan = integration
            .package_exposure_plan(&pkg, &resources)
            .expect("multiple skills in one conventional directory must all qualify");
        assert_eq!(
            plan.provided_resource_identities,
            BTreeSet::from([r_commit.identity(), r_deploy.identity()])
        );
        let _ = fs::remove_dir_all(_root);
    }

    /// E. Unsupported-only capability (no Skill, no MCP) → package is not
    /// generatable at all, and the capability itself correctly reports
    /// Unsupported through the normal per-resource fallback.
    #[test]
    fn matrix_unsupported_only_capability_yields_no_native_package() {
        let root = temp_root("matrix-unsupported-only");
        let pkg_root = root.join("pkg");
        fs::create_dir_all(&pkg_root).unwrap();
        fs::write(
            pkg_root.join("plugin.json"),
            r#"{"name":"hooks-only","version":"1.0.0"}"#,
        )
        .unwrap();
        let id = uze_core::store::PackageId::from_plugin_name(
            "hooks-only",
            &pkg_root.join("plugin.json"),
        )
        .unwrap();
        let pkg = StoredPackage {
            id,
            root: pkg_root.clone(),
            manifest: pkg_root.join("plugin.json"),
            provenance: uze_core::acquisition::Provenance {
                requested: uze_core::acquisition::PackageSource::Local {
                    path: PathBuf::from("/tmp/fake"),
                },
                resolved: uze_core::acquisition::ResolvedSource::Local {
                    path: PathBuf::from("/tmp/fake"),
                },
            },
        };
        let r_hook = Resource::from_package(
            pkg.id.clone(),
            pkg.root.clone(),
            Capability {
                kind: CapabilityKind::Hook,
                representation: Representation::Standard,
                path: pkg_root.join("hooks/pre-commit"),
                payload: Vec::new(),
            },
        );
        assert!(!generatable(&pkg));
        let integration = CodexIntegration::new(root.join("agents"), UzeHome::at(root.join("uze")));
        assert!(
            integration
                .package_exposure_plan(&pkg, &[&r_hook])
                .is_none()
        );
        assert!(matches!(
            integration.exposure_plan(&r_hook).mechanism,
            uze_core::exposure::ExposureMechanism::Unsupported { .. }
        ));
        let _ = fs::remove_dir_all(root);
    }

    /// F. Mixed safe + unsupported: a package with a safely-representable
    /// Skill AND an unsupported capability kind must generate a package
    /// covering only the Skill.
    #[test]
    fn matrix_mixed_safe_and_unsupported_yields_partial_coverage_and_fallback() {
        let (_root, pkg) = make_plain_package("matrix-mixed", false);
        let r_skill = skill_resource(&pkg);
        let r_hook = Resource::from_package(
            pkg.id.clone(),
            pkg.root.clone(),
            Capability {
                kind: CapabilityKind::Hook,
                representation: Representation::Standard,
                path: pkg.root.join("hooks/pre-commit"),
                payload: Vec::new(),
            },
        );
        let resources = vec![&r_skill, &r_hook];
        let integration =
            CodexIntegration::new(_root.join("agents"), UzeHome::at(_root.join("uze")));
        let plan = integration
            .package_exposure_plan(&pkg, &resources)
            .expect("the package is still generatable via its Skill");
        assert_eq!(
            plan.provided_resource_identities,
            BTreeSet::from([r_skill.identity()]),
            "the unsupported Hook must never be claimed as covered"
        );
        assert!(matches!(
            integration.exposure_plan(&r_hook).mechanism,
            uze_core::exposure::ExposureMechanism::Unsupported { .. }
        ));
        let _ = fs::remove_dir_all(_root);
    }

    fn walk(root: &std::path::Path) -> BTreeSet<PathBuf> {
        let mut out = BTreeSet::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path.clone());
                }
                out.insert(path);
            }
        }
        out
    }
}
