//! Gemini's GENERATED native extension envelope: for a canonical UZE package
//! that ships no explicit `gemini-extension.json`, this module
//! deterministically synthesizes one into a UZE-owned derived directory —
//! never into the Store — so the package can still install as one native
//! Gemini extension instead of decomposing into per-capability shims.
//!
//! Generated Native Extension sits between Explicit Native Extension and
//! Native Capability in the delivery hierarchy (ADR-020/ADR-021, refining
//! ADR-013 §2), mirroring `crate::claude::generate` and
//! `crate::codex::generate`'s shape and discipline. Simpler than either:
//! Gemini needs no catalogue (see the module doc on `super::GeminiIntegration`),
//! so a generated extension is `gemini extensions link`ed straight at its
//! derived directory, exactly like an explicit one is linked straight at
//! the Store package — no marketplace root, no path-containment rule to
//! satisfy.

use std::{collections::BTreeSet, fs, path::Path, path::PathBuf};

use uze_core::{
    Result, UzeError,
    home::UzeHome,
    integration::{AttachmentReceipt, ManagedArtifact},
    project::Resource,
    store::StoredPackage,
};

/// The `kind` this module stamps on its own receipts. Distinct from
/// `LINKED_EXTENSION` (the explicit-envelope kind) so a receipt's own shape
/// already announces which lifecycle owns it before `detail["origin"]` is
/// even read.
pub(super) const GENERATED_LINKED_EXTENSION: &str = "linked-extension-generated";

/// Root of every package's generated extension directory. Lives under
/// `$UZE_HOME/state/attachments/gemini/generated/` — the same convention
/// Claude's and Codex's generated envelopes use, never under the Store.
pub(super) fn generated_root(uze_home: &UzeHome) -> PathBuf {
    uze_home
        .state_dir()
        .join("attachments")
        .join("gemini")
        .join("generated")
}

fn generated_package_dir_for_id(uze_home: &UzeHome, package_id: &str) -> PathBuf {
    generated_root(uze_home).join(package_id)
}

/// Whether this package has anything UZE can safely represent as a
/// generated native envelope: no explicit envelope of its own, and at
/// least one of the two structural surfaces UZE synthesizes from (a
/// conventional `skills/` directory, or a root `mcp.json`) — identical
/// eligibility rule to Claude's and Codex's, applied to Gemini's own
/// explicit-envelope marker file.
pub(super) fn generatable(package: &StoredPackage) -> bool {
    !package.root.join("gemini-extension.json").is_file()
        && (package.root.join("skills").is_dir() || package.root.join("mcp.json").is_file())
}

/// The intersection ADR-013 §2 requires (`provided = discovered ∩
/// declared`), computed against the STRUCTURAL surface a generated manifest
/// declares — not by re-parsing a manifest this same module just wrote, so
/// generation and coverage agree by construction. Identical rule to
/// Claude's and Codex's `generated_exact_coverage`.
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

/// The generated `gemini-extension.json` document. Name is the package's
/// own id (deterministic — never read back from disk); version/description
/// come from the package's own canonical `plugin.json`, never invented.
/// `mcpServers` is declared inline (Gemini's explicit format embeds it
/// directly, unlike Codex's external-file reference) only when the
/// package's own `mcp.json` actually has a `mcpServers` object.
fn generated_extension_document(package: &StoredPackage) -> serde_json::Value {
    let (description, version) = read_name_fields(package);

    let mut document = serde_json::json!({
        "name": package.id.as_str(),
        "version": version,
        "description": description,
    });

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
                .unwrap_or("UZE-managed Gemini extension, generated from a vendor-neutral package.")
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
                "UZE-managed Gemini extension, generated from a vendor-neutral package.".to_owned(),
                "0.1.0".to_owned(),
            )
        })
}

/// Materializes (or refreshes) one package's generated extension directory.
/// Idempotent and deterministic: recreated wholesale from the Store package
/// on every call — the directory is entirely UZE-owned and
/// non-authoritative (ADR-013 §4). `skills/` is symlinked to the Store's
/// own bytes, never copied.
pub(super) fn materialize_generated_extension(
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
    fs::create_dir_all(&dir).map_err(|source| UzeError::Write {
        path: dir.clone(),
        source,
    })?;
    let manifest = generated_extension_document(package);
    fs::write(
        dir.join("gemini-extension.json"),
        serde_json::to_vec_pretty(&manifest).expect("generated manifest is serializable"),
    )
    .map_err(|source| UzeError::Write {
        path: dir.join("gemini-extension.json"),
        source,
    })?;

    let skills_source = package.root.join("skills");
    if skills_source.is_dir() {
        symlink(&skills_source, &dir.join("skills"))?;
    }
    Ok(dir)
}

/// Removes one package's generated extension directory by id alone — used
/// at detach time, when only the receipt's `package_id` (not a full
/// `StoredPackage`) is available. Safe unconditionally: this directory is
/// never anything but a Derived Artifact (ADR-013 §4).
pub(super) fn remove_generated_extension_by_id(uze_home: &UzeHome, package_id: &str) -> Result<()> {
    let dir = generated_package_dir_for_id(uze_home, package_id);
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|source| UzeError::Write { path: dir, source })?;
    }
    Ok(())
}

pub(super) fn generated_extension_receipt(
    integration_id: &str,
    package: &StoredPackage,
    source_dir: &Path,
) -> AttachmentReceipt {
    AttachmentReceipt {
        package_id: package.id.as_str().to_owned(),
        resource_identity: None,
        integration: integration_id.to_owned(),
        strategy: "linked-native-extension-generated".to_owned(),
        artifact: ManagedArtifact::IntegrationOwned {
            kind: GENERATED_LINKED_EXTENSION.to_owned(),
            selector: package.id.as_str().to_owned(),
            detail: [
                ("source_path".to_owned(), serde_json::json!(source_dir)),
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

    use super::super::GeminiIntegration;
    use super::*;

    fn temp_root(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "uze-gemini-generated-{label}-{nonce}-{}",
            std::process::id()
        ))
    }

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
        fs::write(pkg.root.join("gemini-extension.json"), "{}").unwrap();
        assert!(!generatable(&pkg));
        let _ = fs::remove_dir_all(_root);
    }

    #[test]
    fn package_exposure_plan_falls_back_to_generated_route_without_an_envelope() {
        let (_root, pkg) = make_plain_package("plan", false);
        let r_a = skill_resource(&pkg);
        let resources = vec![&r_a];
        let integration =
            GeminiIntegration::new(_root.join("agents"), UzeHome::at(_root.join("uze")));
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
        fs::write(
            pkg.root.join("gemini-extension.json"),
            r#"{"name":"flow","version":"9.9.9"}"#,
        )
        .unwrap();
        let r_a = skill_resource(&pkg);
        let resources = vec![&r_a];
        let uze_home = UzeHome::at(_root.join("uze"));
        let integration = GeminiIntegration::new(_root.join("agents"), uze_home.clone());
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
        fs::write(pkg.root.join("gemini-extension.json"), "{not json").unwrap();
        let r_a = skill_resource(&pkg);
        let uze_home = UzeHome::at(_root.join("uze"));
        let integration = GeminiIntegration::new(_root.join("agents"), uze_home.clone());
        let plan = integration
            .package_exposure_plan(&pkg, &[&r_a])
            .expect("a present (even malformed) explicit envelope still takes the explicit route");
        // Unlike Claude's/Codex's manifests, Gemini's explicit Skill
        // coverage is purely structural (the `skills/` convention, not a
        // manifest field) — see `gemini_exact_coverage`'s doc comment — so
        // a malformed manifest still covers the Skill. What proves the
        // explicit branch (not generation) ran is that no generated
        // directory was ever materialized.
        assert_eq!(
            plan.provided_resource_identities,
            BTreeSet::from([r_a.identity()])
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
        let integration = GeminiIntegration::new(_root.join("agents"), uze_home.clone());
        let _plan = integration.package_exposure_plan(&pkg, &resources);
        assert!(
            !generated_root(&uze_home).exists(),
            "computing a plan must never materialize the generated directory"
        );
        let _ = fs::remove_dir_all(_root);
    }

    #[test]
    fn materialize_generated_extension_never_writes_into_the_store_package() {
        let (_root, pkg) = make_plain_package("no-store-mutation", true);
        let uze_home = UzeHome::at(_root.join("uze"));
        let before: BTreeSet<PathBuf> = walk(&pkg.root);
        let dir = materialize_generated_extension(&uze_home, &pkg).unwrap();
        let after: BTreeSet<PathBuf> = walk(&pkg.root);
        assert_eq!(
            before, after,
            "Store package tree must be byte-for-byte unchanged"
        );
        assert!(dir.starts_with(uze_home.state_dir()));
        assert!(dir.join("gemini-extension.json").is_file());
        assert!(dir.join("skills").is_symlink());
        assert_eq!(
            fs::read_link(dir.join("skills")).unwrap(),
            pkg.root.join("skills")
        );
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join("gemini-extension.json")).unwrap()).unwrap();
        assert_eq!(manifest["name"], "flow");
        assert_eq!(manifest["mcpServers"]["mcp-a"]["command"], "a");
        let _ = fs::remove_dir_all(_root);
    }

    #[test]
    fn materialize_generated_extension_is_deterministic_across_rebuilds() {
        let (_root, pkg) = make_plain_package("deterministic", true);
        let uze_home = UzeHome::at(_root.join("uze"));
        materialize_generated_extension(&uze_home, &pkg).unwrap();
        let first = fs::read(
            generated_root(&uze_home)
                .join(pkg.id.as_str())
                .join("gemini-extension.json"),
        )
        .unwrap();
        materialize_generated_extension(&uze_home, &pkg).unwrap();
        let second = fs::read(
            generated_root(&uze_home)
                .join(pkg.id.as_str())
                .join("gemini-extension.json"),
        )
        .unwrap();
        assert_eq!(first, second);
        let _ = fs::remove_dir_all(_root);
    }

    #[test]
    fn remove_generated_extension_deletes_only_the_derived_directory() {
        let (_root, pkg) = make_plain_package("removal", false);
        let uze_home = UzeHome::at(_root.join("uze"));
        materialize_generated_extension(&uze_home, &pkg).unwrap();
        assert!(generated_root(&uze_home).join(pkg.id.as_str()).exists());
        remove_generated_extension_by_id(&uze_home, pkg.id.as_str()).unwrap();
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
        let integration = GeminiIntegration::new(_root.join("agents"), uze_home.clone());
        uze_core::state::record(
            &uze_home,
            uze_core::state::IntegrationRecord {
                harness: integration.id().to_owned(),
                version: None,
                strategy: "test".to_owned(),
                installed: true,
            },
        )
        .unwrap();
        let fallback = integration.exposure_plan(&r_out);
        assert!(!matches!(
            fallback.mechanism,
            uze_core::exposure::ExposureMechanism::Unsupported { .. }
        ));
        let _ = fs::remove_dir_all(_root);
    }

    // --- Generation-eligibility matrix, mirroring Claude's/Codex's --------

    /// A. 1 Skill only → generated native extension.
    #[test]
    fn matrix_single_skill_only_generates() {
        let (_root, pkg) = make_plain_package("matrix-skill-only", false);
        let r_a = skill_resource(&pkg);
        let resources = vec![&r_a];
        let integration =
            GeminiIntegration::new(_root.join("agents"), UzeHome::at(_root.join("uze")));
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

    /// B. 1 MCP only → generated native extension.
    #[test]
    fn matrix_single_mcp_only_generates() {
        let (_root, pkg) = make_plain_package("matrix-mcp-only", true);
        fs::remove_dir_all(pkg.root.join("skills")).unwrap();
        let r_m = mcp_resource(&pkg, "mcp-a");
        let resources = vec![&r_m];
        let integration =
            GeminiIntegration::new(_root.join("agents"), UzeHome::at(_root.join("uze")));
        let plan = integration
            .package_exposure_plan(&pkg, &resources)
            .expect("a single MCP server alone must qualify for generation");
        assert_eq!(
            plan.provided_resource_identities,
            BTreeSet::from([r_m.identity()])
        );
        let _ = fs::remove_dir_all(_root);
    }

    /// C. 1 Skill + 1 MCP → generated native extension, both covered.
    #[test]
    fn matrix_skill_plus_mcp_generates_with_full_coverage() {
        let (_root, pkg) = make_plain_package("matrix-skill-mcp", true);
        let r_a = skill_resource(&pkg);
        let r_m = mcp_resource(&pkg, "mcp-a");
        let resources = vec![&r_a, &r_m];
        let integration =
            GeminiIntegration::new(_root.join("agents"), UzeHome::at(_root.join("uze")));
        let plan = integration
            .package_exposure_plan(&pkg, &resources)
            .expect("skill + MCP together must qualify for generation");
        assert_eq!(
            plan.provided_resource_identities,
            BTreeSet::from([r_a.identity(), r_m.identity()])
        );
        let _ = fs::remove_dir_all(_root);
    }

    /// D. Multiple Skills → generated native extension, all covered.
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
            GeminiIntegration::new(_root.join("agents"), UzeHome::at(_root.join("uze")));
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
        let integration =
            GeminiIntegration::new(root.join("agents"), UzeHome::at(root.join("uze")));
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
    /// Skill AND an unsupported capability kind must generate an extension
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
            GeminiIntegration::new(_root.join("agents"), UzeHome::at(_root.join("uze")));
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
