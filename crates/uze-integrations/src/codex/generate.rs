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
    store::{StoredPackage, is_valid_qualified_id},
};

/// The second, UZE-owned marketplace this module publishes into —
/// deliberately distinct from `MARKETPLACE_NAME` ("uze-local", reserved for
/// explicit envelopes and never touched here) so a generated envelope can
/// never be confused with, or silently override, an author-provided one.
/// Named "uze-store" (shorter than the original "uze-local-generated").
pub(super) const GENERATED_MARKETPLACE_NAME: &str = "uze-store";

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
/// declared`), computed against the SEMANTIC surface a generated manifest
/// can preserve — not by re-parsing a manifest this same module just wrote,
/// so generation and coverage agree by construction (ADR-030 §13).
///
/// A Skill is covered iff it lives under the package's conventional
/// `skills/` directory AND its `invoke:` policy can be preserved by the
/// generated envelope: default and user-only (the envelope materializes the
/// `agents/openai.yaml` sidecar) qualify; model-only degrades on Codex
/// (explicit `$skill` invocation cannot be disabled) and is therefore never
/// claimed — it falls through to capability-level delivery, which reports
/// the Degradation honestly; the invalid combination is never claimed
/// either. An MCP server is covered iff its name appears in the package's
/// own `mcp.json`.
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
                if parent.starts_with("skills") && codex_policy_is_envelope_preservable(resource) {
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

/// Whether the generated envelope can preserve one Skill's canonical
/// invocation policy for Codex (ADR-030 §13). All valid combinations except
/// model-only qualify: Codex's own `agents/openai.yaml` sidecar covers
/// `model=false`, the default needs nothing, and `user=false` cannot be
/// enforced anywhere on Codex.
fn codex_policy_is_envelope_preservable(resource: &Resource) -> bool {
    let policy = resource.skill_invocation();
    !policy.is_invalid() && !(policy.model && !policy.user)
}

/// The generated `.codex-plugin/plugin.json` document. Name/version/
/// description come from the package's own canonical `plugin.json`
/// (`package.manifest`), never invented. `skills` is declared as the fixed
/// `"./skills/"` convention (mirroring the real explicit-envelope fixture
/// shape, `tests/_fixtures/foreign/codex/native-plugin/.codex-plugin/plugin.json`);
/// `mcpServers` names the generated `.mcp.json` sibling file, written only
/// when the package actually has a root `mcp.json` to project.
fn generated_manifest_document(package: &StoredPackage) -> serde_json::Value {
    let (description, version) = read_name_fields(package);

    let mut document = serde_json::json!({
        "name": package.active_name.as_str(),
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

    materialize_generated_skills(package, &dir)?;
    let mcp_source = package.root.join("mcp.json");
    if mcp_source.is_file() {
        symlink(&mcp_source, &dir.join(".mcp.json"))?;
    }
    Ok(dir)
}

/// Materializes the generated envelope's `skills/` surface (ADR-030 §13).
/// Default-policy skills stay byte-preserving whole-directory symlinks; a
/// user-only Skill gets its own UZE-owned directory with a materialized
/// SKILL.md (canonical name/description/body) plus Codex's
/// `agents/openai.yaml` invocation-policy sidecar — the same Derived
/// Artifact discipline as the capability-level wrapper, applied at package
/// level. Invalid or model-only Skills are never materialized here and are
/// excluded from coverage.
fn materialize_generated_skills(package: &StoredPackage, envelope_dir: &Path) -> Result<()> {
    if !package.root.join("skills").is_dir() {
        return Ok(());
    }
    let resources = uze_core::engine::package_resources_at(&package.id, &package.root)?;
    for resource in resources.into_iter().filter(|resource| {
        resource.capability.kind == uze_core::capability::CapabilityKind::AgentSkill
    }) {
        let policy = resource.skill_invocation();
        if policy.is_invalid() || (policy.model && !policy.user) {
            continue;
        }
        let canonical_dir = resource
            .capability
            .path
            .parent()
            .expect("SKILL.md has a parent");
        let skill_name = resource
            .logical_capability_name()
            .unwrap_or_else(|| resource.name());
        let target_dir = envelope_dir.join("skills").join(&skill_name);
        fs::create_dir_all(target_dir.parent().expect("skill dir has a parent")).map_err(
            |source_error| UzeError::Write {
                path: target_dir
                    .parent()
                    .expect("skill dir has a parent")
                    .to_path_buf(),
                source: source_error,
            },
        )?;
        if policy.is_default() {
            match fs::symlink_metadata(&target_dir) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    let current =
                        fs::read_link(&target_dir).map_err(|source_error| UzeError::Read {
                            path: target_dir.clone(),
                            source: source_error,
                        })?;
                    if current != canonical_dir {
                        fs::remove_dir_all(&target_dir).map_err(|source_error| {
                            UzeError::Write {
                                path: target_dir.clone(),
                                source: source_error,
                            }
                        })?;
                        symlink(canonical_dir, &target_dir)?;
                    }
                }
                Ok(_) => return Err(UzeError::ManagedEntryConflict(target_dir)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    symlink(canonical_dir, &target_dir)?;
                }
                Err(error) => {
                    return Err(UzeError::Read {
                        path: target_dir,
                        source: error,
                    });
                }
            }
            continue;
        }
        materialize_user_only_skill_dir(&target_dir, canonical_dir, &skill_name, &policy)?;
    }
    Ok(())
}

/// Writes one materialized user-only Skill directory: SKILL.md with the
/// canonical identity/description/body plus Codex's policy sidecar, with
/// every other canonical file still referenced.
fn materialize_user_only_skill_dir(
    target_dir: &Path,
    canonical_dir: &Path,
    skill_name: &str,
    policy: &uze_core::skill::SkillInvocationPolicy,
) -> Result<()> {
    match fs::symlink_metadata(target_dir) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            fs::remove_file(target_dir).map_err(|source_error| UzeError::Write {
                path: target_dir.to_path_buf(),
                source: source_error,
            })?;
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(UzeError::Read {
                path: target_dir.to_path_buf(),
                source: error,
            });
        }
    }
    fs::create_dir_all(target_dir).map_err(|source_error| UzeError::Write {
        path: target_dir.to_path_buf(),
        source: source_error,
    })?;
    let bytes = fs::read(canonical_dir.join("SKILL.md")).map_err(|error| UzeError::Read {
        path: canonical_dir.join("SKILL.md"),
        source: error,
    })?;
    let (description, body) = crate::shared::skill::parse_skill_body(&bytes);
    let name = crate::shared::skill::frontmatter_value(&bytes, "name")
        .unwrap_or_else(|| skill_name.to_owned());
    let mut document = String::from("---\n");
    document.push_str(&format!("name: {name}\n"));
    if let Some(description) = description {
        let escaped = crate::shared::skill::escape_yaml_double_quoted(&description);
        document.push_str(&format!("description: \"{escaped}\"\n"));
    }
    document.push_str("---\n");
    document.push_str(&body);
    fs::write(target_dir.join("SKILL.md"), document).map_err(|source_error| UzeError::Write {
        path: target_dir.join("SKILL.md"),
        source: source_error,
    })?;
    let policy_file = target_dir.join("agents/openai.yaml");
    if !policy.model {
        fs::create_dir_all(policy_file.parent().expect("policy file has a parent")).map_err(
            |source_error| UzeError::Write {
                path: policy_file
                    .parent()
                    .expect("policy file has a parent")
                    .to_path_buf(),
                source: source_error,
            },
        )?;
        fs::write(&policy_file, super::skills::EXPLICIT_ONLY_POLICY_YAML).map_err(
            |source_error| UzeError::Write {
                path: policy_file,
                source: source_error,
            },
        )?;
    }
    for entry in fs::read_dir(canonical_dir).map_err(|error| UzeError::Read {
        path: canonical_dir.to_path_buf(),
        source: error,
    })? {
        let entry = entry.map_err(|error| UzeError::Read {
            path: canonical_dir.to_path_buf(),
            source: error,
        })?;
        let name = entry.file_name();
        if name == "SKILL.md" {
            continue;
        }
        let source = entry.path();
        let target = target_dir.join(&name);
        if !target.exists() && !target.is_symlink() {
            symlink(&source, &target)?;
        }
    }
    Ok(())
}

/// Removes one package's generated envelope directory by id alone — used at
/// detach time, when only the receipt's `package_id` (not a full
/// `StoredPackage`) is available. Safe unconditionally: this directory is
/// never anything but a Derived Artifact (ADR-013 §4).
pub(super) fn remove_generated_package_by_id(uze_home: &UzeHome, package_id: &str) -> Result<()> {
    // The id comes from the receipt ledger, not a constructor: refuse one
    // that could not have been a real package id instead of joining it into
    // a path and removing whatever the traversal lands on.
    if !is_valid_qualified_id(package_id) {
        return Err(UzeError::ExposureUnavailable(format!(
            "refusing to remove generated envelope for malformed package id `{package_id}`"
        )));
    }
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
                "name": package.active_name.as_str(),
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
            active_name: id.plugin_name().to_owned(),
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
        // Default-policy skills stay byte-preserving whole-directory
        // symlinks; `skills/` itself is now a real envelope subdirectory.
        assert!(dir.join("skills/commit").is_symlink());
        assert_eq!(
            fs::read_link(dir.join("skills/commit")).unwrap(),
            pkg.root.join("skills/commit")
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
            active_name: id.plugin_name().to_owned(),
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
