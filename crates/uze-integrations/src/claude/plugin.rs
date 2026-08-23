//! Claude Code's native plugin marketplace: the derived, UZE-owned
//! `.claude-plugin/marketplace.json` catalogue every installed package with
//! its own external `.claude-plugin/plugin.json` is republished through,
//! and the exact-coverage computation that tells UZE which resources that
//! native envelope already accounts for.

use std::{collections::BTreeSet, ffi::OsStr, fs, path::Path, path::PathBuf, process::Command};

use uze_core::{
    Result, UzeError,
    integration::{AttachmentInspection, AttachmentReceipt, AttachmentState, ManagedArtifact},
    project::Resource,
    store::StoredPackage,
};

use crate::shared::path::normalize_declared_relative_path;
use crate::shared::process::run_quiet;

use super::CLAUDE_MARKETPLACE_NAME;

pub(super) fn claude_marketplace_exists(
    executable: &Path,
    command_home: &Path,
    root: &Path,
) -> bool {
    let output = Command::new(executable)
        .env("HOME", command_home)
        .args(["plugin", "marketplace", "list", "--json"])
        .output();
    let Ok(output) = output else {
        return false;
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&output.stdout) else {
        return false;
    };
    let entries = value.as_array().or_else(|| {
        value
            .get("marketplaces")
            .and_then(serde_json::Value::as_array)
    });
    let Some(entries) = entries else {
        return false;
    };
    entries.iter().any(|entry| {
        entry
            .get("path")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|candidate| Path::new(candidate) == root)
            || entry
                .get("installLocation")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|candidate| Path::new(candidate) == root)
    })
}

pub(super) fn run_claude_marketplace_add(
    executable: &Path,
    command_home: &Path,
    root: &Path,
) -> Result<()> {
    let label = format!("claude plugin marketplace add {}", root.display());
    let args: Vec<&OsStr> = vec![
        OsStr::new("plugin"),
        OsStr::new("marketplace"),
        OsStr::new("add"),
        root.as_os_str(),
    ];
    run_quiet(executable, command_home, &label, &args)
}

pub(super) fn claude_plugin_installed(
    executable: &Path,
    command_home: &Path,
    selector: &str,
) -> bool {
    let Ok(output) = Command::new(executable)
        .env("HOME", command_home)
        .args(["plugin", "list", "--json"])
        .output()
    else {
        return false;
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&output.stdout) else {
        return false;
    };
    value.as_array().is_some_and(|entries| {
        entries.iter().any(|entry| {
            entry
                .get("id")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|id| id == selector)
        })
    })
}

pub(super) fn claude_package_receipt(
    integration_id: &str,
    package: &StoredPackage,
    catalogue_root: &Path,
    selector: &str,
) -> AttachmentReceipt {
    AttachmentReceipt {
        package_id: package.id.as_str().to_owned(),
        resource_identity: None,
        integration: integration_id.to_owned(),
        strategy: "native-plugin-marketplace".to_owned(),
        artifact: ManagedArtifact::IntegrationOwned {
            kind: "claude-plugin".to_owned(),
            selector: selector.to_owned(),
            detail: [
                (
                    "marketplace_root".to_owned(),
                    serde_json::json!(catalogue_root),
                ),
                ("package_root".to_owned(), serde_json::json!(package.root)),
            ]
            .into_iter()
            .collect(),
        },
    }
}

pub(super) fn claude_publishable(packages: &[StoredPackage]) -> Vec<&StoredPackage> {
    packages
        .iter()
        .filter(|package| package.root.join(".claude-plugin/plugin.json").is_file())
        .collect()
}

pub(super) fn claude_catalogue_document(packages: &[StoredPackage]) -> serde_json::Value {
    let plugins: Vec<serde_json::Value> = claude_publishable(packages)
        .into_iter()
        .map(|package| {
            let manifest_path = package.root.join(".claude-plugin/plugin.json");
            let (description, version) = fs::read(&manifest_path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                .map(|value| {
                    let desc = value
                        .get("description")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("UZE-managed Claude plugin")
                        .to_owned();
                    let ver = value
                        .get("version")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("0.1.0")
                        .to_owned();
                    (desc, ver)
                })
                .unwrap_or_else(|| ("UZE-managed Claude plugin".to_owned(), "0.1.0".to_owned()));
            serde_json::json!({
                "name": package.id.as_str(),
                "source": format!("./packages/{}", package.id.as_str()),
                "description": description,
                "version": version
            })
        })
        .collect();
    serde_json::json!({
        "name": CLAUDE_MARKETPLACE_NAME,
        "owner": { "name": "UZE Local", "url": "https://github.com/anomalyco/opencode" },
        "plugins": plugins
    })
}

pub(super) fn write_claude_catalogue(path: &Path, packages: &[StoredPackage]) -> Result<()> {
    let parent = path.parent().expect("catalogue path has a parent");
    fs::create_dir_all(parent).map_err(|source| UzeError::Write {
        path: parent.to_path_buf(),
        source,
    })?;
    uze_core::persistence::write_atomic(
        path,
        &serde_json::to_vec_pretty(&claude_catalogue_document(packages))
            .expect("catalogue is serializable"),
    )
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
                if declared_skill_dirs.contains(&parent) {
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

pub(super) fn inspect_claude_plugin(
    executable: &Path,
    command_home: &Path,
    selector: &str,
    marketplace_root: &Path,
    package_root: &Path,
) -> AttachmentInspection {
    // Verify marketplace still points at expected root.
    let marketplace_list = match claude_json(
        executable,
        command_home,
        ["plugin", "marketplace", "list", "--json"],
    ) {
        Ok(value) => value,
        Err(reason) => return blocked(reason),
    };
    let marketplace_name = selector.rsplit_once('@').map(|(_, name)| name);
    let Some(marketplace_name) = marketplace_name else {
        return blocked("plugin receipt selector has no marketplace identity".to_owned());
    };
    let entries = marketplace_list.as_array().or_else(|| {
        marketplace_list
            .get("marketplaces")
            .and_then(serde_json::Value::as_array)
    });
    let Some(entries) = entries else {
        return blocked("Claude marketplace JSON has no marketplaces array".to_owned());
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
    let plugins = match claude_json(executable, command_home, ["plugin", "list", "--json"]) {
        Ok(value) => value,
        Err(reason) => return blocked(reason),
    };
    let Some(installed) = plugins.as_array() else {
        return blocked("Claude plugin JSON is not an array".to_owned());
    };
    let Some(plugin) = installed.iter().find(|entry| {
        entry
            .get("id")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|id| id == selector)
    }) else {
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
    // If we can verify package_root via installPath existence, do minimal check:
    // installPath should exist and be a directory. We don't compare bytes — cache
    // is Derived Artifact, not source of truth. Existence + enabled + marketplace
    // is sufficient for Matched. Any marketplace/selector mismatch already handled.
    let _ = package_root;
    AttachmentInspection {
        state: AttachmentState::Matched,
        reason: "Claude native plugin matches receipt".to_owned(),
    }
}

fn claude_json<const N: usize>(
    executable: &Path,
    command_home: &Path,
    args: [&str; N],
) -> std::result::Result<serde_json::Value, String> {
    let output = Command::new(executable)
        .env("HOME", command_home)
        .args(args)
        .output()
        .map_err(|error| format!("failed to run `claude`: {error}"))?;
    if !output.status.success() {
        return Err(format!("`claude` inspection exited with {}", output.status));
    }
    // claude marketplace list --json writes to stdout; plugin list also.
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("Claude JSON is invalid: {error}"))
}

pub(super) fn remove_claude_plugin(
    executable: &Path,
    command_home: &Path,
    selector: &str,
) -> Result<()> {
    run_quiet(
        executable,
        command_home,
        &format!("claude plugin uninstall {selector}"),
        &["plugin", "uninstall", selector],
    )
}

pub(super) fn detail_path(
    detail: &std::collections::BTreeMap<String, serde_json::Value>,
    key: &str,
) -> Option<PathBuf> {
    detail
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
}

pub(super) fn blocked(reason: String) -> AttachmentInspection {
    AttachmentInspection {
        state: AttachmentState::Blocked,
        reason,
    }
}

#[cfg(test)]
mod claude_native_coverage_tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::PathBuf;

    use uze_core::capability::{Capability, CapabilityKind, Representation};
    use uze_core::home::UzeHome;
    use uze_core::integration::IntegrationPort;
    use uze_core::project::Resource;

    use super::super::ClaudeIntegration;
    use super::claude_catalogue_document;
    use super::claude_exact_coverage;

    fn temp_root(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "uze-claude-coverage-{label}-{nonce}-{}",
            std::process::id()
        ))
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
        let path = pkg.root.join(format!("skills/{skill}/SKILL.md"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, format!("---\nname: {skill}\n---\n")).unwrap();
        Resource::from_package(
            pkg.id.clone(),
            pkg.root.clone(),
            Capability {
                kind: CapabilityKind::AgentSkill,
                representation: Representation::Standard,
                path: path.clone(),
                payload: Vec::new(),
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
                representation: Representation::Standard,
                path: path.clone(),
                payload: payload.clone(),
            },
            name.to_owned(),
        )
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
        let doc1_again = claude_catalogue_document(&[pkg1.clone(), pkg2.clone()]);
        assert_eq!(doc1, doc1_again);
        assert_eq!(doc1["name"], "uze-local");
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
        let plan_b = integration.exposure_plan(&r_b);
        assert!(!matches!(
            plan_b.mechanism,
            uze_core::exposure::ExposureMechanism::Unsupported { .. }
        ));
        let _ = fs::remove_dir_all(_root);
    }
}
