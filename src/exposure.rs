use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::Serialize;

use crate::{
    capability::Representation,
    error::{Result, UzeError},
    home::UzeHome,
    router::{CompatibilityRoute, VerificationStatus},
};

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
    Unsupported {
        rationale: String,
    },
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
