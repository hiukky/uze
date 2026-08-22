//! Gemini's native package delivery: `gemini extensions link`, pointing
//! directly at a package directory in the Store rather than copying it or
//! publishing a catalogue (Gemini needs neither — see the module doc on
//! [`super::GeminiIntegration`]).

use std::{fs, path::Path, process::Command};

use uze_core::{
    Result, UzeError,
    integration::{
        AttachmentInspection, AttachmentReceipt, AttachmentState, IntegrationPort, ManagedArtifact,
    },
    store::StoredPackage,
};

use super::LINKED_EXTENSION;
use super::blocked;

impl super::GeminiIntegration {
    pub(super) fn receipt(
        &self,
        package: &StoredPackage,
        extension_name: &str,
    ) -> AttachmentReceipt {
        AttachmentReceipt {
            package_id: package.id.as_str().to_owned(),
            resource_identity: None,
            integration: self.id().to_owned(),
            strategy: "linked-native-extension".to_owned(),
            artifact: ManagedArtifact::IntegrationOwned {
                kind: LINKED_EXTENSION.to_owned(),
                selector: extension_name.to_owned(),
                detail: [
                    // What Gemini reports back as the extension's `path` for a
                    // link install. Ownership is proven by identity *and*
                    // source, never by name alone.
                    ("source_path".to_owned(), serde_json::json!(package.root)),
                    ("package_root".to_owned(), serde_json::json!(package.root)),
                ]
                .into_iter()
                .collect(),
            },
        }
    }
}

/// The extension's own declared name, which is the selector every Gemini
/// verb takes. Read from the preserved external manifest — never invented.
pub(super) fn extension_name(package_root: &Path) -> Result<String> {
    let path = package_root.join("gemini-extension.json");
    let bytes = fs::read(&path).map_err(|source| UzeError::Read {
        path: path.clone(),
        source,
    })?;
    let manifest: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|source| UzeError::Json { path, source })?;
    manifest
        .get("name")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| {
            UzeError::ExposureUnavailable("gemini-extension.json has no string name".to_owned())
        })
}

/// Computes which of `resources` (already discovered by UZE's Engine) are
/// actually covered by a linked Gemini extension — the intersection
/// ADR-013 §2 requires (`provided = discovered ∩ declared`), mirroring
/// Claude's `claude_exact_coverage` and Codex's `codex_exact_coverage`.
/// Gemini's schema differs from both: `gemini-extension.json` declares no
/// `skills` path at all — confirmed by
/// `e2e/fixtures/gemini-native-conformance/gemini-extension.json`, which has
/// no `skills` key even though its sibling `skills/uze-plugin-first/SKILL.md`
/// exists — so Skill coverage is convention-based: a skill is covered iff its
/// directory lives directly under the extension root's fixed `skills/`
/// subdirectory, never a manifest-declared path. `mcpServers` **is** declared
/// in the manifest, inline as an object keyed by server name (the fixture
/// confirms this — unlike Codex's external-file reference). A missing or
/// malformed manifest, or an unexpected `mcpServers` shape, contributes no
/// MCP coverage rather than erroring or panicking; Skill coverage never
/// depends on the manifest parsing at all, since it needs no manifest field.
pub(super) fn gemini_exact_coverage(
    package: &StoredPackage,
    resources: &[&uze_core::project::Resource],
) -> std::collections::BTreeSet<String> {
    let manifest_path = package.root.join("gemini-extension.json");
    let declared_mcp: std::collections::BTreeSet<String> = fs::read(&manifest_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|value| {
            value
                .get("mcpServers")
                .and_then(serde_json::Value::as_object)
                .map(|servers| servers.keys().cloned().collect())
        })
        .unwrap_or_default();

    let mut provided = std::collections::BTreeSet::new();
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
                // Component-wise, not a string prefix — "skills-extra" must
                // never be mistaken for inside "skills", same discipline as
                // Codex's `codex_exact_coverage`.
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

/// Reads Gemini's machine-readable output.
///
/// `gemini extensions list --output-format=json` exits 0 and writes its JSON
/// to **stderr**, leaving stdout empty (confirmed against 0.56.0). Preferring
/// stdout and falling back to stderr keeps this correct either way, so a
/// future release that moves the payload to stdout needs no change here.
fn gemini_json(
    command_home: &Path,
    args: &[&str],
) -> std::result::Result<serde_json::Value, String> {
    let output = Command::new("gemini")
        .env("HOME", command_home)
        .args(args)
        .output()
        .map_err(|error| format!("failed to run `gemini`: {error}"))?;
    if !output.status.success() {
        return Err(format!("`gemini` inspection exited with {}", output.status));
    }
    let payload = if output.stdout.iter().any(|byte| !byte.is_ascii_whitespace()) {
        &output.stdout
    } else {
        &output.stderr
    };
    serde_json::from_slice(payload).map_err(|error| format!("Gemini JSON is invalid: {error}"))
}

/// One installed extension, by name, from Gemini's own machine-readable
/// listing. `None` when absent or when the listing itself is unavailable —
/// callers distinguish those two cases.
pub(super) fn linked_extension(command_home: &Path, name: &str) -> Option<serde_json::Value> {
    let listing = gemini_json(
        command_home,
        &["extensions", "list", "--output-format=json"],
    )
    .ok()?;
    listing
        .as_array()?
        .iter()
        .find(|entry| entry.get("name").and_then(serde_json::Value::as_str) == Some(name))
        .cloned()
}

pub(super) fn inspect_linked_extension(
    command_home: &Path,
    name: &str,
    expected_source: &Path,
) -> AttachmentInspection {
    let listing = match gemini_json(
        command_home,
        &["extensions", "list", "--output-format=json"],
    ) {
        Ok(listing) => listing,
        Err(reason) => return blocked(reason),
    };
    inspect_listing(&listing, name, expected_source)
}

/// The ownership decision, separated from the process call so every branch is
/// testable without a Gemini binary.
fn inspect_listing(
    listing: &serde_json::Value,
    name: &str,
    expected_source: &Path,
) -> AttachmentInspection {
    let Some(entries) = listing.as_array() else {
        return blocked("Gemini extension listing is not an array".to_owned());
    };
    let matching: Vec<&serde_json::Value> = entries
        .iter()
        .filter(|entry| entry.get("name").and_then(serde_json::Value::as_str) == Some(name))
        .collect();
    let entry = match matching.as_slice() {
        [] => {
            return AttachmentInspection {
                state: AttachmentState::Missing,
                reason: "Gemini extension is not installed".to_owned(),
            };
        }
        [entry] => entry,
        // Two extensions answering to one name is ambiguous ownership: UZE
        // cannot prove which one it created, so it must not touch either.
        _ => {
            return AttachmentInspection {
                state: AttachmentState::Conflict,
                reason: "more than one Gemini extension answers to this name".to_owned(),
            };
        }
    };
    let actual_source = entry
        .pointer("/installMetadata/source")
        .and_then(serde_json::Value::as_str);
    let install_type = entry
        .pointer("/installMetadata/type")
        .and_then(serde_json::Value::as_str);
    let Some(actual_source) = actual_source else {
        return blocked("Gemini extension entry has no install source".to_owned());
    };
    if Path::new(actual_source) != expected_source {
        return AttachmentInspection {
            state: AttachmentState::Drifted,
            reason: "Gemini extension points at a different source than the receipt".to_owned(),
        };
    }
    if install_type != Some("link") {
        // A name and source UZE recognizes, delivered by a mechanism it did
        // not use. Someone else owns this entry.
        return AttachmentInspection {
            state: AttachmentState::Conflict,
            reason: "Gemini extension was installed by a different mechanism than UZE's link"
                .to_owned(),
        };
    }
    // Enablement is deliberately *not* part of this ownership proof, for two
    // separate reasons.
    //
    // It is not observable: the listing's `isActive` stays `true` after
    // `gemini extensions disable` (confirmed on 0.56.0). The real state lives
    // in `extension-enablement.json` as path-scoped override globs, and
    // reimplementing that resolution would mean guessing at vendor internals
    // — precisely the guessing ADR-009 exists to forbid.
    //
    // And it is not an ownership signal even if it were observable. Drift
    // means someone repointed the artifact; disabling means the user turned
    // off an artifact UZE still demonstrably created. Treating that as drift
    // would block `uze remove` from detaching UZE's own extension, which is
    // worse behaviour, not safer behaviour. Identity, source and install type
    // together already prove UZE created this entry.
    AttachmentInspection {
        state: AttachmentState::Matched,
        reason: "Gemini linked extension matches receipt (enablement is a user preference, not an ownership signal)".to_owned(),
    }
}

pub(super) fn run_gemini(command_home: &Path, args: &[&str], label: &str) -> Result<()> {
    match Command::new("gemini")
        .env("HOME", command_home)
        .args(args)
        .status()
    {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(UzeError::ExposureUnavailable(format!(
            "`{label}` exited with {status}"
        ))),
        Err(error) => Err(UzeError::ExposureUnavailable(format!(
            "failed to run `{label}`: {error}"
        ))),
    }
}

#[cfg(test)]
mod extension_tests {
    use std::fs;
    use std::path::Path;

    use uze_core::integration::AttachmentState;

    use super::{extension_name, inspect_listing};

    fn listing(name: &str, source: &str, install_type: &str) -> serde_json::Value {
        serde_json::json!([{
            "name": name,
            "installMetadata": { "source": source, "type": install_type },
            "isActive": true
        }])
    }

    /// Ownership is proven by identity *and* source, never by name alone.
    #[test]
    fn a_same_named_extension_from_another_source_is_drift() {
        let entries = listing("x", "/somewhere/else", "link");
        let inspection = inspect_listing(&entries, "x", Path::new("/uze/store/packages/x"));
        assert_eq!(inspection.state, AttachmentState::Drifted);
    }

    /// A name and source UZE recognizes, delivered by a mechanism it did not
    /// use, is foreign ownership — never something to detach.
    #[test]
    fn a_differently_installed_extension_is_a_conflict() {
        let entries = listing("x", "/uze/store/packages/x", "local");
        let inspection = inspect_listing(&entries, "x", Path::new("/uze/store/packages/x"));
        assert_eq!(inspection.state, AttachmentState::Conflict);
    }

    #[test]
    fn two_extensions_answering_to_one_name_are_ambiguous() {
        let entries = serde_json::json!([
            { "name": "x", "installMetadata": { "source": "/a", "type": "link" }, "isActive": true },
            { "name": "x", "installMetadata": { "source": "/b", "type": "link" }, "isActive": true },
        ]);
        let inspection = inspect_listing(&entries, "x", Path::new("/a"));
        assert_eq!(inspection.state, AttachmentState::Conflict);
    }

    #[test]
    fn an_absent_extension_is_missing_not_blocked() {
        let entries = serde_json::json!([]);
        let inspection = inspect_listing(&entries, "x", Path::new("/a"));
        assert_eq!(inspection.state, AttachmentState::Missing);
    }

    /// Disabling is a user preference on an artifact UZE still owns, so it
    /// must stay MATCHED — otherwise `uze remove` could never detach it.
    #[test]
    fn a_disabled_extension_is_still_owned_by_uze() {
        let mut entries = listing("x", "/uze/store/packages/x", "link");
        entries[0]["isActive"] = serde_json::json!(false);
        let inspection = inspect_listing(&entries, "x", Path::new("/uze/store/packages/x"));
        assert_eq!(inspection.state, AttachmentState::Matched);
    }

    #[test]
    fn an_extension_name_is_read_from_the_preserved_manifest_never_invented() {
        let root = std::env::temp_dir().join(format!("uze-gemini-name-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("gemini-extension.json"),
            r#"{"name":"declared-name","version":"1.0.0"}"#,
        )
        .unwrap();
        assert_eq!(extension_name(&root).unwrap(), "declared-name");

        fs::write(root.join("gemini-extension.json"), r#"{"version":"1.0.0"}"#).unwrap();
        assert!(extension_name(&root).is_err());
        let _ = fs::remove_dir_all(root);
    }
}

#[cfg(test)]
mod gemini_native_coverage_tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::PathBuf;

    use uze_core::capability::{Capability, CapabilityKind, Representation};
    use uze_core::home::UzeHome;
    use uze_core::integration::IntegrationPort;
    use uze_core::project::Resource;

    use super::super::GeminiIntegration;
    use super::gemini_exact_coverage;

    fn temp_root(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "uze-gemini-coverage-{label}-{nonce}-{}",
            std::process::id()
        ))
    }

    /// `manifest_body` is written into `gemini-extension.json` verbatim — a
    /// test can supply malformed JSON or an unexpected `mcpServers` shape.
    fn make_package(label: &str, manifest_body: &str) -> (PathBuf, uze_core::store::StoredPackage) {
        let root = temp_root(label);
        let pkg_root = root.join("pkg");
        fs::create_dir_all(&pkg_root).unwrap();
        fs::write(pkg_root.join("gemini-extension.json"), manifest_body).unwrap();
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

    fn skill_resource(pkg: &uze_core::store::StoredPackage, dir: &str, skill: &str) -> Resource {
        let path = pkg.root.join(dir).join(skill).join("SKILL.md");
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
                payload,
            },
            name.to_owned(),
        )
    }

    /// A. Manifest declares mcpServers matching everything discovered; the
    /// discovered skill lives under the conventional `skills/` directory —
    /// full coverage.
    #[test]
    fn manifest_declares_all_is_fully_covered() {
        let (_root, pkg) = make_package(
            "all",
            r#"{"name":"ext","mcpServers":{"mcp-a":{"command":"a"}}}"#,
        );
        let r_a = skill_resource(&pkg, "skills", "a");
        let r_m = mcp_resource(&pkg, "mcp-a");
        let resources = vec![&r_a, &r_m];
        let covered = gemini_exact_coverage(&pkg, &resources);
        let expected: BTreeSet<String> = resources.iter().map(|r| r.identity()).collect();
        assert_eq!(covered, expected);
        let _ = fs::remove_dir_all(_root);
    }

    /// B. Manifest declares only a subset of the discovered MCP servers.
    #[test]
    fn manifest_declares_subset_only_that_subset_is_covered() {
        let (_root, pkg) = make_package(
            "subset",
            r#"{"name":"ext","mcpServers":{"mcp-a":{"command":"a"}}}"#,
        );
        let r_covered = mcp_resource(&pkg, "mcp-a");
        let r_uncovered = mcp_resource(&pkg, "mcp-b");
        let resources = vec![&r_covered, &r_uncovered];
        let covered = gemini_exact_coverage(&pkg, &resources);
        assert_eq!(covered, BTreeSet::from([r_covered.identity()]));
        assert!(!covered.contains(&r_uncovered.identity()));
        let _ = fs::remove_dir_all(_root);
    }

    /// C. Store contains a Skill physically outside the conventional
    /// `skills/` directory — not covered, falls back to individual
    /// attachment.
    #[test]
    fn store_has_a_skill_outside_the_conventional_directory_is_not_covered() {
        let (_root, pkg) = make_package("extra-skill", r#"{"name":"ext"}"#);
        let r_in = skill_resource(&pkg, "skills", "a");
        let r_out = skill_resource(&pkg, "extra", "b");
        let resources = vec![&r_in, &r_out];
        let covered = gemini_exact_coverage(&pkg, &resources);
        assert_eq!(covered, BTreeSet::from([r_in.identity()]));
        assert!(!covered.contains(&r_out.identity()));
        let _ = fs::remove_dir_all(_root);
    }

    /// D. Store contains an MCP resource the manifest never names — not
    /// covered.
    #[test]
    fn store_has_an_mcp_server_not_named_in_the_manifest_is_not_covered() {
        let (_root, pkg) = make_package(
            "extra-mcp",
            r#"{"name":"ext","mcpServers":{"mcp-a":{"command":"a"}}}"#,
        );
        let r_named = mcp_resource(&pkg, "mcp-a");
        let r_extra = mcp_resource(&pkg, "mcp-c");
        let resources = vec![&r_named, &r_extra];
        let covered = gemini_exact_coverage(&pkg, &resources);
        assert_eq!(covered, BTreeSet::from([r_named.identity()]));
        let _ = fs::remove_dir_all(_root);
    }

    /// E. `mcpServers` absent from an otherwise-valid manifest — no MCP
    /// coverage claimed, no panic; Skill coverage (convention-based, no
    /// manifest field needed) is unaffected.
    #[test]
    fn manifest_without_mcp_servers_field_yields_no_mcp_coverage() {
        let (_root, pkg) = make_package("no-mcp-field", r#"{"name":"ext"}"#);
        let r_a = skill_resource(&pkg, "skills", "a");
        let r_m = mcp_resource(&pkg, "mcp-a");
        let resources = vec![&r_a, &r_m];
        let covered = gemini_exact_coverage(&pkg, &resources);
        assert_eq!(covered, BTreeSet::from([r_a.identity()]));
        assert!(!covered.contains(&r_m.identity()));
        let _ = fs::remove_dir_all(_root);
    }

    /// F. `gemini-extension.json` exists but is malformed JSON — no MCP
    /// coverage, not a crash; package still installs natively via
    /// `package_exposure_plan`, and Skill coverage is unaffected since it
    /// needs no manifest field.
    #[test]
    fn malformed_manifest_yields_empty_mcp_coverage_but_package_still_deliverable() {
        let (_root, pkg) = make_package("malformed", "{not json");
        let r_a = skill_resource(&pkg, "skills", "a");
        let r_m = mcp_resource(&pkg, "mcp-a");
        let resources = vec![&r_a, &r_m];
        let covered = gemini_exact_coverage(&pkg, &resources);
        assert_eq!(covered, BTreeSet::from([r_a.identity()]));
        let integration =
            GeminiIntegration::new(_root.join("agents"), UzeHome::at(_root.join("uze")));
        let plan = integration.package_exposure_plan(&pkg, &resources);
        assert!(
            plan.is_some(),
            "malformed native manifest must not block native delivery"
        );
        let _ = fs::remove_dir_all(_root);
    }

    /// G. An unexpected JSON shape for `mcpServers` (an array instead of the
    /// documented name-keyed object) is tolerated as "no servers declared"
    /// rather than panicking or matching every resource.
    #[test]
    fn unexpected_mcp_servers_field_shape_is_tolerated_as_no_declaration() {
        let (_root, pkg) = make_package("wrong-shape", r#"{"name":"ext","mcpServers":["mcp-a"]}"#);
        let r_m = mcp_resource(&pkg, "mcp-a");
        let resources = vec![&r_m];
        let covered = gemini_exact_coverage(&pkg, &resources);
        assert!(covered.is_empty());
        let _ = fs::remove_dir_all(_root);
    }

    /// H. Partial native delivery: a package with one conventionally-placed
    /// skill and one skill/MCP outside the manifest's declared surface must
    /// cover only the conventional/declared ones — no duplicate receipt,
    /// nothing missing, and the uncovered resources still route through the
    /// normal capability-level fallback.
    #[test]
    fn partial_native_coverage_leaves_undeclared_resources_on_the_fallback_path() {
        let (_root, pkg) = make_package("partial", r#"{"name":"ext"}"#);
        let r_native = skill_resource(&pkg, "skills", "skill-native");
        let r_extra_skill = skill_resource(&pkg, "extra", "skill-extra");
        let r_extra_mcp = mcp_resource(&pkg, "mcp-extra");
        let resources = vec![&r_native, &r_extra_skill, &r_extra_mcp];
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
