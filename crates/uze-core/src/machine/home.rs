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

    /// Everything UZE records about one project, in one directory.
    ///
    /// It used to be four: `state/tasks/<id>.json`,
    /// `state/conversations/<id>/`, `state/prompt-history/<id>.json` and
    /// `runtime/projects/<id>/`, each keyed by the same
    /// `harness_runtime::project_id_for` — a one-way hash — and only the
    /// last of them recording what the hash *meant*. So the records could
    /// be enumerated and none of them resolved: a sweep of the machine
    /// could see every project's agents and locate not one checkout.
    ///
    /// One directory answers that by holding
    /// [`Self::project_marker_path`], and it pays twice more: forgetting a
    /// project becomes a single removal, and this mirrors
    /// [`Self::runtime_project_dir`] under the same id, so which of the two
    /// is safe to delete is legible from the layout alone.
    pub fn project_dir(&self, project_id: &str) -> PathBuf {
        self.state_dir().join("projects").join(project_id)
    }

    /// The canonical root a project's records were written for — what
    /// makes the id reversible, and the whole input to a sweep of every
    /// project UZE knows.
    ///
    /// The same answer `harness_runtime::PROJECTION_MARKER` already gives
    /// the generated tree: name the root, so a sweep is a `readdir` rather
    /// than an exercise in inverting a hash.
    pub fn project_marker_path(&self, project_id: &str) -> PathBuf {
        self.project_dir(project_id).join("project.json")
    }

    /// Every project UZE has recorded an agent for, one directory each —
    /// the whole input to a machine-wide sweep.
    pub fn projects_dir(&self) -> PathBuf {
        self.state_dir().join("projects")
    }

    /// A project's agents — every launch UZE has made in it, isolated or
    /// not — keyed on the project id so removing a checkout can never
    /// remove history.
    pub fn tasks_path(&self, project_id: &str) -> PathBuf {
        self.project_dir(project_id).join("agents.json")
    }

    /// One agent's recorded conversations, beside the agents that name it.
    /// A file per agent rather than one document for the project, so a
    /// launch is one write and forgetting a project is still one removal.
    pub fn conversation_path(&self, project_id: &str, task_id: &str) -> PathBuf {
        self.project_dir(project_id)
            .join("conversations")
            .join(format!("{task_id}.json"))
    }

    /// What UZE last observed about each harness's machine-level setup:
    /// the version it answered with, and the strategy UZE delivers to it
    /// by.
    ///
    /// Remembered, not recorded. Every field comes back from probing the
    /// harness again — every command records each detected harness on its
    /// way in — so deleting it costs a probe and nothing else. Ownership of
    /// individual package attachments is a different question entirely, and
    /// lives in `attachments.json`, which nothing re-derives.
    pub fn harnesses_cache_path(&self) -> PathBuf {
        self.cache_dir().join("harnesses.json")
    }

    /// Secret-free record of an explicit vendor executable provisioning
    /// attempt. It is deliberately separate from integration preparation and
    /// package attachment ownership.
    ///
    /// A record, unlike [`Self::harnesses_cache_path`] beside it, and the
    /// difference is worth stating because the two look alike: this is the
    /// *history* of an attempt UZE made — what it did, whether it worked,
    /// when — and no probe brings history back. The two `version` fields
    /// are not one fact twice: that one is what is installed now, this one
    /// is what this attempt put there.
    pub fn provisioning_state_path(&self) -> PathBuf {
        self.state_dir().join("provisioning.json")
    }

    pub fn marketplaces_path(&self) -> PathBuf {
        self.state_dir().join("marketplaces.json")
    }

    /// UZE-owned Profiles/Preferences (durable user intent, never
    /// reconstructable from a harness's own config — hence `state_dir()`,
    /// not `cache_dir()`). Harness-specific files are projections of this,
    /// not the source of truth.
    pub fn profiles_path(&self) -> PathBuf {
        self.state_dir().join("profiles.json")
    }

    /// What the TUI was left looking like, in both of its modes (see
    /// `client_layout`).
    pub fn client_layout_path(&self) -> PathBuf {
        self.state_dir().join("layout.json")
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

    /// The operator's own keyboard, applied over the built-in keymap.
    ///
    /// Beside `theme-overrides.json` rather than under `state/` for the
    /// same reason: it is something someone wrote, never something UZE can
    /// rebuild. It holds only what differs from the default, so a chord
    /// nobody had an opinion about still moves when a later release moves
    /// it.
    pub fn keymap_path(&self) -> PathBuf {
        self.root.join("keys.json")
    }

    /// The ledger of which package UZE attached where, per integration.
    ///
    /// The one record ownership lives in: an artifact on a harness's disk
    /// says what it is, never who put it there, so nothing else on the
    /// machine can answer this and nothing re-derives it.
    pub fn attachments_path(&self) -> PathBuf {
        self.state_dir().join("attachments.json")
    }

    /// The process-wide mutation guard for this home (see
    /// [`crate::persistence::MutationLock`]). A lock, not a record: it
    /// carries no shape and nothing reads it across versions.
    pub fn mutation_lock_path(&self) -> PathBuf {
        self.state_dir().join("mutation.lock")
    }

    /// What UZE remembers about its own binary between runs: when it last
    /// asked for the latest release, and what the operator was told.
    pub fn binary_path(&self) -> PathBuf {
        self.state_dir().join("update.json")
    }

    /// The receipt `install.sh` leaves: the file it placed and the release
    /// it was. Inbound — UZE reads it and never writes it — which is why it
    /// is a document of its own rather than part of [`Self::binary_path`].
    pub fn install_receipt_path(&self) -> PathBuf {
        self.state_dir().join("install.json")
    }

    /// Where an update keeps the revision it is replacing, under UZE's own
    /// state rather than beside the plugins: nothing that reads the Store
    /// may mistake it for an installed package. Generated — removing it
    /// costs nothing.
    pub fn superseded_dir(&self) -> PathBuf {
        self.state_dir().join("superseded")
    }

    /// One project's prompt history, beside its other records.
    pub fn prompt_history_path(&self, project_id: &str) -> PathBuf {
        self.project_dir(project_id).join("prompt-history.json")
    }

    /// Where UZE writes its own log when asked to. Disposable, which is
    /// why it sits with the caches rather than with the records.
    pub fn logs_dir(&self) -> PathBuf {
        self.cache_dir().join("logs")
    }

    /// Everything UZE *generates* for one harness to read: generated
    /// marketplaces, staged skill directories, the wrappers a bridge is
    /// made of.
    ///
    /// Under the runtime tree rather than beside the records, because that
    /// is what it is. It used to live at `state/attachments/`, one letter
    /// from `attachments.json` — the ledger that says who owns what in
    /// here — so the directory read as authoritative while every byte in
    /// it is produced again from the Store and the Engine alone. An
    /// operator deciding what is safe to delete had no way to tell the two
    /// apart, and the answer is opposite for each.
    pub fn generated_attachments_dir(&self, vendor: &str) -> PathBuf {
        self.runtime_dir().join("attachments").join(vendor)
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

    /// Cross-invocation cache of registered marketplaces' catalogues (see
    /// `application::marketplace_catalogue`): one checkout per Git
    /// marketplace, so listing what it offers reads a directory instead of
    /// cloning a remote. Reconstructable from the registered source at any
    /// time — never authoritative, hence under `cache_dir()`.
    pub fn marketplace_cache_dir(&self) -> PathBuf {
        self.cache_dir().join("marketplaces")
    }

    /// The runtime tree. Its one tenant, `projects/`, outlives every
    /// invocation and dies with the project root; anything else beneath it
    /// is swept.
    pub fn runtime_dir(&self) -> PathBuf {
        self.root.join("runtime")
    }

    /// Every project UZE has ever projected into, one directory each —
    /// which is also the whole input to `harness_runtime::prune_projections`.
    pub fn runtime_projects_dir(&self) -> PathBuf {
        self.runtime_dir().join("projects")
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

    /// Where a project-scoped runtime projection lives for one integration:
    /// a derived, rebuildable cache meant to persist and be safely shared by
    /// concurrent sessions on the same project — never torn down at session
    /// end.
    pub fn runtime_projection_dir(&self, integration: &str, project_id: &str) -> PathBuf {
        self.runtime_project_dir(project_id).join(integration)
    }

    /// Everything UZE generates for another program to read, and
    /// everything it remembered from observing. Deleting either costs
    /// nothing: the first is produced again, the second observed again.
    ///
    /// Named together because that is the one question an operator staring
    /// at `~/.uze` actually has — *can I delete this* — and the layout
    /// answers it by which directory a thing is in.
    pub fn rebuildable_dirs(&self) -> [PathBuf; 2] {
        [self.runtime_dir(), self.cache_dir()]
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

    /// Resolves the root from the environment, refusing the two spellings
    /// that silently make every UZE path relative to the current directory.
    ///
    /// `env::var_os` answers `Some("")` for a variable set to the empty
    /// string — the ordinary result of `export UZE_HOME="$SOMETHING_UNSET"`
    /// in a wrapper script or a CI step — and an empty root makes
    /// `store_dir()` the bare name `store`. A relative value is the same
    /// failure spread over time: the Store, the ledger and the receipts fork
    /// per working directory, each command finding a different machine. An
    /// empty value is read as "not set" because that is what the shell meant
    /// by it; a relative one is an error, because nothing else it could have
    /// meant is plausible. [`UzeHome::at`] stays permissive: a caller naming
    /// a root in code has already decided.
    fn from_values(
        uze_home: Option<std::ffi::OsString>,
        home: Option<std::ffi::OsString>,
    ) -> Result<Self> {
        if let Some(path) = uze_home.filter(|value| !value.is_empty()) {
            return Ok(Self::at(absolute_or_refuse("UZE_HOME", path)?));
        }
        let home = home
            .filter(|value| !value.is_empty())
            .ok_or(UzeError::MissingHomeDirectory)?;
        Ok(Self::at(absolute_or_refuse("HOME", home)?.join(".uze")))
    }
}

fn absolute_or_refuse(variable: &'static str, value: std::ffi::OsString) -> Result<PathBuf> {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(UzeError::RelativeHomeDirectory {
            variable,
            value: path.to_string_lossy().into_owned(),
        })
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

    /// `export UZE_HOME="$SOMETHING_UNSET"` is how a wrapper script sets a
    /// variable to nothing. It means "not set", never "the directory I
    /// happen to be standing in".
    #[test]
    fn an_empty_uze_home_is_read_as_unset() {
        let home = UzeHome::from_values(Some("".into()), Some("/tmp/user-home".into())).unwrap();
        assert_eq!(home.root(), Path::new("/tmp/user-home/.uze"));
    }

    #[test]
    fn an_empty_home_leaves_no_default_to_derive() {
        assert!(matches!(
            UzeHome::from_values(None, Some("".into())),
            Err(UzeError::MissingHomeDirectory)
        ));
    }

    /// The tier a thing sits in is a promise about what deleting it costs,
    /// and the promise is only worth making if the layout keeps it: every
    /// path UZE owns lands in exactly one tier, and the two rebuildable
    /// ones hold nothing a record needs.
    #[test]
    fn nothing_a_record_needs_sits_in_a_tier_that_can_be_deleted() {
        let home = UzeHome::at("/tmp/uze-tiers");
        let rebuildable = home.rebuildable_dirs();

        let records = [
            home.registry_path(),
            home.marketplaces_path(),
            home.attachments_path(),
            home.profiles_path(),
            home.client_layout_path(),
            home.active_theme_path(),
            home.provisioning_state_path(),
            home.binary_path(),
            home.tasks_path("abc"),
            home.conversation_path("abc", "def"),
            home.prompt_history_path("abc"),
        ];
        for record in records {
            assert!(
                record.starts_with(home.state_dir()),
                "a record belongs with the records: {}",
                record.display()
            );
            for tier in &rebuildable {
                assert!(
                    !record.starts_with(tier),
                    "{} is a record and must not sit where deleting costs nothing",
                    record.display()
                );
            }
        }

        // And the things that *are* rebuildable say so by where they are.
        for rebuilt in [
            home.harnesses_cache_path(),
            home.harness_detection_cache_path(),
            home.inspection_cache_path(),
            home.marketplace_cache_dir(),
            home.logs_dir(),
            home.generated_attachments_dir("claude"),
            home.runtime_project_dir("abc"),
        ] {
            assert!(
                rebuildable.iter().any(|tier| rebuilt.starts_with(tier)),
                "{} is produced or observed again, so it belongs in a tier \
                 that says deleting it costs nothing",
                rebuilt.display()
            );
        }

        // The shims are the one exception, and it is about `PATH`, not
        // about cost: `which claude` reading `~/.uze/shims/claude` says
        // what it is where `~/.uze/runtime/shims/claude` says less.
        assert_eq!(home.shims_dir(), home.root().join("shims"));
    }

    /// A relative root is the same failure spread over time: the same home
    /// resolves to a different tree per working directory.
    #[test]
    fn a_relative_root_is_refused_by_name() {
        assert!(matches!(
            UzeHome::from_values(Some("relative/uze".into()), None),
            Err(UzeError::RelativeHomeDirectory {
                variable: "UZE_HOME",
                ..
            })
        ));
        assert!(matches!(
            UzeHome::from_values(None, Some("user-home".into())),
            Err(UzeError::RelativeHomeDirectory {
                variable: "HOME",
                ..
            })
        ));
    }
}
