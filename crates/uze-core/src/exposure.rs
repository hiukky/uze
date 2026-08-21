//! Package and capability delivery plans.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use crate::{project::Resource, store::PackageId};

use serde::{Deserialize, Serialize};

use crate::{
    capability::Representation,
    error::{Result, UzeError},
    home::UzeHome,
    router::{CompatibilityRoute, VerificationStatus},
};

/// Secret-free declaration of a process environment value UZE may pass
/// through to an MCP server. Values are intentionally never persisted in an
/// attachment receipt; integrations can only report MATCHED when their
/// vendor surface exposes an equivalent reference rather than an opaque
/// literal.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct McpEnvironmentReference {
    pub name: String,
}

/// How an integration makes a resource available. This is deliberately
/// separate from `representation`: a STANDARD resource does not imply that a
/// harness can discover it from a UZE store path.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExposureMechanism {
    DirectNative {
        resource_path: PathBuf,
    },
    RuntimeBridge {
        bridge: String,
        arguments: Vec<String>,
    },
    FilesystemProjection {
        source: PathBuf,
        target_relative: PathBuf,
    },
    /// A persistent, UZE-owned reference placed once in a harness's
    /// user-scope discovery directory (e.g. `~/.claude/skills`,
    /// `~/.agents/skills`), pointing at content inside the UZE store. Unlike
    /// `FilesystemProjection`, it is not tied to any one project workspace
    /// or session and is expected to outlive a single harness invocation.
    /// See ADR-006.
    ManagedUserScopeReference {
        discovery_root: PathBuf,
        entry_name: String,
        source: PathBuf,
    },
    /// A generated entry in a harness's own global/user-scope vendor
    /// configuration (e.g. `~/.claude.json`'s `mcpServers`,
    /// `~/.codex/config.toml`'s `[mcp_servers.*]`), produced by shelling
    /// out to that harness's own management CLI rather than by a
    /// filesystem symlink. This is the "Runtime Attachment" category named
    /// in ADR-006: unlike `ManagedUserScopeReference`, there is no shared
    /// discovery directory to point at, so this variant carries no generic
    /// attach/detach method — the registration command differs per
    /// harness, and each integration's own `attach()` reads this data to
    /// build its own invocation. See ADR-007.
    ManagedVendorConfig {
        entry_name: String,
        transport: String,
        command: PathBuf,
        args: Vec<String>,
        cwd: Option<PathBuf>,
        environment: Vec<McpEnvironmentReference>,
        enabled: Option<bool>,
    },
    /// UZE owns a delimited region inside a shared text file it does not
    /// otherwise own — never the whole file. `target_file` and
    /// `region_identity` name the artifact; `expected_content` is what
    /// belongs between its markers. See `crate::text_region`, which this
    /// variant's `attach`/`detach` methods delegate to; that module carries
    /// no knowledge of what `target_file` is or why `region_identity` was
    /// chosen — both are supplied entirely by the caller.
    ManagedTextRegion {
        target_file: PathBuf,
        region_identity: String,
        expected_content: String,
    },
    Unsupported {
        rationale: String,
    },
}

impl ExposureMechanism {
    /// Idempotently creates or refreshes a persistent, UZE-owned reference at
    /// the harness's user-scope discovery location so a plain harness
    /// invocation can resolve it without further action. Only valid for
    /// `ManagedUserScopeReference`; returns the created/verified entry path.
    /// Never touches an entry it does not already own.
    pub fn attach(&self) -> Result<PathBuf> {
        let ExposureMechanism::ManagedUserScopeReference {
            discovery_root,
            entry_name,
            source,
        } = self
        else {
            return Err(UzeError::ExposureUnavailable(
                "this exposure mechanism does not support persistent attachment".to_owned(),
            ));
        };
        fs::create_dir_all(discovery_root).map_err(|source_error| UzeError::Write {
            path: discovery_root.clone(),
            source: source_error,
        })?;
        let target = discovery_root.join(entry_name);
        match fs::symlink_metadata(&target) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let current = fs::read_link(&target).map_err(|source_error| UzeError::Read {
                    path: target.clone(),
                    source: source_error,
                })?;
                if &current != source {
                    // A managed-looking name is not ownership proof: users
                    // may repoint an earlier UZE reference. Preserve it.
                    return Err(UzeError::ManagedEntryDrift(target));
                }
            }
            Ok(_) => return Err(UzeError::ManagedEntryConflict(target)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                create_symlink(source, &target)?;
            }
            Err(error) => {
                return Err(UzeError::Read {
                    path: target,
                    source: error,
                });
            }
        }
        Ok(target)
    }

    /// Idempotently creates or verifies the region a `ManagedTextRegion`
    /// mechanism describes. Only valid for that variant; delegates entirely
    /// to `crate::text_region`, which owns every safety rule.
    pub fn attach_text_region(&self) -> Result<PathBuf> {
        let ExposureMechanism::ManagedTextRegion {
            target_file,
            region_identity,
            expected_content,
        } = self
        else {
            return Err(UzeError::ExposureUnavailable(
                "this exposure mechanism is not a managed text region".to_owned(),
            ));
        };
        crate::text_region::attach(target_file, region_identity, expected_content)?;
        Ok(target_file.clone())
    }

    /// Removes only the UZE-owned reference this mechanism describes, and
    /// only if it still points at `source`. Never removes an entry it did
    /// not create, and never removes the shared discovery directory itself.
    pub fn detach(&self) -> Result<()> {
        let ExposureMechanism::ManagedUserScopeReference {
            discovery_root,
            entry_name,
            source,
        } = self
        else {
            return Ok(());
        };
        let target = discovery_root.join(entry_name);
        let Ok(metadata) = fs::symlink_metadata(&target) else {
            return Ok(());
        };
        if !metadata.file_type().is_symlink() {
            return Ok(());
        }
        if fs::read_link(&target).ok().as_deref() != Some(source.as_path()) {
            return Ok(());
        }
        fs::remove_file(&target).map_err(|source_error| UzeError::Write {
            path: target,
            source: source_error,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ExposurePlan {
    pub representation: Representation,
    pub route: CompatibilityRoute,
    /// Result of a previous real conformance attempt, if any. Plans begin
    /// `UNVERIFIED`; strategy selection does not fabricate execution evidence.
    pub verification: VerificationStatus,
    pub mechanism: ExposureMechanism,
    pub evidence: String,
}

/// Package-level planning is intentionally separate from `ExposurePlan`:
/// Plugin is the distribution unit; resources/capabilities remain the unit
/// of compatibility. A native package plan declares exactly which resources
/// it consumes so callers never also attach them individually.
///
/// It deliberately carries no mechanism. The Core needs to know only *that*
/// a harness consumes this package as a native unit and which resources that
/// covers — never how. Every value an earlier `PackageExposureMechanism`
/// held (catalog root, catalog name, plugin selector) was derivable by the
/// owning integration from the package and UZE's own layout, so the enum
/// carried vendor vocabulary rather than information.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PackageExposurePlan {
    pub package_id: PackageId,
    pub route: CompatibilityRoute,
    pub verification: VerificationStatus,
    pub provided_resource_identities: BTreeSet<String>,
    pub evidence: String,
}

impl PackageExposurePlan {
    pub fn provides(&self, resource: &Resource) -> bool {
        self.provided_resource_identities
            .contains(&resource.identity())
    }
}

#[derive(Debug)]
pub struct PreparedExposure {
    pub working_directory: PathBuf,
    pub arguments: Vec<String>,
    pub runtime_directory: Option<PathBuf>,
    managed: Option<ManagedExposureArtifact>,
}

#[derive(Debug)]
struct ManagedExposureArtifact {
    target: PathBuf,
    runtime_directory: PathBuf,
    created_directories: Vec<PathBuf>,
}

impl PreparedExposure {
    /// Removes only the artifact UZE created, plus empty parent directories it
    /// created for that artifact. It never removes project-owned content.
    pub fn cleanup(&mut self) -> Result<()> {
        let Some(managed) = self.managed.take() else {
            return Ok(());
        };
        remove_managed_artifact(&managed)
    }

    pub fn managed_artifact_path(&self) -> Option<&Path> {
        self.managed
            .as_ref()
            .map(|artifact| artifact.target.as_path())
    }
}

impl Drop for PreparedExposure {
    fn drop(&mut self) {
        if let Some(managed) = self.managed.take() {
            let _ = remove_managed_artifact(&managed);
        }
    }
}

impl ExposurePlan {
    pub fn prepare(
        &self,
        home: &UzeHome,
        integration: &str,
        session: &str,
        workspace: &Path,
    ) -> Result<PreparedExposure> {
        match &self.mechanism {
            ExposureMechanism::DirectNative { .. } => Ok(PreparedExposure {
                working_directory: workspace.to_path_buf(),
                arguments: Vec::new(),
                runtime_directory: None,
                managed: None,
            }),
            ExposureMechanism::RuntimeBridge { arguments, .. } => Ok(PreparedExposure {
                working_directory: workspace.to_path_buf(),
                arguments: arguments.clone(),
                runtime_directory: None,
                managed: None,
            }),
            ExposureMechanism::FilesystemProjection {
                source,
                target_relative,
            } => {
                let runtime = home.runtime_session_dir(integration, session);
                let target = workspace.join(target_relative);
                if target.exists() || target.is_symlink() {
                    return Err(UzeError::RuntimePathExists(target));
                }
                let parent = target.parent().expect("projection target has a parent");
                let created_directories = create_missing_directories(parent)?;
                fs::create_dir_all(parent).map_err(|source_error| UzeError::Write {
                    path: parent.to_path_buf(),
                    source: source_error,
                })?;
                create_symlink(source, &target)?;
                fs::create_dir_all(&runtime).map_err(|source_error| UzeError::Write {
                    path: runtime.clone(),
                    source: source_error,
                })?;
                let metadata = serde_json::json!({
                    "managed_by": "uze",
                    "integration": integration,
                    "session": session,
                    "workspace": workspace,
                    "target": target,
                    "source": source,
                });
                fs::write(
                    runtime.join("managed-exposure.json"),
                    serde_json::to_vec_pretty(&metadata)
                        .expect("metadata serialization is infallible"),
                )
                .map_err(|source_error| UzeError::Write {
                    path: runtime.join("managed-exposure.json"),
                    source: source_error,
                })?;
                Ok(PreparedExposure {
                    working_directory: workspace.to_path_buf(),
                    arguments: Vec::new(),
                    runtime_directory: Some(runtime),
                    managed: Some(ManagedExposureArtifact {
                        target,
                        runtime_directory: home.runtime_session_dir(integration, session),
                        created_directories,
                    }),
                })
            }
            ExposureMechanism::ManagedUserScopeReference { .. } => {
                // A persistent, user-scope reference is not a session-scoped
                // managed artifact: it is created/refreshed once via
                // `ExposureMechanism::attach`, not per invocation, and must
                // not be torn down when a `PreparedExposure` is dropped.
                Ok(PreparedExposure {
                    working_directory: workspace.to_path_buf(),
                    arguments: Vec::new(),
                    runtime_directory: None,
                    managed: None,
                })
            }
            ExposureMechanism::ManagedVendorConfig { .. } => {
                // Same rationale as `ManagedUserScopeReference` above: the
                // real attachment path for generated vendor config is each
                // integration's own `attach()`, called once at `uze add`
                // time, not `prepare()`.
                Ok(PreparedExposure {
                    working_directory: workspace.to_path_buf(),
                    arguments: Vec::new(),
                    runtime_directory: None,
                    managed: None,
                })
            }
            ExposureMechanism::ManagedTextRegion { .. } => {
                // Same rationale again: a managed text region is a
                // persistent, package-lifecycle artifact created once via
                // `attach_text_region`/`IntegrationPort::attach`, not a
                // per-session projection this type prepares or tears down.
                Ok(PreparedExposure {
                    working_directory: workspace.to_path_buf(),
                    arguments: Vec::new(),
                    runtime_directory: None,
                    managed: None,
                })
            }
            ExposureMechanism::Unsupported { rationale } => {
                Err(UzeError::ExposureUnavailable(rationale.clone()))
            }
        }
    }
}

fn create_missing_directories(parent: &Path) -> Result<Vec<PathBuf>> {
    let mut missing = Vec::new();
    let mut cursor = parent;
    while !cursor.exists() {
        missing.push(cursor.to_path_buf());
        cursor = cursor
            .parent()
            .ok_or_else(|| UzeError::RuntimePathExists(parent.to_path_buf()))?;
    }
    missing.reverse();
    Ok(missing)
}

fn remove_managed_artifact(managed: &ManagedExposureArtifact) -> Result<()> {
    if managed.target.is_symlink() || managed.target.is_file() {
        fs::remove_file(&managed.target).map_err(|source| UzeError::Write {
            path: managed.target.clone(),
            source,
        })?;
    }
    for directory in managed.created_directories.iter().rev() {
        match fs::remove_dir(directory) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => {}
            Err(source) => {
                return Err(UzeError::Write {
                    path: directory.clone(),
                    source,
                });
            }
        }
    }
    if managed.runtime_directory.exists() {
        fs::remove_dir_all(&managed.runtime_directory).map_err(|source| UzeError::Write {
            path: managed.runtime_directory.clone(),
            source,
        })?;
    }
    Ok(())
}

#[cfg(unix)]
fn create_symlink(source: &Path, target: &Path) -> Result<()> {
    std::os::unix::fs::symlink(source, target).map_err(|source_error| UzeError::Write {
        path: target.to_path_buf(),
        source: source_error,
    })
}

#[cfg(not(unix))]
fn create_symlink(_source: &Path, target: &Path) -> Result<()> {
    Err(UzeError::UnsupportedRuntimeProjection(target.to_path_buf()))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "uze-exposure-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    fn managed_reference(discovery_root: &Path, source: &Path) -> ExposureMechanism {
        ExposureMechanism::ManagedUserScopeReference {
            discovery_root: discovery_root.to_path_buf(),
            entry_name: "uze-example".to_owned(),
            source: source.to_path_buf(),
        }
    }

    #[test]
    fn attach_creates_a_symlink_and_is_idempotent() {
        let root = temp_dir("attach");
        let discovery_root = root.join("skills");
        let source = root.join("store-entry");
        fs::create_dir_all(&source).unwrap();

        let mechanism = managed_reference(&discovery_root, &source);
        let target = mechanism.attach().unwrap();
        assert_eq!(target, discovery_root.join("uze-example"));
        assert!(target.is_symlink());
        assert_eq!(fs::read_link(&target).unwrap(), source);

        // Second attach is a no-op, not an error and not a re-link.
        let target_again = mechanism.attach().unwrap();
        assert_eq!(target_again, target);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn attach_never_overwrites_an_entry_it_does_not_own() {
        let root = temp_dir("conflict");
        let discovery_root = root.join("skills");
        fs::create_dir_all(&discovery_root).unwrap();
        fs::create_dir_all(discovery_root.join("uze-example")).unwrap();
        let source = root.join("store-entry");
        fs::create_dir_all(&source).unwrap();

        let mechanism = managed_reference(&discovery_root, &source);
        let error = mechanism.attach().unwrap_err();
        assert!(matches!(error, UzeError::ManagedEntryConflict(_)));

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn detach_removes_only_the_reference_it_owns() {
        let root = temp_dir("detach");
        let discovery_root = root.join("skills");
        let source = root.join("store-entry");
        fs::create_dir_all(&source).unwrap();

        let mechanism = managed_reference(&discovery_root, &source);
        let target = mechanism.attach().unwrap();
        assert!(target.exists());

        mechanism.detach().unwrap();
        assert!(!target.exists());
        assert!(
            discovery_root.exists(),
            "discovery root itself is preserved"
        );

        // Detaching again is a no-op, not an error.
        mechanism.detach().unwrap();

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn detach_does_not_remove_an_entry_repointed_elsewhere() {
        let root = temp_dir("repointed");
        let discovery_root = root.join("skills");
        let source = root.join("store-entry");
        let other = root.join("unrelated-entry");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&other).unwrap();

        let mechanism = managed_reference(&discovery_root, &source);
        let target = mechanism.attach().unwrap();
        fs::remove_file(&target).unwrap();
        create_symlink(&other, &target).unwrap();

        mechanism.detach().unwrap();
        assert!(
            target.exists(),
            "entry no longer pointing at source is left alone"
        );

        fs::remove_dir_all(&root).unwrap();
    }
}
