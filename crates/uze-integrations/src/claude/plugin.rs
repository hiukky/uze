//! Claude Code's native plugin marketplaces: the dialect UZE's derived
//! `.claude-plugin/marketplace.json` catalogues are written in, the
//! exact-coverage computation that tells UZE which resources a package's
//! own envelope already accounts for, and the `claude plugin` verbs.

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    fs,
    path::Path,
    path::PathBuf,
};

use uze_core::{
    Result,
    capability::Resource,
    integration::{AttachmentInspection, AttachmentState},
    skill::SkillInvocationPolicy,
    store::StoredPackage,
};

use crate::shared::marketplace::{
    MarketplaceDialect, Origin, manifest_fields, marketplace_entries,
};
use crate::shared::path::normalize_declared_relative_path;
use crate::shared::plan::blocked;
use crate::shared::process::{json, run_quiet};

/// The owner every catalogue UZE writes into Claude's marketplace UI
/// declares. Named once so the two documents that carry it cannot drift
/// into attributing UZE's local marketplace to someone else.
const MARKETPLACE_OWNER_URL: &str = "https://github.com/hiukky/uze";

pub(super) struct ClaudeMarketplace;

impl MarketplaceDialect for ClaudeMarketplace {
    const VENDOR: &'static str = "claude";
    const SETUP_NAME: &'static str = "claude";
    const CATALOGUE_NOUN: &'static str = "Claude marketplace";
    const ENVELOPE_DIR: &'static str = ".claude-plugin";
    const CATALOGUE_PATH: &'static str = ".claude-plugin/marketplace.json";
    const EXPLICIT_KIND: &'static str = "claude-plugin";
    const GENERATED_KIND: &'static str = "claude-plugin-generated";
    const EXPLICIT_EVIDENCE: &'static str = "The preserved external .claude-plugin/plugin.json is exposed through UZE's derived Claude marketplace. Claude Code owns Skill and MCP loading for this plugin, so UZE must not attach them a second time.";
    const GENERATED_EVIDENCE: &'static str = "No .claude-plugin/plugin.json was provided. UZE synthesizes one deterministically into a UZE-owned derived directory (never the Store) covering exactly the package's conventional skills/ directory and mcp.json-declared servers, published through a second, generated-only Claude marketplace.";

    fn catalogue_document(
        name: &str,
        display_name: &str,
        plugins: Vec<serde_json::Value>,
    ) -> serde_json::Value {
        serde_json::json!({
            "name": name,
            "owner": { "name": display_name, "url": MARKETPLACE_OWNER_URL },
            "plugins": plugins
        })
    }

    fn catalogue_entry(
        package: &StoredPackage,
        source: String,
        origin: Origin,
    ) -> serde_json::Value {
        let (description, version) = match origin {
            Origin::Explicit => manifest_fields(
                &package.root.join(".claude-plugin/plugin.json"),
                "UZE-managed Claude plugin",
            ),
            Origin::Generated => {
                manifest_fields(&package.manifest, super::generate::GENERATED_DESCRIPTION)
            }
        };
        serde_json::json!({
            "name": package.active_name.as_str(),
            "source": source,
            "description": description,
            "version": version
        })
    }

    fn explicit_coverage(package: &StoredPackage, resources: &[&Resource]) -> BTreeSet<String> {
        claude_exact_coverage(package, resources)
    }

    /// Claude's own frontmatter markers carry every valid combination, so
    /// only the invalid policy is left out.
    fn envelope_preserves(policy: SkillInvocationPolicy) -> bool {
        !policy.is_invalid()
    }

    fn materialize_envelope(package: &StoredPackage, dir: &Path) -> Result<()> {
        super::generate::materialize_envelope(package, dir)
    }

    fn generated_receipt_root(package: &StoredPackage, _envelope_dir: &Path) -> PathBuf {
        package.root.clone()
    }

    fn marketplace_exists(executable: &Path, home: &Path, root: &Path) -> bool {
        let Ok(listing) = json(
            executable,
            home,
            &["plugin", "marketplace", "list", "--json"],
            "claude",
        ) else {
            return false;
        };
        marketplace_entries(&listing).is_some_and(|entries| {
            entries.iter().any(|entry| {
                ["path", "installLocation"].iter().any(|key| {
                    entry
                        .get(*key)
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|candidate| Path::new(candidate) == root)
                })
            })
        })
    }

    fn add_marketplace(executable: &Path, home: &Path, root: &Path) -> Result<()> {
        let label = format!("claude plugin marketplace add {}", root.display());
        let args: Vec<&OsStr> = vec![
            OsStr::new("plugin"),
            OsStr::new("marketplace"),
            OsStr::new("add"),
            root.as_os_str(),
        ];
        run_quiet(executable, home, &label, &args)
    }

    /// Installed only when absent: Claude's behavior for re-installing an
    /// installed plugin is not relied on.
    fn install_plugin(executable: &Path, home: &Path, selector: &str) -> Result<()> {
        let installed = json(executable, home, &["plugin", "list", "--json"], "claude")
            .is_ok_and(|listing| installed_entry(&listing, selector).is_some());
        if installed {
            return Ok(());
        }
        run_quiet(
            executable,
            home,
            &format!("claude plugin install `{selector}`"),
            &["plugin", "install", selector],
        )
    }

    fn inspect_plugin(
        executable: &Path,
        home: &Path,
        selector: &str,
        marketplace_root: &Path,
        _detail: &BTreeMap<String, serde_json::Value>,
    ) -> AttachmentInspection {
        inspect_claude_plugin(executable, home, selector, marketplace_root)
    }

    fn remove_plugin(executable: &Path, home: &Path, selector: &str) -> Result<()> {
        run_quiet(
            executable,
            home,
            &format!("claude plugin uninstall {selector}"),
            &["plugin", "uninstall", selector],
        )
    }
}

fn installed_entry<'a>(
    listing: &'a serde_json::Value,
    selector: &str,
) -> Option<&'a serde_json::Value> {
    listing.as_array()?.iter().find(|entry| {
        entry
            .get("id")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|id| id == selector)
    })
}

pub(super) fn claude_exact_coverage(
    package: &StoredPackage,
    resources: &[&Resource],
) -> BTreeSet<String> {
    let manifest_path = package.root.join(".claude-plugin/plugin.json");
    let bytes = match fs::read(&manifest_path) {
        Ok(bytes) => bytes,
        Err(_) => return BTreeSet::new(),
    };
    let value: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(_) => return BTreeSet::new(),
    };

    let mut declared_skill_dirs: BTreeSet<String> = BTreeSet::new();
    if let Some(skills) = value.get("skills").and_then(serde_json::Value::as_array) {
        for entry in skills {
            let Some(raw) = entry.as_str() else {
                continue;
            };
            // `normalize_declared_relative_path` rejects absolute/escaping/
            // empty/dot-only declarations outright — see
            // `crate::shared::path`'s own doc comment for why this must
            // never strip a leading `/` and let it through as relative.
            let Some(normalized) = normalize_declared_relative_path(raw) else {
                continue;
            };
            declared_skill_dirs.insert(normalized.to_string_lossy().into_owned());
        }
    }

    let mut declared_mcp: BTreeSet<String> = BTreeSet::new();
    if let Some(servers) = value
        .get("mcpServers")
        .and_then(serde_json::Value::as_object)
    {
        for key in servers.keys() {
            declared_mcp.insert(key.clone());
        }
    }

    let mut provided = BTreeSet::new();
    for resource in resources {
        match resource.capability.kind {
            uze_core::capability::CapabilityKind::AgentSkill => {
                let Some(parent) = resource
                    .capability
                    .path
                    .strip_prefix(&package.root)
                    .ok()
                    .and_then(|p| p.parent())
                    .map(|p| p.to_string_lossy().into_owned())
                else {
                    continue;
                };
                // Parent is like "skills/skill-a"
                if !declared_skill_dirs.contains(&parent) {
                    continue;
                }
                // A path match alone is not enough (ADR-030 §13): UZE never
                // rewrites an author's explicit-envelope content, so the
                // delivered bytes are whatever the canonical SKILL.md itself
                // contains, and Claude's defaults are model+user. A Skill is
                // only honestly claimed as covered when its canonical
                // `invoke:` policy matches what the shipped bytes declare to
                // Claude:
                //   - default policy  → Claude's own defaults apply;
                //   - model=false     → the bytes must already carry
                //     `disable-model-invocation: true`;
                //   - user=false      → the bytes must already carry
                //     `user-invocable: false`;
                //   - invalid         → never covered (falls through to
                //     capability-level delivery, which refuses it).
                let policy = resource.skill_invocation();
                let preserved = if policy.is_invalid() {
                    false
                } else if !policy.model {
                    crate::shared::skill::has_disable_model_invocation(&resource.capability.payload)
                } else if !policy.user {
                    crate::shared::skill::has_user_invocable_false(&resource.capability.payload)
                } else {
                    true
                };
                if preserved {
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

fn inspect_claude_plugin(
    executable: &Path,
    command_home: &Path,
    selector: &str,
    marketplace_root: &Path,
) -> AttachmentInspection {
    // Verify marketplace still points at expected root.
    let marketplace_list = match json(
        executable,
        command_home,
        &["plugin", "marketplace", "list", "--json"],
        "claude",
    ) {
        Ok(value) => value,
        Err(reason) => return blocked(reason),
    };
    let marketplace_name = selector.rsplit_once('@').map(|(_, name)| name);
    let Some(marketplace_name) = marketplace_name else {
        return blocked("plugin receipt selector has no marketplace identity");
    };
    let Some(entries) = marketplace_entries(&marketplace_list) else {
        return blocked("Claude marketplace JSON has no marketplaces array");
    };
    let matching = entries.iter().find(|entry| {
        entry
            .get("name")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|name| name == marketplace_name)
    });
    let Some(matching) = matching else {
        return AttachmentInspection {
            state: AttachmentState::Missing,
            reason: "Claude marketplace is absent".to_owned(),
        };
    };
    let actual_root = matching
        .get("path")
        .or_else(|| matching.get("installLocation"))
        .or_else(|| matching.get("root"))
        .and_then(serde_json::Value::as_str);
    if actual_root.is_none_or(|root| Path::new(root) != marketplace_root) {
        return AttachmentInspection {
            state: AttachmentState::Drifted,
            reason: "Claude marketplace root differs from receipt".to_owned(),
        };
    }
    // Verify plugin installed.
    let plugins = match json(
        executable,
        command_home,
        &["plugin", "list", "--json"],
        "claude",
    ) {
        Ok(value) => value,
        Err(reason) => return blocked(reason),
    };
    if !plugins.is_array() {
        return blocked("Claude plugin JSON is not an array");
    }
    let Some(plugin) = installed_entry(&plugins, selector) else {
        return AttachmentInspection {
            state: AttachmentState::Missing,
            reason: "Claude plugin is not installed".to_owned(),
        };
    };
    let enabled = plugin.get("enabled").and_then(serde_json::Value::as_bool);
    if enabled == Some(false) {
        return AttachmentInspection {
            state: AttachmentState::Drifted,
            reason: "Claude plugin is disabled".to_owned(),
        };
    }
    // Existence of the cached installPath is deliberately not checked: the
    // cache is a Derived Artifact, not a source of truth. Marketplace root
    // identity + enabled + selector is sufficient for Matched; any
    // marketplace/selector mismatch already returned Drifted above.
    AttachmentInspection {
        state: AttachmentState::Matched,
        reason: "Claude native plugin matches receipt".to_owned(),
    }
}

#[cfg(test)]
mod claude_native_coverage_tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::PathBuf;

    use uze_core::capability::Resource;
    use uze_core::capability::{Capability, CapabilityKind};
    use uze_core::home::UzeHome;
    use uze_core::integration::IntegrationPort;

    use super::super::ClaudeIntegration;
    use super::ClaudeMarketplace;
    use crate::shared::marketplace::{Origin, catalogue_document};

    fn claude_catalogue_document(packages: &[uze_core::store::StoredPackage]) -> serde_json::Value {
        catalogue_document::<ClaudeMarketplace>(packages, Origin::Explicit)
    }
    use super::claude_exact_coverage;

    fn temp_root(label: &str) -> PathBuf {
        uze_testkit::temp::scratch(label)
    }

    fn make_package_with_plugin(
        label: &str,
        plugin_json: &str,
    ) -> (PathBuf, uze_core::store::StoredPackage) {
        let root = temp_root(label);
        let pkg_root = root.join("pkg");
        fs::create_dir_all(pkg_root.join(".claude-plugin")).unwrap();
        fs::write(pkg_root.join(".claude-plugin/plugin.json"), plugin_json).unwrap();
        fs::write(pkg_root.join("plugin.json"), r#"{"name":"test-pkg"}"#).unwrap();
        let id =
            uze_core::store::PackageId::from_plugin_name("test-pkg", &pkg_root.join("plugin.json"))
                .unwrap();
        let pkg = uze_core::store::StoredPackage {
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

    fn skill_resource(pkg: &uze_core::store::StoredPackage, skill: &str) -> Resource {
        skill_resource_with(pkg, skill, &format!("---\nname: {skill}\n---\n"))
    }

    fn skill_resource_with(
        pkg: &uze_core::store::StoredPackage,
        skill: &str,
        body: &str,
    ) -> Resource {
        let path = pkg.root.join(format!("skills/{skill}/SKILL.md"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, body).unwrap();
        Resource::from_package(
            pkg.id.clone(),
            pkg.root.clone(),
            Capability {
                kind: CapabilityKind::AgentSkill,
                path,
                payload: body.as_bytes().to_vec(),
            },
        )
    }

    fn mcp_resource(pkg: &uze_core::store::StoredPackage, name: &str) -> Resource {
        let path = pkg.root.join("mcp.json");
        let payload = serde_json::json!({"command":"node","args":[name]})
            .to_string()
            .into_bytes();
        Resource::from_package_named(
            pkg.id.clone(),
            pkg.root.clone(),
            Capability {
                kind: CapabilityKind::Mcp,
                path,
                payload,
            },
            name.to_owned(),
        )
    }

    /// ADR-030 §13: an explicit envelope Skill is only claimed as covered
    /// when the canonical `invoke:` policy is actually preserved by the
    /// vendor bytes the author shipped — UZE never rewrites
    /// explicit-envelope content, and Claude's defaults are model+user.
    #[test]
    fn explicit_user_only_skill_with_the_vendor_marker_is_covered() {
        let (_root, pkg) = make_package_with_plugin(
            "policy-marker",
            r#"{"name":"test-pkg","skills":["./skills/review"]}"#,
        );
        let r = skill_resource_with(
            &pkg,
            "review",
            "---\ndescription: Review\ndisable-model-invocation: true\ninvoke:\n  model: false\n  user: true\n---\nBody.\n",
        );
        let covered = claude_exact_coverage(&pkg, &[&r]);
        assert_eq!(covered, BTreeSet::from([r.identity()]));
        let _ = fs::remove_dir_all(_root);
    }

    #[test]
    fn explicit_user_only_skill_without_the_vendor_marker_is_not_covered() {
        let (_root, pkg) = make_package_with_plugin(
            "policy-no-marker",
            r#"{"name":"test-pkg","skills":["./skills/review"]}"#,
        );
        let r = skill_resource_with(
            &pkg,
            "review",
            "---\ndescription: Review\ninvoke:\n  model: false\n  user: true\n---\nBody.\n",
        );
        let covered = claude_exact_coverage(&pkg, &[&r]);
        assert!(
            covered.is_empty(),
            "a path-matched user-only Skill must not be claimed as covered without its own disable-model-invocation marker"
        );
        let _ = fs::remove_dir_all(_root);
    }

    #[test]
    fn explicit_model_only_skill_requires_user_invocable_false() {
        let (_root, pkg) = make_package_with_plugin(
            "policy-model-only",
            r#"{"name":"test-pkg","skills":["./skills/legacy"]}"#,
        );
        let covered_with = skill_resource_with(
            &pkg,
            "legacy",
            "---\nuser-invocable: false\ninvoke:\n  model: true\n  user: false\n---\nBody.\n",
        );
        assert_eq!(
            claude_exact_coverage(&pkg, &[&covered_with]),
            BTreeSet::from([covered_with.identity()])
        );
        let covered_without = skill_resource_with(
            &pkg,
            "legacy",
            "---\ninvoke:\n  model: true\n  user: false\n---\nBody.\n",
        );
        assert!(
            claude_exact_coverage(&pkg, &[&covered_without]).is_empty(),
            "model-only semantics degrade without the vendor's own user-invocable: false marker"
        );
        let _ = fs::remove_dir_all(_root);
    }

    #[test]
    fn explicit_invalid_policy_skill_is_never_covered() {
        let (_root, pkg) = make_package_with_plugin(
            "policy-invalid",
            r#"{"name":"test-pkg","skills":["./skills/dead"]}"#,
        );
        let r = skill_resource_with(
            &pkg,
            "dead",
            "---\ninvoke:\n  model: false\n  user: false\n---\nBody.\n",
        );
        let covered = claude_exact_coverage(&pkg, &[&r]);
        assert!(covered.is_empty());
        let _ = fs::remove_dir_all(_root);
    }

    #[test]
    fn envelope_declares_all_is_fully_covered() {
        let (_root, pkg) = make_package_with_plugin(
            "all",
            r#"{"name":"test-pkg","version":"0.1.0","skills":["./skills/a","./skills/b"],"mcpServers":{"mcp-x":{"command":"x"}}}"#,
        );
        let r_a = skill_resource(&pkg, "a");
        let r_b = skill_resource(&pkg, "b");
        let r_m = mcp_resource(&pkg, "mcp-x");
        let resources = vec![&r_a, &r_b, &r_m];
        let covered = claude_exact_coverage(&pkg, &resources);
        let expected: BTreeSet<String> = resources.iter().map(|r| r.identity()).collect();
        assert_eq!(covered, expected);
        let _ = fs::remove_dir_all(_root);
    }

    #[test]
    fn envelope_declares_subset_only_that_subset_is_covered() {
        let (_root, pkg) = make_package_with_plugin(
            "subset",
            r#"{"name":"test-pkg","version":"0.1.0","skills":["./skills/a"]}"#,
        );
        let r_a = skill_resource(&pkg, "a");
        let r_b = skill_resource(&pkg, "b");
        let r_m = mcp_resource(&pkg, "mcp-x");
        let resources = vec![&r_a, &r_b, &r_m];
        let covered = claude_exact_coverage(&pkg, &resources);
        assert_eq!(covered, BTreeSet::from([r_a.identity()]));
        assert!(!covered.contains(&r_b.identity()));
        assert!(!covered.contains(&r_m.identity()));
        let _ = fs::remove_dir_all(_root);
    }

    #[test]
    fn envelope_references_nonexistent_resource_is_ignored() {
        let (_root, pkg) = make_package_with_plugin(
            "ghost",
            r#"{"name":"test-pkg","skills":["./skills/ghost"]}"#,
        );
        let r_a = skill_resource(&pkg, "a");
        let resources = vec![&r_a];
        let covered = claude_exact_coverage(&pkg, &resources);
        assert!(covered.is_empty());
        let _ = fs::remove_dir_all(_root);
    }

    #[test]
    fn store_has_extra_resource_not_declared_is_not_covered() {
        let (_root, pkg) =
            make_package_with_plugin("extra", r#"{"name":"test-pkg","skills":["./skills/a"]}"#);
        let r_a = skill_resource(&pkg, "a");
        let r_extra = skill_resource(&pkg, "extra");
        // Also create a file for extra on disk already via skill_resource
        let resources = vec![&r_a, &r_extra];
        let covered = claude_exact_coverage(&pkg, &resources);
        assert_eq!(covered, BTreeSet::from([r_a.identity()]));
        let _ = fs::remove_dir_all(_root);
    }

    #[test]
    fn malformed_envelope_yields_empty_coverage_but_package_still_deliverable() {
        let (_root, pkg) = make_package_with_plugin("malformed", r#"{"name":"bad", "skills": [}"#);
        let r_a = skill_resource(&pkg, "a");
        let resources = vec![&r_a];
        let covered = claude_exact_coverage(&pkg, &resources);
        assert!(covered.is_empty());
        // package_exposure_plan still returns Some even with empty coverage
        let integration =
            ClaudeIntegration::new(_root.join("claude"), UzeHome::at(_root.join("uze")));
        let plan = integration.package_exposure_plan(&pkg, &resources);
        assert!(plan.is_some());
        assert!(plan.unwrap().provided_resource_identities.is_empty());
        let _ = fs::remove_dir_all(_root);
    }

    #[test]
    fn empty_coverage_when_no_skills_or_mcp_declared() {
        let (_root, pkg) =
            make_package_with_plugin("empty", r#"{"name":"test-pkg","version":"0.1.0"}"#);
        let r_a = skill_resource(&pkg, "a");
        let resources = vec![&r_a];
        let covered = claude_exact_coverage(&pkg, &resources);
        assert!(covered.is_empty());
        let _ = fs::remove_dir_all(_root);
    }

    #[test]
    fn duplicate_declarations_are_deduplicated() {
        let (_root, pkg) = make_package_with_plugin(
            "dup",
            r#"{"name":"test-pkg","skills":["./skills/a","./skills/a","./skills/a"]}"#,
        );
        let r_a = skill_resource(&pkg, "a");
        let resources = vec![&r_a];
        let covered = claude_exact_coverage(&pkg, &resources);
        assert_eq!(covered.len(), 1);
        let _ = fs::remove_dir_all(_root);
    }

    #[test]
    fn path_normalization_and_escape_attempts_are_ignored() {
        let (_root, pkg) = make_package_with_plugin(
            "escape",
            r#"{"name":"test-pkg","skills":["./skills/a","../escape","/absolute","./skills/b/../c"]}"#,
        );
        let r_a = skill_resource(&pkg, "a");
        let resources = vec![&r_a];
        let covered = claude_exact_coverage(&pkg, &resources);
        // Only ./skills/a is valid; others contain .. or absolute or are normalized with ..
        assert_eq!(covered, BTreeSet::from([r_a.identity()]));
        let _ = fs::remove_dir_all(_root);
    }

    /// Regression test for a real bug (see `crate::shared::path`'s own doc
    /// comment): the previous per-entry normalization stripped a leading
    /// `/` before checking `is_absolute()`, so `/skills/commit` silently
    /// became the relative declaration `skills/commit` and was ACCEPTED —
    /// wrongly covering a real skill that lives at exactly that path. The
    /// escape-attempts test above never caught this because none of its
    /// declared paths collided with a resource that actually exists;
    /// this one deliberately does.
    #[test]
    fn leading_slash_declaration_is_rejected_even_when_it_would_collide_with_a_real_skill() {
        let (_root, pkg) = make_package_with_plugin(
            "leading-slash-collision",
            r#"{"name":"test-pkg","skills":["/skills/commit"]}"#,
        );
        let commit = skill_resource(&pkg, "commit");
        let resources = vec![&commit];
        let covered = claude_exact_coverage(&pkg, &resources);
        assert!(
            covered.is_empty(),
            "`/skills/commit` is an absolute declaration and must never be silently \
             relativized into covering the real `skills/commit` resource: got {covered:?}"
        );
        let _ = fs::remove_dir_all(_root);
    }

    /// Same regression, guarding against whitespace padding defeating the
    /// absolute check the same way a bare leading `/` would.
    #[test]
    fn whitespace_padded_absolute_declaration_is_also_rejected() {
        let (_root, pkg) = make_package_with_plugin(
            "whitespace-padded-absolute",
            r#"{"name":"test-pkg","skills":["  /skills/commit  "]}"#,
        );
        let commit = skill_resource(&pkg, "commit");
        let resources = vec![&commit];
        let covered = claude_exact_coverage(&pkg, &resources);
        assert!(covered.is_empty(), "got {covered:?}");
        let _ = fs::remove_dir_all(_root);
    }

    #[test]
    fn mcp_coverage_exact_by_name() {
        let (_root, pkg) = make_package_with_plugin(
            "mcp",
            r#"{"name":"test-pkg","mcpServers":{"mcp-a":{"command":"a"},"mcp-b":{"command":"b"}}}"#,
        );
        let r_a = mcp_resource(&pkg, "mcp-a");
        let r_b = mcp_resource(&pkg, "mcp-b");
        let r_c = mcp_resource(&pkg, "mcp-c");
        let resources = vec![&r_a, &r_b, &r_c];
        let covered = claude_exact_coverage(&pkg, &resources);
        assert_eq!(covered, BTreeSet::from([r_a.identity(), r_b.identity()]));
        let _ = fs::remove_dir_all(_root);
    }

    #[test]
    fn marketplace_republish_is_deterministic() {
        let (_root, pkg1) = make_package_with_plugin(
            "pub1",
            r#"{"name":"test-pkg","version":"1.2.3","description":"desc"}"#,
        );
        // Need distinct package ids for two packages
        let pkg1_id = pkg1.id.clone();
        let pkg1_root = pkg1.root.clone();
        // Second package
        let root2 = temp_root("pub2");
        let pkg2_root = root2.join("pkg2");
        fs::create_dir_all(pkg2_root.join(".claude-plugin")).unwrap();
        fs::write(
            pkg2_root.join(".claude-plugin/plugin.json"),
            r#"{"name":"pkg-two","version":"0.1.0"}"#,
        )
        .unwrap();
        fs::write(pkg2_root.join("plugin.json"), r#"{"name":"pkg-two"}"#).unwrap();
        let pkg2 = uze_core::store::StoredPackage {
            id: uze_core::store::PackageId::from_plugin_name(
                "pkg-two",
                &pkg2_root.join("plugin.json"),
            )
            .unwrap(),
            active_name: "pkg-two".to_owned(),
            root: pkg2_root.clone(),
            manifest: pkg2_root.join("plugin.json"),
            provenance: uze_core::acquisition::Provenance {
                requested: uze_core::acquisition::PackageSource::Local {
                    path: PathBuf::from("/tmp/fake2"),
                },
                resolved: uze_core::acquisition::ResolvedSource::Local {
                    path: PathBuf::from("/tmp/fake2"),
                },
            },
        };
        let doc1 = claude_catalogue_document(&[pkg1.clone(), pkg2.clone()]);
        let doc1_again = claude_catalogue_document(&[pkg1, pkg2]);
        assert_eq!(doc1, doc1_again);
        assert_eq!(doc1["name"], "uze-local");
        assert_eq!(
            doc1["owner"]["url"], "https://github.com/hiukky/uze",
            "the owner Claude's marketplace UI shows is this project, not another"
        );
        assert_eq!(doc1["plugins"].as_array().unwrap().len(), 2);
        let _ = fs::remove_dir_all(_root);
        let _ = fs::remove_dir_all(root2);
        let _ = pkg1_id;
        let _ = pkg1_root;
    }

    #[test]
    fn fallback_without_envelope_returns_none() {
        let root = temp_root("fallback");
        let pkg_root = root.join("pkg");
        fs::create_dir_all(&pkg_root).unwrap();
        fs::write(pkg_root.join("plugin.json"), r#"{"name":"no-claude"}"#).unwrap();
        let pkg = uze_core::store::StoredPackage {
            id: uze_core::store::PackageId::from_plugin_name(
                "no-claude",
                &pkg_root.join("plugin.json"),
            )
            .unwrap(),
            active_name: "no-claude".to_owned(),
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
        let integration =
            ClaudeIntegration::new(root.join("claude"), UzeHome::at(root.join("uze")));
        let resources: Vec<&Resource> = vec![];
        assert!(
            integration
                .package_exposure_plan(&pkg, &resources)
                .is_none()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn package_receipt_is_integration_owned_and_uncovered_fallback_remains() {
        let (_root, pkg) =
            make_package_with_plugin("receipt", r#"{"name":"test-pkg","skills":["./skills/a"]}"#);
        let r_a = skill_resource(&pkg, "a");
        let r_b = skill_resource(&pkg, "b");
        let resources = vec![&r_a, &r_b];
        let integration =
            ClaudeIntegration::new(_root.join("claude"), UzeHome::at(_root.join("uze")));
        let plan = integration
            .package_exposure_plan(&pkg, &resources)
            .expect("should have plan");
        assert_eq!(plan.provided_resource_identities.len(), 1);
        assert!(plan.provided_resource_identities.contains(&r_a.identity()));
        assert!(!plan.provided_resource_identities.contains(&r_b.identity()));
        // r_b should still be attachable via capability fallback
        uze_core::state::record(
            &UzeHome::at(_root.join("uze")),
            integration.id(),
            uze_core::state::IntegrationRecord::default(),
        )
        .unwrap();
        let plan_b = integration.exposure_plan(&r_b);
        assert!(!matches!(
            plan_b.mechanism,
            uze_core::exposure::ExposureMechanism::Unsupported { .. }
        ));
        let _ = fs::remove_dir_all(_root);
    }
}
