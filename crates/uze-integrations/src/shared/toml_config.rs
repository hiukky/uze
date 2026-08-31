//! Non-destructive read/set/write for a shared vendor TOML config file
//! (Codex's `config.toml`). Uses `toml_edit`'s document model rather than a
//! plain `toml::Value` round-trip so an untouched key keeps its original
//! formatting and comments — the file is the user's, not UZE's.

use std::{fs, path::Path};

use toml_edit::{DocumentMut, Item, Table};
use uze_core::{Result, UzeError, persistence::write_atomic};

/// Parses `path` as a TOML document. A missing file is an empty document; a
/// file that fails to parse is refused rather than silently discarded.
pub(crate) fn read_document(path: &Path) -> std::result::Result<DocumentMut, String> {
    match fs::read_to_string(path) {
        Ok(text) => text
            .parse::<DocumentMut>()
            .map_err(|error| format!("`{}` is not valid TOML: {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(DocumentMut::new()),
        Err(error) => Err(format!("`{}` cannot be read: {error}", path.display())),
    }
}

/// Sets a dot-path of top-level/nested tables (e.g.
/// `["sandbox_workspace_write", "network_access"]`), creating missing
/// intermediate tables. Refuses to descend through a key that already holds
/// a non-table value.
pub(crate) fn set_path(
    document: &mut DocumentMut,
    path: &[&str],
    value: impl Into<toml_edit::Value>,
) -> std::result::Result<(), String> {
    let Some((last, ancestors)) = path.split_last() else {
        return Err("empty config key path".to_owned());
    };
    let mut table: &mut Table = document.as_table_mut();
    for key in ancestors {
        let entry = table
            .entry(key)
            .or_insert_with(|| Item::Table(Table::new()));
        table = entry
            .as_table_mut()
            .ok_or_else(|| format!("`{key}` already holds a non-table value; preserved"))?;
    }
    table.insert(last, Item::Value(value.into()));
    Ok(())
}

/// Removes a dot-path when all of its ancestors are tables. A missing path is
/// already absent; a non-table ancestor is foreign configuration and is left
/// untouched.
pub(crate) fn remove_path(document: &mut DocumentMut, path: &[&str]) {
    let Some((last, ancestors)) = path.split_last() else {
        return;
    };
    let mut table: &mut Table = document.as_table_mut();
    for key in ancestors {
        let Some(entry) = table.get_mut(key) else {
            return;
        };
        let Some(child) = entry.as_table_mut() else {
            return;
        };
        table = child;
    }
    table.remove(last);
}

/// Writes `document` back atomically.
pub(crate) fn write_document(path: &Path, document: &DocumentMut) -> Result<()> {
    write_atomic(path, document.to_string().as_bytes())
}

/// Convenience for a `PreferencePort::apply` implementation: read, apply one
/// mutation, write.
pub(crate) fn merge(
    path: &Path,
    mutate: impl FnOnce(&mut DocumentMut) -> std::result::Result<(), String>,
) -> Result<()> {
    let mut document = read_document(path).map_err(|reason| {
        UzeError::ExposureUnavailable(format!("cannot update preferences: {reason}"))
    })?;
    mutate(&mut document).map_err(|reason| {
        UzeError::ExposureUnavailable(format!("cannot update preferences: {reason}"))
    })?;
    write_document(path, &document)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(label: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "uze-toml-config-{label}-{}-{nonce}.toml",
            std::process::id()
        ))
    }

    #[test]
    fn missing_file_reads_as_an_empty_document() {
        let path = temp_path("missing");
        assert_eq!(read_document(&path).unwrap().to_string(), "");
    }

    #[test]
    fn set_path_preserves_foreign_keys_tables_and_comments() {
        let path = temp_path("preserve");
        fs::write(
            &path,
            "# a user comment\nmodel = \"gpt-5.6\"\n\n[model_providers.openai]\nname = \"OpenAI\"\n",
        )
        .unwrap();
        let mut document = read_document(&path).unwrap();
        set_path(&mut document, &["approval_policy"], "never").unwrap();
        let rendered = document.to_string();
        assert!(rendered.contains("# a user comment"));
        assert!(rendered.contains("model = \"gpt-5.6\""));
        assert!(rendered.contains("[model_providers.openai]"));
        assert!(rendered.contains("approval_policy = \"never\""));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn set_path_creates_missing_intermediate_tables() {
        let mut document = DocumentMut::new();
        set_path(
            &mut document,
            &["sandbox_workspace_write", "network_access"],
            true,
        )
        .unwrap();
        assert!(document.to_string().contains("[sandbox_workspace_write]"));
        assert!(document.to_string().contains("network_access = true"));
    }

    #[test]
    fn set_path_refuses_a_foreign_non_table_intermediate() {
        let mut document = "sandbox_workspace_write = 1"
            .parse::<DocumentMut>()
            .unwrap();
        assert!(
            set_path(
                &mut document,
                &["sandbox_workspace_write", "network_access"],
                true
            )
            .is_err()
        );
    }

    #[test]
    fn remove_path_leaves_foreign_siblings_intact() {
        let mut document = "[sandbox_workspace_write]\nnetwork_access = false\nextra = true\n"
            .parse::<DocumentMut>()
            .unwrap();
        remove_path(
            &mut document,
            &["sandbox_workspace_write", "network_access"],
        );
        let rendered = document.to_string();
        assert!(!rendered.contains("network_access"));
        assert!(rendered.contains("extra = true"));
    }

    #[test]
    fn merge_round_trips_through_disk_atomically() {
        let path = temp_path("merge");
        merge(&path, |document| {
            set_path(document, &["sandbox_mode"], "workspace-write")
        })
        .unwrap();
        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("sandbox_mode = \"workspace-write\""));
        let _ = fs::remove_file(&path);
    }
}
