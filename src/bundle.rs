use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::{
    capability::{EnhancementKind, ItemKind, PortableKind},
    error::{Result, UzeError},
    project::{files_named, read_file},
};

#[derive(Clone, Debug, Serialize)]
pub struct ImportedBundle {
    pub root: PathBuf,
    pub manifest: PathBuf,
    pub standard_items: Vec<BundleItem>,
    pub optional_enhancements: Vec<BundleItem>,
    pub compatibility_fallback: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct BundleItem {
    pub path: PathBuf,
    pub kind: ItemKind,
    pub byte_len: usize,
}

pub fn import_bundle(root: impl AsRef<Path>) -> Result<ImportedBundle> {
    let root = root.as_ref();
    if !root.exists() {
        return Err(UzeError::MissingPath(root.to_path_buf()));
    }
    if !root.is_dir() {
        return Err(UzeError::NotDirectory(root.to_path_buf()));
    }
    let root = root.canonicalize().map_err(|source| UzeError::Read {
        path: root.to_path_buf(),
        source,
    })?;
    let manifest = [
        root.join(".claude-plugin/plugin.json"),
        root.join("plugin.json"),
    ]
    .into_iter()
    .find(|path| path.is_file())
    .ok_or_else(|| UzeError::MissingManifest(root.clone()))?;
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
            standard_items.push(bundle_item(path, ItemKind::Portable(PortableKind::Skill))?);
        }
    }
    let mut optional_enhancements = Vec::new();
    for (directory, kind) in [
        ("commands", EnhancementKind::Command),
        ("agents", EnhancementKind::Subagent),
        ("hooks", EnhancementKind::Hook),
    ] {
        let directory_path = root.join(directory);
        if directory_path.is_dir() {
            optional_enhancements.push(bundle_item(directory_path, ItemKind::Enhancement(kind))?);
        }
    }
    standard_items.sort_by(|left, right| left.path.cmp(&right.path));
    optional_enhancements.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(ImportedBundle {
        root,
        manifest,
        standard_items,
        optional_enhancements,
        compatibility_fallback: true,
    })
}

fn bundle_item(path: PathBuf, kind: ItemKind) -> Result<BundleItem> {
    let byte_len = if path.is_file() {
        read_file(&path)?.len()
    } else {
        0
    };
    Ok(BundleItem {
        path,
        kind,
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
        assert!(imported.compatibility_fallback);
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

    fn temp_bundle(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("uze-{label}-{}-{nonce}", std::process::id()))
    }
}
