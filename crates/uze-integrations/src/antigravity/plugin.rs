//! Antigravity native package delivery: `agy plugin install
//! <package-directory>`, pointing straight at a package directory (the
//! Store's, or a UZE-owned generated one), never publishing a catalogue
//! (Antigravity needs none — see the module doc on
//! [`super::AntigravityIntegration`]).
//!
//! Unlike link-based installers, there is no link route: `plugin
//! install` stages a **byte copy** at `~/.gemini/config/plugins/<name>/`
//! (symlinks are dereferenced — verified against 1.1.19), so the staged
//! tree is deliberately treated as a Derived Artifact (ADR-013 §4): UZE
//! rebuilds it from the Store on attach, records a content fingerprint as
//! its ownership proof, and removes it through `agy plugin uninstall` on
//! detach. No Store bytes are ever read from the staged copy.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use uze_core::{
    Result, UzeError,
    integration::{AttachmentInspection, AttachmentReceipt, AttachmentState, IntegrationPort},
    project::Resource,
    store::StoredPackage,
};

use super::AntigravityIntegration;
use crate::shared::process::run_quiet;

/// The `kind` stamped on explicit-plugin receipts. Only this module and its
/// composition root interpret it.
pub(super) const PLUGIN_KIND: &str = "antigravity-plugin";
/// The `kind` stamped on generated-plugin receipts (see `generate.rs`).
pub(super) const GENERATED_PLUGIN_KIND: &str = "antigravity-plugin-generated";

/// The vendor's plugin-name pattern (`plugin.json` docs + `invalid plugin
/// name` error): alphanumerics, hyphens, underscores.
pub(super) fn valid_plugin_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == '_'
        })
}

/// The canonical manifest's `name` field, when it is a usable Antigravity
/// plugin name. The canonical UZE `plugin.json` doubles as the vendor
/// manifest, so this — not a separate vendor-specific file — is what
/// decides explicitness. A missing or unparseable manifest, or a name that
/// does not satisfy the vendor pattern, yields `None` (the package falls
/// back to capability-level delivery; no generated name is ever invented).
pub(super) fn plugin_manifest_name(package: &StoredPackage) -> Option<String> {
    let bytes = fs::read(&package.manifest).ok()?;
    let manifest: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let name = manifest.get("name")?.as_str()?;
    valid_plugin_name(name).then(|| name.to_owned())
}

/// MCP server names declared by an author-shipped `mcp_config.json` at the
/// package root — the one Antigravity-specific file an author may provide.
/// Shared with `generate.rs`'s coverage logic.
pub(super) fn author_mcp_config_servers(package: &StoredPackage) -> BTreeSet<String> {
    declared_servers(&package.root.join("mcp_config.json"))
}

pub(super) fn declared_servers(path: &Path) -> BTreeSet<String> {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|value| {
            value
                .get("mcpServers")
                .and_then(serde_json::Value::as_object)
                .map(|servers| servers.keys().cloned().collect())
        })
        .unwrap_or_default()
}

/// Computes which of `resources` are actually covered by an explicit
/// Antigravity plugin — the intersection ADR-013 §2 requires
/// (`provided = discovered ∩ declared`), mirroring the other
/// integrations' exact-coverage functions. Antigravity's schema declares
/// no `skills` paths at all: coverage is structural, and semantic-aware
/// (ADR-030 §13) — a Skill is covered iff its directory lives under the
/// fixed `skills/` subdirectory AND its canonical `invoke:` policy is the
/// default, because Antigravity has no explicit-only mechanism and Skills
/// stay model-discoverable and slash-invocable; a non-default policy falls
/// through to capability-level delivery, which reports the degradation
/// honestly. An MCP server is covered iff its name is declared in the
/// root `mcp_config.json` (a missing or malformed file contributes no
/// coverage; it never errors).
pub(super) fn exact_coverage(package: &StoredPackage, resources: &[&Resource]) -> BTreeSet<String> {
    let declared_mcp = author_mcp_config_servers(package);
    let mut provided = BTreeSet::new();
    for resource in resources {
        match resource.capability.kind {
            uze_core::capability::CapabilityKind::AgentSkill => {
                if under_conventional_dir(&resource.capability.path, &package.root, "skills")
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
            _ => {}
        }
    }
    provided
}

/// Component-wise containment test: `relative` (resource path relative to
/// `root`) must start with exactly the `conventional` directory — a
/// `skills-extra` sibling is never mistaken for inside `skills`, the same
/// discipline Codex's coverage function uses.
fn under_conventional_dir(path: &Path, root: &Path, conventional: &str) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    let Some(parent) = relative.parent() else {
        return false;
    };
    parent.starts_with(conventional)
}

// --- CLI verbs ---------------------------------------------------------------

pub(super) fn run_agy(
    executable: &str,
    command_home: &Path,
    args: &[&str],
    label: &str,
) -> Result<()> {
    run_quiet(Path::new(executable), command_home, label, args)
}

/// `agy plugin list` writes its machine-readable JSON document to stdout
/// and exits 0 (verified against 1.1.19; `{"imports":[...]}`). Falling back
/// to stderr keeps this correct if a future release moves the payload — the
/// same defensive stdout-first choice the other integrations make.
pub(super) fn agy_json(
    executable: &str,
    command_home: &Path,
    args: &[&str],
) -> std::result::Result<serde_json::Value, String> {
    use std::process::Command;
    let output = Command::new(executable)
        .env("HOME", command_home)
        .args(args)
        .output()
        .map_err(|error| format!("failed to run `agy`: {error}"))?;
    if !output.status.success() {
        return Err(format!("`agy` inspection exited with {}", output.status));
    }
    let payload = if output.stdout.iter().any(|byte| !byte.is_ascii_whitespace()) {
        &output.stdout
    } else {
        &output.stderr
    };
    serde_json::from_slice(payload).map_err(|error| format!("agy JSON is invalid: {error}"))
}

/// The full `agy plugin list` document. An unreadable listing is an error
/// — inspection must never guess about ownership from silence.
pub(super) fn installed_plugins(
    executable: &str,
    command_home: &Path,
) -> std::result::Result<serde_json::Value, String> {
    agy_json(executable, command_home, &["plugin", "list"])
}

/// The ownership decision for one installed plugin, separated from the
/// process call so every branch is testable without an `agy` binary.
/// Ownership is proven by registration + staged identity + content
/// fingerprint — the vendor's manifest has no source path, so the
/// fingerprint is the only thing that distinguishes UZE's copy from a
/// user-imported same-name plugin.
pub(super) fn inspect_installed_plugin(
    listing: &serde_json::Value,
    name: &str,
    staged_dir: &Path,
    expected_fingerprint: &str,
) -> AttachmentInspection {
    let Some(entries) = listing.get("imports").and_then(serde_json::Value::as_array) else {
        return AttachmentInspection {
            state: AttachmentState::Blocked,
            reason: "agy plugin list has no imports array".to_owned(),
        };
    };
    let matching: Vec<&serde_json::Value> = entries
        .iter()
        .filter(|entry| entry.get("name").and_then(serde_json::Value::as_str) == Some(name))
        .collect();
    match matching.as_slice() {
        [] => {
            return AttachmentInspection {
                state: AttachmentState::Missing,
                reason: "Antigravity plugin is not imported".to_owned(),
            };
        }
        [_] => {}
        _ => {
            return AttachmentInspection {
                state: AttachmentState::Conflict,
                reason: "more than one Antigravity plugin answers to this name".to_owned(),
            };
        }
    }
    if !staged_dir.is_dir() {
        return AttachmentInspection {
            state: AttachmentState::Missing,
            reason: "Antigravity plugin staging directory is missing".to_owned(),
        };
    }
    if !staged_dir.join("plugin.json").is_file() {
        return AttachmentInspection {
            state: AttachmentState::Drifted,
            reason: "Antigravity plugin staging directory no longer holds a plugin.json".to_owned(),
        };
    }
    let actual_fingerprint = match fingerprint_dir(staged_dir) {
        Ok(fingerprint) => fingerprint,
        Err(error) => {
            return AttachmentInspection {
                state: AttachmentState::Blocked,
                reason: error.to_string(),
            };
        }
    };
    if actual_fingerprint != expected_fingerprint {
        return AttachmentInspection {
            state: AttachmentState::Drifted,
            reason: "Antigravity plugin staged content differs from the receipt".to_owned(),
        };
    }
    // Enablement is deliberately not part of this ownership proof: `disable`
    // is a user preference on an artifact UZE still demonstrably created
    // (the same rationale every other integration applies to its vendor's
    // enablement signal).
    AttachmentInspection {
        state: AttachmentState::Matched,
        reason: "Antigravity plugin staged copy matches receipt (enablement is a user preference, not an ownership signal)"
            .to_owned(),
    }
}

// --- Attachment --------------------------------------------------------------

/// Attaches a package whose canonical `plugin.json` is itself a valid
/// Antigravity plugin manifest, straight from the Store.
pub(super) fn attach_explicit_plugin(
    executable: &str,
    integration: &AntigravityIntegration,
    package: &StoredPackage,
) -> Result<Option<AttachmentReceipt>> {
    let name = plugin_manifest_name(package).ok_or_else(|| {
        UzeError::ExposureUnavailable("package has no usable plugin name".to_owned())
    })?;
    if !preflight_name_free(executable, integration, &name) {
        return Ok(None);
    }
    let args: Vec<&Path> = vec![Path::new("plugin"), Path::new("install"), &package.root];
    run_quiet(
        Path::new(executable),
        &integration.command_home,
        &format!("agy plugin install `{name}`"),
        &args,
    )?;
    let staged_dir = integration.plugins_dir.join(&name);
    let fingerprint = fingerprint_dir(&staged_dir)?;
    Ok(Some(AttachmentReceipt {
        package_id: package.id.as_str().to_owned(),
        resource_identity: None,
        integration: integration.id().to_owned(),
        strategy: "native-plugin-install".to_owned(),
        artifact: uze_core::integration::ManagedArtifact::IntegrationOwned {
            kind: PLUGIN_KIND.to_owned(),
            selector: name,
            detail: [
                ("source_path".to_owned(), serde_json::json!(package.root)),
                ("staged_path".to_owned(), serde_json::json!(staged_dir)),
                ("package_root".to_owned(), serde_json::json!(package.root)),
                ("fingerprint".to_owned(), serde_json::json!(fingerprint)),
            ]
            .into_iter()
            .collect(),
        },
    }))
}

/// Attaches a package that needs a generated envelope (canonical MCP
/// translation) through a UZE-owned derived directory.
pub(super) fn attach_generated_plugin(
    executable: &str,
    integration: &AntigravityIntegration,
    package: &StoredPackage,
) -> Result<Option<AttachmentReceipt>> {
    let name = plugin_manifest_name(package).ok_or_else(|| {
        UzeError::ExposureUnavailable("package has no usable plugin name".to_owned())
    })?;
    if !preflight_name_free(executable, integration, &name) {
        return Ok(None);
    }
    let derived_dir =
        super::generate::materialize_generated_plugin(&integration.uze_home, package)?;
    let args: Vec<&Path> = vec![Path::new("plugin"), Path::new("install"), &derived_dir];
    run_quiet(
        Path::new(executable),
        &integration.command_home,
        &format!("agy plugin install `{name}`"),
        &args,
    )?;
    // Antigravity stages a copy named after the *source* directory it was
    // given, not the plugin's own declared manifest name — and `derived_dir`
    // is named by the qualified Store id (`generated_package_dir_for_id`),
    // not `name`. Using `name` here would look for the staged copy under
    // the wrong directory whenever the two differ (always, since
    // `PackageId`s are marketplace-qualified — see ADR-036).
    let staged_dir = integration.plugins_dir.join(
        derived_dir
            .file_name()
            .expect("generated plugin dir has a name"),
    );
    let fingerprint = fingerprint_dir(&staged_dir)?;
    Ok(Some(AttachmentReceipt {
        package_id: package.id.as_str().to_owned(),
        resource_identity: None,
        integration: integration.id().to_owned(),
        strategy: "native-plugin-generated".to_owned(),
        artifact: uze_core::integration::ManagedArtifact::IntegrationOwned {
            kind: GENERATED_PLUGIN_KIND.to_owned(),
            selector: name,
            detail: [
                ("source_path".to_owned(), serde_json::json!(derived_dir)),
                ("staged_path".to_owned(), serde_json::json!(staged_dir)),
                ("package_root".to_owned(), serde_json::json!(package.root)),
                ("fingerprint".to_owned(), serde_json::json!(fingerprint)),
                ("origin".to_owned(), serde_json::json!("generated")),
            ]
            .into_iter()
            .collect(),
        },
    }))
}

/// UZE never overwrites an import it does not own. The vendor's install
/// verb merges over an existing same-name plugin (verified: stale files
/// survive a re-install), so a name already registered — with no receipt in
/// the ledger — is foreign state, not something to clobber or silently
/// resume. It is nevertheless a successful no-op: the harness already
/// exposes a native plugin under the requested name, and UZE must neither
/// replace it nor present ordinary setup as failed.
fn preflight_name_free(executable: &str, integration: &AntigravityIntegration, name: &str) -> bool {
    // If the listing itself is unreadable (agy absent, malformed output),
    // let the install verb speak for itself rather than double-guessing here.
    if let Ok(listing) = installed_plugins(executable, &integration.command_home) {
        let already = listing
            .get("imports")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|entries| {
                entries.iter().any(|entry| {
                    entry.get("name").and_then(serde_json::Value::as_str) == Some(name)
                })
            });
        if already {
            return false;
        }
    }
    true
}

/// Deterministic content fingerprint of a directory tree (relative path +
/// bytes, files only, sorted). FNV-1a 64 — stable across Rust releases and
/// platforms, which receipts persisted across versions require. The staged
/// copy is byte-identical to the installed source (verified against 1.1.19:
/// `diff -r` on the staged tree), so the fingerprint recorded at attach time
/// and the one recomputed at inspection time agree by construction.
pub(super) fn fingerprint_dir(dir: &Path) -> Result<String> {
    let mut files: Vec<PathBuf> = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = fs::read_dir(&current).map_err(|source| UzeError::Read {
            path: current.clone(),
            source,
        })?;
        for entry in entries.flatten() {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|source| UzeError::Read {
                path: path.clone(),
                source,
            })?;
            if metadata.is_symlink() {
                // Never follow symlinks out of the tree; a symlink adds no
                // content of its own and the install verb dereferences it.
                continue;
            }
            if metadata.is_dir() {
                stack.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files.sort();
    let mut digest = String::new();
    for path in files {
        let relative = path.strip_prefix(dir).map_err(|_| {
            UzeError::ExposureUnavailable("fingerprint path escaped its root".to_owned())
        })?;
        let bytes = fs::read(&path).map_err(|source| UzeError::Read {
            path: path.clone(),
            source,
        })?;
        digest.push_str(&relative.to_string_lossy());
        digest.push('\0');
        digest.push_str(&bytes.len().to_string());
        digest.push('\0');
        digest.push_str(&fnv1a64(&bytes));
        digest.push('\n');
    }
    Ok(digest)
}

/// FNV-1a 64-bit digest, implemented locally (no hash-crate dependency) so
/// it is stable across Rust releases and platforms — receipts persisted
/// across versions require that, and `DefaultHasher`'s algorithm is not
/// guaranteed stable.
fn fnv1a64(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod plugin_tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::{Path, PathBuf};

    use uze_core::capability::{Capability, CapabilityKind, Representation};
    use uze_core::home::UzeHome;
    use uze_core::integration::{AttachmentState, IntegrationPort};
    use uze_core::project::Resource;
    use uze_core::store::StoredPackage;

    use super::super::AntigravityIntegration;
    use super::*;

    fn temp_root(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "uze-antigravity-plugin-{label}-{nonce}-{}",
            std::process::id()
        ))
    }

    fn make_package(label: &str, manifest: &str) -> (PathBuf, StoredPackage) {
        let root = temp_root(label);
        let pkg_root = root.join("pkg");
        fs::create_dir_all(&pkg_root).unwrap();
        fs::write(pkg_root.join("plugin.json"), manifest).unwrap();
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

    fn skill_resource(pkg: &StoredPackage, dir: &str, name: &str) -> Resource {
        let path = pkg.root.join(dir).join(name).join("SKILL.md");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, format!("---\nname: {name}\n---\n")).unwrap();
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

    fn policy_skill_resource(
        pkg: &StoredPackage,
        dir: &str,
        name: &str,
        payload: &str,
    ) -> Resource {
        let path = pkg.root.join(dir).join(name).join("SKILL.md");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, payload).unwrap();
        Resource::from_package(
            pkg.id.clone(),
            pkg.root.clone(),
            Capability {
                kind: CapabilityKind::AgentSkill,
                representation: Representation::Standard,
                path,
                payload: payload.as_bytes().to_vec(),
            },
        )
    }

    fn mcp_resource(pkg: &StoredPackage, name: &str) -> Resource {
        let path = pkg.root.join("mcp_config.json");
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

    // --- Manifest/name rules ----------------------------------------------

    #[test]
    fn valid_plugin_names_follow_the_vendor_pattern() {
        assert!(valid_plugin_name("flow"));
        assert!(valid_plugin_name("my-plugin"));
        assert!(valid_plugin_name("my_plugin"));
        assert!(!valid_plugin_name("Bad Name!"));
        assert!(!valid_plugin_name("flow:review"));
        assert!(!valid_plugin_name(""));
    }

    #[test]
    fn a_valid_manifest_name_decides_the_explicit_route() {
        let (_root, pkg) = make_package("name-ok", r#"{"name":"flow","version":"1.0.0"}"#);
        assert_eq!(plugin_manifest_name(&pkg).as_deref(), Some("flow"));
        let (_root2, pkg2) = make_package("name-bad", r#"{"name":"Bad Name!","version":"1.0.0"}"#);
        assert_eq!(plugin_manifest_name(&pkg2), None);
        let (_root3, pkg3) = make_package("name-missing", r#"{"version":"1.0.0"}"#);
        assert_eq!(plugin_manifest_name(&pkg3), None);
    }

    // --- Exact coverage -----------------------------------------------------

    fn listing_with(name: &str) -> serde_json::Value {
        serde_json::json!({"imports":[{"name": name, "source":"antigravity", "components":["skills"]}]})
    }

    /// A. Conventional default-policy skill + declared MCP → covered; skill
    /// outside `skills/` → not covered; a non-default policy Skill → never
    /// claimed (semantic degradation, ADR-030 §13).
    #[test]
    fn explicit_coverage_covers_exactly_the_conventional_and_declared_surface() {
        let (_root, pkg) = make_package("explicit-full", r#"{"name":"flow"}"#);
        fs::write(
            pkg.root.join("mcp_config.json"),
            r#"{"mcpServers":{"mcp-a":{"command":"a"}}}"#,
        )
        .unwrap();
        let r_skill = skill_resource(&pkg, "skills", "commit");
        let r_out = skill_resource(&pkg, "extra", "outside");
        let r_user_only = policy_skill_resource(
            &pkg,
            "skills",
            "review",
            "---\nname: review\ninvoke:\n  model: false\n  user: true\n---\nBody.\n",
        );
        let r_mcp = mcp_resource(&pkg, "mcp-a");
        let resources = vec![&r_skill, &r_out, &r_user_only, &r_mcp];
        let covered = exact_coverage(&pkg, &resources);
        assert_eq!(
            covered,
            BTreeSet::from([r_skill.identity(), r_mcp.identity()])
        );
        assert!(!covered.contains(&r_out.identity()));
        assert!(
            !covered.contains(&r_user_only.identity()),
            "a non-default policy degrades on Antigravity and must never be claimed as native coverage"
        );
        let _ = fs::remove_dir_all(_root);
    }

    /// B. A missing mcp_config.json contributes no MCP coverage, never an error.
    #[test]
    fn missing_author_mcp_config_yields_no_mcp_coverage() {
        let (_root, pkg) = make_package("explicit-no-mcp", r#"{"name":"flow"}"#);
        let r_m = mcp_resource(&pkg, "mcp-a");
        let resources = vec![&r_m];
        assert!(exact_coverage(&pkg, &resources).is_empty());
        let _ = fs::remove_dir_all(_root);
    }

    /// C. A malformed author mcp_config.json is tolerated as no declaration.
    #[test]
    fn malformed_author_mcp_config_is_tolerated_as_no_declaration() {
        let (_root, pkg) = make_package("explicit-malformed", r#"{"name":"flow"}"#);
        fs::write(pkg.root.join("mcp_config.json"), "{not json").unwrap();
        let r_m = mcp_resource(&pkg, "mcp-a");
        let covered = exact_coverage(&pkg, &[&r_m]);
        assert!(covered.is_empty());
        let _ = fs::remove_dir_all(_root);
    }

    // --- Fingerprint ---------------------------------------------------------

    #[test]
    fn fingerprint_is_deterministic_and_content_sensitive() {
        let root = temp_root("fingerprint");
        fs::create_dir_all(root.join("a/skills/x")).unwrap();
        fs::write(root.join("a/plugin.json"), r#"{"name":"x"}"#).unwrap();
        fs::write(root.join("a/skills/x/SKILL.md"), "body").unwrap();
        let one = fingerprint_dir(&root.join("a")).unwrap();
        let two = fingerprint_dir(&root.join("a")).unwrap();
        assert_eq!(one, two);
        fs::write(root.join("a/skills/x/SKILL.md"), "different").unwrap();
        assert_ne!(one, fingerprint_dir(&root.join("a")).unwrap());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn fingerprint_is_order_independent() {
        let root = temp_root("fingerprint-order");
        let a = root.join("a");
        let b = root.join("b");
        for dir in [&a, &b] {
            fs::create_dir_all(dir.join("skills")).unwrap();
        }
        // Same final content, different creation order (a: plugin.json first;
        // b: skill first) — the digest must not depend on insertion order.
        fs::write(a.join("plugin.json"), r#"{"name":"x"}"#).unwrap();
        fs::write(b.join("plugin.json"), r#"{"name":"x"}"#).unwrap();
        fs::write(b.join("skills/SKILL.md"), "same").unwrap();
        fs::write(a.join("skills/SKILL.md"), "same").unwrap();
        assert_eq!(fingerprint_dir(&a).unwrap(), fingerprint_dir(&b).unwrap());
        let _ = fs::remove_dir_all(root);
    }

    // --- Inspection (pure, no binary) ---------------------------------------

    fn staged_tree(root: &Path, name: &str, content: &str) -> PathBuf {
        let dir = root.join("config/plugins").join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("plugin.json"), content).unwrap();
        dir
    }

    #[test]
    fn an_absent_plugin_is_missing_not_blocked() {
        let listing = serde_json::json!({"imports":[]});
        let inspection =
            inspect_installed_plugin(&listing, "flow", Path::new("/nope/flow"), "fingerprint");
        assert_eq!(inspection.state, AttachmentState::Missing);
    }

    #[test]
    fn a_registered_matching_plugin_is_matched() {
        let root = temp_root("inspect-matched");
        let staged = staged_tree(&root, "flow", r#"{"name":"flow"}"#);
        let fingerprint = fingerprint_dir(&staged).unwrap();
        let listing = listing_with("flow");
        let inspection = inspect_installed_plugin(&listing, "flow", &staged, &fingerprint);
        assert_eq!(inspection.state, AttachmentState::Matched);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn changed_staged_content_is_drift() {
        let root = temp_root("inspect-drift");
        let staged = staged_tree(&root, "flow", r#"{"name":"flow"}"#);
        let fingerprint = fingerprint_dir(&staged).unwrap();
        fs::write(
            staged.join("plugin.json"),
            r#"{"name":"flow","description":"tampered"}"#,
        )
        .unwrap();
        let inspection =
            inspect_installed_plugin(&listing_with("flow"), "flow", &staged, &fingerprint);
        assert_eq!(inspection.state, AttachmentState::Drifted);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_registered_plugin_with_a_missing_staging_dir_is_missing() {
        let root = temp_root("inspect-missing-dir");
        let inspection = inspect_installed_plugin(
            &listing_with("flow"),
            "flow",
            &root.join("config/plugins/flow"),
            "fingerprint",
        );
        assert_eq!(inspection.state, AttachmentState::Missing);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn two_entries_answering_to_one_name_are_ambiguous() {
        let listing = serde_json::json!({"imports":[
            {"name":"flow","source":"antigravity"},
            {"name":"flow","source":"other"}
        ]});
        let inspection =
            inspect_installed_plugin(&listing, "flow", Path::new("/nope/flow"), "fingerprint");
        assert_eq!(inspection.state, AttachmentState::Conflict);
    }

    /// Disabling is a user preference on an artifact UZE still owns, so it
    /// must stay MATCHED — otherwise `uze remove` could never detach it.
    #[test]
    fn a_registered_matching_plugin_is_matched_when_no_enablement_field_exists() {
        let root = temp_root("inspect-enablement");
        let staged = staged_tree(&root, "flow", r#"{"name":"flow"}"#);
        let fingerprint = fingerprint_dir(&staged).unwrap();
        // The vendor's list carries no enablement signal for imported
        // plugins (only installs do); identity + fingerprint prove ownership.
        let inspection =
            inspect_installed_plugin(&listing_with("flow"), "flow", &staged, &fingerprint);
        assert_eq!(inspection.state, AttachmentState::Matched);
        let _ = fs::remove_dir_all(root);
    }

    // --- Plan-level precedence (no binary needed) ---------------------------

    #[test]
    fn a_canonical_package_with_no_mcp_takes_the_explicit_route() {
        let (_root, pkg) = make_package("plan-explicit", r#"{"name":"flow","description":"d"}"#);
        let r_a = skill_resource(&pkg, "skills", "commit");
        let uze_home = UzeHome::at(_root.join("uze"));
        let integration = AntigravityIntegration::new(_root.join("agents"), uze_home);
        let plan = integration
            .package_exposure_plan(&pkg, &[&r_a])
            .expect("explicit route applies");
        assert_eq!(plan.route, uze_core::router::CompatibilityRoute::Native);
        assert_eq!(
            plan.provided_resource_identities,
            BTreeSet::from([r_a.identity()])
        );
        let _ = fs::remove_dir_all(_root);
    }

    #[test]
    fn an_invalid_plugin_name_takes_no_native_package_route() {
        let (_root, pkg) = make_package("plan-invalid", r#"{"name":"Bad Name!"}"#);
        let r_a = skill_resource(&pkg, "skills", "commit");
        let uze_home = UzeHome::at(_root.join("uze"));
        let integration = AntigravityIntegration::new(_root.join("agents"), uze_home);
        assert!(
            integration.package_exposure_plan(&pkg, &[&r_a]).is_none(),
            "a package whose canonical name is not a valid Antigravity plugin name must fall back to capability-level delivery"
        );
        let _ = fs::remove_dir_all(_root);
    }
}
