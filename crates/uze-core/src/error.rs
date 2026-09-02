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
    #[error("invalid portable hook manifest at {path}: {reason}")]
    InvalidHookManifest { path: PathBuf, reason: String },
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
        "plugin name `{name}` is already active as `{existing}`; installing `{requested}` under it would silently shadow one of the two — pass a replace or alias resolution"
    )]
    PluginNameCollision {
        name: String,
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
    #[error("unsupported agents.lock version {found}; expected {expected}")]
    UnsupportedLockVersion { found: u32, expected: u32 },
    #[error("no task `{0}` is recorded for this repository")]
    UnknownTask(String),
    #[error("could not discard the task: {0}")]
    Discard(String),
    #[error("unsupported state schema {found} in {path}; this uze writes {expected}")]
    UnsupportedStateSchema {
        path: PathBuf,
        found: u32,
        expected: u32,
    },
    #[error("malformed agents.lock at {path}: {reason}")]
    MalformedLock { path: PathBuf, reason: String },
    #[error(
        "marketplace source conflict for `{marketplace}`: lock has {lock_source}, global has {global_source}"
    )]
    MarketplaceSourceConflict {
        marketplace: String,
        lock_source: String,
        global_source: String,
    },
    #[error("marketplace mismatch for plugin `{plugin}`: expected `{expected}`, found `{found}`")]
    MarketplaceMismatch {
        plugin: String,
        expected: String,
        found: String,
    },
    #[error("marketplace declares plugin `{0}` more than once")]
    DuplicateMarketplacePlugin(String),
    /// A real conflict, distinct from re-adding the same marketplace from
    /// the same source (idempotent, not an error — see
    /// `state::marketplace_add`): the requested source disagrees with what
    /// is already registered under this name.
    #[error("marketplace `{name}` is already registered from {existing}, not {requested}")]
    MarketplaceConflict {
        name: String,
        existing: String,
        requested: String,
    },
    #[error("marketplace `{0}` is reserved and cannot be added or removed")]
    ReservedMarketplace(String),
    #[error("invalid plugin spec: {0}")]
    InvalidPluginSpec(String),
    /// `uze remove` is strictly project-scoped (no fallback to machine-level
    /// removal) — see ADR-019. Distinct from `PluginNotUsedByProject`: this
    /// is "there is no project here to remove anything from."
    #[error(
        "no project environment found here; run `uze plugin remove {plugin}` to remove it from this machine"
    )]
    NoProjectEnvironment { plugin: String },
    /// A project exists (an `agents.lock` was found) but does not declare
    /// this plugin — distinct from `NoProjectEnvironment`.
    #[error(
        "`{plugin}` is not used by this project; run `uze plugin remove {plugin}` to remove it from this machine"
    )]
    PluginNotUsedByProject { plugin: String },
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
    /// Two distinct canonical resources need the same vendor-visible
    /// physical entry with incompatible representations — a projection
    /// ownership conflict (e.g. a legacy receipt and a Skill both
    /// projecting `flow:commit` into the shared `~/.agents/skills` root,
    /// or a reused artifact that cannot carry the reusing integration's
    /// invocation encoding). Distinct from `ManagedEntryDrift`: nothing
    /// drifted; the conflict is deterministically detectable before any
    /// attachment happens.
    #[error("{0}")]
    ProjectionConflict(Box<ProjectionConflictDetails>),

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
    #[error("unknown profile `{0}`")]
    UnknownProfile(String),
    #[error("profile `{0}` already exists")]
    ProfileAlreadyExists(String),
    #[error("cannot remove the only profile")]
    CannotDeleteOnlyProfile,
    #[error("invalid profile id `{0}`: use lowercase letters, digits, `-`, or `_`")]
    InvalidProfileId(String),
    #[error("failed to run `{program}`: {source}")]
    Process {
        program: String,
        source: std::io::Error,
    },
    /// A runtime hook dispatch (the `hook-exec` wrapper) could not honor the
    /// portable command ABI — unknown adapter/event/effect, unreadable native
    /// payload, or an adapter rejection.
    #[error("hook dispatch failed: {0}")]
    HookDispatch(String),
}

/// Payload of [`UzeError::ProjectionConflict`], boxed so `UzeError` stays
/// under Clippy's `result_large_err` threshold.
#[derive(Debug)]
pub struct ProjectionConflictDetails {
    pub entry: PathBuf,
    pub requested: String,
    pub requested_integration: String,
    pub requested_target: PathBuf,
    pub existing: String,
    pub existing_integration: String,
    pub existing_target: PathBuf,
}

impl std::fmt::Display for ProjectionConflictDetails {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "projection conflict at `{}`: {} ({}) cannot be exposed from {} because {} ({}) already \
             owns this entry (target {}); remove or rename one of the capabilities",
            self.entry.display(),
            self.requested,
            self.requested_integration,
            self.requested_target.display(),
            self.existing,
            self.existing_integration,
            self.existing_target.display()
        )
    }
}

pub type Result<T> = std::result::Result<T, UzeError>;
