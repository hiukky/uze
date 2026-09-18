//! Claude Code's GENERATED native plugin envelope: for a canonical UZE
//! package that ships no explicit `.claude-plugin/plugin.json`, this module
//! deterministically synthesizes one into a UZE-owned derived directory —
//! never into the Store — so the package can still install as one native
//! Claude plugin instead of decomposing into per-capability shims.
//!
//! Generated Native Package sits between Explicit Native Package and Native
//! Capability in the delivery hierarchy (ADR-013 §3): a
//! source package's absence of a vendor envelope no longer forces
//! capability-level decomposition by itself — only the absence of anything
//! UZE can safely represent does.
//!
//! Safe synthesis is deliberately structural for the default model+user
//! policy: the generated manifest declares the package's whole conventional
//! `skills/` directory (mirroring Codex's own `"./skills/"` convention)
//! verbatim, and its `mcp.json`'s `mcpServers` object verbatim — nothing is
//! translated, reinterpreted, or invented beyond name/version/description
//! already declared in the package's own canonical `plugin.json`.
//!
//! A Skill whose canonical `invoke:` policy is not the default is the one
//! deliberate exception (ADR-030): Claude
//! has no vendor-neutral `invoke:` concept, but it does honor its own
//! frontmatter fields (`disable-model-invocation: true` → user-only;
//! `user-invocable: false` → model-only), so the generated envelope
//! materializes one real SKILL.md per non-default Skill carrying those
//! markers — never a symlink — while preserving the canonical
//! name/description/body. Still a Derived Artifact (ADR-013 §5) under
//! `$UZE_HOME`, never the Store: only the physical representation of the
//! Skill changed, not its ownership.

use std::{fs, path::Path};

use uze_core::{Result, UzeError, store::StoredPackage};

use crate::shared::marketplace::{canonical_mcp_manifest_value, manifest_fields};
use crate::shared::skill::{link_extras, link_or_repair, recreate_dir, write_file};

/// The description a generated manifest declares when the package's own
/// canonical `plugin.json` states none.
pub(super) const GENERATED_DESCRIPTION: &str =
    "UZE-managed Claude plugin, generated from a vendor-neutral package.";

/// Writes the generated `.claude-plugin/plugin.json` and the `skills/`
/// surface it declares into a fresh envelope directory. Name/version/
/// description come from the package's own canonical `plugin.json`, never
/// invented; `skills`/`mcpServers` are declared only when the structural
/// surface they describe exists on disk, and `mcpServers` is carried inline
/// verbatim.
pub(super) fn materialize_envelope(package: &StoredPackage, dir: &Path) -> Result<()> {
    let (description, version) = manifest_fields(&package.manifest, GENERATED_DESCRIPTION);
    let mut manifest = serde_json::json!({
        "$schema": "https://anthropic.com/claude-code/plugin.schema.json",
        "name": package.active_name.as_str(),
        "version": version,
        "description": description,
    });
    if package.root.join("skills").is_dir() {
        manifest["skills"] = serde_json::json!(["./skills"]);
    }
    if let Some(servers) = canonical_mcp_manifest_value(package) {
        manifest["mcpServers"] = servers;
    }
    let plugin_dir = dir.join(".claude-plugin");
    fs::create_dir_all(&plugin_dir).map_err(|source| UzeError::Write {
        path: plugin_dir.clone(),
        source,
    })?;
    write_file(
        &plugin_dir.join("plugin.json"),
        &serde_json::to_vec_pretty(&manifest).expect("generated manifest is serializable"),
    )?;
    materialize_generated_skills(package, dir)
}

/// Materializes the generated envelope's `skills/` surface.
///
/// A Skill whose canonical `invoke:` policy is the default is symlinked
/// wholesale (byte-preserving, the Store stays the single source of
/// truth). A Skill with a non-default policy gets one UZE-owned
/// materialized SKILL.md carrying the canonical name/description/body plus
/// Claude's own invocation markers — every other file in the canonical
/// skill directory stays referenced — because Claude has no `invoke:`
/// concept of its own. The invalid policy is never materialized and is
/// excluded from coverage, so generation and coverage keep agreeing by
/// construction (ADR-030 §13).
fn materialize_generated_skills(package: &StoredPackage, envelope_dir: &Path) -> Result<()> {
    if !package.root.join("skills").is_dir() {
        return Ok(());
    }
    let skills_dir = envelope_dir.join("skills");
    fs::create_dir_all(&skills_dir).map_err(|source| UzeError::Write {
        path: skills_dir.clone(),
        source,
    })?;
    let resources = uze_core::engine::package_resources_at(&package.id, &package.root)?;
    for resource in resources.into_iter().filter(|resource| {
        resource.capability.kind == uze_core::capability::CapabilityKind::AgentSkill
    }) {
        let policy = resource.skill_invocation();
        if policy.is_invalid() {
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
        let target_dir = skills_dir.join(&skill_name);
        if policy.is_default() {
            link_or_repair(&target_dir, canonical_dir)?;
            continue;
        }
        recreate_dir(&target_dir)?;
        let bytes = fs::read(canonical_dir.join("SKILL.md")).map_err(|error| UzeError::Read {
            path: canonical_dir.join("SKILL.md"),
            source: error,
        })?;
        let document = super::skills::claude_wrapper_skill_document(&bytes, &policy, &skill_name);
        write_file(&target_dir.join("SKILL.md"), document.as_bytes())?;
        link_extras(canonical_dir, &target_dir, &[])?;
    }
    Ok(())
}

#[cfg(test)]
mod generated_native_tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::PathBuf;

    use uze_core::capability::Resource;
    use uze_core::capability::{Capability, CapabilityKind};
    use uze_core::home::UzeHome;
    use uze_core::integration::IntegrationPort;

    use super::super::ClaudeIntegration;
    use super::super::plugin::ClaudeMarketplace;
    use crate::shared::marketplace;
    use uze_core::store::StoredPackage;

    fn generatable(package: &StoredPackage) -> bool {
        marketplace::generatable::<ClaudeMarketplace>(package)
    }

    fn generated_root(uze_home: &UzeHome) -> PathBuf {
        marketplace::generated_root::<ClaudeMarketplace>(uze_home)
    }

    fn generated_package_dir_for_id(uze_home: &UzeHome, package_id: &str) -> PathBuf {
        marketplace::generated_package_dir::<ClaudeMarketplace>(uze_home, package_id)
    }

    fn generated_exact_coverage(
        package: &StoredPackage,
        resources: &[&Resource],
    ) -> BTreeSet<String> {
        marketplace::generated_exact_coverage::<ClaudeMarketplace>(package, resources)
    }

    fn materialize_generated_package(
        uze_home: &UzeHome,
        package: &StoredPackage,
    ) -> uze_core::Result<PathBuf> {
        marketplace::materialize_generated_package::<ClaudeMarketplace>(uze_home, package)
    }

    fn remove_generated_package_by_id(
        uze_home: &UzeHome,
        package_id: &str,
    ) -> uze_core::Result<()> {
        marketplace::remove_generated_package::<ClaudeMarketplace>(uze_home, package_id)
    }

    fn temp_root(label: &str) -> PathBuf {
        uze_testkit::temp::scratch(label)
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
        // Default-policy skills stay byte-preserving whole-directory
        // symlinks; `skills/` itself is now a real envelope subdirectory.
        assert!(dir.join("skills/commit").is_symlink());
        assert_eq!(
            fs::read_link(dir.join("skills/commit")).unwrap(),
            pkg.root.join("skills/commit")
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
                path: pkg.root.join("extra/SKILL.md"),
                payload: Vec::new(),
            },
        );
        let resources = vec![&r_in, &r_out];
        let covered = generated_exact_coverage(&pkg, &resources);
        assert_eq!(covered, BTreeSet::from([r_in.identity()]));
        assert!(!covered.contains(&r_out.identity()));

        let uze_home = UzeHome::at(_root.join("uze"));
        let integration = ClaudeIntegration::new(_root.join("claude"), uze_home.clone());
        uze_core::state::record(
            &uze_home,
            integration.id(),
            uze_core::state::IntegrationRecord::default(),
        )
        .unwrap();
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
    // capability-based, not resource-count-based (ADR-013): a single Skill
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

    // --- ADR-030: invocation policy drives envelope materialization -----

    fn add_invoke(pkg: &StoredPackage, skill_dir: &str, invoke: &str, description: &str) {
        let dir = pkg.root.join("skills").join(skill_dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {skill_dir}\ndescription: {description}\ninvoke:\n{invoke}---\n\nSkill body.\n"),
        )
        .unwrap();
    }

    fn plain_skill(pkg: &StoredPackage, skill_dir: &str) -> Resource {
        Resource::from_package(
            pkg.id.clone(),
            pkg.root.clone(),
            Capability {
                kind: CapabilityKind::AgentSkill,
                path: pkg.root.join("skills").join(skill_dir).join("SKILL.md"),
                payload: fs::read(pkg.root.join("skills").join(skill_dir).join("SKILL.md"))
                    .unwrap(),
            },
        )
    }

    #[test]
    fn user_only_skill_is_materialized_with_the_claude_marker() {
        let (_root, pkg) = make_plain_package("policy-user-only", false);
        add_invoke(
            &pkg,
            "review",
            "  model: false\n  user: true\n",
            "Review code",
        );
        let uze_home = UzeHome::at(_root.join("uze"));
        let dir = materialize_generated_package(&uze_home, &pkg).unwrap();

        let skill_file = dir.join("skills/review/SKILL.md");
        assert!(
            !skill_file.is_symlink(),
            "a non-default policy Skill is materialized, never symlinked"
        );
        let content = fs::read_to_string(&skill_file).unwrap();
        assert!(
            content.contains("disable-model-invocation: true\n"),
            "model=false must translate into Claude's user-only marker: {content}"
        );
        assert!(content.contains("name: review\n"));
        assert!(content.contains("description: \"Review code\"\n"));
        assert!(content.ends_with("---\n\nSkill body.\n"));
        let _ = fs::remove_dir_all(_root);
    }

    #[test]
    fn model_only_skill_is_materialized_with_the_catalog_hiding_marker() {
        let (_root, pkg) = make_plain_package("policy-model-only", false);
        add_invoke(
            &pkg,
            "legacy",
            "  model: true\n  user: false\n",
            "Legacy knowledge",
        );
        let uze_home = UzeHome::at(_root.join("uze"));
        let dir = materialize_generated_package(&uze_home, &pkg).unwrap();
        let content = fs::read_to_string(dir.join("skills/legacy/SKILL.md")).unwrap();
        assert!(
            content.contains("user-invocable: false\n"),
            "user=false must translate into Claude's catalog-hiding marker: {content}"
        );
        let _ = fs::remove_dir_all(_root);
    }

    #[test]
    fn invalid_policy_skill_is_never_materialized_nor_covered() {
        let (_root, pkg) = make_plain_package("policy-invalid", false);
        add_invoke(
            &pkg,
            "dead",
            "  model: false\n  user: false\n",
            "Uninvokable",
        );
        let r_dead = plain_skill(&pkg, "dead");
        let uze_home = UzeHome::at(_root.join("uze"));
        let dir = materialize_generated_package(&uze_home, &pkg).unwrap();
        assert!(
            !dir.join("skills/dead").exists(),
            "a Skill nobody may invoke is never written into the generated envelope"
        );
        let covered = generated_exact_coverage(&pkg, &[&r_dead]);
        assert!(
            covered.is_empty(),
            "invalid policy is never claimed as covered"
        );
        let _ = fs::remove_dir_all(_root);
    }

    #[test]
    fn materialized_skill_keeps_auxiliary_files_referenced() {
        let (_root, pkg) = make_plain_package("policy-aux", false);
        add_invoke(&pkg, "deploy", "  model: false\n  user: true\n", "Deploy");
        fs::create_dir_all(pkg.root.join("skills/deploy/scripts")).unwrap();
        fs::write(pkg.root.join("skills/deploy/scripts/run.sh"), "#!/bin/sh\n").unwrap();
        let uze_home = UzeHome::at(_root.join("uze"));
        let dir = materialize_generated_package(&uze_home, &pkg).unwrap();
        assert!(
            dir.join("skills/deploy/scripts").is_symlink(),
            "auxiliary files stay referenced, never silently dropped"
        );
        assert!(dir.join("skills/deploy/SKILL.md").is_file());
        let _ = fs::remove_dir_all(_root);
    }

    /// A description crafted to look like it could inject a second
    /// frontmatter key — in particular one that would override the real
    /// marker with `false` — must stay inert, safely escaped inside its own
    /// quoted scalar. The generated marker line must appear exactly once,
    /// and it must be `true`.
    #[test]
    fn tricky_description_cannot_forge_or_duplicate_the_marker() {
        let (_root, pkg) = make_plain_package("policy-tricky", false);
        let tricky = "Has: a colon, \"quotes\", a\nnewline, and disable-model-invocation: false";
        let dir = pkg.root.join("skills/review");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: review\ndescription: {tricky}\ninvoke:\n  model: false\n  user: true\n---\n\nBody.\n"),
        )
        .unwrap();
        let uze_home = UzeHome::at(_root.join("uze"));
        materialize_generated_package(&uze_home, &pkg).unwrap();
        let generated = generated_root(&uze_home).join(pkg.id.as_str());
        let content = fs::read_to_string(generated.join("skills/review/SKILL.md")).unwrap();
        assert_eq!(
            content.matches("disable-model-invocation").count(),
            1,
            "the marker must appear exactly once, never forged or duplicated by the description"
        );
        assert!(content.contains("disable-model-invocation: true\n"));
        assert!(!content.contains("disable-model-invocation: false"));
        assert!(content.ends_with("---\n\nBody.\n"));
        let _ = fs::remove_dir_all(_root);
    }

    #[test]
    fn policy_materialization_is_deterministic_across_rebuilds() {
        let (_root, pkg) = make_plain_package("policy-deterministic", false);
        add_invoke(
            &pkg,
            "review",
            "  model: false\n  user: true\n",
            "Review code",
        );
        let uze_home = UzeHome::at(_root.join("uze"));
        materialize_generated_package(&uze_home, &pkg).unwrap();
        let first = fs::read(
            generated_root(&uze_home)
                .join(pkg.id.as_str())
                .join("skills/review/SKILL.md"),
        )
        .unwrap();
        materialize_generated_package(&uze_home, &pkg).unwrap();
        let second = fs::read(
            generated_root(&uze_home)
                .join(pkg.id.as_str())
                .join("skills/review/SKILL.md"),
        )
        .unwrap();
        assert_eq!(first, second);
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

    #[test]
    fn remove_generated_package_by_id_refuses_a_path_traversal_id() {
        // A receipt's package_id arrives from the ledger, so removal must
        // re-check it before joining it into a path: a tampered id must fail
        // without touching anything outside the generated root.
        let root = temp_root("malicious-receipt");
        fs::create_dir_all(&root).unwrap();
        let victim = temp_root("victim-dir");
        fs::create_dir_all(&victim).unwrap();

        let result =
            remove_generated_package_by_id(&UzeHome::at(root.join("uze")), "../victim-dir@local");
        assert!(result.is_err(), "traversal id must be refused");
        assert!(
            victim.exists(),
            "removal must never follow a traversal id outside the generated root"
        );

        // An id that cannot be parsed as qualified is refused too.
        let result = remove_generated_package_by_id(&UzeHome::at(root.join("uze")), "unqualified");
        assert!(result.is_err());

        // A legitimate id removes only its own generated directory.
        let home = UzeHome::at(root.join("uze"));
        home.ensure_layout().unwrap();
        let legit = generated_package_dir_for_id(&home, "flow@local");
        fs::create_dir_all(&legit).unwrap();
        let marker = legit.join("marker.txt");
        fs::write(&marker, "x").unwrap();
        remove_generated_package_by_id(&home, "flow@local").unwrap();
        assert!(!legit.exists());
        assert!(victim.exists(), "the sibling victim dir is untouched");

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(victim);
    }
}
