use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::Serialize;

use crate::{
    capability::Representation,
    error::{Result, UzeError},
    home::UzeHome,
    router::{CompatibilityRoute, ExposureState},
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
    pub exposure: ExposureState,
    pub mechanism: ExposureMechanism,
    pub evidence: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedExposure {
    pub working_directory: PathBuf,
    pub arguments: Vec<String>,
    pub runtime_directory: Option<PathBuf>,
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
            }),
            ExposureMechanism::RuntimeBridge { arguments, .. } => Ok(PreparedExposure {
                working_directory: workspace.to_path_buf(),
                arguments: arguments.clone(),
                runtime_directory: None,
            }),
            ExposureMechanism::FilesystemProjection {
                source,
                target_relative,
            } => {
                let runtime = home.runtime_session_dir(integration, session);
                let working_directory = runtime.join("workspace");
                let target = working_directory.join(target_relative);
                if target.exists() || target.is_symlink() {
                    return Err(UzeError::RuntimePathExists(target));
                }
                let parent = target.parent().expect("projection target has a parent");
                fs::create_dir_all(parent).map_err(|source_error| UzeError::Write {
                    path: parent.to_path_buf(),
                    source: source_error,
                })?;
                create_symlink(source, &target)?;
                Ok(PreparedExposure {
                    working_directory,
                    arguments: Vec::new(),
                    runtime_directory: Some(runtime),
                })
            }
            ExposureMechanism::Unsupported { rationale } => {
                Err(UzeError::ExposureUnavailable(rationale.clone()))
            }
        }
    }
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
