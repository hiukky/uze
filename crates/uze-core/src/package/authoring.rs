//! Producing the bytes a package is acquired *from* — the authoring surface
//! beside `acquisition`, which is the same question asked after those bytes
//! exist.
//!
//! - **Scaffold** — a marketplace (`marketplace.json`, `plugins/`, a Git
//!   repository with its initial commit) and a plugin inside one
//!   (`plugin.json`, a skill, the optional capability files). Every
//!   template carries commented field documentation, and every layout this
//!   module writes must pass `check` — the invariant guarded by the tests
//!   beside the templates, so the documentation cannot drift from what the
//!   parsers accept.
//! - **Check** — the validation an install would apply, delivered offline
//!   and before any install, through the same parsers the delivery engine
//!   uses. Never a second grammar.

use std::fs;
use std::path::{Path, PathBuf};

use crate::capability::CapabilityKind;
use crate::package::acquisition::marketplace;
use crate::package::store;
use crate::{PackageId, Result, UzeError};

/// The optional capability files a scaffold writes, one flag each.
#[derive(Clone, Copy, Debug, Default)]
pub struct ScaffoldCapabilities {
    pub hook: bool,
    pub mcp: bool,
    pub agent: bool,
    pub instructions: bool,
}

/// The manifest every marketplace carries.
const MARKETPLACE_MANIFEST: &str = "marketplace.json";

/// Where a marketplace's plugins live, by the convention the official
/// marketplace itself follows.
const PLUGINS_DIRECTORY: &str = "plugins";

/// The initial commit message a scaffolded marketplace is born with.
const INITIAL_COMMIT_MESSAGE: &str = "chore: scaffold marketplace";

/// Scaffolds a **local** marketplace: the named project is itself the
/// marketplace, the way this repository is one — `marketplace.json` at the
/// project root, the plugins in `plugins_dir` beside it (default
/// `plugins/`, renameable).
///
/// No Git is touched: the project's own repository *is* the marketplace's
/// repository, so its identity travels with the project the way every other
/// project file does, and the commit is the project's own flow's to make.
/// A project that already carries a `marketplace.json` is refused with that
/// fact — adding the plugin directly is the answer, not a second manifest.
pub fn scaffold_local_marketplace(
    name: &str,
    description: Option<&str>,
    project_root: &Path,
    plugins_dir: &str,
) -> Result<(PathBuf, PathBuf)> {
    if !store::is_valid_package_name(name) {
        return Err(UzeError::InvalidPackageName {
            name: name.to_owned(),
            path: project_root.to_path_buf(),
        });
    }
    if !store::is_valid_package_name(plugins_dir) {
        return Err(UzeError::InvalidPackageName {
            name: plugins_dir.to_owned(),
            path: project_root.to_path_buf(),
        });
    }
    let manifest_path = project_root.join(MARKETPLACE_MANIFEST);
    if manifest_path.is_file() {
        let existing = fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|body| serde_json::from_str::<serde_json::Value>(&body).ok())
            .and_then(|value| {
                value
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_default();
        return Err(UzeError::MarketplaceScaffold(format!(
            "`{}` already carries a marketplace.json (named `{}`) — this project is already a \
             marketplace; add the plugin directly with \
             `uze agent plugin create <name> --market <its name>`",
            project_root.display(),
            existing
        )));
    }
    let plugins = project_root.join(plugins_dir);
    fs::create_dir_all(&plugins).map_err(|source| UzeError::Write {
        path: plugins.clone(),
        source,
    })?;
    let manifest = serde_json::json!({
        "name": name,
        "description": description.unwrap_or("This project's own plugins."),
        "plugins": [],
    });
    let body = serde_json::to_string_pretty(&manifest).expect("a scaffolded manifest serializes");
    fs::write(&manifest_path, format!("{body}\n")).map_err(|source| UzeError::Write {
        path: manifest_path.clone(),
        source,
    })?;
    Ok((project_root.to_path_buf(), plugins))
}

/// Scaffolds a marketplace at `at`: `marketplace.json` (name, description,
/// an empty `plugins` list), an empty `plugins/` tree, a README, and a Git
/// repository with an initial commit — the identity contract a marketplace
/// is held to.
///
/// The commit is made with the machine's own Git identity. When none is
/// configured, the answer names what to set — the same detect → explain →
/// hand-the-command shape the requirements check uses. UZE never fabricates
/// an author line in the author's repository.
pub fn scaffold_marketplace(name: &str, description: Option<&str>, at: &Path) -> Result<PathBuf> {
    if !store::is_valid_package_name(name) {
        return Err(UzeError::InvalidPackageName {
            name: name.to_owned(),
            path: at.to_path_buf(),
        });
    }
    if at.exists() {
        return Err(UzeError::MarketplaceScaffold(format!(
            "`{}` already exists — `--at` must name an absent or empty directory",
            at.display()
        )));
    }
    fs::create_dir_all(at.join(PLUGINS_DIRECTORY)).map_err(|source| UzeError::Write {
        path: at.to_path_buf(),
        source,
    })?;
    let manifest = serde_json::json!({
        "name": name,
        "description": description.unwrap_or("A marketplace of agent plugins."),
        "plugins": [],
    });
    let manifest_path = at.join(MARKETPLACE_MANIFEST);
    let manifest_body =
        serde_json::to_string_pretty(&manifest).expect("a scaffolded manifest serializes");
    fs::write(&manifest_path, format!("{manifest_body}\n")).map_err(|source| UzeError::Write {
        path: manifest_path.clone(),
        source,
    })?;
    let readme = at.join("README.md");
    fs::write(&readme, include_str!("authoring/marketplace-readme.md")).map_err(|source| {
        UzeError::Write {
            path: readme.clone(),
            source,
        }
    })?;
    write_initial_commit(at)?;
    Ok(at.to_path_buf())
}

/// The initial commit, through `uze-git` like every other Git write.
fn write_initial_commit(at: &Path) -> Result<()> {
    if let Some(setting) = missing_git_identity(at) {
        return Err(UzeError::MarketplaceScaffold(format!(
            "Git has no identity configured — set one before scaffolding:\n  git config --global \
             user.name \"Your Name\"\n  git config --global user.email {setting}"
        )));
    }
    let init = match uze_git::write(at, &["init", "-q", "-b", "main"]) {
        Ok(output) => output,
        Err(error) => {
            return Err(UzeError::MarketplaceScaffold(format!(
                "`git init` failed: {error}"
            )));
        }
    };
    if !init.is_success() {
        return Err(UzeError::MarketplaceScaffold(format!(
            "`git init` failed: {}",
            init.successful().err().unwrap_or_default()
        )));
    }
    let commits: [&[&str]; 2] = [
        &["add", "-A"],
        &["commit", "-q", "-m", INITIAL_COMMIT_MESSAGE],
    ];
    for arguments in commits {
        let output = match uze_git::write(at, arguments) {
            Ok(output) => output,
            Err(error) => {
                return Err(UzeError::MarketplaceScaffold(format!(
                    "`git {}` failed: {error}",
                    arguments[0]
                )));
            }
        };
        if !output.is_success() {
            return Err(UzeError::MarketplaceScaffold(format!(
                "`git {}` failed: {}",
                arguments[0],
                output.successful().err().unwrap_or_default()
            )));
        }
    }
    Ok(())
}

/// The `user.email` the machine's own Git would commit with — what a
/// commit itself reads, not what a process environment happens to carry.
fn missing_git_identity(at: &Path) -> Option<&'static str> {
    let configured = |key: &str| {
        uze_git::read(at, &["config", "--get", key])
            .ok()
            .and_then(|output| output.successful().ok())
            .map(|stdout| stdout.trim().to_owned())
            .is_some_and(|value| !value.is_empty())
    };
    if !configured("user.email") {
        Some("you@example.invalid")
    } else {
        None
    }
}

/// Scaffolds one plugin inside a marketplace's checkout — `plugin.json`, a
/// skill carrying the canonical `invoke:` policy block, the optional
/// capability files for each flag — and adds the `plugins[]` entry that
/// makes it installable. Refuses to overwrite an existing plugin.
pub fn scaffold_plugin(
    market_root: &Path,
    name: &str,
    description: Option<&str>,
    caps: &ScaffoldCapabilities,
) -> Result<PathBuf> {
    if !store::is_valid_package_name(name) {
        return Err(UzeError::InvalidPackageName {
            name: name.to_owned(),
            path: market_root.to_path_buf(),
        });
    }
    if !market_root.join(MARKETPLACE_MANIFEST).is_file() {
        return Err(UzeError::MarketplaceScaffold(format!(
            "`{}` does not name a marketplace — no {MARKETPLACE_MANIFEST} is there",
            market_root.display()
        )));
    }
    let plugin_root = market_root.join(PLUGINS_DIRECTORY).join(name);
    if plugin_root.exists() {
        return Err(UzeError::MarketplaceScaffold(format!(
            "`{}` already exists — scaffolding never overwrites",
            plugin_root.display()
        )));
    }
    fs::create_dir_all(plugin_root.join("skills").join(name)).map_err(|source| {
        UzeError::Write {
            path: plugin_root.clone(),
            source,
        }
    })?;

    let manifest = serde_json::json!({
        "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
        "name": name,
        "description": description.unwrap_or("What this plugin offers."),
    });
    let plugin_json = plugin_root.join("plugin.json");
    let manifest_body =
        serde_json::to_string_pretty(&manifest).expect("a scaffolded manifest serializes");
    fs::write(&plugin_json, format!("{manifest_body}\n")).map_err(|source| UzeError::Write {
        path: plugin_json.clone(),
        source,
    })?;

    let skill = plugin_root.join("skills").join(name).join("SKILL.md");
    fs::write(
        &skill,
        include_str!("authoring/SKILL.md.template")
            .replace("{SKILL_NAME}", name)
            .replace(
                "{SKILL_DESCRIPTION}",
                description.unwrap_or("What this skill does. The long text is what the model matches an invocation against."),
            ),
    )
    .map_err(|source| UzeError::Write { path: skill.clone(), source })?;

    let hook = caps.hook;
    if hook {
        let hooks = plugin_root.join("hooks.json");
        fs::write(&hooks, include_str!("authoring/hooks.json")).map_err(|source| {
            UzeError::Write {
                path: hooks.clone(),
                source,
            }
        })?;
        let scripts = plugin_root.join("scripts");
        fs::create_dir_all(&scripts).map_err(|source| UzeError::Write {
            path: plugin_root.clone(),
            source,
        })?;
        let guard = scripts.join("guard");
        fs::write(&guard, include_str!("authoring/guard.sh")).map_err(|source| {
            UzeError::Write {
                path: guard.clone(),
                source,
            }
        })?;
        // A hook command the harness cannot run is the 127 that fails the
        // group silently; the stub ships runnable.
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&guard, fs::Permissions::from_mode(0o755)).map_err(|source| {
            UzeError::Write {
                path: guard.clone(),
                source,
            }
        })?;
    }
    if caps.mcp {
        let servers = plugin_root.join("mcp.json");
        fs::write(&servers, include_str!("authoring/mcp.json")).map_err(|source| {
            UzeError::Write {
                path: servers.clone(),
                source,
            }
        })?;
        // The stub the manifest's example key runs: the reference is real
        // from the moment the scaffold exists, and check stays honest.
        let server = plugin_root.join("scripts").join("example_server.py");
        fs::write(&server, include_str!("authoring/example_server.py")).map_err(|source| {
            UzeError::Write {
                path: server.clone(),
                source,
            }
        })?;
    }
    if caps.agent {
        let file = plugin_root.join("agents").join(format!("{name}.md"));
        fs::create_dir_all(file.parent().expect("the agents directory")).map_err(|source| {
            UzeError::Write {
                path: plugin_root.clone(),
                source,
            }
        })?;
        fs::write(&file, include_str!("authoring/agent.md.template")).map_err(|source| {
            UzeError::Write {
                path: file.clone(),
                source,
            }
        })?;
    }
    if caps.instructions {
        let file = plugin_root.join("AGENTS.md");
        fs::write(&file, include_str!("authoring/instructions.md")).map_err(|source| {
            UzeError::Write {
                path: file.clone(),
                source,
            }
        })?;
    }

    add_marketplace_entry(market_root, name, description)?;
    Ok(plugin_root)
}

/// Adds the `plugins[]` entry the scaffolded plugin is resolved through. An
/// entry for a name the manifest already carries is a conflict, never a
/// duplicate.
fn add_marketplace_entry(market_root: &Path, name: &str, description: Option<&str>) -> Result<()> {
    let manifest_path = market_root.join(MARKETPLACE_MANIFEST);
    let bytes = fs::read(&manifest_path).map_err(|source| UzeError::Read {
        path: manifest_path.clone(),
        source,
    })?;
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|source| UzeError::Json {
            path: manifest_path.clone(),
            source,
        })?;
    let plugins = manifest
        .as_object_mut()
        .expect("a marketplace manifest is an object")
        .entry("plugins")
        .or_insert_with(|| serde_json::json!([]));
    let plugins = plugins.as_array_mut().expect("`plugins` is a list");
    if plugins
        .iter()
        .any(|entry| entry.get("name").and_then(serde_json::Value::as_str) == Some(name))
    {
        return Err(UzeError::MarketplaceScaffold(format!(
            "the marketplace already names `{name}` — scaffolding never overwrites"
        )));
    }
    plugins.push(serde_json::json!({
        "name": name,
        "source": format!("./{PLUGINS_DIRECTORY}/{name}"),
        "description": description.unwrap_or("What this plugin offers."),
    }));
    let body = serde_json::to_string_pretty(&manifest).expect("a manifest serializes");
    fs::write(&manifest_path, format!("{body}\n")).map_err(|source| UzeError::Write {
        path: manifest_path.clone(),
        source,
    })
}

/// What `check` found. Empty `findings` is a clean artifact; every finding
/// names where it is and why, in the same words the install-time parser
/// used, so check and install cannot disagree about what a file said.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ValidationReport {
    /// What the artifact would deliver: one identity per capability — the
    /// engine's own answer about the bytes.
    pub delivers: Vec<String>,
    /// Where and why, one per finding. Empty is clean.
    pub findings: Vec<String>,
}

impl ValidationReport {
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }
}

/// Validates an authored plugin directory offline — the same parsers the
/// install would run, before any install.
pub fn check_plugin(root: &Path) -> Result<ValidationReport> {
    let mut findings = Vec::new();
    let manifest = match store::read_plugin_manifest(root) {
        Ok(manifest) => manifest,
        Err(error) => {
            findings.push(error.to_string());
            return Ok(ValidationReport {
                delivers: Vec::new(),
                findings,
            });
        }
    };
    let id = match PackageId::from_plugin_name(&manifest.name, &root.join("plugin.json")) {
        Ok(id) => id,
        Err(error) => {
            findings.push(error.to_string());
            return Ok(ValidationReport {
                delivers: Vec::new(),
                findings,
            });
        }
    };
    let mut delivers = Vec::new();
    match crate::engine::package_resources_at(&id, root) {
        Ok(resources) => {
            for resource in &resources {
                // A skill present but carrying a broken `invoke:` block is
                // delivered today as the default policy; a check says so
                // out loud instead of letting the author find it in the TUI.
                if resource.capability.kind == CapabilityKind::AgentSkill
                    && let Some(policy) =
                        crate::skill::parse_skill_invocation(&resource.capability.payload)
                    && policy.is_invalid()
                {
                    findings.push(format!(
                        "{}: `invoke:` block is malformed — the delivery treats it as the \
                         default policy",
                        resource.capability.path.display()
                    ));
                }
                delivers.push(resource.identity());
            }
        }
        Err(error) => findings.push(error.to_string()),
    }
    Ok(ValidationReport { delivers, findings })
}

/// Validates an authored marketplace: the manifest itself, every entry's
/// source resolving inside the marketplace, and each resolved plugin's own
/// check.
pub fn check_marketplace(root: &Path) -> Result<ValidationReport> {
    let mut findings = Vec::new();
    let mut delivers = Vec::new();
    let manifest_path = root.join(MARKETPLACE_MANIFEST);
    if !manifest_path.is_file() {
        findings.push(format!(
            "{MARKETPLACE_MANIFEST} is missing — this directory is not a marketplace"
        ));
        return Ok(ValidationReport { delivers, findings });
    }
    let bytes = fs::read(&manifest_path).map_err(|source| UzeError::Read {
        path: manifest_path.clone(),
        source,
    })?;
    let manifest = match marketplace::parse_manifest(&bytes) {
        Ok(manifest) => manifest,
        Err(error) => {
            findings.push(error.to_string());
            return Ok(ValidationReport { delivers, findings });
        }
    };
    for entry in &manifest.plugins {
        match marketplace::resolve_plugin_source(&manifest, &entry.name, root) {
            Ok(resolved) => {
                delivers.push(entry.name.clone());
                let plugin = check_plugin(&resolved)?;
                findings.extend(
                    plugin
                        .findings
                        .into_iter()
                        .map(|finding| format!("{}: {finding}", entry.name)),
                );
            }
            Err(error) => findings.push(format!("{}: {error}", entry.name)),
        }
    }
    Ok(ValidationReport { delivers, findings })
}

#[cfg(test)]
mod tests;
