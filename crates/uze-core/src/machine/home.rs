//! UZE-owned local paths.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use crate::{
    error::{Result, UzeError},
    store::PackageId,
};

/// The sole authority for UZE-owned filesystem locations.
///
/// The CLI resolves `UZE_HOME` once through `from_env`. Tests and embedded
/// callers should prefer `at` so they never mutate a process-global variable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UzeHome {
    root: PathBuf,
}

impl UzeHome {
    pub fn from_env() -> Result<Self> {
        Self::from_values(env::var_os("UZE_HOME"), env::var_os("HOME"))
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Root the package tree is published under. Several harnesses resolve a
    /// package path relative to the root of their own catalogue, so an
    /// integration that maintains such a catalogue places it here — but the
    /// layout stays UZE's, and this module names no harness.
    pub fn store_dir(&self) -> PathBuf {
        self.root.join("store")
    }

    pub fn plugins_dir(&self) -> PathBuf {
        self.store_dir().join("plugins")
    }

    pub fn plugin_dir(&self, id: &PackageId) -> PathBuf {
        self.plugins_dir()
            .join(id.marketplace())
            .join(id.plugin_name())
    }

    pub fn state_dir(&self) -> PathBuf {
        self.root.join("state")
    }

    pub fn registry_path(&self) -> PathBuf {
        self.state_dir().join("packages.json")
    }

    /// A project's task graph — every agent launch UZE has made in it, keyed
    /// on the project id so removing a checkout can never remove history.
    pub fn tasks_path(&self, project_id: &str) -> PathBuf {
        self.state_dir()
            .join("tasks")
            .join(format!("{project_id}.json"))
    }

    /// Per-harness machine integration setup facts. Ownership of individual
    /// package attachments lives exclusively in `attachments.json`.
    pub fn integrations_state_path(&self) -> PathBuf {
        self.state_dir().join("integrations.json")
    }

    /// Secret-free record of an explicit vendor executable provisioning
    /// attempt. It is deliberately separate from integration preparation and
    /// package attachment ownership.
    pub fn provisioning_state_path(&self) -> PathBuf {
        self.state_dir().join("provisioning.json")
    }

    pub fn marketplaces_path(&self) -> PathBuf {
        self.state_dir().join("marketplaces.json")
    }

    pub fn plugin_marketplaces_path(&self) -> PathBuf {
        self.state_dir().join("plugin_marketplaces.json")
    }

    /// UZE-owned Profiles/Preferences (durable user intent, never
    /// reconstructable from a harness's own config — hence `state_dir()`,
    /// not `cache_dir()`). Harness-specific files are projections of this,
    /// not the source of truth.
    pub fn profiles_path(&self) -> PathBuf {
        self.state_dir().join("profiles.json")
    }

    /// What the workspace client's sidebar was left looking like (see
    /// `sidebar_layout`).
    pub fn sidebar_layout_path(&self) -> PathBuf {
        self.state_dir().join("sidebar.json")
    }

    /// Where a user's own themes live, one file per theme, named by the
    /// theme's own id. Durable user intent like Profiles are — a theme is
    /// something someone wrote, never something UZE can rebuild — so
    /// `state_dir()`'s sibling under the root rather than a cache.
    pub fn themes_dir(&self) -> PathBuf {
        self.root.join("themes")
    }

    /// Which theme is active. Machine-scoped, like every other appearance
    /// choice: a project does not get to decide what the operator's
    /// terminal looks like.
    pub fn active_theme_path(&self) -> PathBuf {
        self.state_dir().join("theme.json")
    }

    /// The operator's own last word on appearance, applied over whichever
    /// theme is active.
    ///
    /// Beside `themes/` rather than inside it, because it is not a theme:
    /// it never becomes selectable, and it does not stop applying when the
    /// theme changes. That is exactly what it is for — a Nerd Font's glyphs
    /// belong to the machine, not to whichever palette is on today, and
    /// forking every theme to carry them is how they get out of step.
    pub fn theme_overrides_path(&self) -> PathBuf {
        self.root.join("theme-overrides.json")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }

    /// Cross-invocation cache of `IntegrationPort::detect()` results (see
    /// `detection_cache`). Reconstructable at any time from a live probe —
    /// never authoritative, hence `cache_dir()` rather than `state_dir()`.
    pub fn harness_detection_cache_path(&self) -> PathBuf {
        self.cache_dir().join("harness_detection.json")
    }

    /// Cross-invocation cache of per-receipt attachment *read* results
    /// (see `application::inspection_cache` and ADR 018). Same
    /// reconstructable-optimization caveat as the detection cache: never
    /// authoritative, and mutating paths always re-inspect live.
    pub fn inspection_cache_path(&self) -> PathBuf {
        self.cache_dir().join("inspection.json")
    }

    /// The runtime tree, whose two tenants have opposite lifetimes and are
    /// therefore kept in named siblings rather than interleaved by
    /// integration: `projects/` outlives every invocation and dies with the
    /// project root, `sessions/` dies with the invocation that made it.
    /// A sweep that had to tell them apart by guessing at a name would be
    /// one rename away from deleting the wrong one.
    pub fn runtime_dir(&self) -> PathBuf {
        self.root.join("runtime")
    }

    /// Every project UZE has ever projected into, one directory each —
    /// which is also the whole input to `harness_runtime::prune_projections`.
    pub fn runtime_projects_dir(&self) -> PathBuf {
        self.runtime_dir().join("projects")
    }

    pub fn runtime_sessions_dir(&self) -> PathBuf {
        self.runtime_dir().join("sessions")
    }

    pub fn runtime_session_dir(&self, integration: &str, session: &str) -> PathBuf {
        self.runtime_sessions_dir().join(integration).join(session)
    }

    /// Where the PATH shim (`claude`, `codex`, `opencode`, `antigravity`,
    /// …) lives. Never on `PATH` unless the operator has explicitly enabled
    /// runtime integration and added it themselves — UZE does not edit
    /// shell rc files.
    pub fn shims_dir(&self) -> PathBuf {
        self.root.join("shims")
    }

    /// One project's own corner of the runtime tree, keyed by
    /// `harness_runtime::project_id_for` — the parent of every integration's
    /// projection for it, and of the marker naming the root they were all
    /// built for.
    ///
    /// Project-first rather than integration-first because the lifetime is
    /// the project's: everything below shares one answer to "does the root
    /// still exist", so a dead project is one `remove_dir_all` and one
    /// marker to read, not one of each per harness.
    pub fn runtime_project_dir(&self, project_id: &str) -> PathBuf {
        self.runtime_projects_dir().join(project_id)
    }

    /// Where a project-scoped runtime projection lives for one integration.
    /// Distinct from `runtime_session_dir`: a runtime projection is a
    /// derived, rebuildable cache meant to persist and be safely shared by
    /// concurrent sessions on the same project — never torn down at session
    /// end.
    pub fn runtime_projection_dir(&self, integration: &str, project_id: &str) -> PathBuf {
        self.runtime_project_dir(project_id).join(integration)
    }

    pub fn ensure_layout(&self) -> Result<()> {
        for directory in [
            self.plugins_dir(),
            self.state_dir(),
            self.cache_dir(),
            self.runtime_dir(),
        ] {
            fs::create_dir_all(&directory).map_err(|source| UzeError::Write {
                path: directory,
                source,
            })?;
        }
        Ok(())
    }

    fn from_values(
        uze_home: Option<std::ffi::OsString>,
        home: Option<std::ffi::OsString>,
    ) -> Result<Self> {
        if let Some(path) = uze_home {
            return Ok(Self::at(path));
        }
        let home = home.ok_or(UzeError::MissingHomeDirectory)?;
        Ok(Self::at(PathBuf::from(home).join(".uze")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_uze_home_wins_over_default_home() {
        let home = UzeHome::from_values(
            Some("/tmp/explicit-uze".into()),
            Some("/tmp/user-home".into()),
        )
        .unwrap();
        assert_eq!(home.root(), Path::new("/tmp/explicit-uze"));
    }

    #[test]
    fn default_home_is_derived_only_when_uze_home_is_missing() {
        let home = UzeHome::from_values(None, Some("/tmp/user-home".into())).unwrap();
        assert_eq!(home.root(), Path::new("/tmp/user-home/.uze"));
    }
}
