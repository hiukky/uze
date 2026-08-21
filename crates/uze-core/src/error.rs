//! Shared product error model.

use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum UzeError {
    #[error("could not determine the default home directory; set UZE_HOME")]
    MissingHomeDirectory,
    #[error("project path does not exist: {0}")]
    MissingPath(PathBuf),
    #[error("expected a directory: {0}")]
    NotDirectory(PathBuf),
    #[error("failed to read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to write {path}: {source}")]
    Write {
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
    #[error("Agent Plugin manifest is missing a string name: {0}")]
    MissingPackageName(PathBuf),
    #[error("invalid Agent Plugin name `{name}` in {path}")]
    InvalidPackageName { path: PathBuf, name: String },
    #[error("package `{id}` is already registered from {existing}, not {requested}")]
    PackageConflict {
        id: String,
        existing: String,
        requested: String,
    },
    #[error(
        "package is not self-contained: `{link}` resolves to `{target}`, outside the package root"
    )]
    PackageEscapesRoot { link: PathBuf, target: PathBuf },
    #[error(
        "refusing a URL carrying inline credentials; UZE never stores a secret, \
         and authenticated Git is a separate mechanism"
    )]
    CredentialBearingUrl,
    #[error("could not acquire package: {0}")]
    AcquisitionFailed(String),
    /// The operator declined. Distinct from `TrustRequired`: a decision was
    /// made, and repeating the command unchanged should not change it.
    #[error("trust denied for `{0}`; nothing was installed")]
    TrustDenied(String),
    /// Nobody could be asked — a non-interactive process. Structured so a
    /// pipeline can act on it instead of guessing.
    #[error(
        "TRUST_REQUIRED: `{package}` declares an executable capability and this process cannot \
         prompt. Re-run with an explicit trust flag after reviewing: {detail}"
    )]
    TrustRequired { package: String, detail: String },
    #[error("unknown UZE package `{0}`")]
    UnknownPackage(String),
    #[error("marketplace declares plugin `{0}` more than once")]
    DuplicateMarketplacePlugin(String),
    #[error("runtime projection target already exists: {0}")]
    RuntimePathExists(PathBuf),
    #[error("runtime filesystem projection is unavailable on this platform: {0}")]
    UnsupportedRuntimeProjection(PathBuf),
    #[error("no exposure route is available: {0}")]
    ExposureUnavailable(String),
    #[error("a non-UZE managed entry already exists at {0}")]
    ManagedEntryConflict(PathBuf),
    #[error("a managed entry has drifted and was preserved at {0}")]
    ManagedEntryDrift(PathBuf),
    #[error(
        "a managed text region's content differs from what was requested; user content at {0} was preserved"
    )]
    ManagedRegionDrift(PathBuf),
    #[error(
        "a managed text region's markers are duplicated, out of order, or only half present at {0}"
    )]
    ManagedRegionConflict(PathBuf),
    #[error(
        "managed text region identity `{0}` contains characters outside the safe marker charset"
    )]
    InvalidRegionIdentity(String),
    #[error("{0} is not valid UTF-8 text")]
    InvalidTextEncoding(PathBuf),
    #[error("another UZE mutation is already in progress at {0}")]
    MutationInProgress(PathBuf),
    #[error("failed to run `{program}`: {source}")]
    Process {
        program: String,
        source: std::io::Error,
    },
}

pub type Result<T> = std::result::Result<T, UzeError>;
