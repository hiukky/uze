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
use std::path::{Component, Path, PathBuf};

use noyalib::{ParserConfig, compat::serde_yaml, from_str_with_config};

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

/// The `marketplace.json` key a marketplace records a plugins directory
/// other than the conventional one under — the only place a born-empty
/// manifest can carry that choice to the plugin created after it.
const PLUGINS_DIRECTORY_KEY: &str = "pluginsDirectory";

/// The token a plugin's own files are referenced through from `hooks.json`
/// and `mcp.json`; every delivery resolves it to the installed root.
const PLUGIN_ROOT_TOKEN: &str = "${PLUGIN_ROOT}";

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
    create_dir(&plugins)?;
    let mut manifest = serde_json::json!({
        "name": name,
        "description": description.unwrap_or("This project's own plugins."),
        "plugins": [],
    });
    if plugins_dir != PLUGINS_DIRECTORY {
        manifest[PLUGINS_DIRECTORY_KEY] = serde_json::json!(plugins_dir);
    }
    write_json(&manifest_path, &manifest)?;
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
    if !is_absent_or_empty_directory(at) {
        return Err(UzeError::MarketplaceScaffold(format!(
            "`{}` already holds something — `--at` must name an absent or empty directory",
            at.display()
        )));
    }
    require_git_identity(at)?;
    create_dir(&at.join(PLUGINS_DIRECTORY))?;
    let manifest = serde_json::json!({
        "name": name,
        "description": description.unwrap_or("A marketplace of agent plugins."),
        "plugins": [],
    });
    write_json(&at.join(MARKETPLACE_MANIFEST), &manifest)?;
    write_file(
        &at.join("README.md"),
        include_str!("authoring/marketplace-readme.md"),
    )?;
    write_initial_commit(at)?;
    Ok(at.to_path_buf())
}

fn is_absent_or_empty_directory(at: &Path) -> bool {
    match fs::read_dir(at) {
        Ok(mut entries) => entries.next().is_none(),
        Err(_) => !at.exists(),
    }
}

/// Asked before the first byte is written: a scaffold that stopped at the
/// commit would leave a directory the next attempt refuses as occupied.
fn require_git_identity(at: &Path) -> Result<()> {
    let asked_from = at
        .ancestors()
        .find(|directory| directory.is_dir())
        .unwrap_or(at);
    if let Some(setting) = missing_git_identity(asked_from) {
        return Err(UzeError::MarketplaceScaffold(format!(
            "Git has no identity configured — set one before scaffolding:\n  git config --global \
             user.name \"Your Name\"\n  git config --global user.email {setting}"
        )));
    }
    Ok(())
}

/// The initial commit, through `uze-git` like every other Git write.
fn write_initial_commit(at: &Path) -> Result<()> {
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
    let manifest_path = market_root.join(MARKETPLACE_MANIFEST);
    if !manifest_path.is_file() {
        return Err(UzeError::MarketplaceScaffold(format!(
            "`{}` does not name a marketplace — no {MARKETPLACE_MANIFEST} is there",
            market_root.display()
        )));
    }
    let mut manifest = read_json(&manifest_path)?;
    if manifest_names_plugin(&manifest, name) {
        return Err(UzeError::MarketplaceScaffold(format!(
            "the marketplace already names `{name}` — scaffolding never overwrites"
        )));
    }
    let plugins_directory = plugins_directory(&manifest);
    let plugin_root = market_root.join(&plugins_directory).join(name);
    if plugin_root.exists() {
        return Err(UzeError::MarketplaceScaffold(format!(
            "`{}` already exists — scaffolding never overwrites",
            plugin_root.display()
        )));
    }

    write_plugin_files(&plugin_root, name, description, caps)?;

    manifest
        .as_object_mut()
        .expect("a marketplace manifest is an object")
        .entry("plugins")
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
        .expect("`plugins` is a list")
        .push(serde_json::json!({
            "name": name,
            "source": format!("./{plugins_directory}/{name}"),
            "description": description.unwrap_or("What this plugin offers."),
        }));
    write_json(&manifest_path, &manifest)?;
    Ok(plugin_root)
}

fn write_plugin_files(
    plugin_root: &Path,
    name: &str,
    description: Option<&str>,
    caps: &ScaffoldCapabilities,
) -> Result<()> {
    let skill_directory = plugin_root.join("skills").join(name);
    create_dir(&skill_directory)?;
    write_json(
        &plugin_root.join("plugin.json"),
        &serde_json::json!({
            "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
            "name": name,
            "description": description.unwrap_or("What this plugin offers."),
        }),
    )?;
    write_file(
        &skill_directory.join("SKILL.md"),
        include_str!("authoring/SKILL.md.template")
            .replace("{SKILL_NAME}", name)
            .replace(
                "{SKILL_DESCRIPTION}",
                &yaml_double_quoted(description.unwrap_or(
                    "What this skill does. The long text is what the model matches an \
                     invocation against.",
                )),
            ),
    )?;

    if caps.hook {
        write_file(
            &plugin_root.join("hooks.json"),
            include_str!("authoring/hooks.json"),
        )?;
        let scripts = plugin_root.join("scripts");
        create_dir(&scripts)?;
        let guard = scripts.join("guard");
        write_file(&guard, include_str!("authoring/guard.sh"))?;
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
        write_file(
            &plugin_root.join("mcp.json"),
            include_str!("authoring/mcp.json"),
        )?;
        // The stub the manifest's example key runs: the reference is real
        // from the moment the scaffold exists, and check stays honest.
        let scripts = plugin_root.join("scripts");
        create_dir(&scripts)?;
        write_file(
            &scripts.join("example_server.py"),
            include_str!("authoring/example_server.py"),
        )?;
    }
    if caps.agent {
        let agents = plugin_root.join("agents");
        create_dir(&agents)?;
        write_file(
            &agents.join(format!("{name}.md")),
            include_str!("authoring/agent.md.template"),
        )?;
    }
    if caps.instructions {
        write_file(
            &plugin_root.join("AGENTS.md"),
            include_str!("authoring/instructions.md"),
        )?;
    }
    Ok(())
}

/// A description as a YAML double-quoted scalar. JSON's string escaping is
/// a subset of YAML's double-quoted style, so `: `, `#` and quotes in the
/// author's text cannot end the value early or start a comment.
fn yaml_double_quoted(text: &str) -> String {
    serde_json::to_string(text).expect("a string serializes")
}

fn manifest_names_plugin(manifest: &serde_json::Value, name: &str) -> bool {
    manifest
        .get("plugins")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|plugins| {
            plugins
                .iter()
                .any(|entry| entry.get("name").and_then(serde_json::Value::as_str) == Some(name))
        })
}

/// Where the next plugin goes: what the marketplace recorded when it was
/// scaffolded, else the directory its existing entries already live in,
/// else the convention. A name that would not be a valid directory of the
/// marketplace's own is ignored rather than followed out of it.
fn plugins_directory(manifest: &serde_json::Value) -> String {
    let recorded = manifest
        .get(PLUGINS_DIRECTORY_KEY)
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let inhabited = || {
        manifest
            .get("plugins")
            .and_then(serde_json::Value::as_array)?
            .iter()
            .filter_map(|entry| entry.get("source").and_then(serde_json::Value::as_str))
            .find_map(|source| {
                let parts: Vec<_> = Path::new(source)
                    .components()
                    .filter(|component| *component != Component::CurDir)
                    .collect();
                match parts.as_slice() {
                    [Component::Normal(directory), Component::Normal(_)] => {
                        directory.to_str().map(str::to_owned)
                    }
                    _ => None,
                }
            })
    };
    recorded
        .or_else(inhabited)
        .filter(|directory| store::is_valid_package_name(directory))
        .unwrap_or_else(|| PLUGINS_DIRECTORY.to_owned())
}

fn read_json(path: &Path) -> Result<serde_json::Value> {
    let bytes = fs::read(path).map_err(|source| UzeError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| UzeError::Json {
        path: path.to_path_buf(),
        source,
    })
}

fn write_json(path: &Path, value: &serde_json::Value) -> Result<()> {
    let body = serde_json::to_string_pretty(value).expect("a JSON value serializes");
    write_file(path, format!("{body}\n"))
}

fn write_file(path: &Path, contents: impl AsRef<[u8]>) -> Result<()> {
    fs::write(path, contents).map_err(|source| UzeError::Write {
        path: path.to_path_buf(),
        source,
    })
}

fn create_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path).map_err(|source| UzeError::Write {
        path: path.to_path_buf(),
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
                if resource.capability.kind == CapabilityKind::AgentSkill {
                    let path = resource.capability.path.display();
                    if let Some(policy) =
                        crate::skill::parse_skill_invocation(&resource.capability.payload)
                        && policy.is_invalid()
                    {
                        findings.push(format!(
                            "{path}: `invoke:` block is malformed — the delivery treats it as \
                             the default policy"
                        ));
                    }
                    if let Some(reason) = skill_frontmatter_fault(&resource.capability.payload) {
                        findings.push(format!("{path}: {reason}"));
                    }
                }
                delivers.push(resource.identity());
            }
        }
        Err(error) => findings.push(error.to_string()),
    }
    findings.extend(escaping_references(root));
    Ok(ValidationReport { delivers, findings })
}

/// What a harness reading this `SKILL.md` would trip over in its
/// frontmatter. The install-side reader is deliberately lenient — it
/// extracts only the `invoke:` booleans and keeps the bytes verbatim — so
/// this is the one place the frontmatter is read as the YAML a harness
/// parses it as.
fn skill_frontmatter_fault(payload: &[u8]) -> Option<String> {
    let Ok(text) = std::str::from_utf8(payload) else {
        return Some("is not UTF-8".to_owned());
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let head = text
        .strip_prefix("---\n")
        .and_then(|rest| rest.find("\n---\n").map(|end| &rest[..end]));
    let Some(head) = head else {
        return Some(
            "has no frontmatter — it opens with a `---` line, carries `description:`, and \
             closes with another `---` line"
                .to_owned(),
        );
    };
    let frontmatter: serde_yaml::Value =
        match from_str_with_config(head, &ParserConfig::serde_yaml_compat()) {
            Ok(frontmatter) => frontmatter,
            Err(error) => {
                return Some(format!(
                    "frontmatter is not valid YAML ({error}) — quote a value that carries `: ` \
                     or starts with a special character"
                ));
            }
        };
    let described = frontmatter
        .get("description")
        .and_then(serde_yaml::Value::as_str)
        .is_some_and(|description| !description.trim().is_empty());
    (!described).then(|| {
        "frontmatter has no `description` — it is what the model matches an invocation against"
            .to_owned()
    })
}

/// Every command a `hooks.json` handler runs and every `command`/`args`
/// entry an `mcp.json` server starts with, when it names a path outside
/// the plugin: the installed copy is somewhere else, so a `..` or an
/// absolute path reaches something the plugin does not carry.
fn escaping_references(root: &Path) -> Vec<String> {
    let mut findings = Vec::new();
    let mut report = |file: &str, words: Vec<&str>| {
        for word in words {
            if let Some(token) = word
                .split_whitespace()
                .map(|token| token.trim_matches(|c| c == '"' || c == '\''))
                .find(|token| reaches_outside_the_plugin(token))
            {
                findings.push(format!(
                    "{}: `{token}` reaches outside the plugin — name its own files as \
                     `{PLUGIN_ROOT_TOKEN}/…`, and anything else through PATH",
                    root.join(file).display()
                ));
            }
        }
    };
    if let Ok(hooks) = read_json(&root.join(crate::hook::HOOKS_FILE_NAME)) {
        let commands = hooks
            .get("hooks")
            .and_then(serde_json::Value::as_object)
            .into_iter()
            .flat_map(|events| events.values())
            .filter_map(serde_json::Value::as_array)
            .flatten()
            .filter_map(|group| group.get("hooks").and_then(serde_json::Value::as_array))
            .flatten()
            .filter_map(|handler| handler.get("command").and_then(serde_json::Value::as_str))
            .collect();
        report(crate::hook::HOOKS_FILE_NAME, commands);
    }
    if let Ok(servers) = read_json(&root.join("mcp.json")) {
        let launched = servers
            .get("mcpServers")
            .and_then(serde_json::Value::as_object)
            .into_iter()
            .flat_map(|servers| servers.values())
            .flat_map(|server| {
                let command = server.get("command").into_iter();
                let args = server
                    .get("args")
                    .and_then(serde_json::Value::as_array)
                    .into_iter()
                    .flatten();
                command.chain(args)
            })
            .filter_map(serde_json::Value::as_str)
            .collect();
        report("mcp.json", launched);
    }
    findings
}

fn reaches_outside_the_plugin(token: &str) -> bool {
    let (inside_the_plugin, path) = match token.strip_prefix(PLUGIN_ROOT_TOKEN) {
        Some(rest) => (true, rest.trim_start_matches('/')),
        None => (false, token),
    };
    let path = Path::new(path);
    (!inside_the_plugin && path.is_absolute())
        || path
            .components()
            .any(|component| component == Component::ParentDir)
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
