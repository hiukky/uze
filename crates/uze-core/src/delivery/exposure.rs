//! Package and capability delivery plans.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use crate::{capability::Resource, store::PackageId};

use serde::{Deserialize, Serialize};

use crate::{
    error::{Result, UzeError},
    hook::HookEvent,
    integration::{AttachmentInspection, AttachmentState},
    router::CompatibilityRoute,
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

/// How an integration makes a resource available: the artifact UZE will own
/// once attached — described exactly as its receipt records it — or why
/// there is none.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExposureMechanism {
    Managed(ManagedArtifact),
    Unsupported { rationale: String },
}

/// One harness-owned side effect UZE is responsible for. As a plan it says
/// what attaching will create; inside a receipt it is the ownership proof
/// every later inspection and detach is judged against.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ManagedArtifact {
    /// A persistent reference placed once in a harness's user-scope
    /// discovery directory (e.g. `~/.claude/skills/<name>`), pointing at
    /// content inside the UZE store. Tied to no project or session
    /// (ADR-006).
    SymlinkReference { path: PathBuf, target: PathBuf },
    /// A generated entry in a harness's own user-scope vendor configuration
    /// (e.g. `~/.claude.json`'s `mcpServers`), registered through that
    /// harness's management CLI. The registration command differs per
    /// harness, so the owning integration attaches it (ADR-007).
    VendorConfigEntry {
        entry_name: String,
        transport: String,
        command: PathBuf,
        args: Vec<String>,
        cwd: Option<PathBuf>,
        environment: Vec<McpEnvironmentReference>,
        enabled: Option<bool>,
    },
    /// A delimited region inside a shared text file UZE does not otherwise
    /// own. Every safety rule lives in `crate::text_region`, which knows
    /// nothing about what `target_file` is or why `region_identity` was
    /// chosen.
    ManagedTextRegion {
        target_file: PathBuf,
        region_identity: String,
        expected_content: String,
    },
    /// A delivery whose ownership proof only the owning integration can
    /// interpret. The Core routes it by `receipt.integration`, never reads
    /// `detail`, and refuses to inspect or detach it generically.
    IntegrationOwned {
        kind: String,
        selector: String,
        #[serde(flatten, default)]
        detail: BTreeMap<String, serde_json::Value>,
    },
    /// A UZE-namespaced entry inside the harness's shared hook
    /// configuration (ADR-033). `entry_name` is the stable UZE identity,
    /// `event` the manifest group's semantic event where the target shape
    /// is event-keyed, and `expected` the exact serialized entry content.
    /// Inspection and detach are integration-owned: the Core knows the
    /// identity, never the file's shape.
    HookConfigEntry {
        config_file: PathBuf,
        entry_name: String,
        event: HookEvent,
        expected: String,
        /// The generated wrapper this entry runs: materialized on attach,
        /// verified by content identity, removed once no entry needs it.
        wrapper: PathBuf,
    },
    /// A whole derived file the harness loads from its own discovery
    /// directory (the OpenCode hook bridge): no configuration entry exists
    /// to merge, so `path` is the entire artifact. The owning integration
    /// attaches, inspects and detaches it.
    ManagedHookFile { path: PathBuf },
}

impl ManagedArtifact {
    /// Creates or verifies an artifact whose full desired state the artifact
    /// itself carries — a symlink reference or a text region. Never touches
    /// an entry UZE does not already own. Every other artifact is attached
    /// by its owning integration.
    pub fn attach_standard(&self) -> Result<()> {
        match self {
            Self::SymlinkReference { path, target } => attach_symlink(path, target),
            Self::ManagedTextRegion {
                target_file,
                region_identity,
                expected_content,
            } => crate::text_region::attach(target_file, region_identity, expected_content),
            _ => Err(UzeError::ExposureUnavailable(
                "this artifact is attached by its owning integration".to_owned(),
            )),
        }
    }

    /// Inspection for artifacts whose ownership proof does not depend on a
    /// harness schema. Vendor integrations call this explicitly rather than
    /// redispatching through `IntegrationPort` from an override.
    pub fn inspect_standard(&self) -> AttachmentInspection {
        match self {
            Self::SymlinkReference { path, target } => inspect_symlink(path, target),
            Self::ManagedTextRegion {
                target_file,
                region_identity,
                expected_content,
            } => crate::text_region::inspect(target_file, region_identity, expected_content),
            _ => AttachmentInspection {
                state: AttachmentState::Blocked,
                reason: "integration must inspect this vendor artifact".to_owned(),
            },
        }
    }

    /// Removes only a currently matched standard artifact. Any non-matched
    /// inspection is returned unchanged, so drift never turns into a
    /// destructive operation.
    pub fn detach_standard(&self) -> Result<AttachmentInspection> {
        match self {
            // `text_region::detach` inspects the region in the same read it
            // removes it from, so drift never becomes a removal (ADR-009).
            Self::ManagedTextRegion {
                target_file,
                region_identity,
                expected_content,
            } => crate::text_region::detach(target_file, region_identity, expected_content),
            Self::SymlinkReference { path, target } => {
                let inspection = inspect_symlink(path, target);
                if inspection.state != AttachmentState::Matched {
                    return Ok(inspection);
                }
                fs::remove_file(path).map_err(|source| UzeError::Write {
                    path: path.clone(),
                    source,
                })?;
                Ok(AttachmentInspection {
                    state: AttachmentState::Missing,
                    reason: "managed artifact detached".to_owned(),
                })
            }
            _ => Ok(self.inspect_standard()),
        }
    }

    /// The physical exposure name this artifact claims. `None` for a shape
    /// with no single physical name of its own: a text region spans a
    /// portion of a shared file, and an integration-owned artifact's naming
    /// is opaque to the Core by design.
    pub fn exposure_name(&self) -> Option<String> {
        match self {
            Self::SymlinkReference { path, .. } | Self::ManagedHookFile { path } => {
                path.file_name()?.to_str().map(str::to_owned)
            }
            Self::VendorConfigEntry { entry_name, .. }
            | Self::HookConfigEntry { entry_name, .. } => Some(entry_name.clone()),
            Self::ManagedTextRegion { .. } | Self::IntegrationOwned { .. } => None,
        }
    }

    /// A human-readable locator. Display-only: the artifact itself remains
    /// the source of truth.
    pub fn location(&self) -> PathBuf {
        match self {
            Self::SymlinkReference { path, .. } | Self::ManagedHookFile { path } => path.clone(),
            Self::VendorConfigEntry { entry_name, .. } => {
                PathBuf::from(format!("mcp:{entry_name}"))
            }
            Self::HookConfigEntry {
                config_file,
                entry_name,
                ..
            } => PathBuf::from(format!("{}#{entry_name}", config_file.display())),
            Self::ManagedTextRegion {
                target_file,
                region_identity,
                ..
            } => PathBuf::from(format!("{}#{region_identity}", target_file.display())),
            Self::IntegrationOwned { kind, selector, .. } => {
                PathBuf::from(format!("{kind}:{selector}"))
            }
        }
    }

    /// A cheap, vendor-neutral fingerprint of the filesystem surface the
    /// artifact lives on — the freshness half of the inspection cache
    /// (ADR 018).
    ///
    /// Only a symlink reference has a directly stat-able presence, and it
    /// always produces one: a missing link is a real, checkable state, not
    /// the absence of a fingerprint. Everything else lives inside vendor
    /// files whose locations this layer deliberately does not know, so its
    /// verdicts are bounded by TTL and mutation invalidation alone.
    pub fn fingerprint(&self) -> Option<String> {
        let Self::SymlinkReference { path, target } = self else {
            return None;
        };
        let state = fs::symlink_metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos())
            .unwrap_or(u128::MAX); // absent/untimed: a state, never "no info"
        let link = fs::read_link(path)
            .map(|resolved| resolved.to_string_lossy().into_owned())
            .unwrap_or_default();
        Some(format!("{state}:{link}:{}", target.display()))
    }

    /// Whether the artifact is still physically in place, answered only for
    /// a symlink reference (the link exists and points where it should).
    /// Everything else is the owning integration's verdict, so the answer is
    /// `false` and callers fall back to receipt existence.
    pub fn is_in_place(&self) -> bool {
        let Self::SymlinkReference { path, target } = self else {
            return false;
        };
        fs::read_link(path).is_ok_and(|resolved| resolved == *target)
    }
}

fn attach_symlink(path: &Path, target: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| UzeError::Write {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            let current = fs::read_link(path).map_err(|source| UzeError::Read {
                path: path.to_path_buf(),
                source,
            })?;
            if current != target {
                // A managed-looking name is not ownership proof: users may
                // repoint an earlier UZE reference. Preserve it.
                return Err(UzeError::ManagedEntryDrift(path.to_path_buf()));
            }
            Ok(())
        }
        Ok(_) => Err(UzeError::ManagedEntryConflict(path.to_path_buf())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            crate::persistence::create_symlink(target, path)
        }
        Err(source) => Err(UzeError::Read {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn inspect_symlink(path: &Path, target: &Path) -> AttachmentInspection {
    let (state, reason) = match fs::read_link(path) {
        Ok(actual) if actual == target => (
            AttachmentState::Matched,
            "managed symlink target matches receipt".to_owned(),
        ),
        Ok(_) => (
            AttachmentState::Drifted,
            "symlink target differs from receipt".to_owned(),
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (
            AttachmentState::Missing,
            "managed symlink is missing".to_owned(),
        ),
        Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => (
            AttachmentState::Conflict,
            "managed path is occupied by a non-symlink".to_owned(),
        ),
        Err(error) => (AttachmentState::Blocked, error.to_string()),
    };
    AttachmentInspection { state, reason }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ExposurePlan {
    pub route: CompatibilityRoute,
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
    pub provided_resource_identities: BTreeSet<String>,
    pub evidence: String,
}

impl PackageExposurePlan {
    pub fn provides(&self, resource: &Resource) -> bool {
        self.provided_resource_identities
            .contains(&resource.identity())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn managed_reference(discovery_root: &Path, target: &Path) -> ManagedArtifact {
        ManagedArtifact::SymlinkReference {
            path: discovery_root.join("uze-example"),
            target: target.to_path_buf(),
        }
    }

    #[test]
    fn attach_creates_a_symlink_and_is_idempotent() {
        let root = uze_testkit::temp::scratch("attach");
        let discovery_root = root.join("skills");
        let source = root.join("store-entry");
        fs::create_dir_all(&source).unwrap();

        let artifact = managed_reference(&discovery_root, &source);
        artifact.attach_standard().unwrap();
        let link = discovery_root.join("uze-example");
        assert!(link.is_symlink());
        assert_eq!(fs::read_link(&link).unwrap(), source);

        // Second attach is a no-op, not an error and not a re-link.
        artifact.attach_standard().unwrap();
        assert_eq!(fs::read_link(&link).unwrap(), source);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn attach_never_overwrites_an_entry_it_does_not_own() {
        let root = uze_testkit::temp::scratch("conflict");
        let discovery_root = root.join("skills");
        fs::create_dir_all(&discovery_root).unwrap();
        fs::create_dir_all(discovery_root.join("uze-example")).unwrap();
        let source = root.join("store-entry");
        fs::create_dir_all(&source).unwrap();

        let error = managed_reference(&discovery_root, &source)
            .attach_standard()
            .unwrap_err();
        assert!(matches!(error, UzeError::ManagedEntryConflict(_)));

        fs::remove_dir_all(&root).unwrap();
    }
}
