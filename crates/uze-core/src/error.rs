//! Shared product error model.

use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum UzeError {
    #[error("could not determine the default home directory; set UZE_HOME")]
    MissingHomeDirectory,
    #[error(
        "{variable} is set to `{value}`, which is not an absolute path: UZE would put its \
         store, state and receipts under whichever directory each command happened to run from"
    )]
    RelativeHomeDirectory {
        variable: &'static str,
        value: String,
    },
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
        "package holds `{first}` and `{second}` in one directory, names a \
         case-insensitive filesystem cannot tell apart; on macOS or Windows \
         only one would be installed and the package decides which"
    )]
    PackageNameCollides { first: PathBuf, second: PathBuf },
    #[error(
        "refusing a URL carrying inline credentials; UZE never stores a secret, \
         and authenticated Git is a separate mechanism"
    )]
    CredentialBearingUrl,
    #[error("could not acquire package: {0}")]
    AcquisitionFailed(String),
    /// The local terminal runtime failed. Its own right to a variant: a
    /// socket that cannot be reached has nothing to do with acquiring a
    /// package, and borrowing that variant is how `uze terminal stop`
    /// reported a missing socket as `could not acquire package`.
    #[error("terminal runtime: {0}")]
    TerminalRuntime(String),
    /// `uze upgrade` could not replace the binary, or was not allowed to.
    #[error("upgrade: {0}")]
    Upgrade(String),
    #[error("setup incomplete: {0}")]
    ProvisioningIncomplete(String),
    /// A lifecycle mutation the safety check refused (ADR-009): drift or a
    /// conflicting receipt stopped it and nothing was removed or updated.
    /// Its own variant for the same reason `ProvisioningIncomplete` has
    /// one — the report is still worth printing, and the exit status still
    /// has to say the machine is unchanged.
    #[error("{0}")]
    LifecycleBlocked(String),
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
    #[error(
        "`{path}` is not a Git repository with a commit. A marketplace is a Git repository, \
         local or remote: its commits are what tell UZE whether the bytes it installed are \
         still the bytes there, and whether anything newer exists."
    )]
    MarketplaceNotARepository { path: PathBuf },
    #[error("unknown UZE package `{0}`")]
    UnknownPackage(String),
    #[error(
        "agents.lock is version {found}, and this UZE writes {expected}. The lock is generated \
         and carries nothing you wrote: delete it and run `uze install`."
    )]
    UnsupportedLockVersion { found: u32, expected: u32 },
    #[error("no task `{0}` is recorded for this repository")]
    UnknownTask(String),
    #[error("cannot resume task: {0}")]
    ResumeFailed(String),
    #[error("could not discard the task: {0}")]
    Discard(String),
    /// A proposed name was refused. Its text is written for the agent that
    /// proposed it: it says which half was wrong and what this project
    /// accepts, because a refusal is that agent's only feedback channel.
    #[error("{0}")]
    TaskNaming(String),
    /// Artifacts the project declares do not draw as written. The report
    /// printed before this says which and why; this is the verdict, so a
    /// check is a gate rather than something to read.
    #[error("{0}")]
    ArtifactsNotDrawable(String),
    /// An agent could not be placed where it was asked for. Nothing was
    /// started: a launch that lands somewhere other than what the operator
    /// chose is worse than no launch, and a notice after the fact would not
    /// undo it.
    #[error("could not place the agent: {0}")]
    AgentPlacement(String),
    #[error("unsupported state schema {found} in {path}; this uze writes {expected}")]
    UnsupportedStateSchema {
        path: PathBuf,
        found: u32,
        expected: u32,
    },
    #[error("malformed agents.lock at {path}: {reason}")]
    MalformedLock { path: PathBuf, reason: String },
    /// The bytes acquired are not the bytes the lock pinned. Named
    /// separately from every other install failure because the remedy is
    /// different and the situation is not routine: either the source moved
    /// under a reference that promised not to, or somebody replaced it.
    #[error(
        "`{plugin}` does not match what agents.lock pinned\n  expected {expected}\n  found    {found}\nNothing was installed. If the source legitimately changed, re-add the plugin so the lock records the new bytes."
    )]
    IntegrityMismatch {
        plugin: String,
        expected: String,
        found: String,
    },
    /// The authored manifest — distinct from `MalformedLock` because the
    /// remedy differs: a lock is regenerated, a manifest is a file only its
    /// author can fix.
    #[error("malformed agents.yaml at {path}: {reason}")]
    MalformedManifest { path: PathBuf, reason: String },
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
    #[error("marketplace `{0}` not found")]
    UnknownMarketplace(String),
    #[error("marketplace `{0}` still has installed plugins; remove them first")]
    MarketplaceInUse(String),
    #[error("marketplace `{0}` is reserved and cannot be added or removed")]
    ReservedMarketplace(String),
    #[error("invalid plugin spec: {0}")]
    InvalidPluginSpec(String),
    /// A theme that will not load. Its own variant rather than a borrowed
    /// one: an operator told their theme file could not be *acquired* would
    /// go looking for a network problem.
    #[error("{0}")]
    UnusableTheme(String),
    /// The operator's settings file does not parse. Refused rather than read
    /// as empty, because reading on would undo every choice in it.
    #[error("{path} is not valid TOML: {reason}")]
    MalformedConfig {
        path: std::path::PathBuf,
        reason: String,
    },
    /// `uze remove` is strictly project-scoped (no fallback to machine-level
    /// removal) — see ADR-019. Distinct from `PluginNotUsedByProject`: this
    /// is "there is no project here to remove anything from."
    #[error(
        "no project environment found here; run `uze remove {plugin} -m` to remove it from this machine"
    )]
    NoProjectEnvironment { plugin: String },
    /// A project exists (an `agents.lock` was found) but does not declare
    /// this plugin — distinct from `NoProjectEnvironment`.
    #[error(
        "`{plugin}` is not used by this project; run `uze remove {plugin} -m` to remove it from this machine"
    )]
    PluginNotUsedByProject { plugin: String },
    /// The directory is no project: no `agents.yaml`, no repository root,
    /// no `AGENTS.md`. A machine that is not inside a project is not
    /// broken, so this is a question of scope, not a fault — `hint` names
    /// what to do instead.
    #[error("not a project here (no agents.yaml, repository root or AGENTS.md); {hint}")]
    NoProject { hint: String },
    #[error("symbolic links are unavailable on this platform: {0}")]
    SymlinkUnsupported(PathBuf),
    #[error("the plugin store cannot preserve special filesystem entry `{0}`")]
    UnpreservableEntry(PathBuf),
    #[error("official marketplace plugin `{0}` is protected and cannot be removed")]
    ProtectedPackage(String),
    #[error("unknown harness `{requested}` (registered: {known})")]
    UnknownHarness { requested: String, known: String },
    /// A harness's own configuration could not be read or changed the way
    /// UZE needs; the text names the file's problem.
    #[error("{0}")]
    HarnessConfig(String),
    /// A harness's own CLI could not be run, or refused what UZE asked of it.
    #[error("{0}")]
    HarnessCommand(String),
    /// The repository could not be reached with this machine's
    /// credentials. Separate from `AcquisitionFailed` because the action is
    /// different: nothing about the package is wrong, and what the operator
    /// needs is to know which marketplace, and that it is access.
    #[error("could not access the repository with this machine's credentials\n{detail}")]
    RepositoryAccessRefused { detail: String },
    /// The repository's host does not resolve from here. Not a question of
    /// access, so no other transport is tried and none is blamed.
    #[error("this machine is offline, or cannot resolve the repository's host\n{detail}")]
    RepositoryOffline { detail: String },
    /// A short locator matched nothing UZE can read: a bare word, an
    /// ambiguous name, an alias this machine does not have.
    #[error("{0}")]
    UnreadableLocator(String),
    /// A host alias the operator asked for cannot be recorded.
    #[error("{0}")]
    HostAlias(String),
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
    #[error("{markers} at {0}", markers = crate::text_region::MALFORMED_MARKERS)]
    ManagedRegionConflict(PathBuf),
    #[error(
        "managed text region identity `{0}` contains characters outside the safe marker charset"
    )]
    InvalidRegionIdentity(String),
    #[error("{0} is not valid UTF-8 text")]
    InvalidTextEncoding(PathBuf),
    #[error(
        "another UZE mutation is already in progress at {path}{}",
        .pid.map(|pid| format!(" (process {pid})")).unwrap_or_default()
    )]
    MutationInProgress { path: PathBuf, pid: Option<u32> },
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

/// A record's own failures, in the domain's vocabulary.
///
/// `uze-document` is a leaf and names no domain, so the mapping lives here
/// rather than there. Each variant has an exact counterpart already, which
/// is why the durability rule could move out without the domain's error
/// surface growing.
impl From<uze_document::DocumentError> for UzeError {
    fn from(error: uze_document::DocumentError) -> Self {
        use uze_document::DocumentError;
        match error {
            DocumentError::Read { path, source } => Self::Read { path, source },
            DocumentError::Write { path, source } => Self::Write { path, source },
            DocumentError::Unreadable { path, source } => Self::Json { path, source },
            DocumentError::UnsupportedShape {
                path,
                found,
                expected,
            } => Self::UnsupportedStateSchema {
                path,
                found,
                expected,
            },
        }
    }
}

pub type Result<T> = std::result::Result<T, UzeError>;
