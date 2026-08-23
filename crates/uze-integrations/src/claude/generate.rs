//! Claude Code's GENERATED native plugin envelope: for a canonical UZE
//! package that ships no explicit `.claude-plugin/plugin.json`, this module
//! deterministically synthesizes one into a UZE-owned derived directory —
//! never into the Store — so the package can still install as one native
//! Claude plugin instead of decomposing into per-capability shims.
//!
//! Generated Native Package sits between Explicit Native Package and Native
//! Capability in the delivery hierarchy (ADR-020, refining ADR-013 §2): a
//! source package's absence of a vendor envelope no longer forces
//! capability-level decomposition by itself — only the absence of anything
//! UZE can safely represent does.
//!
//! Safe synthesis is deliberately structural, not semantic (ADR-013's "no
//! universal projection abstraction", and this milestone's non-goals): the
//! generated manifest declares the package's whole conventional `skills/`
//! directory (mirroring Codex's own `"./skills/"` convention) and its
//! `mcp.json`'s `mcpServers` object verbatim — nothing is translated,
//! reinterpreted, or invented beyond name/version/description already
//! declared in the package's own canonical `plugin.json`.

use std::{collections::BTreeSet, fs, path::Path, path::PathBuf};

use uze_core::{
    Result, UzeError,
    home::UzeHome,
    integration::{AttachmentReceipt, ManagedArtifact},
    project::Resource,
    store::StoredPackage,
};

/// The second, UZE-owned marketplace this module publishes into —
/// deliberately distinct from `CLAUDE_MARKETPLACE_NAME` ("uze-local", which
/// stays reserved for explicit envelopes and is never touched by this
/// module) so a generated envelope can never be confused with, or silently
/// override, an author-provided one.
pub(super) const GENERATED_MARKETPLACE_NAME: &str = "uze-local-generated";

/// The `kind` this module stamps on its own receipts. Distinct from
/// `claude-plugin` (the explicit-envelope kind) even though both are
/// installed the same way, so a receipt's own shape already announces
/// which lifecycle owns it before `detail["origin"]` is even read.
pub(super) const GENERATED_PLUGIN_KIND: &str = "claude-plugin-generated";

/// Root of every package's generated envelope directory. Lives under
/// `$UZE_HOME/state/attachments/claude/` — the same convention already used
/// by the per-Skill shim (`claude/skills.rs`'s `materialize_shim`) — never
/// under the Store, so writing here can never mutate canonical package
/// bytes.
pub(super) fn generated_root(uze_home: &UzeHome) -> PathBuf {
    uze_home
        .state_dir()
        .join("attachments")
        .join("claude")
        .join("generated")
}

fn generated_package_dir_for_id(uze_home: &UzeHome, package_id: &str) -> PathBuf {
    generated_root(uze_home).join(package_id)
}

/// Whether this package has anything UZE can safely represent as a
/// generated native envelope: no explicit envelope of its own, and at
/// least one of the three structural surfaces UZE synthesizes from (a
/// conventional `skills/` directory, a canonical `commands/` directory, or
/// a root `mcp.json`).
pub(super) fn generatable(package: &StoredPackage) -> bool {
    !package.root.join(".claude-plugin/plugin.json").is_file()
        && (package.root.join("skills").is_dir()
            || package.root.join("commands").is_dir()
            || package.root.join("mcp.json").is_file())
}

/// The intersection ADR-013 §2 requires (`provided = discovered ∩
/// declared`), computed against the STRUCTURAL surface a generated manifest
/// declares — not by re-parsing a manifest this same module just wrote, so
/// generation and coverage agree by construction. A Skill is covered iff it
/// lives under the package's conventional `skills/` directory; a Command
/// iff it lives under the conventional `commands/` directory (the same
/// surface the generated envelope materializes); an MCP server is covered
/// iff its name appears in the package's own `mcp.json`.
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
            uze_core::capability::CapabilityKind::Command => {
                let Some(direct_parent) = resource
                    .capability
                    .path
                    .strip_prefix(&package.root)
                    .ok()
                    .and_then(|relative| relative.parent().map(|parent| parent.to_path_buf()))
                else {
                    continue;
                };
                if direct_parent == std::path::Path::new("commands") {
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

/// The generated `.claude-plugin/plugin.json` document. Name/version/
/// description come from the package's own canonical `plugin.json`
/// (`package.manifest`) — the same fields Claude's explicit-envelope
/// catalogue already surfaces (`claude_catalogue_document`), never
/// invented. `skills`/`mcpServers` are declared only when the structural
/// surface they describe actually exists on disk.
fn generated_manifest_document(package: &StoredPackage) -> serde_json::Value {
    let (description, version) = read_name_fields(package);

    let mut document = serde_json::json!({
        "$schema": "https://anthropic.com/claude-code/plugin.schema.json",
        "name": package.id.as_str(),
        "version": version,
        "description": description,
    });

    if package.root.join("skills").is_dir() {
        document["skills"] = serde_json::json!(["./skills"]);
    }

    if package.root.join("commands").is_dir() {
        // Claude's `commands` manifest field *replaces* the default
        // `commands/` scan, and `commands/` is also the default location —
        // declaring the conventional path explicitly keeps the generated
        // manifest self-describing without changing what is discovered.
        document["commands"] = serde_json::json!(["./commands"]);
    }

    if let Some(servers) = fs::read(package.root.join("mcp.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|value| value.get("mcpServers").cloned())
    {
        document["mcpServers"] = servers;
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
                .unwrap_or("UZE-managed Claude plugin, generated from a vendor-neutral package.")
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
                "UZE-managed Claude plugin, generated from a vendor-neutral package.".to_owned(),
                "0.1.0".to_owned(),
            )
        })
}

/// Materializes (or refreshes) one package's generated envelope directory.
/// Idempotent and deterministic: recreated wholesale from the Store package
/// on every call, never incrementally patched, because the directory is
/// entirely UZE-owned and non-authoritative (ADR-013 §4) — there is nothing
/// here a partial rebuild could safely preserve that a full rebuild would
/// lose.
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
    let plugin_dir = dir.join(".claude-plugin");
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
    let commands_source = package.root.join("commands");
    if commands_source.is_dir() {
        symlink(&commands_source, &dir.join("commands"))?;
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
/// Mirrors `claude_publishable`'s role for the explicit marketplace.
fn generated_publishable(packages: &[StoredPackage]) -> Vec<&StoredPackage> {
    packages
        .iter()
        .filter(|package| generatable(package))
        .collect()
}

fn generated_catalogue_document(packages: &[StoredPackage]) -> serde_json::Value {
    let plugins: Vec<serde_json::Value> = generated_publishable(packages)
        .into_iter()
        .map(|package| {
            let (description, version) = read_name_fields(package);
            serde_json::json!({
                "name": package.id.as_str(),
                "source": format!("./{}", package.id.as_str()),
                "description": description,
                "version": version,
            })
        })
        .collect();
    serde_json::json!({
        "name": GENERATED_MARKETPLACE_NAME,
        "owner": { "name": "UZE Local (generated)", "url": "https://github.com/anomalyco/opencode" },
        "plugins": plugins
    })
}

/// Rebuilds every generatable package's derived directory and the generated
/// marketplace catalogue referencing them. Safe to call repeatedly: each
/// package directory is rebuilt wholesale, and a package that stopped being
/// `generatable` (e.g. it gained an explicit envelope) simply drops out of
/// the catalogue — its now-orphaned directory is cleaned up at detach time
/// by `remove_generated_package`, not by this function.
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
    let catalogue_path = root.join(".claude-plugin/marketplace.json");
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
    let catalogue_path = generated_root(uze_home).join(".claude-plugin/marketplace.json");
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
                ("package_root".to_owned(), serde_json::json!(package.root)),
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

    use super::super::ClaudeIntegration;
    use super::*;

    fn temp_root(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "uze-claude-generated-{label}-{nonce}-{}",
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
        fs::create_dir_all(pkg.root.join(".claude-plugin")).unwrap();
        fs::write(pkg.root.join(".claude-plugin/plugin.json"), "{}").unwrap();
        assert!(!generatable(&pkg));
        let _ = fs::remove_dir_all(_root);
    }

    #[test]
    fn package_exposure_plan_falls_back_to_generated_route_without_an_envelope() {
        let (_root, pkg) = make_plain_package("plan", false);
        let r_a = skill_resource(&pkg);
        let resources = vec![&r_a];
        let integration =
            ClaudeIntegration::new(_root.join("claude"), UzeHome::at(_root.join("uze")));
        let plan = integration
            .package_exposure_plan(&pkg, &resources)
            .expect("generated route should apply");
        assert_eq!(
            plan.provided_resource_identities,
            BTreeSet::from([r_a.identity()])
        );
        let _ = fs::remove_dir_all(_root);
    }

    #[test]
    fn explicit_envelope_still_takes_precedence_over_generation() {
        let (_root, pkg) = make_plain_package("precedence", false);
        fs::create_dir_all(pkg.root.join(".claude-plugin")).unwrap();
        fs::write(
            pkg.root.join(".claude-plugin/plugin.json"),
            r#"{"name":"flow","version":"9.9.9","skills":["./skills/commit"]}"#,
        )
        .unwrap();
        let r_a = skill_resource(&pkg);
        let resources = vec![&r_a];
        let integration =
            ClaudeIntegration::new(_root.join("claude"), UzeHome::at(_root.join("uze")));
        let plan = integration
            .package_exposure_plan(&pkg, &resources)
            .expect("explicit route should apply");
        assert_eq!(
            plan.provided_resource_identities,
            BTreeSet::from([r_a.identity()])
        );
        assert!(
            !generated_root(&UzeHome::at(_root.join("uze")))
                .join(pkg.id.as_str())
                .exists()
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
        let integration = ClaudeIntegration::new(_root.join("claude"), uze_home.clone());
        let _plan = integration.package_exposure_plan(&pkg, &resources);
        assert!(
            !generated_root(&uze_home).exists(),
            "computing a plan must never materialize the generated directory"
        );
        let _ = fs::remove_dir_all(_root);
    }

    #[test]
    fn mcp_only_package_with_no_skills_dir_is_still_generatable() {
        let root = temp_root("mcp-only");
        let pkg_root = root.join("pkg");
        fs::create_dir_all(&pkg_root).unwrap();
        fs::write(
            pkg_root.join("plugin.json"),
            r#"{"name":"mcp-only","version":"1.0.0"}"#,
        )
        .unwrap();
        fs::write(
            pkg_root.join("mcp.json"),
            r#"{"mcpServers":{"mcp-a":{"command":"a"}}}"#,
        )
        .unwrap();
        let id =
            uze_core::store::PackageId::from_plugin_name("mcp-only", &pkg_root.join("plugin.json"))
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
        assert!(generatable(&pkg));
        let r_m = mcp_resource(&pkg, "mcp-a");
        let covered = generated_exact_coverage(&pkg, &[&r_m]);
        assert_eq!(covered, BTreeSet::from([r_m.identity()]));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn package_with_neither_surface_is_not_generatable() {
        let root = temp_root("neither");
        let pkg_root = root.join("pkg");
        fs::create_dir_all(&pkg_root).unwrap();
        fs::write(pkg_root.join("plugin.json"), r#"{"name":"empty"}"#).unwrap();
        let id =
            uze_core::store::PackageId::from_plugin_name("empty", &pkg_root.join("plugin.json"))
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
        assert!(!generatable(&pkg));
        let _ = fs::remove_dir_all(root);
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
        assert!(dir.join(".claude-plugin/plugin.json").is_file());
        assert!(dir.join("skills").is_symlink());
        assert_eq!(
            fs::read_link(dir.join("skills")).unwrap(),
            pkg.root.join("skills")
        );
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join(".claude-plugin/plugin.json")).unwrap())
                .unwrap();
        assert_eq!(manifest["mcpServers"]["mcp-a"]["command"], "a");
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
                .join(".claude-plugin/plugin.json"),
        )
        .unwrap();
        materialize_generated_package(&uze_home, &pkg).unwrap();
        let second = fs::read(
            generated_root(&uze_home)
                .join(pkg.id.as_str())
                .join(".claude-plugin/plugin.json"),
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
        let integration = ClaudeIntegration::new(_root.join("claude"), uze_home);
        let fallback = integration.exposure_plan(&r_out);
        assert!(!matches!(
            fallback.mechanism,
            uze_core::exposure::ExposureMechanism::Unsupported { .. }
        ));
        let _ = fs::remove_dir_all(_root);
    }

    // --- Generation-eligibility matrix ---------------------------------
    //
    // A package's eligibility for Generated Native Package is
    // capability-based, not resource-count-based (ADR-020): a single Skill
    // or a single MCP server, alone, already qualifies. These tests make
    // that matrix explicit rather than leaving it implied by the tests
    // above.

    /// A. 1 Skill only → generated native package.
    /// (Already covered above by `package_exposure_plan_falls_back_to_generated_route_without_an_envelope`;
    /// restated here for matrix completeness under its own name.)
    #[test]
    fn matrix_single_skill_only_generates() {
        let (_root, pkg) = make_plain_package("matrix-skill-only", false);
        let r_a = skill_resource(&pkg);
        let resources = vec![&r_a];
        let integration =
            ClaudeIntegration::new(_root.join("claude"), UzeHome::at(_root.join("uze")));
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
        let root = temp_root("matrix-mcp-only");
        let pkg_root = root.join("pkg");
        fs::create_dir_all(&pkg_root).unwrap();
        fs::write(
            pkg_root.join("plugin.json"),
            r#"{"name":"mcp-solo","version":"1.0.0"}"#,
        )
        .unwrap();
        fs::write(
            pkg_root.join("mcp.json"),
            r#"{"mcpServers":{"mcp-a":{"command":"a"}}}"#,
        )
        .unwrap();
        let id =
            uze_core::store::PackageId::from_plugin_name("mcp-solo", &pkg_root.join("plugin.json"))
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
        let r_m = mcp_resource(&pkg, "mcp-a");
        let resources = vec![&r_m];
        let integration =
            ClaudeIntegration::new(root.join("claude"), UzeHome::at(root.join("uze")));
        let plan = integration
            .package_exposure_plan(&pkg, &resources)
            .expect("a single MCP server alone must qualify for generation");
        assert_eq!(plan.route, uze_core::router::CompatibilityRoute::Native);
        assert_eq!(
            plan.provided_resource_identities,
            BTreeSet::from([r_m.identity()])
        );
        let _ = fs::remove_dir_all(root);
    }

    /// C. 1 Skill + 1 MCP → generated native package, both covered.
    #[test]
    fn matrix_skill_plus_mcp_generates_with_full_coverage() {
        let (_root, pkg) = make_plain_package("matrix-skill-mcp", true);
        let r_a = skill_resource(&pkg);
        let r_m = mcp_resource(&pkg, "mcp-a");
        let resources = vec![&r_a, &r_m];
        let integration =
            ClaudeIntegration::new(_root.join("claude"), UzeHome::at(_root.join("uze")));
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
            ClaudeIntegration::new(_root.join("claude"), UzeHome::at(_root.join("uze")));
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
    /// Unsupported through the normal per-resource fallback — no native
    /// package is fabricated to cover something UZE cannot represent.
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
        let integration =
            ClaudeIntegration::new(root.join("claude"), UzeHome::at(root.join("uze")));
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
    /// Skill AND an unsupported capability kind (e.g. a Hook) must generate
    /// a package covering only the Skill — the Hook is never silently
    /// claimed, and it still routes through normal per-resource fallback
    /// (Unsupported, since Claude models only AgentSkill/Mcp).
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
            ClaudeIntegration::new(_root.join("claude"), UzeHome::at(_root.join("uze")));
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

    /// G. A malformed but PRESENT explicit envelope must never silently
    /// fall through to generation — presence alone (not validity) decides
    /// the branch, matching `claude/plugin.rs`'s own malformed-envelope
    /// precedent for the explicit path itself. The generated marketplace
    /// must never be touched in this case.
    #[test]
    fn matrix_malformed_explicit_envelope_does_not_fall_through_to_generation() {
        let (_root, pkg) = make_plain_package("matrix-malformed-explicit", false);
        fs::create_dir_all(pkg.root.join(".claude-plugin")).unwrap();
        fs::write(pkg.root.join(".claude-plugin/plugin.json"), "{not json").unwrap();
        let r_a = skill_resource(&pkg);
        let uze_home = UzeHome::at(_root.join("uze"));
        let integration = ClaudeIntegration::new(_root.join("claude"), uze_home.clone());
        let plan = integration
            .package_exposure_plan(&pkg, &[&r_a])
            .expect("a present (even malformed) explicit envelope still takes the explicit route");
        // Malformed explicit manifests yield empty coverage (see
        // `claude/plugin.rs`'s `claude_exact_coverage`), not the
        // generated route's coverage — proving the explicit branch, not
        // generation, actually ran.
        assert!(plan.provided_resource_identities.is_empty());
        assert!(
            !generated_root(&uze_home).join(pkg.id.as_str()).exists(),
            "generation must never be attempted when an explicit envelope file is present"
        );
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
