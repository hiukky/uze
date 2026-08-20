use std::path::{Path, PathBuf};

use crate::{
    bundle::{BundleItem, ImportedBundle},
    capability::{CapabilityKind, Representation},
    error::{Result, UzeError},
    project::{files_named, read_file},
};

pub trait ForeignImporter {
    fn import(&self, root: &Path) -> Result<Option<ImportedBundle>>;
}

pub struct ClaudePluginImporter;

impl ForeignImporter for ClaudePluginImporter {
    fn import(&self, root: &Path) -> Result<Option<ImportedBundle>> {
        let manifest = root.join(".claude-plugin/plugin.json");
        manifest
            .is_file()
            .then(|| import_from_manifest(root, manifest, "claude-plugin"))
            .transpose()
    }
}

pub struct AgentPluginImporter;

impl ForeignImporter for AgentPluginImporter {
    fn import(&self, root: &Path) -> Result<Option<ImportedBundle>> {
        let manifest = root.join("plugin.json");
        manifest
            .is_file()
            .then(|| import_from_manifest(root, manifest, "agent-plugin"))
            .transpose()
    }
}

pub fn import_bundle(root: impl AsRef<Path>) -> Result<ImportedBundle> {
    let root = checked_root(root.as_ref())?;
    for importer in [
        &ClaudePluginImporter as &dyn ForeignImporter,
        &AgentPluginImporter,
    ] {
        if let Some(imported) = importer.import(&root)? {
            return Ok(imported);
        }
    }
    Err(UzeError::MissingManifest(root))
}

fn checked_root(root: &Path) -> Result<PathBuf> {
    if !root.exists() {
        return Err(UzeError::MissingPath(root.to_path_buf()));
    }
    if !root.is_dir() {
        return Err(UzeError::NotDirectory(root.to_path_buf()));
    }
    root.canonicalize().map_err(|source| UzeError::Read {
        path: root.to_path_buf(),
        source,
    })
}

fn import_from_manifest(root: &Path, manifest: PathBuf, importer: &str) -> Result<ImportedBundle> {
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
    for (directory, kind) in [
        ("commands", CapabilityKind::Action),
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
            .any(|component| matches!(component, std::path::Component::ParentDir))
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

    #[test]
    fn imports_skills_without_changing_their_bytes() {
        let root = temp_bundle("roundtrip");
        fs::create_dir_all(root.join("skills/review")).unwrap();
        fs::write(root.join("plugin.json"), "{\"name\":\"demo\"}\n").unwrap();
        let original = b"---\nname: review\n---\nKeep exact bytes.\n";
        fs::write(root.join("skills/review/SKILL.md"), original).unwrap();

        let imported = import_bundle(&root).unwrap();
        let skill = &imported.standard_items[0];
        assert_eq!(fs::read(&skill.path).unwrap(), original);
        assert_eq!(imported.importer, "agent-plugin");
        fs::remove_dir_all(root).unwrap();
    }

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
            import_bundle(&root),
            Err(UzeError::UnsafePathReference { .. })
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn imports_the_same_skill_used_by_the_conformance_playground() {
        let root =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("playground/agent-plugin-package");
        let imported = import_bundle(&root).unwrap();
        let imported_skill = imported
            .standard_items
            .first()
            .unwrap()
            .path
            .canonicalize()
            .unwrap();
        let packaged_skill = root.join("skills/uze-e2e/SKILL.md").canonicalize().unwrap();

        assert_eq!(imported.importer, "agent-plugin");
        assert_eq!(imported_skill, packaged_skill);
    }

    fn temp_bundle(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("uze-{label}-{}-{nonce}", std::process::id()))
    }
}
