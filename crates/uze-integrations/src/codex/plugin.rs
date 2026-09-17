//! Codex's native plugin marketplaces: the dialect UZE's derived
//! `.agents/plugins/marketplace.json` catalogues are written in, the
//! exact-coverage computation for a package's own `.codex-plugin/plugin.json`,
//! and the `codex plugin` verbs and their inspection JSON.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use uze_core::{
    Result,
    capability::Resource,
    integration::{AttachmentInspection, AttachmentState},
    skill::SkillInvocationPolicy,
    store::StoredPackage,
};

use crate::shared::marketplace::{MarketplaceDialect, Origin, detail_path, marketplace_entries};
use crate::shared::path::normalize_declared_relative_path;
use crate::shared::plan::blocked;
use crate::shared::process::{json, run_quiet};

pub(super) struct CodexMarketplace;

impl MarketplaceDialect for CodexMarketplace {
    const VENDOR: &'static str = "codex";
    const SETUP_NAME: &'static str = "codex";
    const CATALOGUE_NOUN: &'static str = "Codex catalogue";
    const ENVELOPE_DIR: &'static str = ".codex-plugin";
    const CATALOGUE_PATH: &'static str = ".agents/plugins/marketplace.json";
    const EXPLICIT_KIND: &'static str = "marketplace-plugin";
    const GENERATED_KIND: &'static str = "marketplace-plugin-generated";
    const EXPLICIT_EVIDENCE: &'static str = "The preserved external .codex-plugin/plugin.json is exposed through UZE's generated, standard Codex local marketplace catalog for exactly the skills/mcpServers it declares; undeclared resources fall back to individual attachment.";
    const GENERATED_EVIDENCE: &'static str = "No .codex-plugin/plugin.json was provided. UZE synthesizes one deterministically into a UZE-owned derived directory (never the Store) covering exactly the package's conventional skills/ directory and mcp.json-declared servers, published through a second, generated-only Codex marketplace.";

    fn catalogue_document(
        name: &str,
        display_name: &str,
        plugins: Vec<serde_json::Value>,
    ) -> serde_json::Value {
        serde_json::json!({
            "name": name,
            "interface": { "displayName": display_name },
            "plugins": plugins,
        })
    }

    /// Codex requires `policy`/`category` on every entry, and reads
    /// `source.path` relative to the marketplace root.
    fn catalogue_entry(
        package: &StoredPackage,
        source: String,
        _origin: Origin,
    ) -> serde_json::Value {
        serde_json::json!({
            "name": package.active_name.as_str(),
            "source": { "source": "local", "path": source },
            "policy": { "installation": "AVAILABLE", "authentication": "ON_INSTALL" },
            "category": "Developer tools"
        })
    }

    fn explicit_coverage(package: &StoredPackage, resources: &[&Resource]) -> BTreeSet<String> {
        codex_exact_coverage(package, resources)
    }

    /// The `agents/openai.yaml` sidecar covers `model=false` and the default
    /// needs nothing; `user=false` cannot be enforced anywhere on Codex.
    fn envelope_preserves(policy: SkillInvocationPolicy) -> bool {
        !policy.is_invalid() && !(policy.model && !policy.user)
    }

    fn materialize_envelope(package: &StoredPackage, dir: &Path) -> Result<()> {
        super::generate::materialize_envelope(package, dir)
    }

    /// Codex reports an installed plugin at the directory it was catalogued
    /// from — the generated one, not the Store package — so recording the
    /// Store root would read as permanent drift against Codex's own report.
    fn generated_receipt_root(_package: &StoredPackage, envelope_dir: &Path) -> PathBuf {
        envelope_dir.to_path_buf()
    }

    fn marketplace_exists(executable: &Path, home: &Path, root: &Path) -> bool {
        let Ok(listing) = json(
            executable,
            home,
            &["plugin", "marketplace", "list", "--json"],
            "codex",
        ) else {
            return false;
        };
        marketplace_entries(&listing).is_some_and(|entries| {
            entries.iter().any(|entry| {
                entry
                    .get("root")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|candidate| Path::new(candidate) == root)
            })
        })
    }

    fn add_marketplace(executable: &Path, home: &Path, root: &Path) -> Result<()> {
        run_quiet(
            executable,
            home,
            "codex plugin marketplace add",
            &[
                std::ffi::OsStr::new("plugin"),
                std::ffi::OsStr::new("marketplace"),
                std::ffi::OsStr::new("add"),
                root.as_os_str(),
            ],
        )
    }

    fn install_plugin(executable: &Path, home: &Path, selector: &str) -> Result<()> {
        run_quiet(
            executable,
            home,
            &format!("codex plugin add `{selector}`"),
            &["plugin", "add", selector],
        )
    }

    fn inspect_plugin(
        executable: &Path,
        home: &Path,
        selector: &str,
        marketplace_root: &Path,
        detail: &BTreeMap<String, serde_json::Value>,
    ) -> AttachmentInspection {
        let Some(package_root) = detail_path(detail, "package_root") else {
            return blocked("plugin receipt has no package root");
        };
        inspect_codex_plugin(executable, home, selector, marketplace_root, &package_root)
    }

    fn remove_plugin(executable: &Path, home: &Path, selector: &str) -> Result<()> {
        run_quiet(
            executable,
            home,
            &format!("codex plugin remove {selector}"),
            &["plugin", "remove", selector],
        )
    }
}

fn inspect_codex_plugin(
    executable: &Path,
    command_home: &Path,
    selector: &str,
    marketplace_root: &Path,
    package_root: &Path,
) -> AttachmentInspection {
    let marketplace = match json(
        executable,
        command_home,
        &["plugin", "marketplace", "list", "--json"],
        "codex",
    ) {
        Ok(value) => value,
        Err(reason) => return blocked(reason),
    };
    let marketplace_name = selector.rsplit_once('@').map(|(_, name)| name);
    let Some(marketplace_name) = marketplace_name else {
        return blocked("plugin receipt selector has no marketplace identity");
    };
    let Some(entries) = marketplace_entries(&marketplace) else {
        return blocked("Codex marketplace JSON has no marketplaces array");
    };
    let matching_name = entries.iter().find(|entry| {
        entry
            .get("name")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|name| name == marketplace_name)
    });
    let Some(matching_name) = matching_name else {
        return AttachmentInspection {
            state: AttachmentState::Missing,
            reason: "Codex marketplace is absent".to_owned(),
        };
    };
    if matching_name
        .get("root")
        .and_then(serde_json::Value::as_str)
        .is_none_or(|root| Path::new(root) != marketplace_root)
    {
        return AttachmentInspection {
            state: AttachmentState::Drifted,
            reason: "Codex marketplace root differs from receipt".to_owned(),
        };
    }
    let plugins = match json(
        executable,
        command_home,
        &["plugin", "list", "--json"],
        "codex",
    ) {
        Ok(value) => value,
        Err(reason) => return blocked(reason),
    };
    inspect_codex_plugin_value(&plugins, selector, package_root)
}

fn inspect_codex_plugin_value(
    value: &serde_json::Value,
    selector: &str,
    package_root: &Path,
) -> AttachmentInspection {
    let Some(installed) = value.get("installed").and_then(serde_json::Value::as_array) else {
        return blocked("Codex plugin JSON has no installed array");
    };
    let Some(plugin) = installed.iter().find(|entry| {
        ["pluginId", "id", "plugin_id", "selector"]
            .iter()
            .filter_map(|field| entry.get(*field).and_then(serde_json::Value::as_str))
            .any(|candidate| candidate == selector)
    }) else {
        return AttachmentInspection {
            state: AttachmentState::Missing,
            reason: "Codex plugin is not installed".to_owned(),
        };
    };
    let Some(enabled) = plugin.get("enabled").and_then(serde_json::Value::as_bool) else {
        return blocked("Codex plugin JSON has no enabled state");
    };
    let Some(installed_state) = plugin.get("installed").and_then(serde_json::Value::as_bool) else {
        return blocked("Codex plugin JSON has no installed state");
    };
    let Some((_, marketplace_name)) = selector.rsplit_once('@') else {
        return blocked("plugin receipt selector has no marketplace identity");
    };
    let Some(actual_marketplace) = plugin
        .get("marketplaceName")
        .or_else(|| plugin.get("marketplace_name"))
        .and_then(serde_json::Value::as_str)
    else {
        return blocked("Codex plugin JSON has no marketplace identity");
    };
    let source = plugin
        .get("path")
        .or_else(|| plugin.pointer("/source/path"))
        .and_then(serde_json::Value::as_str);
    let Some(source) = source else {
        return blocked("Codex plugin JSON has no package source path");
    };
    if !enabled
        || !installed_state
        || actual_marketplace != marketplace_name
        || Path::new(source) != package_root
    {
        return AttachmentInspection {
            state: AttachmentState::Drifted,
            reason: "Codex plugin enabled state or source differs from receipt".to_owned(),
        };
    }
    AttachmentInspection {
        state: AttachmentState::Matched,
        reason: "Codex native plugin matches receipt".to_owned(),
    }
}

/// Computes which of `resources` (already discovered by UZE's engine) are
/// actually declared by `.codex-plugin/plugin.json` — the intersection
/// ADR-013 §2 requires (`provided = discovered ∩ declared`), mirroring
/// Claude's `claude_exact_coverage` in `claude::plugin`. Unlike Claude's
/// manifest, Codex's does not enumerate individual skills or inline MCP
/// servers: `skills` names one directory whose entire subtree is covered
/// (confirmed by `tests/_fixtures/foreign/codex/native-plugin/.codex-plugin/
/// plugin.json`: `"skills": "./skills/"`), and `mcpServers` names one
/// external file (`"./.mcp.json"`) holding the standard Agent Plugins
/// `{"mcpServers": {...}}` shape — so a server is declared by name, read
/// from that file, not from `plugin.json` itself. A missing, malformed, or
/// unsafely-escaping declaration for either field contributes no coverage
/// for that field rather than erroring — the package still installs
/// natively, just with a smaller (possibly empty) `provided_resource_identities`,
/// exactly like Claude's malformed-manifest handling.
pub(super) fn codex_exact_coverage(
    package: &StoredPackage,
    resources: &[&uze_core::capability::Resource],
) -> std::collections::BTreeSet<String> {
    let manifest_path = package.root.join(".codex-plugin/plugin.json");
    let bytes = match fs::read(&manifest_path) {
        Ok(bytes) => bytes,
        Err(_) => return std::collections::BTreeSet::new(),
    };
    let value: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => return std::collections::BTreeSet::new(),
    };

    let declared_skills_dir = value
        .get("skills")
        .and_then(serde_json::Value::as_str)
        .and_then(normalize_declared_relative_path);

    let declared_mcp: std::collections::BTreeSet<String> = value
        .get("mcpServers")
        .and_then(serde_json::Value::as_str)
        .and_then(normalize_declared_relative_path)
        .and_then(|relative| fs::read(package.root.join(relative)).ok())
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|document| {
            document
                .get("mcpServers")
                .and_then(serde_json::Value::as_object)
                .map(|servers| servers.keys().cloned().collect())
        })
        .unwrap_or_default();

    let mut provided = std::collections::BTreeSet::new();
    for resource in resources {
        match resource.capability.kind {
            uze_core::capability::CapabilityKind::AgentSkill => {
                let Some(declared_dir) = &declared_skills_dir else {
                    continue;
                };
                let Some(relative) = resource.capability.path.strip_prefix(&package.root).ok()
                else {
                    continue;
                };
                let Some(parent) = relative.parent() else {
                    continue;
                };
                // The skill's own directory (e.g. "skills/skill-a") must
                // live inside the declared skills root, not merely share a
                // string prefix — `Path::starts_with` compares components,
                // so "skills-extra" is never mistaken for inside "skills".
                if parent.starts_with(declared_dir)
                    && explicit_envelope_preserves_policy(resource, &package.root.join(parent))
                {
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

/// Whether the author's own bytes already carry Codex's encoding of the
/// Skill's canonical `invoke:` policy (ADR-030 §6): UZE never rewrites an
/// explicit envelope, so a path match alone is not coverage. A user-only
/// Skill needs its own `agents/openai.yaml` explicit-only sidecar; a
/// model-only Skill degrades on Codex and an invalid one is never projected,
/// so neither is ever claimed — both fall through to capability-level
/// delivery, which reports them honestly.
fn explicit_envelope_preserves_policy(
    resource: &uze_core::capability::Resource,
    skill_dir: &Path,
) -> bool {
    let policy = resource.skill_invocation();
    if policy.is_invalid() || !policy.user {
        return false;
    }
    policy.model || crate::shared::skill::has_explicit_only_sidecar(skill_dir)
}

#[cfg(test)]
mod plugin_tests {
    use std::path::Path;

    use uze_core::integration::AttachmentState;

    use super::inspect_codex_plugin_value;

    #[test]
    fn native_plugin_receipt_requires_installed_identity_and_expected_source() {
        let package = Path::new("/uze/store/example");
        let exact = serde_json::json!({
            "installed": [{"pluginId":"example@uze-local", "enabled":true, "installed":true, "marketplaceName":"uze-local", "source":{"path":"/uze/store/example"}}]
        });
        assert_eq!(
            inspect_codex_plugin_value(&exact, "example@uze-local", package).state,
            AttachmentState::Matched
        );
        let changed = serde_json::json!({
            "installed": [{"id":"example@uze-local", "enabled":false, "installed":true, "marketplaceName":"uze-local", "path":"/uze/store/example"}]
        });
        assert_eq!(
            inspect_codex_plugin_value(&changed, "example@uze-local", package).state,
            AttachmentState::Drifted
        );
        let absent = serde_json::json!({"installed": []});
        assert_eq!(
            inspect_codex_plugin_value(&absent, "example@uze-local", package).state,
            AttachmentState::Missing
        );
    }
}

#[cfg(test)]
mod codex_native_coverage_tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::PathBuf;

    use uze_core::capability::Resource;
    use uze_core::capability::{Capability, CapabilityKind};
    use uze_core::home::UzeHome;
    use uze_core::integration::IntegrationPort;

    use super::super::CodexIntegration;
    use super::codex_exact_coverage;

    fn temp_root(label: &str) -> PathBuf {
        uze_testkit::temp::scratch(label)
    }

    /// `skills_field` and `mcp_field` are written into `.codex-plugin/plugin.json`
    /// verbatim (as raw JSON snippets, so a test can supply a non-string
    /// shape, an escaping path, or omit the key with `None`). `mcp_file`, if
    /// given, is written to `.mcp.json` at the package root with that exact
    /// byte content — a test can supply malformed JSON here too.
    fn make_package(
        label: &str,
        skills_field: Option<&str>,
        mcp_field: Option<&str>,
        mcp_file: Option<&str>,
    ) -> (PathBuf, uze_core::store::StoredPackage) {
        let root = temp_root(label);
        let pkg_root = root.join("pkg");
        fs::create_dir_all(pkg_root.join(".codex-plugin")).unwrap();
        let mut fields = vec![
            "\"name\":\"test-pkg\"".to_owned(),
            "\"version\":\"0.1.0\"".to_owned(),
        ];
        if let Some(skills) = skills_field {
            fields.push(format!("\"skills\":{skills}"));
        }
        if let Some(mcp) = mcp_field {
            fields.push(format!("\"mcpServers\":{mcp}"));
        }
        let manifest = format!("{{{}}}", fields.join(","));
        fs::write(pkg_root.join(".codex-plugin/plugin.json"), manifest).unwrap();
        if let Some(contents) = mcp_file {
            fs::write(pkg_root.join(".mcp.json"), contents).unwrap();
        }
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

    fn skill_resource(pkg: &uze_core::store::StoredPackage, dir: &str, skill: &str) -> Resource {
        let path = pkg.root.join(dir).join(skill).join("SKILL.md");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, format!("---\nname: {skill}\n---\n")).unwrap();
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

    fn policy_skill_resource(
        pkg: &uze_core::store::StoredPackage,
        skill: &str,
        body: &str,
    ) -> Resource {
        let path = pkg.root.join("skills").join(skill).join("SKILL.md");
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

    const USER_ONLY: &str =
        "---\nname: review\ninvoke:\n  model: false\n  user: true\n---\nBody.\n";

    /// ADR-030 §6: an explicit envelope never rewritten by UZE covers a
    /// user-only Skill only when the author shipped Codex's own
    /// explicit-only sidecar beside it.
    #[test]
    fn explicit_user_only_skill_is_covered_only_with_the_authors_policy_sidecar() {
        let (_root, pkg) = make_package("policy-sidecar", Some(r#""./skills/""#), None, None);
        let without = policy_skill_resource(&pkg, "review", USER_ONLY);
        assert!(
            codex_exact_coverage(&pkg, &[&without]).is_empty(),
            "a path-matched user-only Skill without the sidecar would silently become model-invocable"
        );
        let sidecar = pkg.root.join("skills/review/agents/openai.yaml");
        fs::create_dir_all(sidecar.parent().unwrap()).unwrap();
        fs::write(&sidecar, "policy:\n  allow_implicit_invocation: false\n").unwrap();
        assert_eq!(
            codex_exact_coverage(&pkg, &[&without]),
            BTreeSet::from([without.identity()])
        );
        let _ = fs::remove_dir_all(_root);
    }

    #[test]
    fn explicit_model_only_and_invalid_skills_are_never_covered() {
        let (_root, pkg) = make_package("policy-degraded", Some(r#""./skills/""#), None, None);
        let model_only = policy_skill_resource(
            &pkg,
            "legacy",
            "---\ninvoke:\n  model: true\n  user: false\n---\nBody.\n",
        );
        let invalid = policy_skill_resource(
            &pkg,
            "dead",
            "---\ninvoke:\n  model: false\n  user: false\n---\nBody.\n",
        );
        assert!(codex_exact_coverage(&pkg, &[&model_only, &invalid]).is_empty());
        let _ = fs::remove_dir_all(_root);
    }

    const MCP_FILE_ONE_SERVER: &str = r#"{"mcpServers":{"mcp-a":{"command":"a"}}}"#;

    /// A. Manifest declares everything discovered.
    #[test]
    fn manifest_declares_all_is_fully_covered() {
        let (_root, pkg) = make_package(
            "all",
            Some(r#""./skills/""#),
            Some(r#""./.mcp.json""#),
            Some(MCP_FILE_ONE_SERVER),
        );
        let r_a = skill_resource(&pkg, "skills", "a");
        let r_m = mcp_resource(&pkg, "mcp-a");
        let resources = vec![&r_a, &r_m];
        let covered = codex_exact_coverage(&pkg, &resources);
        let expected: BTreeSet<String> = resources.iter().map(|r| r.identity()).collect();
        assert_eq!(covered, expected);
        let _ = fs::remove_dir_all(_root);
    }

    /// B. Manifest declares only a subset of what's discovered.
    #[test]
    fn manifest_declares_subset_only_that_subset_is_covered() {
        let (_root, pkg) = make_package(
            "subset",
            Some(r#""./skills/""#),
            Some(r#""./.mcp.json""#),
            Some(MCP_FILE_ONE_SERVER),
        );
        let r_a = skill_resource(&pkg, "skills", "a");
        let r_m_covered = mcp_resource(&pkg, "mcp-a");
        let r_m_uncovered = mcp_resource(&pkg, "mcp-b");
        let resources = vec![&r_a, &r_m_covered, &r_m_uncovered];
        let covered = codex_exact_coverage(&pkg, &resources);
        assert_eq!(
            covered,
            BTreeSet::from([r_a.identity(), r_m_covered.identity()])
        );
        assert!(!covered.contains(&r_m_uncovered.identity()));
        let _ = fs::remove_dir_all(_root);
    }

    /// C. Store contains a Skill physically outside the declared skills
    /// directory — not covered, falls back to individual attachment.
    #[test]
    fn store_has_a_skill_outside_the_declared_directory_is_not_covered() {
        let (_root, pkg) = make_package("extra-skill", Some(r#""./skills/""#), None, None);
        let r_in = skill_resource(&pkg, "skills", "a");
        let r_out = skill_resource(&pkg, "extra", "b");
        let resources = vec![&r_in, &r_out];
        let covered = codex_exact_coverage(&pkg, &resources);
        assert_eq!(covered, BTreeSet::from([r_in.identity()]));
        assert!(!covered.contains(&r_out.identity()));
        let _ = fs::remove_dir_all(_root);
    }

    /// D. Store contains an MCP resource the referenced `.mcp.json` never
    /// names — not covered.
    #[test]
    fn store_has_an_mcp_server_not_named_in_the_referenced_file_is_not_covered() {
        let (_root, pkg) = make_package(
            "extra-mcp",
            None,
            Some(r#""./.mcp.json""#),
            Some(MCP_FILE_ONE_SERVER),
        );
        let r_named = mcp_resource(&pkg, "mcp-a");
        let r_extra = mcp_resource(&pkg, "mcp-c");
        let resources = vec![&r_named, &r_extra];
        let covered = codex_exact_coverage(&pkg, &resources);
        assert_eq!(covered, BTreeSet::from([r_named.identity()]));
        let _ = fs::remove_dir_all(_root);
    }

    /// E. Manifest references a `.mcp.json` that does not exist on disk —
    /// no MCP coverage claimed, no panic, Skill coverage is unaffected.
    #[test]
    fn manifest_references_a_missing_mcp_file_yields_no_mcp_coverage() {
        let (_root, pkg) = make_package(
            "missing-mcp-file",
            Some(r#""./skills/""#),
            Some(r#""./.mcp.json""#),
            None, // .mcp.json is declared but never written
        );
        let r_a = skill_resource(&pkg, "skills", "a");
        let r_m = mcp_resource(&pkg, "mcp-a");
        let resources = vec![&r_a, &r_m];
        let covered = codex_exact_coverage(&pkg, &resources);
        assert_eq!(covered, BTreeSet::from([r_a.identity()]));
        assert!(!covered.contains(&r_m.identity()));
        let _ = fs::remove_dir_all(_root);
    }

    /// F. The referenced `.mcp.json` exists but is malformed JSON — treated
    /// as no declared MCP servers, not a crash; package still installs
    /// natively via `package_exposure_plan`.
    #[test]
    fn malformed_mcp_file_yields_empty_mcp_coverage_but_package_still_deliverable() {
        let (_root, pkg) = make_package(
            "malformed-mcp",
            Some(r#""./skills/""#),
            Some(r#""./.mcp.json""#),
            Some("{not json"),
        );
        let r_a = skill_resource(&pkg, "skills", "a");
        let r_m = mcp_resource(&pkg, "mcp-a");
        let resources = vec![&r_a, &r_m];
        let covered = codex_exact_coverage(&pkg, &resources);
        assert_eq!(covered, BTreeSet::from([r_a.identity()]));
        let integration =
            CodexIntegration::new(_root.join("agents"), UzeHome::at(_root.join("uze")));
        let plan = integration.package_exposure_plan(&pkg, &resources);
        assert!(
            plan.is_some(),
            "malformed native MCP file must not block native delivery"
        );
        let _ = fs::remove_dir_all(_root);
    }

    /// G. An unexpected JSON shape for `skills` (an array instead of the
    /// documented single directory string) is tolerated as "no skills
    /// declared" rather than panicking or matching every resource.
    #[test]
    fn unexpected_skills_field_shape_is_tolerated_as_no_declaration() {
        let (_root, pkg) = make_package("wrong-shape", Some(r#"["./skills/a"]"#), None, None);
        let r_a = skill_resource(&pkg, "skills", "a");
        let resources = vec![&r_a];
        let covered = codex_exact_coverage(&pkg, &resources);
        assert!(covered.is_empty());
        let _ = fs::remove_dir_all(_root);
    }

    /// H. A `skills` declaration that escapes the package root via `..` is
    /// rejected — never resolved, never covers anything.
    #[test]
    fn path_escape_in_skills_declaration_is_rejected() {
        let (_root, pkg) = make_package("escape", Some(r#""../../etc""#), None, None);
        let r_a = skill_resource(&pkg, "skills", "a");
        let resources = vec![&r_a];
        let covered = codex_exact_coverage(&pkg, &resources);
        assert!(covered.is_empty());
        let _ = fs::remove_dir_all(_root);
    }

    /// I. An absolute `mcpServers` path is rejected — never read, even if a
    /// file happens to exist at that absolute location.
    #[test]
    fn absolute_mcp_servers_path_is_rejected() {
        let (_root, pkg) = make_package("absolute", None, Some(r#""/etc/mcp.json""#), None);
        let r_m = mcp_resource(&pkg, "mcp-a");
        let resources = vec![&r_m];
        let covered = codex_exact_coverage(&pkg, &resources);
        assert!(covered.is_empty());
        let _ = fs::remove_dir_all(_root);
    }

    /// Regression companion to Claude's
    /// `leading_slash_declaration_is_rejected_even_when_it_would_collide_with_a_real_skill`:
    /// proves the shared `normalize_declared_relative_path` helper rejects
    /// an absolute `skills` declaration through Codex's own call site too,
    /// against a resource that actually exists at the path an unfixed
    /// normalizer would have relativized it to.
    #[test]
    fn absolute_skills_declaration_is_rejected_even_when_it_would_collide_with_a_real_skill() {
        let (_root, pkg) = make_package(
            "absolute-skills-collision",
            Some(r#""/skills/""#),
            None,
            None,
        );
        let r_a = skill_resource(&pkg, "skills", "a");
        let resources = vec![&r_a];
        let covered = codex_exact_coverage(&pkg, &resources);
        assert!(
            covered.is_empty(),
            "`/skills/` is an absolute declaration and must never be silently relativized into \
             covering the real `skills/a` resource: got {covered:?}"
        );
        let _ = fs::remove_dir_all(_root);
    }

    /// J. Empty/absent declarations for both fields yield empty coverage,
    /// not an error — the package still installs natively with an empty
    /// `provided_resource_identities`, exactly like Claude's equivalent case.
    #[test]
    fn empty_declarations_yield_empty_coverage_but_package_still_deliverable() {
        let (_root, pkg) = make_package("empty", Some(r#""""#), None, None);
        let r_a = skill_resource(&pkg, "skills", "a");
        let resources = vec![&r_a];
        let covered = codex_exact_coverage(&pkg, &resources);
        assert!(covered.is_empty());
        let integration =
            CodexIntegration::new(_root.join("agents"), UzeHome::at(_root.join("uze")));
        let plan = integration
            .package_exposure_plan(&pkg, &resources)
            .expect("native route still applies with empty coverage");
        assert!(plan.provided_resource_identities.is_empty());
        // The uncovered skill must still be attachable through the normal
        // capability-level fallback — never silently dropped.
        uze_core::state::record(
            &UzeHome::at(_root.join("uze")),
            integration.id(),
            uze_core::state::IntegrationRecord::default(),
        )
        .unwrap();
        let fallback = integration.exposure_plan(&r_a);
        assert!(!matches!(
            fallback.mechanism,
            uze_core::exposure::ExposureMechanism::Unsupported { .. }
        ));
        let _ = fs::remove_dir_all(_root);
    }

    /// Partial native delivery: a package with one declared skill and one
    /// undeclared skill plus one undeclared MCP server must cover only the
    /// declared skill — no duplicate receipt, nothing missing.
    #[test]
    fn partial_native_coverage_leaves_undeclared_resources_on_the_fallback_path() {
        let (_root, pkg) = make_package(
            "partial",
            Some(r#""./skills/""#),
            None, // mcpServers not declared by the native envelope at all
            None,
        );
        let r_native = skill_resource(&pkg, "skills", "skill-native");
        let r_extra_skill = skill_resource(&pkg, "extra", "skill-extra");
        let r_extra_mcp = mcp_resource(&pkg, "mcp-extra");
        let resources = vec![&r_native, &r_extra_skill, &r_extra_mcp];
        let uze_home = UzeHome::at(_root.join("uze"));
        let integration = CodexIntegration::new(_root.join("agents"), uze_home.clone());
        uze_core::state::record(
            &uze_home,
            integration.id(),
            uze_core::state::IntegrationRecord {
                version: None,
                strategy: "test".to_owned(),
            },
        )
        .unwrap();
        let plan = integration
            .package_exposure_plan(&pkg, &resources)
            .expect("native envelope still applies");
        assert_eq!(
            plan.provided_resource_identities,
            BTreeSet::from([r_native.identity()])
        );
        assert!(
            !plan
                .provided_resource_identities
                .contains(&r_extra_skill.identity())
        );
        assert!(
            !plan
                .provided_resource_identities
                .contains(&r_extra_mcp.identity())
        );
        // Both uncovered resources still route through the normal
        // capability-level fallback rather than disappearing.
        for uncovered in [&r_extra_skill, &r_extra_mcp] {
            let fallback = integration.exposure_plan(uncovered);
            assert!(!matches!(
                fallback.mechanism,
                uze_core::exposure::ExposureMechanism::Unsupported { .. }
            ));
        }
        let _ = fs::remove_dir_all(_root);
    }
}
