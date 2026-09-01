//! The canonical-acquisition importer: recognizes the standard Agent
//! Plugins manifest (`plugin.json`) and preserves its standard-native
//! contents byte-for-byte into evidence and portable resources.
//!
//! **Foreign (vendor-authored) format import is not currently
//! implemented.** A `ClaudePluginImporter` — recognizing a package that
//! ships only a foreign `.claude-plugin/plugin.json`, with no canonical
//! `plugin.json` of its own — existed here previously, structurally
//! separate from `ClaudeIntegration` per this module's own original design
//! intent (delivery-time vendor knowledge and acquisition-time
//! foreign-format knowledge were deliberately kept apart — see ADR-005).
//! It was removed by that same ADR: `Store::ingest` (the only real
//! acquisition path) never called it, nothing in the CLI or
//! `uze-application` reached it, and dead vendor-specific code sitting in
//! this otherwise vendor-neutral crate cost more than it proved. If
//! reverse/foreign-format import returns, it should be designed against a
//! real acquisition requirement, in a boundary chosen deliberately then —
//! not resurrected merely because this comment once existed.

use std::path::{Path, PathBuf};

use crate::{
    bundle::{BundleItem, ImportedBundle},
    capability::{CapabilityKind, Representation},
    error::{Result, UzeError},
    project::{files_named, read_file},
};

mod agent_plugin;

pub use agent_plugin::AgentPluginImporter;

/// Imports a representation owned by an external ecosystem into evidence and
/// portable resources. This is distinct from `IntegrationPort`, which exposes
/// already-composed resources to a harness.
///
/// `AgentPluginImporter` is, today, this trait's only implementor —
/// `Store::ingest` depends on the trait rather than the concrete type on
/// purpose (the acquisition/canonicalization contract is the stable thing;
/// which canonical format satisfies it is not assumed to stay singular
/// forever), not because a second implementor is expected imminently.
pub trait ForeignImporter {
    fn import(&self, root: &Path) -> Result<Option<ImportedBundle>>;
}

pub(crate) fn import_from_manifest(
    root: &Path,
    manifest: PathBuf,
    importer: &str,
) -> Result<ImportedBundle> {
    let payload = read_file(&manifest)?;
    let manifest_json: serde_json::Value =
        serde_json::from_slice(&payload).map_err(|source| UzeError::Json {
            path: manifest.clone(),
            source,
        })?;
    validate_references(&manifest_json, &manifest)?;

    let mut standard_items = Vec::new();
    let skills = root.join("skills");
    if skills.is_dir() {
        for path in files_named(&skills, "SKILL.md")? {
            standard_items.push(bundle_item(
                path,
                CapabilityKind::AgentSkill,
                Representation::Standard,
            )?);
        }
    }

    let mut optional_enhancements = Vec::new();
    // `commands/` is deliberately NOT here (ADR-030): explicit-action
    // semantics are carried by a Skill's `invoke:` invocation policy, and
    // a vendor-authored `commands/` directory inside an explicit plugin
    // envelope is delivered natively — never re-discovered canonically.
    for (directory, kind) in [
        ("agents", CapabilityKind::Agent),
        ("hooks", CapabilityKind::Hook),
    ] {
        let directory_path = root.join(directory);
        if directory_path.is_dir() {
            optional_enhancements.push(bundle_item(directory_path, kind, Representation::Foreign)?);
        }
    }
    standard_items.sort_by(|left, right| left.path.cmp(&right.path));
    optional_enhancements.sort_by(|left, right| left.path.cmp(&right.path));

    Ok(ImportedBundle {
        root: root.to_path_buf(),
        manifest,
        importer: importer.to_owned(),
        standard_items,
        optional_enhancements,
        compatibility_fallback: true,
    })
}

fn bundle_item(
    path: PathBuf,
    kind: CapabilityKind,
    representation: Representation,
) -> Result<BundleItem> {
    let byte_len = if path.is_file() {
        read_file(&path)?.len()
    } else {
        0
    };
    Ok(BundleItem {
        path,
        kind,
        representation,
        byte_len,
    })
}

fn validate_references(value: &serde_json::Value, manifest: &Path) -> Result<()> {
    match value {
        serde_json::Value::Object(entries) => {
            for (key, value) in entries {
                let key = key.to_ascii_lowercase();
                if (key.contains("path") || key.contains("file"))
                    && let serde_json::Value::String(reference) = value
                {
                    validate_reference(reference, manifest)?;
                }
                validate_references(value, manifest)?;
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                validate_references(value, manifest)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_reference(reference: &str, manifest: &Path) -> Result<()> {
    let path = Path::new(reference);
    if path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(UzeError::UnsafePathReference {
            path: manifest.to_path_buf(),
            reference: reference.to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    /// Still-valid canonical invariant, now proven directly against the
    /// live `AgentPluginImporter` — this used to go through the dead
    /// multi-importer `import_bundle()` dispatcher (removed by ADR-005),
    /// which added no coverage of its own beyond what calling the one
    /// real importer directly already proves.
    #[test]
    fn imports_skills_without_changing_their_bytes() {
        let root = temp_bundle("roundtrip");
        fs::create_dir_all(root.join("skills/review")).unwrap();
        fs::write(root.join("plugin.json"), "{\"name\":\"demo\"}\n").unwrap();
        let original = b"---\nname: review\n---\nKeep exact bytes.\n";
        fs::write(root.join("skills/review/SKILL.md"), original).unwrap();
        let imported = AgentPluginImporter.import(&root).unwrap().unwrap();
        assert_eq!(
            fs::read(&imported.standard_items[0].path).unwrap(),
            original
        );
        assert_eq!(imported.importer, "agent-plugin");
        fs::remove_dir_all(root).unwrap();
    }

    /// Same reasoning: still a live, load-bearing safety invariant
    /// (`Store::ingest` depends on it transitively), now exercised
    /// directly against `AgentPluginImporter` instead of the removed
    /// dispatcher.
    #[test]
    fn rejects_parent_directory_manifest_reference() {
        let root = temp_bundle("unsafe");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("plugin.json"),
            "{\"scriptPath\":\"../outside.sh\"}",
        )
        .unwrap();
        assert!(matches!(
            AgentPluginImporter.import(&root),
            Err(UzeError::UnsafePathReference { .. })
        ));
        fs::remove_dir_all(root).unwrap();
    }

    fn temp_bundle(label: &str) -> PathBuf {
        uze_testkit::temp::scratch(label)
    }
}
