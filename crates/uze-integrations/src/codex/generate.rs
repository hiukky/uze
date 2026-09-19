//! Codex's GENERATED native plugin envelope: for a canonical UZE package
//! that ships no explicit `.codex-plugin/plugin.json`, this module
//! deterministically synthesizes one into a UZE-owned derived directory —
//! never into the Store — so the package can still install as one native
//! Codex plugin instead of decomposing into per-capability shims.
//!
//! Generated Native Package sits between Explicit Native Package and Native
//! Capability in the delivery hierarchy (ADR-013 §3), exactly as it does
//! for Claude
//! (`crate::claude::generate`) — this module mirrors that one's shape and
//! discipline, adapted to Codex's own manifest format: `skills` names one
//! directory (not an inline list), and `mcpServers` names one external file
//! rather than embedding servers inline.

use std::{fs, path::Path};

use uze_core::{Result, UzeError, store::StoredPackage};

use crate::shared::marketplace::manifest_fields;
use crate::shared::skill::write_file;

/// Writes the generated `.codex-plugin/plugin.json` and the surfaces it
/// declares into a fresh envelope directory. Name/version/description come
/// from the package's own canonical `plugin.json`, never invented. `skills`
/// is the fixed `"./skills/"` convention (the shape of the explicit-envelope
/// fixture, `tests/_fixtures/foreign/codex/native-plugin/.codex-plugin/plugin.json`);
/// `mcpServers` names a `.mcp.json` sibling, written only when the package
/// has a root `mcp.json` to project.
///
/// The envelope carries real bytes, never symlinks into the Store. `codex
/// plugin add` stages its own copy of the envelope under
/// `~/.codex/plugins/cache/` and that copy does not follow symlinks: a
/// symlinked skill directory or `.mcp.json` is simply absent from the cache,
/// so the skill never reaches the model and the server never reaches
/// `codex mcp list` (verified against codex-cli 0.149.0 through 0.152.1).
/// Rebuilding wholesale from the Store on every materialization is what
/// keeps the envelope non-authoritative; `codex plugin add` re-stages the
/// cache from it even for an unchanged version (verified against 0.152.1).
pub(super) fn materialize_envelope(package: &StoredPackage, dir: &Path) -> Result<()> {
    let (description, version) = manifest_fields(
        &package.manifest,
        "UZE-managed Codex plugin, generated from a vendor-neutral package.",
    );
    let mut manifest = serde_json::json!({
        "name": package.active_name.as_str(),
        "version": version,
        "description": description,
    });
    if package.root.join("skills").is_dir() {
        manifest["skills"] = serde_json::json!("./skills/");
    }
    let mcp_source = package.root.join("mcp.json");
    if mcp_source.is_file() {
        manifest["mcpServers"] = serde_json::json!("./.mcp.json");
    }
    let plugin_dir = dir.join(".codex-plugin");
    fs::create_dir_all(&plugin_dir).map_err(|source| UzeError::Write {
        path: plugin_dir.clone(),
        source,
    })?;
    write_file(
        &plugin_dir.join("plugin.json"),
        &serde_json::to_vec_pretty(&manifest).expect("generated manifest is serializable"),
    )?;
    let package_root = fs::canonicalize(&package.root).map_err(|source| UzeError::Read {
        path: package.root.clone(),
        source,
    })?;
    materialize_generated_skills(package, &package_root, dir)?;
    if mcp_source.is_file() {
        mirror_file(&mcp_source, &dir.join(".mcp.json"))?;
    }
    Ok(())
}

/// Materializes the generated envelope's `skills/` surface (ADR-030 §13).
/// A default-policy Skill is mirrored byte for byte; a user-only Skill gets
/// its own UZE-owned directory with a materialized SKILL.md (canonical
/// name/description/body) plus Codex's `agents/openai.yaml`
/// invocation-policy sidecar — the same Derived Artifact discipline as the
/// capability-level wrapper, applied at package level. Invalid or
/// model-only Skills are never materialized here and are excluded from
/// coverage.
fn materialize_generated_skills(
    package: &StoredPackage,
    package_root: &Path,
    envelope_dir: &Path,
) -> Result<()> {
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
        if policy.is_default() {
            mirror_tree(canonical_dir, &target_dir, package_root)?;
            continue;
        }
        materialize_user_only_skill_dir(
            &target_dir,
            canonical_dir,
            package_root,
            &skill_name,
            &policy,
        )?;
    }
    Ok(())
}

/// Writes one materialized user-only Skill directory: SKILL.md with the
/// canonical identity/description/body plus Codex's policy sidecar, with
/// every other canonical file mirrored beside it.
fn materialize_user_only_skill_dir(
    target_dir: &Path,
    canonical_dir: &Path,
    package_root: &Path,
    skill_name: &str,
    policy: &uze_core::skill::SkillInvocationPolicy,
) -> Result<()> {
    fs::create_dir_all(target_dir).map_err(|source_error| UzeError::Write {
        path: target_dir.to_path_buf(),
        source: source_error,
    })?;
    let bytes = fs::read(canonical_dir.join("SKILL.md")).map_err(|error| UzeError::Read {
        path: canonical_dir.join("SKILL.md"),
        source: error,
    })?;
    let name = crate::shared::skill::frontmatter_value(&bytes, "name")
        .unwrap_or_else(|| skill_name.to_owned());
    crate::shared::skill::write_file(
        &target_dir.join("SKILL.md"),
        crate::shared::skill::render_skill_wrapper(&name, &bytes, &[]).as_bytes(),
    )?;
    if !policy.model {
        crate::shared::skill::write_explicit_only_sidecar(target_dir)?;
    }
    for entry in sorted_entries(canonical_dir)? {
        let name = entry.file_name();
        if name == "SKILL.md" {
            continue;
        }
        mirror_entry(&entry.path(), &target_dir.join(&name), package_root)?;
    }
    Ok(())
}

/// Mirrors one canonical directory into the envelope as real files and
/// directories. A symlink inside the package is resolved to the bytes it
/// names when it stays inside the package root; a symlinked directory is
/// left out, exactly as package discovery never descends into one (the
/// containment invariant), and a link that escapes the package or dangles
/// is refused by name rather than silently dropped — a silent drop is the
/// failure this mirror exists to prevent.
fn mirror_tree(source: &Path, destination: &Path, package_root: &Path) -> Result<()> {
    fs::create_dir_all(destination).map_err(|source_error| UzeError::Write {
        path: destination.to_path_buf(),
        source: source_error,
    })?;
    for entry in sorted_entries(source)? {
        mirror_entry(
            &entry.path(),
            &destination.join(entry.file_name()),
            package_root,
        )?;
    }
    Ok(())
}

fn mirror_entry(source: &Path, destination: &Path, package_root: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(source).map_err(|source_error| UzeError::Read {
        path: source.to_path_buf(),
        source: source_error,
    })?;
    if metadata.is_dir() {
        return mirror_tree(source, destination, package_root);
    }
    if metadata.is_file() {
        return mirror_file(source, destination);
    }
    if !metadata.file_type().is_symlink() {
        return Err(UzeError::ExposureUnavailable(format!(
            "the Codex envelope cannot carry special filesystem entry `{}`",
            source.display()
        )));
    }
    let resolved = fs::canonicalize(source).map_err(|source_error| UzeError::Read {
        path: source.to_path_buf(),
        source: source_error,
    })?;
    if !resolved.starts_with(package_root) {
        return Err(UzeError::ExposureUnavailable(format!(
            "the Codex envelope refuses symlink `{}`: it resolves outside the package",
            source.display()
        )));
    }
    if resolved.is_dir() {
        return Ok(());
    }
    mirror_file(&resolved, destination)
}

/// `fs::copy` carries the permission bits, so a skill's helper script stays
/// executable in the envelope.
fn mirror_file(source: &Path, destination: &Path) -> Result<()> {
    fs::copy(source, destination)
        .map(|_| ())
        .map_err(|source_error| UzeError::Write {
            path: destination.to_path_buf(),
            source: source_error,
        })
}

/// Sorted so the envelope is written in one deterministic order on every
/// rebuild.
fn sorted_entries(dir: &Path) -> Result<Vec<fs::DirEntry>> {
    let mut entries = fs::read_dir(dir)
        .map_err(|source_error| UzeError::Read {
            path: dir.to_path_buf(),
            source: source_error,
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|source_error| UzeError::Read {
            path: dir.to_path_buf(),
            source: source_error,
        })?;
    entries.sort_by_key(fs::DirEntry::file_name);
    Ok(entries)
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

    use super::super::CodexIntegration;
    use super::super::plugin::CodexMarketplace;
    use super::*;
    use crate::shared::marketplace;
    use uze_core::store::StoredPackage;

    fn generatable(package: &StoredPackage) -> bool {
        marketplace::generatable::<CodexMarketplace>(package)
    }

    fn generated_root(uze_home: &UzeHome) -> PathBuf {
        marketplace::generated_root::<CodexMarketplace>(uze_home)
    }

    fn generated_exact_coverage(
        package: &StoredPackage,
        resources: &[&Resource],
    ) -> BTreeSet<String> {
        marketplace::generated_exact_coverage::<CodexMarketplace>(package, resources)
    }

    fn materialize_generated_package(
        uze_home: &UzeHome,
        package: &StoredPackage,
    ) -> uze_core::Result<PathBuf> {
        marketplace::materialize_generated_package::<CodexMarketplace>(uze_home, package)
    }

    fn remove_generated_package_by_id(
        uze_home: &UzeHome,
        package_id: &str,
    ) -> uze_core::Result<()> {
        marketplace::remove_generated_package::<CodexMarketplace>(uze_home, package_id)
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
        assert!(
            dir.starts_with(uze_home.runtime_dir()),
            "a generated package is produced again from the Store and the \
             Engine alone, so it belongs with what UZE generates rather than \
             with the records"
        );
        assert!(dir.join(".codex-plugin/plugin.json").is_file());
        // Codex stages the envelope into its plugin cache without following
        // symlinks, so a default-policy Skill and the MCP file are real
        // bytes — byte-identical to the Store's.
        assert_eq!(
            fs::read(dir.join("skills/commit/SKILL.md")).unwrap(),
            fs::read(pkg.root.join("skills/commit/SKILL.md")).unwrap()
        );
        assert_eq!(
            fs::read(dir.join(".mcp.json")).unwrap(),
            fs::read(pkg.root.join("mcp.json")).unwrap()
        );
        assert!(
            symlinks_under(&dir).is_empty(),
            "the envelope must be self-contained: {:?}",
            symlinks_under(&dir)
        );
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join(".codex-plugin/plugin.json")).unwrap())
                .unwrap();
        assert_eq!(manifest["skills"], "./skills/");
        assert_eq!(manifest["mcpServers"], "./.mcp.json");
        let _ = fs::remove_dir_all(_root);
    }

    /// The envelope mirrors a Skill's supporting files too, keeping a helper
    /// script executable, and resolves a file symlink the package keeps
    /// inside itself to the bytes it names — Codex's cache copy would drop
    /// the link.
    #[test]
    fn envelope_mirrors_supporting_files_and_resolves_in_package_symlinks() {
        use std::os::unix::fs::PermissionsExt;
        let (_root, pkg) = make_plain_package("mirror-support", false);
        let scripts = pkg.root.join("skills/commit/scripts");
        fs::create_dir_all(&scripts).unwrap();
        fs::write(scripts.join("run.sh"), "#!/bin/sh\necho run\n").unwrap();
        fs::set_permissions(scripts.join("run.sh"), fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(pkg.root.join("shared.txt"), "shared bytes").unwrap();
        std::os::unix::fs::symlink(
            pkg.root.join("shared.txt"),
            pkg.root.join("skills/commit/shared.txt"),
        )
        .unwrap();
        let uze_home = UzeHome::at(_root.join("uze"));
        let dir = materialize_generated_package(&uze_home, &pkg).unwrap();
        let script = dir.join("skills/commit/scripts/run.sh");
        assert_eq!(
            fs::read_to_string(&script).unwrap(),
            "#!/bin/sh\necho run\n"
        );
        assert_ne!(
            fs::metadata(&script).unwrap().permissions().mode() & 0o111,
            0,
            "a helper script stays executable in the envelope"
        );
        let shared = dir.join("skills/commit/shared.txt");
        assert!(!shared.is_symlink());
        assert_eq!(fs::read_to_string(&shared).unwrap(), "shared bytes");
        assert!(symlinks_under(&dir).is_empty());
        let _ = fs::remove_dir_all(_root);
    }

    /// A symlink that escapes the package is refused by name — never
    /// followed into foreign bytes, never silently dropped.
    #[test]
    fn envelope_refuses_a_symlink_that_escapes_the_package() {
        let (_root, pkg) = make_plain_package("mirror-escape", false);
        fs::write(_root.join("outside.txt"), "not package bytes").unwrap();
        std::os::unix::fs::symlink(
            _root.join("outside.txt"),
            pkg.root.join("skills/commit/outside.txt"),
        )
        .unwrap();
        let uze_home = UzeHome::at(_root.join("uze"));
        let error = materialize_generated_package(&uze_home, &pkg).unwrap_err();
        assert!(
            error.to_string().contains("outside.txt"),
            "the refusal names the link: {error}"
        );
        let _ = fs::remove_dir_all(_root);
    }

    fn symlinks_under(dir: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(current) = stack.pop() {
            for entry in fs::read_dir(&current).unwrap().flatten() {
                let path = entry.path();
                let metadata = fs::symlink_metadata(&path).unwrap();
                if metadata.file_type().is_symlink() {
                    found.push(path);
                } else if metadata.is_dir() {
                    stack.push(path);
                }
            }
        }
        found
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
                path: pkg.root.join("extra/SKILL.md"),
                payload: Vec::new(),
            },
        );
        let resources = vec![&r_in, &r_out];
        let covered = generated_exact_coverage(&pkg, &resources);
        assert_eq!(covered, BTreeSet::from([r_in.identity()]));
        assert!(!covered.contains(&r_out.identity()));

        let uze_home = UzeHome::at(_root.join("uze"));
        let integration = CodexIntegration::new(_root.join("agents"), uze_home.clone());
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
