//! Non-destructive read/set/write for a shared vendor JSON config file.
//!
//! Same invariants as `hooks.rs`'s hook-merge primitive (read whole as a
//! generic `Value`; a missing file is an empty object; refuse to touch a
//! non-object shape rather than overwrite it) — genuinely identical logic
//! needed by every JSON-configured integration (Claude, OpenCode,
//! Antigravity), so it lives here rather than being copied three times.
//! Every write into a vendor config (here, in `hooks.rs`, and in OpenCode's
//! MCP attach/detach) goes through `persistence::write_atomic` so a crash
//! mid-merge can never corrupt a user config file.

use std::{fs, path::Path};

use uze_core::{Result, UzeError, persistence::write_atomic};

/// Reads `path` as a JSON object. A missing file is an empty object; a
/// non-object root is refused rather than silently discarded.
pub(crate) fn read_object(path: &Path) -> std::result::Result<serde_json::Value, String> {
    match fs::read(path) {
        Ok(bytes) if bytes.is_empty() => Ok(serde_json::json!({})),
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| format!("`{}` is not readable JSON: {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(serde_json::json!({})),
        Err(error) => Err(format!("`{}` cannot be read: {error}", path.display())),
    }
    .and_then(|value| {
        if value.is_object() {
            Ok(value)
        } else {
            Err(format!("`{}` root must be a JSON object", path.display()))
        }
    })
}

/// Sets a dot-path (e.g. `["permissions", "defaultMode"]`) inside `config`,
/// creating missing intermediate objects. Refuses to descend through a key
/// that already holds a non-object value — a foreign shape UZE must not
/// clobber (e.g. some other tool already turned `permissions` into an array).
pub(crate) fn set_path(
    config: &mut serde_json::Value,
    path: &[&str],
    value: serde_json::Value,
) -> std::result::Result<(), String> {
    let Some((last, ancestors)) = path.split_last() else {
        return Err("empty config key path".to_owned());
    };
    let mut cursor = config
        .as_object_mut()
        .ok_or_else(|| "config root must be a JSON object".to_owned())?;
    for key in ancestors {
        cursor = cursor
            .entry(*key)
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .ok_or_else(|| format!("`{key}` already holds a non-object value; preserved"))?;
    }
    cursor.insert((*last).to_owned(), value);
    Ok(())
}

/// Writes `config` back with a trailing newline, atomically.
/// Removes `path` if it exists, leaving every other key untouched. An
/// absent key is a successful no-op: the caller asked for it to be gone.
pub(crate) fn remove_path(config: &mut serde_json::Value, path: &[&str]) {
    let Some((last, ancestors)) = path.split_last() else {
        return;
    };
    let mut cursor = match config.as_object_mut() {
        Some(object) => object,
        None => return,
    };
    for key in ancestors {
        cursor = match cursor
            .get_mut(*key)
            .and_then(serde_json::Value::as_object_mut)
        {
            Some(next) => next,
            None => return,
        };
    }
    cursor.remove(*last);
}

pub(crate) fn write_object(path: &Path, config: &serde_json::Value) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(config).expect("preference config serializes");
    bytes.push(b'\n');
    write_atomic(path, &bytes)
}

/// Convenience for a `PreferencePort::apply` implementation: read, apply one
/// mutation, write — surfacing a merge failure as `UzeError::ExposureUnavailable`
/// the same way `hooks.rs` does for its own merge failures.
pub(crate) fn merge(
    path: &Path,
    mutate: impl FnOnce(&mut serde_json::Value) -> std::result::Result<(), String>,
) -> Result<()> {
    let mut config = read_object(path).map_err(|reason| {
        UzeError::ExposureUnavailable(format!("cannot update preferences: {reason}"))
    })?;
    mutate(&mut config).map_err(|reason| {
        UzeError::ExposureUnavailable(format!("cannot update preferences: {reason}"))
    })?;
    write_object(path, &config)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(label: &str) -> std::path::PathBuf {
        uze_testkit::temp::scratch(label).join("json-config.json")
    }

    #[test]
    fn missing_file_reads_as_empty_object() {
        let path = temp_path("missing");
        assert_eq!(read_object(&path).unwrap(), serde_json::json!({}));
    }

    #[test]
    fn a_non_object_root_is_refused() {
        let path = temp_path("non-object");
        fs::write(&path, "[1,2,3]").unwrap();
        assert!(read_object(&path).is_err());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn set_path_creates_intermediate_objects_and_preserves_foreign_keys() {
        let mut config =
            serde_json::json!({"foreignKey": "untouched", "permissions": {"allow": ["Bash(ls)"]}});
        set_path(
            &mut config,
            &["permissions", "defaultMode"],
            serde_json::json!("acceptEdits"),
        )
        .unwrap();
        assert_eq!(config["foreignKey"], "untouched");
        assert_eq!(
            config["permissions"]["allow"],
            serde_json::json!(["Bash(ls)"])
        );
        assert_eq!(config["permissions"]["defaultMode"], "acceptEdits");
    }

    #[test]
    fn set_path_refuses_a_foreign_non_object_intermediate() {
        let mut config = serde_json::json!({"permissions": "not-an-object"});
        assert!(
            set_path(
                &mut config,
                &["permissions", "defaultMode"],
                serde_json::json!("auto")
            )
            .is_err()
        );
    }

    #[test]
    fn merge_round_trips_through_disk_atomically() {
        let path = temp_path("merge");
        merge(&path, |config| {
            set_path(config, &["model"], serde_json::json!("opus"))
        })
        .unwrap();
        let written = read_object(&path).unwrap();
        assert_eq!(written["model"], "opus");
        // Scoped to this test's own file name: `path.parent()` is the shared
        // system temp dir, where other tests write concurrently.
        let file_name = path.file_name().unwrap().to_string_lossy().into_owned();
        let leftovers = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .any(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                name.starts_with(&format!(".{file_name}.")) && name.ends_with(".tmp")
            });
        assert!(!leftovers);
        let _ = fs::remove_file(&path);
    }
}
