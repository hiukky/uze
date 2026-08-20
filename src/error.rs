use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum UzeError {
    #[error("project path does not exist: {0}")]
    MissingPath(PathBuf),
    #[error("expected a directory: {0}")]
    NotDirectory(PathBuf),
    #[error("failed to read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to parse JSON in {path}: {source}")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("bundle manifest is missing in {0}")]
    MissingManifest(PathBuf),
    #[error("unsafe path reference in {path}: {reference}")]
    UnsafePathReference { path: PathBuf, reference: String },
}

pub type Result<T> = std::result::Result<T, UzeError>;
