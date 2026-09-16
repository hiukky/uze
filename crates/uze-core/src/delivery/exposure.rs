//! Package and capability delivery plans.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use crate::{project::Resource, store::PackageId};

use serde::{Deserialize, Serialize};

use crate::{
    error::{Result, UzeError},
    hook::HookEvent,
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

/// How an integration makes a resource available.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExposureMechanism {
    /// A persistent, UZE-owned reference placed once in a harness's
    /// user-scope discovery directory (e.g. `~/.claude/skills`,
    /// `~/.agents/skills`), pointing at content inside the UZE store. It is
    /// tied to no project or session. See ADR-006.
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
    /// One namespaced entry inside a shared harness hook configuration
    /// (ADR-033). `entry_name` is the stable UZE identity for the entry
    /// (`<package>:<hook-id>`); `event` names the manifest group's semantic
    /// event where the target shape is event-keyed; `expected` is the exact
    /// serialized entry UZE owns, used for content-identity inspection and
    /// drift-safe removal. Every merge/inspect/detach rule lives in the
    /// owning integration, which knows the target file's shape; the Core
    /// only routes the mechanism.
    ManagedHookConfig {
        config_file: PathBuf,
        entry_name: String,
        event: HookEvent,
        expected: String,
        /// The generated wrapper the entry runs. Owned alongside the entry:
        /// materialized on attach, verified by content identity, removed
        /// once no entry needs it.
        wrapper: PathBuf,
    },
    /// A whole, UZE-owned derived file loaded by the harness directly from
    /// its own discovery directory — e.g. the OpenCode hook bridge
    /// (`<config root>/plugins/hooks-<package>.ts`, auto-discovered by
    /// the harness, so there is no configuration entry to merge). The
    /// integration owns attach/inspect/detach; the Core only routes it.
    ManagedHookFile {
        path: PathBuf,
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
        let Self::ManagedUserScopeReference {
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
        // An entry name may carry vendor-namespacing path components (e.g.
        // a harness whose namespaced physical representation is nested
        // directories). Creating the entry's parent directory is generic
        // filesystem preparation, not vendor syntax — the integration owns
        // the shape, this method only makes the write possible.
        if let Some(parent) = target.parent()
            && parent != discovery_root
        {
            fs::create_dir_all(parent).map_err(|source_error| UzeError::Write {
                path: parent.to_path_buf(),
                source: source_error,
            })?;
        }
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
        let Self::ManagedTextRegion {
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

    fn managed_reference(discovery_root: &Path, source: &Path) -> ExposureMechanism {
        ExposureMechanism::ManagedUserScopeReference {
            discovery_root: discovery_root.to_path_buf(),
            entry_name: "uze-example".to_owned(),
            source: source.to_path_buf(),
        }
    }

    #[test]
    fn attach_creates_a_symlink_and_is_idempotent() {
        let root = uze_testkit::temp::scratch("attach");
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
        let root = uze_testkit::temp::scratch("conflict");
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
}
