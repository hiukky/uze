//! Managed File — UZE ownership of one *whole* generated file inside a
//! vendor-managed directory, never a delimited region and never the
//! directory itself.
//!
//! This module knows nothing about any harness, any package format, or
//! Commands specifically. It answers exactly one question: "does the file
//! at `path` currently hold exactly `expected_content`, and can it be
//! safely created/inspected/removed without touching anything else?"
//!
//! Ownership is proven by exact content identity — the strictest proof a
//! whole-file artifact supports, and the same discipline ADR-009 applies
//! to managed text regions: never overwrite, never delete what drifted.
//! Everything else (what the content *means*, what format it is, which
//! harness reads it) belongs to the integration that computed
//! `expected_content`; this module is content-agnostic by construction.
//!
//! This is the delivery mechanism for generated vendor representation that
//! cannot be a reference to Store bytes (e.g. a generated vendor command
//! file, a format translation of the canonical markdown). The file it
//! manages is a Derived Artifact (ADR-013 §4): recreatable, never
//! canonical, and never inside the Store.

use std::{fs, path::Path};

use crate::{
    error::{Result, UzeError},
    integration::{AttachmentInspection, AttachmentState},
    persistence::write_atomic,
};

fn blocked(reason: impl Into<String>) -> AttachmentInspection {
    AttachmentInspection {
        state: AttachmentState::Blocked,
        reason: reason.into(),
    }
}

/// Ownership state of one managed file — always scoped to that exact file.
pub fn inspect(path: &Path, expected_content: &str) -> AttachmentInspection {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return match fs::metadata(path) {
            Ok(_) => AttachmentInspection {
                state: AttachmentState::Conflict,
                reason: "managed path exists but cannot be read".to_owned(),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => AttachmentInspection {
                state: AttachmentState::Missing,
                reason: "managed file is absent".to_owned(),
            },
            Err(error) => blocked(format!("managed file cannot be inspected: {error}")),
        };
    };
    if metadata.is_symlink() || !metadata.is_file() {
        return AttachmentInspection {
            state: AttachmentState::Conflict,
            reason: "managed path is occupied by a non-regular-file".to_owned(),
        };
    }
    match fs::read(path) {
        Ok(content) if content == expected_content.as_bytes() => AttachmentInspection {
            state: AttachmentState::Matched,
            reason: "managed file matches expected content".to_owned(),
        },
        Ok(_) => AttachmentInspection {
            state: AttachmentState::Drifted,
            reason: "managed file content differs from expected".to_owned(),
        },
        Err(error) => blocked(format!("managed file cannot be read: {error}")),
    }
}

/// Idempotently creates the file if it is currently `Missing`. Never touches
/// a file that is already `Matched`. Refuses — never overwrites — when it
/// is `Drifted` or `Conflict`: the only safe outcome besides "already
/// correct" is "correctly created," never "silently repaired."
pub fn attach(path: &Path, expected_content: &str) -> Result<()> {
    match inspect(path, expected_content).state {
        AttachmentState::Matched => Ok(()),
        AttachmentState::Missing => write_atomic(path, expected_content.as_bytes()),
        AttachmentState::Drifted => Err(UzeError::ManagedEntryDrift(path.to_path_buf())),
        AttachmentState::Conflict => Err(UzeError::ManagedEntryConflict(path.to_path_buf())),
        AttachmentState::Blocked => Err(UzeError::Read {
            path: path.to_path_buf(),
            source: std::io::Error::other("managed file is in a blocked state"),
        }),
    }
}

/// Removes only a currently matched managed file. Never destructive on a
/// drifted/conflicting file; `Missing` is a safe no-op (already gone). Reads
/// the file once more immediately before unlinking, per the same ADR-009
/// discipline `detach_standard_receipt` applies to symlinks.
pub fn detach(path: &Path, expected_content: &str) -> Result<AttachmentInspection> {
    let inspection = inspect(path, expected_content);
    if inspection.state != AttachmentState::Matched {
        return Ok(inspection);
    }
    let fresh = inspect(path, expected_content);
    if fresh.state != AttachmentState::Matched {
        return Ok(fresh);
    }
    fs::remove_file(path).map_err(|source| UzeError::Write {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(AttachmentInspection {
        state: AttachmentState::Missing,
        reason: "managed file detached".to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(label: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "uze-managed-file-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn attach_creates_then_is_idempotent_and_inspects_matched() {
        let dir = temp("attach");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("command.toml");
        assert_eq!(inspect(&path, "x").state, AttachmentState::Missing);
        attach(&path, "x").unwrap();
        assert_eq!(inspect(&path, "x").state, AttachmentState::Matched);
        attach(&path, "x").unwrap();
        assert!(path.is_file());
        assert_eq!(fs::read_to_string(&path).unwrap(), "x");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn drifted_content_never_overwritten_and_never_deleted() {
        let dir = temp("drift");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("command.toml");
        fs::write(&path, "user edit").unwrap();
        assert!(matches!(
            attach(&path, "expected"),
            Err(UzeError::ManagedEntryDrift(_))
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), "user edit");
        assert_eq!(inspect(&path, "expected").state, AttachmentState::Drifted);
        assert_eq!(
            detach(&path, "expected").unwrap().state,
            AttachmentState::Drifted
        );
        assert!(path.is_file());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn detach_removes_only_when_still_matched() {
        let dir = temp("detach");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("command.toml");
        attach(&path, "x").unwrap();
        assert_eq!(detach(&path, "x").unwrap().state, AttachmentState::Missing);
        assert!(!path.exists());
        assert_eq!(
            detach(&path, "x").unwrap().state,
            AttachmentState::Missing,
            "detaching an already-absent file is a safe no-op"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn occupied_directory_is_a_conflict() {
        let dir = temp("conflict");
        fs::create_dir_all(dir.join("occupied")).unwrap();
        assert_eq!(
            inspect(&dir.join("occupied"), "x").state,
            AttachmentState::Conflict
        );
        assert!(matches!(
            attach(&dir.join("occupied"), "x"),
            Err(UzeError::ManagedEntryConflict(_))
        ));
        fs::remove_dir_all(&dir).unwrap();
    }
}
