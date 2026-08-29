//! Antigravity's GENERATED native plugin envelope: for a canonical UZE
//! package whose plugin.json is a valid Antigravity manifest but whose MCP
//! servers live in canonical `mcp.json` — which the plugin system does not
//! read (`mcp_config.json` is the vendor name) — this module
//! deterministically synthesizes the plugin into a UZE-owned derived
//! directory and installs that, so the package still ships as one native
//! plugin instead of decomposing into per-capability shims.
//!
//! Generated Native Plugin sits between Explicit Native Plugin and Native
//! Capability (ADR-020/ADR-021, refining ADR-013 §2), mirroring the other
//! integrations' generated-envelope discipline.
//! Simpler than either in one way (no catalogue — `plugin install` points
//! straight at the directory) and costlier in another (the vendor stages a
//! byte copy; see [`super::plugin`]'s module doc for why that stays a
//! Derived Artifact).

use std::{collections::BTreeSet, fs, path::Path, path::PathBuf};

use uze_core::{Result, UzeError, home::UzeHome, project::Resource, store::StoredPackage};

use crate::hooks as hook_projection;

/// Root of every package's generated plugin directory. Lives under
/// `$UZE_HOME/state/attachments/antigravity/plugins/` — the same convention
/// every other integration's generated envelopes use, never under the Store.
pub(super) fn generated_root(uze_home: &UzeHome) -> PathBuf {
    uze_home
        .state_dir()
        .join("attachments")
        .join("antigravity")
        .join("plugins")
}

fn generated_package_dir_for_id(uze_home: &UzeHome, package_id: &str) -> PathBuf {
    generated_root(uze_home).join(sanitize_for_agy_path(package_id))
}

/// `agy plugin install <path>` parses a final path segment shaped like
/// `name@marketplace` as a marketplace-qualified selector rather than a
/// literal filesystem path, and fails with "unknown marketplace: ..." for
/// any marketplace name not registered with AGY itself (verified against
/// real agy 1.1.22). `PackageId`s are always marketplace-qualified this way
/// (ADR-036), so the `@` is replaced before it ever reaches the vendor CLI;
/// `remove_generated_plugin_by_id` uses the same helper so create/remove
/// always agree on the directory name.
fn sanitize_for_agy_path(package_id: &str) -> String {
    package_id.replace('@', "--")
}

/// The canonical `mcp.json`'s declared server names, `Some` only when the
/// file exists and carries a non-empty `mcpServers` object. `None` for an
/// absent, malformed, or server-less file — in which case nothing needs
/// translating and the explicit route handles the package.
pub(super) fn canonical_mcp_servers(package: &StoredPackage) -> Option<BTreeSet<String>> {
    let entries: BTreeSet<String> = fs::read(package.root.join("mcp.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|value| {
            value
                .get("mcpServers")
                .and_then(serde_json::Value::as_object)
                .map(|servers| servers.keys().cloned().collect())
        })
        .unwrap_or_default();
    (!entries.is_empty()).then_some(entries)
}

/// Whether the package declares canonical portable hooks: a root
/// `hooks.json` that parses. The Antigravity plugin system reads `hooks.json`
/// in its own named-entry form — never the canonical shape — so any such
/// package must take the generated route rather than being installed whole.
pub(super) fn canonical_hook_groups(package: &StoredPackage) -> bool {
    hook_projection::package_hook_groups(&package.root).is_ok_and(|groups| !groups.is_empty())
}

/// The intersection ADR-013 §2 requires, computed against the SEMANTIC
/// surface a generated plugin preserves: canonical `skills/` are carried
/// verbatim, and the MCP servers declared in canonical `mcp.json` are
/// translated into the generated `mcp_config.json`. Coverage is
/// semantic-aware (ADR-030 §13): a Skill is covered only when its
/// `invoke:` policy is the default — Antigravity has no explicit-only
/// mechanism and cannot hide a Skill from the model or the user, so a
/// non-default policy degrades and is never claimed; it falls through to
/// capability-level delivery, which reports it honestly. Coverage and
/// generation agree by construction.
pub(super) fn generated_exact_coverage(
    package: &StoredPackage,
    resources: &[&Resource],
) -> BTreeSet<String> {
    let declared_mcp = canonical_mcp_servers(package).unwrap_or_default();
    let has_hooks = canonical_hook_groups(package);
    let mut provided = BTreeSet::new();
    for resource in resources {
        match resource.capability.kind {
            uze_core::capability::CapabilityKind::AgentSkill => {
                if under(package, &resource.capability.path, "skills")
                    && resource.skill_invocation().is_default()
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
            uze_core::capability::CapabilityKind::Hook
                if has_hooks && resource.capability.path == package.root.join("hooks.json") =>
            {
                provided.insert(resource.identity());
            }
            _ => {}
        }
    }
    provided
}

fn under(package: &StoredPackage, path: &Path, conventional: &str) -> bool {
    let Ok(relative) = path.strip_prefix(&package.root) else {
        return false;
    };
    let Some(parent) = relative.parent() else {
        return false;
    };
    parent.starts_with(conventional)
}

/// The generated `plugin.json` document. Name is the canonical manifest's
/// own (the plan guaranteed it satisfies the vendor pattern); description
/// comes from the package's own canonical manifest, never invented.
fn generated_plugin_document(package: &StoredPackage) -> serde_json::Value {
    let (name, description) = super::plugin::plugin_manifest_name(package)
        .map(|name| {
            let description = fs::read(&package.manifest)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                .and_then(|value| {
                    value
                        .get("description")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned)
                })
                .unwrap_or_else(|| {
                    "UZE-managed Antigravity plugin, generated from a vendor-neutral package."
                        .to_owned()
                });
            (name, description)
        })
        .expect("package_exposure_plan gates generation on a valid plugin name");
    serde_json::json!({
        "name": name,
        "description": description,
    })
}

/// Translates canonical `mcp.json` servers into the vendor's
/// `mcp_config.json` form: every server entry passes through as-is, with
/// the legacy remote keys `url`/`httpUrl` rewritten to the modern
/// `serverUrl` — the exact mapping Antigravity's own official
/// legacy-migration path performs (official docs: "Legacy schema keys:
/// `url` or `httpUrl`; Modern schema key: `serverUrl`"). Nothing is
/// dropped; stdio `command`, `args`, `env`, `cwd` stay untouched.
fn translated_mcp_config(package: &StoredPackage) -> serde_json::Value {
    let mut document = serde_json::json!({ "mcpServers": {} });
    if let Some(servers) = fs::read(package.root.join("mcp.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|value| {
            value
                .get("mcpServers")
                .and_then(serde_json::Value::as_object)
                .cloned()
        })
    {
        for (name, mut server) in servers {
            if let Some(value) = server.as_object_mut()
                && let Some(url) = value.remove("url").or_else(|| value.remove("httpUrl"))
            {
                value.insert("serverUrl".to_owned(), url);
            }
            document["mcpServers"][name] = server;
        }
    }
    document
}

/// Materializes (or refreshes) one package's generated plugin directory.
/// Idempotent and deterministic: recreated wholesale from the Store package
/// on every call — the directory is entirely UZE-owned and
/// non-authoritative (ADR-013 §4). Default-policy `skills/` are symlinked
/// to the Store's own bytes, never copied (the vendor's install verb
/// dereferences them when it stages its copy); canonical MCP servers are
/// translated into the vendor `mcp_config.json`. `commands/` is no longer a
/// canonical surface (ADR-030): a vendor-authored `commands/` directory is
/// only ever delivered through an explicit plugin the author shipped.
pub(super) fn materialize_generated_plugin(
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
    let manifest = generated_plugin_document(package);
    fs::write(
        dir.join("plugin.json"),
        serde_json::to_vec_pretty(&manifest).expect("generated manifest is serializable"),
    )
    .map_err(|source| UzeError::Write {
        path: dir.join("plugin.json"),
        source,
    })?;

    let skills_source = package.root.join("skills");
    if skills_source.is_dir() {
        symlink(&skills_source, &dir.join("skills"))?;
    }
    if canonical_mcp_servers(package).is_some() {
        let mcp = translated_mcp_config(package);
        fs::write(
            dir.join("mcp_config.json"),
            serde_json::to_vec_pretty(&mcp).expect("generated MCP config is serializable"),
        )
        .map_err(|source| UzeError::Write {
            path: dir.join("mcp_config.json"),
            source,
        })?;
    }
    if canonical_hook_groups(package) {
        // The plugin system reads `hooks.json` in its own named-entry form,
        // never the canonical shape, so the canonical groups are translated
        // into the named document with the hook-exec wrapper carrying the
        // portable ABI (ADR-033).
        let groups = hook_projection::package_hook_groups(&package.root)?;
        let references: Vec<&uze_core::hook::PortableHook> = groups.iter().collect();
        let executable = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("uze"));
        let document = hook_projection::agy_hook_document(&references, &executable, &package.root);
        fs::write(dir.join("hooks.json"), document).map_err(|source| UzeError::Write {
            path: dir.join("hooks.json"),
            source,
        })?;
    }
    Ok(dir)
}

/// Removes one package's generated plugin directory by id alone — used at
/// detach time, when only the receipt's `package_id` (not a full
/// `StoredPackage`) is available. Safe unconditionally: this directory is
/// never anything but a Derived Artifact (ADR-013 §4).
pub(super) fn remove_generated_plugin_by_id(uze_home: &UzeHome, package_id: &str) -> Result<()> {
    let dir = generated_package_dir_for_id(uze_home, package_id);
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|source| UzeError::Write { path: dir, source })?;
    }
    Ok(())
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

    use super::super::AntigravityIntegration;
    use super::*;

    fn temp_root(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "uze-antigravity-generated-{label}-{nonce}-{}",
            std::process::id()
        ))
    }

    fn make_package_with_mcp(label: &str) -> (PathBuf, StoredPackage) {
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
        fs::write(
            pkg_root.join("mcp.json"),
            r#"{"mcpServers":{"mcp-a":{"command":"a"},"remote-b":{"url":"https://example.com/mcp"}}}"#,
        )
        .unwrap();
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

    fn make_package_with_hooks(label: &str) -> (PathBuf, StoredPackage) {
        let (root, pkg) = make_package_with_mcp(label);
        // Hooks replace the MCP surface for this fixture's purpose.
        fs::remove_file(pkg.root.join("mcp.json")).unwrap();
        fs::write(
            pkg.root.join("hooks.json"),
            r#"{"hooks":{"PreToolUse":[{"id":"protect-env","matcher":"shell","effect":"deny","hooks":[{"type":"command","command":"${PLUGIN_ROOT}/check"}]}],"Stop":[{"hooks":[{"type":"command","command":"archive"}]}]}}"#,
        )
        .unwrap();
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
    fn canonical_mcp_is_detected_only_for_a_real_declaration() {
        let (root, pkg) = make_package_with_mcp("canonical-mcp");
        assert_eq!(
            canonical_mcp_servers(&pkg),
            Some(BTreeSet::from(["mcp-a".to_owned(), "remote-b".to_owned()]))
        );
        fs::remove_file(pkg.root.join("mcp.json")).unwrap();
        assert!(canonical_mcp_servers(&pkg).is_none());
        fs::write(pkg.root.join("mcp.json"), "{not json").unwrap();
        assert!(canonical_mcp_servers(&pkg).is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn package_with_canonical_mcp_takes_the_generated_route() {
        let (root, pkg) = make_package_with_mcp("plan-generated");
        let r_skill = skill_resource(&pkg);
        let r_mcp = mcp_resource(&pkg, "mcp-a");
        let resources = vec![&r_skill, &r_mcp];
        let uze_home = UzeHome::at(root.join("uze"));
        let integration = AntigravityIntegration::new(root.join("agents"), uze_home.clone());
        let plan = integration
            .package_exposure_plan(&pkg, &resources)
            .expect("generated route applies");
        assert_eq!(plan.route, uze_core::router::CompatibilityRoute::Native);
        assert_eq!(
            plan.provided_resource_identities,
            BTreeSet::from([r_skill.identity(), r_mcp.identity()])
        );
        assert!(
            !generated_root(&uze_home).join(pkg.id.as_str()).exists(),
            "planning must stay read-only"
        );
        let _ = fs::remove_dir_all(root);
    }

    /// ADR-030 §13: a non-default invoke policy must not enter an unchanged
    /// Antigravity plugin tree. The package is decomposed so every resource
    /// follows its own policy-aware capability route.
    #[test]
    fn non_default_policy_skill_disables_the_generated_package_route() {
        let (root, pkg) = make_package_with_mcp("plan-policy");
        let user_only = Resource::from_package(
            pkg.id.clone(),
            pkg.root.clone(),
            Capability {
                kind: CapabilityKind::AgentSkill,
                representation: Representation::Standard,
                path: pkg.root.join("skills/commit/SKILL.md"),
                payload: b"---\nname: commit\ninvoke:\n  model: false\n  user: true\n---\n"
                    .to_vec(),
            },
        );
        let resources = vec![&user_only];
        let uze_home = UzeHome::at(root.join("uze"));
        let integration = AntigravityIntegration::new(root.join("agents"), uze_home);
        assert!(
            integration
                .package_exposure_plan(&pkg, &resources)
                .is_none(),
            "the unchanged generated plugin must not carry the user-only Skill"
        );
        let fallback = integration.exposure_plan(&user_only);
        assert_eq!(
            fallback.route,
            uze_core::router::CompatibilityRoute::Adaptable,
            "the capability-level fallback reports the degradation honestly"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn materialize_translates_and_never_writes_into_the_store() {
        let (root, pkg) = make_package_with_mcp("materialize");
        let uze_home = UzeHome::at(root.join("uze"));
        let before: BTreeSet<PathBuf> = walk(&pkg.root);
        let dir = materialize_generated_plugin(&uze_home, &pkg).unwrap();
        let after: BTreeSet<PathBuf> = walk(&pkg.root);
        assert_eq!(
            before, after,
            "Store package tree must be byte-for-byte unchanged"
        );
        assert!(dir.starts_with(uze_home.state_dir()));
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join("plugin.json")).unwrap()).unwrap();
        assert_eq!(manifest["name"], "flow");
        assert!(dir.join("skills").is_symlink());
        assert!(
            !dir.join("commands").exists(),
            "commands/ is no longer a canonical surface; the generated plugin never carries it"
        );
        let mcp: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join("mcp_config.json")).unwrap()).unwrap();
        assert_eq!(mcp["mcpServers"]["mcp-a"]["command"], "a");
        assert_eq!(
            mcp["mcpServers"]["remote-b"]["serverUrl"],
            "https://example.com/mcp"
        );
        assert!(
            mcp["mcpServers"]["remote-b"].get("url").is_none(),
            "legacy url key must be rewritten"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn generated_dir_never_carries_an_at_sign() {
        // `agy plugin install <path>` parses a final path segment shaped
        // like `name@marketplace` as a marketplace selector, not a literal
        // path, and fails with "unknown marketplace: ..." — regression
        // coverage for that real-CLI quirk (verified against agy 1.1.22).
        let (root, pkg) = make_package_with_mcp("no-at-sign");
        assert!(
            pkg.id.as_str().contains('@'),
            "fixture must be marketplace-qualified"
        );
        let uze_home = UzeHome::at(root.join("uze"));
        let dir = materialize_generated_plugin(&uze_home, &pkg).unwrap();
        assert!(
            !dir.file_name().unwrap().to_str().unwrap().contains('@'),
            "generated dir name must not contain '@': {dir:?}"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn materialize_is_deterministic_across_rebuilds() {
        let (root, pkg) = make_package_with_mcp("deterministic");
        let uze_home = UzeHome::at(root.join("uze"));
        materialize_generated_plugin(&uze_home, &pkg).unwrap();
        let first = fs::read(
            generated_package_dir_for_id(&uze_home, pkg.id.as_str()).join("mcp_config.json"),
        )
        .unwrap();
        materialize_generated_plugin(&uze_home, &pkg).unwrap();
        let second = fs::read(
            generated_package_dir_for_id(&uze_home, pkg.id.as_str()).join("mcp_config.json"),
        )
        .unwrap();
        assert_eq!(first, second);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn canonical_hooks_are_detected_only_for_a_parsable_manifest() {
        let (root, pkg) = make_package_with_hooks("canonical-hooks");
        assert!(canonical_hook_groups(&pkg));
        fs::write(pkg.root.join("hooks.json"), "{not json").unwrap();
        assert!(
            !canonical_hook_groups(&pkg),
            "malformed hooks are not translated"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn package_with_hooks_takes_the_generated_named_route() {
        let (root, pkg) = make_package_with_hooks("plan-hooks");
        let resources = uze_core::engine::package_resources_at(&pkg.id, &pkg.root).unwrap();
        let references: Vec<&Resource> = resources.iter().collect();
        let uze_home = UzeHome::at(root.join("uze"));
        let integration = AntigravityIntegration::new(root.join("agents"), uze_home.clone());
        let plan = integration
            .package_exposure_plan(&pkg, &references)
            .expect("generated route applies");
        assert_eq!(plan.route, uze_core::router::CompatibilityRoute::Native);
        assert_eq!(
            plan.provided_resource_identities,
            references
                .iter()
                .map(|resource| resource.identity())
                .collect(),
            "every canonical hook group is covered by the generated plugin"
        );
        assert!(
            !generated_root(&uze_home).join(pkg.id.as_str()).exists(),
            "planning must stay read-only"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn materialize_translates_canonical_hooks_into_named_entries() {
        let (root, pkg) = make_package_with_hooks("materialize-hooks");
        let uze_home = UzeHome::at(root.join("uze"));
        let dir = materialize_generated_plugin(&uze_home, &pkg).unwrap();
        assert!(
            pkg.root.join("hooks.json").is_file(),
            "Store bytes stay untouched"
        );
        let hooks: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join("hooks.json")).unwrap()).unwrap();
        let protect = &hooks["hooks"]["protect-env"]["PreToolUse"][0];
        assert_eq!(
            protect["matcher"], "run_command",
            "portable aliases translate to AGY tool names"
        );
        let command = protect["hooks"][0]["command"].as_str().unwrap();
        assert!(
            command.contains("hook-exec"),
            "the wrapper carries the portable ABI"
        );
        assert!(command.contains("--adapter 'antigravity'"));
        assert!(command.contains("--command '${PLUGIN_ROOT}/check'"));
        // The unnamed Stop group gets its deterministic derived id and
        // carries no matcher (match-all).
        assert_eq!(hooks["hooks"]["stop-0"]["Stop"][0].get("matcher"), None);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn remove_generated_plugin_deletes_only_the_derived_directory() {
        let (root, pkg) = make_package_with_mcp("removal");
        let uze_home = UzeHome::at(root.join("uze"));
        materialize_generated_plugin(&uze_home, &pkg).unwrap();
        assert!(generated_package_dir_for_id(&uze_home, pkg.id.as_str()).exists());
        remove_generated_plugin_by_id(&uze_home, pkg.id.as_str()).unwrap();
        assert!(!generated_package_dir_for_id(&uze_home, pkg.id.as_str()).exists());
        assert!(
            pkg.root.join("skills/commit/SKILL.md").is_file(),
            "Store bytes untouched"
        );
        let _ = fs::remove_dir_all(root);
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
