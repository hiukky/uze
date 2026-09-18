//! A small declarative scenario builder.
//!
//! The goal is not a DSL: it is to replace the ~70-line
//! mkdir/copy/json-manipulation prologue of a scenario test with intent:
//!
//! ```ignore
//! let scenario = Scenario::new()
//!     .plugin("flow", fixtures::canonical("skill-plugin"))
//!     .marketplace("ai", r#"{ "name": "ai", "plugins": [ ... ] }"#)
//!     .lock_plugin_from_market("ai", "flow")
//!     .project_file("AGENTS.md", "# project\n")
//!     .materialize(&env);
//! ```
//!
//! `materialize` writes the marketplace (recording its *absolute* path in
//! the lock), the project files and `agents.lock` into `env.project`.

use std::path::{Path, PathBuf};

use crate::temp::TestEnvironment;

/// Declarative description of a deliberate system state.
#[derive(Default)]
pub struct Scenario {
    marketplace: Option<MarketplaceSpec>,
    marketplace_plugins: Vec<(String, PathBuf)>,
    lock_plugins: Vec<LockedPlugin>,
    project_files: Vec<(String, String)>,
}

struct MarketplaceSpec {
    name: String,
    marketplace_json: String,
}

struct LockedPlugin {
    marketplace: String,
    plugin: String,
}

/// The materialized state: paths a scenario test then points at.
pub struct MaterializedScenario {
    pub marketplace: Option<PathBuf>,
    pub project: PathBuf,
    pub lock: PathBuf,
}

impl Scenario {
    /// An empty scenario.
    pub fn new() -> Self {
        Scenario::default()
    }

    /// Declares a marketplace root: `marketplace.json` content written to
    /// `<env root>/market-<name>/marketplace.json` at materialize time.
    pub fn marketplace(mut self, name: &str, marketplace_json: &str) -> Self {
        self.marketplace = Some(MarketplaceSpec {
            name: name.to_owned(),
            marketplace_json: marketplace_json.to_owned(),
        });
        self
    }

    /// Adds a plugin **directory** to the declared marketplace by copying
    /// `source` into `<market>/plugins/<name>` (a real marketplace serves
    /// its plugin bytes from a local path).
    pub fn marketplace_plugin(mut self, name: &str, source: impl AsRef<Path>) -> Self {
        self.marketplace_plugins
            .push((name.to_owned(), source.as_ref().to_path_buf()));
        self
    }

    /// Declares a plugin locked from a declared marketplace. The lock
    /// records the marketplace's absolute materialized path, so the same
    /// scenario reproduces on any machine.
    pub fn lock_plugin_from_market(mut self, marketplace: &str, plugin: &str) -> Self {
        self.lock_plugins.push(LockedPlugin {
            marketplace: marketplace.to_owned(),
            plugin: plugin.to_owned(),
        });
        self
    }

    /// Adds a file relative to the project root (created at materialize).
    pub fn project_file(mut self, rel: impl Into<String>, contents: impl Into<String>) -> Self {
        self.project_files.push((rel.into(), contents.into()));
        self
    }

    /// Writes everything into `env`: the marketplace under the env root,
    /// project files and `agents.lock` under `env.project`.
    pub fn materialize(self, env: &TestEnvironment) -> MaterializedScenario {
        let staged = self.marketplace.as_ref().map(|spec| {
            let directory = env.root().join(format!("market-{}", spec.name));
            let revision = crate::marketplace::stage(
                &directory,
                &spec.marketplace_json,
                &self.marketplace_plugins,
            );
            (spec.name.as_str(), directory, revision)
        });

        for (rel, contents) in &self.project_files {
            let path = env.project.join(rel);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .expect("scenario: project file parent must be creatable");
            }
            std::fs::write(&path, contents).expect("scenario: project file must be writable");
        }

        let lock = env.project.join("agents.lock");
        if !self.lock_plugins.is_empty() {
            let (name, market, revision) = staged
                .as_ref()
                .unwrap_or_else(|| panic!("scenario: lock_plugin_from_market needs a marketplace"));
            // A local marketplace is a clone, and a clone's path is a valid
            // Git URL: the lock names the repository and the commit, the
            // same as it would for a remote one.
            let mut yaml = String::from("version: 1\nmarketplaces:\n");
            yaml.push_str(&format!(
                "  {name}:\n    git: {}\n    revision: {revision}\n",
                market.display()
            ));
            yaml.push_str("plugins:\n");
            for plugin in &self.lock_plugins {
                yaml.push_str(&format!(
                    "  {}:\n    marketplace: {}\n",
                    plugin.plugin, plugin.marketplace
                ));
            }
            std::fs::write(&lock, yaml).expect("scenario: agents.lock must be writable");
        }

        MaterializedScenario {
            marketplace: staged.map(|(_, directory, _)| directory),
            project: env.project.clone(),
            lock,
        }
    }
}
