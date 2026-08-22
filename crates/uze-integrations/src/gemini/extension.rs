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
