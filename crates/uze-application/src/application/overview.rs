//! Overview workspace summary — the smallest semantic read model that
//! answers "what kind of UZE workspace am I in, and is it ready to work",
//! composed entirely from existing core/application primitives:
//!
//! - workspace detection (`uze_core::workspace`) for root + kind
//! - `agents.lock` parsing (`uze_core::project_lock`) for the consumer side
//! - `marketplace.json` parsing (`acquisition::marketplace`) for the marketplace side
//! - Store package ids (`installed_packages`) for installed vs required
//! - `context_inspect` for the memory/portability half
//!
//! This is deliberately a *projection*: files (agents.lock, marketplace.json,
//! `.agents/`, paths, counts-by-inspection) are evidence used *here* to
//! compute states, but the states are the product. The TUI renders these
//! enums verbatim; it never re-derives a `Ready` from lock bytes.
//!
//! No Store writes, no acquisition, no network, no vendor CLI: every
//! field is computable from the cwd + the current Store index in
//! milliseconds (full per-receipt vendor inspection stays on the Doctor
//! report, where it is served by the inspection cache — see ADR 018).

#![allow(clippy::empty_line_after_doc_comments)]

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use serde::Serialize;

use uze_core::{
    PackageSource, Result,
    acquisition::marketplace,
    project_lock,
    workspace::{self, WorkspaceKind},
};

use super::{services::Workspace, *};

impl Workspace<'_> {
    /// What kind of UZE workspace `cwd` is inside, and the semantic state
    /// of its project/marketplace halves. Total for workspace-shaped
    /// inputs: a malformed `agents.lock` or `marketplace.json` is reported as a
    /// state (`Invalid`/`InvalidManifest`), never an error — the Overview
    /// exists to show exactly that, not to refuse to run because of it.
    /// The only `Err` is an unresolvable cwd.
    pub fn summary(&self, cwd: &Path) -> Result<OverviewWorkspaceSummary> {
        let resolved = workspace::resolve_workspace(cwd)?;
        let root = resolved.root.clone();
        let has_lock = matches!(
            resolved.kind,
            WorkspaceKind::Consumer | WorkspaceKind::Hybrid
        );
        let has_manifest = matches!(
            resolved.kind,
            WorkspaceKind::Marketplace | WorkspaceKind::Hybrid
        );
        Ok(OverviewWorkspaceSummary {
            cwd: cwd.to_path_buf(),
            root: root.clone(),
            kind: resolved.kind,
            agents_directory_present: root.join(".agents").is_dir(),
            project: self.project_overview(&root, has_lock),
            marketplace: has_manifest.then(|| Self::marketplace_overview(&root)),
        })
    }

    /// The project half — always present, so a directory without
    /// `agents.lock` still answers "not configured" instead of nothing.
    fn project_overview(&self, root: &Path, has_lock: bool) -> ProjectOverview {
        let loaded = has_lock.then(|| project_lock::load_lock(root));
        let (environment, declared, installed, missing) = match loaded {
            Some(Ok(Some(lock))) => {
                let installed_ids: BTreeSet<String> = self
                    .0
                    .installed_packages()
                    .into_iter()
                    .map(|package| package.id.as_str().to_owned())
                    .collect();
                let declared = lock.plugins.len();
                let missing: Vec<String> = lock
                    .plugins
                    .iter()
                    .filter(|(name, locked)| {
                        !installed_ids.contains(&UzeApplication::locked_plugin_id(name, locked))
                    })
                    .map(|(name, _)| name.clone())
                    .collect();
                let environment = if missing.is_empty() {
                    // Nothing declared, nothing required — or everything
                    // declared is installed. Either way there is no known
                    // unsatisfied requirement.
                    ProjectEnvironmentState::Ready
                } else {
                    ProjectEnvironmentState::InstallRequired
                };
                (environment, declared, declared - missing.len(), missing)
            }
            // A malformed/unreadable `agents.lock` is a real, reported
            // state — never silently "ready" or "not configured".
            Some(Err(_)) | Some(Ok(None)) => (ProjectEnvironmentState::Invalid, 0, 0, Vec::new()),
            None => (ProjectEnvironmentState::NotConfigured, 0, 0, Vec::new()),
        };
        let agents_md = root.join("AGENTS.md").is_file();
        let portability = self
            .0
            .context()
            .inspect(root)
            .ok()
            .map(|status| status.portability);
        ProjectOverview {
            environment,
            memory: derive_memory(agents_md, portability.as_ref()),
            declared_plugins: declared,
            installed_plugins: installed,
            missing_plugins: missing,
        }
    }

    fn marketplace_overview(root: &Path) -> OverviewMarketplace {
        match UzeApplication::load_marketplace_manifest(&PackageSource::Local {
            path: root.to_path_buf(),
        }) {
            Ok((_, manifest)) => {
                let package_count = manifest.plugins.len();
                let invalid_packages = manifest
                    .plugins
                    .iter()
                    .filter(|entry| {
                        marketplace::resolve_plugin_source(&manifest, &entry.name, root).is_err()
                    })
                    .count();
                OverviewMarketplace {
                    name: Some(manifest.name),
                    package_count,
                    invalid_packages,
                    state: MarketplaceState::Valid,
                }
            }
            Err(_) => OverviewMarketplace {
                name: None,
                package_count: 0,
                invalid_packages: 0,
                state: MarketplaceState::InvalidManifest,
            },
        }
    }
}

/// The Overview's workspace-aware lower section: root, kind, and the
/// semantic project/marketplace projections (whichever the kind implies).
#[derive(Clone, Debug, Serialize)]
pub struct OverviewWorkspaceSummary {
    pub cwd: PathBuf,
    /// Nearest ancestor (or cwd) carrying an anchor; equals `cwd` when no
    /// anchor exists anywhere on the path.
    pub root: PathBuf,
    pub kind: WorkspaceKind,
    /// Whether the project carries a `.agents/` directory. This is an
    /// observation for contextual clients; the Application owns the file
    /// inspection so presentation never probes project files directly.
    pub agents_directory_present: bool,
    /// Always present — a directory without `agents.lock` is a legitimate
    /// "not configured" project, not an absence of information.
    pub project: ProjectOverview,
    /// Present for `Marketplace`/`Hybrid` kinds.
    pub marketplace: Option<OverviewMarketplace>,
}

/// The user-facing state of the project half — derived here, rendered
/// verbatim by the TUI. `Ready` means: `agents.lock` exists and parses,
/// and every plugin it declares is installed in the Store. It deliberately
/// does NOT claim attachment/reconciliation state — that is only provable
/// through per-receipt vendor inspection (the Doctor screen's job).
#[derive(Clone, Debug, Serialize)]
pub struct ProjectOverview {
    pub environment: ProjectEnvironmentState,
    pub memory: MemoryState,
    /// Plugins declared by `agents.lock` (0 when there is no valid lock).
    pub declared_plugins: usize,
    /// Declared plugins present in the Store.
    pub installed_plugins: usize,
    /// Declared plugins NOT present in the Store. Empty for `Ready`.
    pub missing_plugins: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProjectEnvironmentState {
    /// No `agents.lock` — the project has no UZE environment declared.
    NotConfigured,
    /// `agents.lock` exists but cannot be parsed (malformed, unsupported
    /// version, unreadable).
    Invalid,
    /// The lock is valid but declares plugins not yet installed.
    InstallRequired,
    /// Valid lock, every declared plugin installed.
    Ready,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MemoryState {
    /// No `AGENTS.md` and no vendor-only context.
    None,
    /// `AGENTS.md` present and the context is portable across harnesses.
    Ready,
    /// Context exists but is not portable everywhere — a bridge gap, or
    /// vendor-specific files carrying content with no shared `AGENTS.md`.
    Issue,
}

/// The user-facing state of the marketplace half.
#[derive(Clone, Debug, Serialize)]
pub struct OverviewMarketplace {
    /// The marketplace's own declared name, when the manifest parses.
    pub name: Option<String>,
    /// Packages declared by `marketplace.json`.
    pub package_count: usize,
    /// Declared packages whose source directory is missing or escapes the
    /// root — a manifest can be valid JSON while pointing at nothing.
    pub invalid_packages: usize,
    pub state: MarketplaceState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MarketplaceState {
    /// Manifest parses and validates (unique names, right shape).
    Valid,
    /// Manifest is missing, malformed, or structurally invalid.
    InvalidManifest,
}

/// The `MemoryState` truth table, pure and testable: `AGENTS.md` presence
/// plus the portability verdict `context_inspect` produced (or `None` when
/// inspection was unavailable). `Issue` means "context exists but is not
/// portable everywhere" — a bridge gap behind a present `AGENTS.md`, or
/// vendor-specific files carrying content with no shared `AGENTS.md`.
fn derive_memory(agents_md: bool, portability: Option<&Portability>) -> MemoryState {
    match (agents_md, portability) {
        (true, Some(Portability::Portable)) => MemoryState::Ready,
        (true, Some(_)) => MemoryState::Issue,
        (true, None) => {
            // Inspection unavailable: the one fact we can still verify is
            // the file itself. "Present", not "portable everywhere".
            MemoryState::Ready
        }
        (false, Some(Portability::VendorLocked { .. })) => MemoryState::Issue,
        (false, _) => MemoryState::None,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use uze_core::{PackageSource, UzeHome, trust::AlwaysTrust};

    use super::*;

    fn write_plugin(root: &Path, plugin_name: &str) {
        let dir = root.join(plugin_name);
        fs::create_dir_all(dir.join("skills/uze-test")).unwrap();
        fs::write(
            dir.join("plugin.json"),
            format!(r#"{{"name": "{plugin_name}"}}"#),
        )
        .unwrap();
        fs::write(dir.join("skills/uze-test/SKILL.md"), "# Test skill\n").unwrap();
    }

    fn write_manifest(root: &Path, marketplace_name: &str, plugins: &[&str]) {
        fs::create_dir_all(root).unwrap();
        let entries: Vec<String> = plugins
            .iter()
            .map(|name| format!(r#"{{"name": "{name}", "source": "{name}"}}"#))
            .collect();
        fs::write(
            root.join(workspace::MARKETPLACE_MANIFEST_NAME),
            format!(
                r#"{{"name": "{marketplace_name}", "plugins": [{}]}}"#,
                entries.join(",")
            ),
        )
        .unwrap();
    }

    fn write_lock(root: &Path, plugin_names: &[&str], marketplace: &str) {
        let mut lock = project_lock::ProjectLock::default();
        for name in plugin_names {
            lock.plugins.insert(
                name.to_string(),
                project_lock::LockedPlugin {
                    source: project_lock::PluginSource::Marketplace {
                        marketplace: marketplace.to_owned(),
                        plugin: name.to_string(),
                    },
                    resolved: project_lock::ResolvedPlugin {
                        revision: None,
                        version: None,
                        integrity: None,
                    },
                },
            );
        }
        project_lock::save_lock(root, &lock).unwrap();
    }

    struct Fixture {
        app: UzeApplication,
        _drop_root: PathBuf,
    }

    impl Fixture {
        fn new(label: &str) -> Self {
            let base = uze_testkit::temp::scratch(label);
            let home = base.join("home");
            let app = UzeApplication::new(UzeHome::at(&home), Vec::new());
            Self {
                app,
                _drop_root: base,
            }
        }

        /// Installs as though acquired through marketplace `marketplace` —
        /// what a real `add_project_plugin` install does — so the Store's
        /// package id agrees with what `write_lock` declared, matching
        /// production behavior instead of the always-`local` shortcut
        /// `add_plugin` takes for a bare `uze add <path>`.
        fn install_from(&self, source: &Path, marketplace: &str) {
            let materialized = self
                .app
                .acquire(&PackageSource::Local {
                    path: source.to_path_buf(),
                })
                .unwrap();
            self.app
                .install_materialized_from_marketplace(
                    materialized,
                    marketplace,
                    &AlwaysTrust,
                    &[],
                    false,
                    &uze_core::naming::NoNameCollisionAuthority,
                )
                .unwrap();
        }

        fn project(&self, cwd: &Path) -> ProjectOverview {
            self.app.workspace().summary(cwd).unwrap().project
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self._drop_root);
        }
    }

    // --- Environment semantics ---------------------------------------------

    #[test]
    fn random_directory_is_not_configured_and_no_error() {
        let fx = Fixture::new("none");
        let dir = fx._drop_root.join("random");
        fs::create_dir_all(&dir).unwrap();
        let project = fx.project(&dir);
        assert_eq!(project.environment, ProjectEnvironmentState::NotConfigured);
        assert_eq!(project.memory, MemoryState::None);
        assert_eq!(project.declared_plugins, 0);
        assert_eq!(project.installed_plugins, 0);
        assert!(project.missing_plugins.is_empty());
    }

    #[test]
    fn agents_md_alone_has_memory_but_no_environment() {
        let fx = Fixture::new("agents-md-only");
        let dir = fx._drop_root.join("docs");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("AGENTS.md"), "# hi\n").unwrap();
        let project = fx.project(&dir);
        assert_eq!(project.environment, ProjectEnvironmentState::NotConfigured);
        assert_eq!(project.memory, MemoryState::Ready);
    }

    #[test]
    fn valid_lock_with_everything_installed_is_ready() {
        let fx = Fixture::new("ready");
        let root = fx._drop_root.join("project");
        let market = fx._drop_root.join("market");
        fs::create_dir_all(&root).unwrap();
        write_manifest(&market, "test", &["flow", "std"]);
        write_plugin(&market, "flow");
        write_plugin(&market, "std");
        write_lock(&root, &["flow", "std"], "test");
        fx.install_from(&market.join("flow"), "test");
        fx.install_from(&market.join("std"), "test");

        let project = fx.project(&root);
        assert_eq!(project.environment, ProjectEnvironmentState::Ready);
        assert_eq!(
            (project.declared_plugins, project.installed_plugins),
            (2, 2)
        );
        assert!(project.missing_plugins.is_empty());
    }

    #[test]
    fn valid_lock_with_nothing_installed_requires_install() {
        let fx = Fixture::new("nothing-installed");
        let root = fx._drop_root.join("project");
        let market = fx._drop_root.join("market");
        fs::create_dir_all(&root).unwrap();
        write_manifest(&market, "test", &["flow", "std"]);
        write_plugin(&market, "flow");
        write_plugin(&market, "std");
        write_lock(&root, &["flow", "std"], "test");

        let project = fx.project(&root);
        assert_eq!(
            project.environment,
            ProjectEnvironmentState::InstallRequired
        );
        assert_eq!(project.installed_plugins, 0);
        assert_eq!(project.missing_plugins.len(), 2);
    }

    #[test]
    fn partially_installed_lock_requires_install_and_reports_which() {
        let fx = Fixture::new("partial");
        let root = fx._drop_root.join("project");
        let market = fx._drop_root.join("market");
        fs::create_dir_all(&root).unwrap();
        write_manifest(&market, "test", &["flow", "std"]);
        write_plugin(&market, "flow");
        write_plugin(&market, "std");
        write_lock(&root, &["flow", "std"], "test");
        fx.install_from(&market.join("flow"), "test");

        let project = fx.project(&root);
        assert_eq!(
            project.environment,
            ProjectEnvironmentState::InstallRequired
        );
        assert_eq!(
            (project.declared_plugins, project.installed_plugins),
            (2, 1)
        );
        assert_eq!(project.missing_plugins, vec!["std".to_owned()]);
    }

    #[test]
    fn malformed_lock_is_invalid_never_ready() {
        let fx = Fixture::new("malformed");
        let root = fx._drop_root.join("project");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("agents.lock"), "version: 1\nplugins: [broken").unwrap();

        let project = fx.project(&root);
        assert_eq!(project.environment, ProjectEnvironmentState::Invalid);
        assert_eq!(project.installed_plugins, 0);
    }

    #[test]
    fn unsupported_lock_version_is_invalid() {
        let fx = Fixture::new("unsupported");
        let root = fx._drop_root.join("project");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("agents.lock"), "version: 99\n").unwrap();

        let project = fx.project(&root);
        assert_eq!(project.environment, ProjectEnvironmentState::Invalid);
    }

    #[test]
    fn empty_lock_is_ready_with_nothing_declared() {
        let fx = Fixture::new("empty-lock");
        let root = fx._drop_root.join("project");
        fs::create_dir_all(&root).unwrap();
        write_lock(&root, &[], "test");

        let project = fx.project(&root);
        assert_eq!(project.environment, ProjectEnvironmentState::Ready);
        assert_eq!(project.declared_plugins, 0);
    }

    #[test]
    fn ready_is_never_emitted_when_a_declared_plugin_is_missing() {
        let fx = Fixture::new("never-ready");
        let root = fx._drop_root.join("project");
        let market = fx._drop_root.join("market");
        fs::create_dir_all(&root).unwrap();
        write_manifest(&market, "test", &["flow", "std"]);
        write_plugin(&market, "flow");
        write_plugin(&market, "std");

        for installed in [0usize, 1] {
            // Rebuild a fresh fixture per scenario: the Store accumulates.
            let root = fx._drop_root.join(format!("project-{installed}"));
            fs::create_dir_all(&root).unwrap();
            write_lock(&root, &["flow", "std"], "test");
            if installed > 0 {
                fx.install_from(&market.join("flow"), "test");
            }
            let project = fx.project(&root);
            assert_eq!(
                project.environment,
                ProjectEnvironmentState::InstallRequired,
                "declared-but-missing must never read as Ready (installed={installed})"
            );
        }
    }

    #[test]
    fn memory_truth_table_is_exact() {
        use Portability::{NoContext, PartiallyPortable, Portable, VendorLocked};
        // (agents_md, portability) → MemoryState, covering every reachable
        // combination plus the inspection-unavailable case.
        assert_eq!(derive_memory(false, None), MemoryState::None);
        assert_eq!(derive_memory(true, None), MemoryState::Ready);
        assert_eq!(derive_memory(false, Some(&NoContext)), MemoryState::None);
        assert_eq!(derive_memory(false, Some(&Portable)), MemoryState::None);
        assert_eq!(
            derive_memory(false, Some(&VendorLocked { files: vec![] })),
            MemoryState::Issue
        );
        assert_eq!(derive_memory(true, Some(&NoContext)), MemoryState::Issue);
        assert_eq!(derive_memory(true, Some(&Portable)), MemoryState::Ready);
        assert_eq!(
            derive_memory(true, Some(&VendorLocked { files: vec![] })),
            MemoryState::Issue
        );
        assert_eq!(
            derive_memory(true, Some(&PartiallyPortable { gaps: vec![] })),
            MemoryState::Issue
        );
    }

    #[test]
    fn memory_is_ready_with_agents_md_and_none_without_it() {
        let fx = Fixture::new("memory-presence");
        let root = fx._drop_root.join("project");
        fs::create_dir_all(&root).unwrap();
        write_lock(&root, &[], "test");

        assert_eq!(fx.project(&root).memory, MemoryState::None);
        fs::write(root.join("AGENTS.md"), "# hi\n").unwrap();
        assert_eq!(fx.project(&root).memory, MemoryState::Ready);
    }

    // --- Marketplace semantics ---------------------------------------------

    #[test]
    fn marketplace_valid_lists_name_and_package_count() {
        let fx = Fixture::new("market-valid");
        let root = fx._drop_root.join("market");
        fs::create_dir_all(root.join("plugins")).unwrap();
        write_manifest(&root, "acme", &["flow", "std"]);
        write_plugin(&root, "flow");
        write_plugin(&root, "std");

        let summary = fx.app.workspace().summary(&root).unwrap();
        assert_eq!(summary.kind, WorkspaceKind::Marketplace);
        let market = summary.marketplace.as_ref().unwrap();
        assert_eq!(market.state, MarketplaceState::Valid);
        assert_eq!(market.name.as_deref(), Some("acme"));
        assert_eq!(market.package_count, 2);
        assert_eq!(market.invalid_packages, 0);
    }

    #[test]
    fn marketplace_invalid_manifest_is_reported_not_errored() {
        let fx = Fixture::new("market-invalid");
        let root = fx._drop_root.join("market");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(workspace::MARKETPLACE_MANIFEST_NAME), "not json").unwrap();

        let summary = fx.app.workspace().summary(&root).unwrap();
        assert_eq!(summary.kind, WorkspaceKind::Marketplace);
        let market = summary.marketplace.as_ref().unwrap();
        assert_eq!(market.state, MarketplaceState::InvalidManifest);
        assert_eq!(market.name, None);
    }

    #[test]
    fn marketplace_counts_packages_with_missing_sources() {
        let fx = Fixture::new("market-missing");
        let root = fx._drop_root.join("market");
        fs::create_dir_all(&root).unwrap();
        // "review" is declared but its directory never created.
        write_manifest(&root, "acme", &["flow", "review"]);
        write_plugin(&root, "flow");

        let market = fx
            .app
            .workspace()
            .summary(&root)
            .unwrap()
            .marketplace
            .unwrap();
        assert_eq!(market.state, MarketplaceState::Valid);
        assert_eq!(market.package_count, 2);
        assert_eq!(market.invalid_packages, 1);
    }

    // --- Kind composition ---------------------------------------------------

    #[test]
    fn hybrid_reports_both_halves() {
        let fx = Fixture::new("hybrid");
        let root = fx._drop_root.join("hybrid");
        fs::create_dir_all(&root).unwrap();
        write_lock(&root, &["flow", "std"], "test");
        write_manifest(&root, "acme", &["flow", "std"]);
        write_plugin(&root, "flow");
        write_plugin(&root, "std");

        let summary = fx.app.workspace().summary(&root).unwrap();
        assert_eq!(summary.kind, WorkspaceKind::Hybrid);
        assert!(summary.project.environment != ProjectEnvironmentState::Ready);
        assert_eq!(
            summary.project.environment,
            ProjectEnvironmentState::InstallRequired
        );
        assert!(summary.marketplace.is_some());
    }

    #[test]
    fn nested_workspace_resolves_nearest() {
        let fx = Fixture::new("nested");
        let outer = fx._drop_root.join("marketplace");
        let inner = outer.join("plugins/consumer");
        fs::create_dir_all(inner.join("src")).unwrap();
        write_manifest(&outer, "acme", &[]);
        write_lock(&inner, &["flow"], "test");

        let summary = fx.app.workspace().summary(&inner.join("src")).unwrap();
        assert_eq!(summary.kind, WorkspaceKind::Consumer);
        assert_eq!(summary.root, inner.canonicalize().unwrap());
        assert_eq!(
            summary.project.environment,
            ProjectEnvironmentState::InstallRequired
        );
        assert!(summary.marketplace.is_none());
    }
}
