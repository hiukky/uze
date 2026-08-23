// ADR-005: the Core Engine composes peer-harness inputs without named harness rules.
use crate::{
    capability::{Capability, CapabilityKind, Representation},
    error::{Result, UzeError},
    project::{EffectiveEnvironment, Resource, resolve_project_resources},
    store::{PackageId, UzeStore},
};

/// Composes the effective environment owned by the user: project resources
/// remain project-owned and UZE-installed packages remain store-owned.
#[derive(Clone, Debug)]
pub struct UzeEngine {
    store: UzeStore,
}

impl UzeEngine {
    pub fn new(store: UzeStore) -> Self {
        Self { store }
    }

    pub fn store(&self) -> &UzeStore {
        &self.store
    }

    /// Compose every locally installed package with the supplied project's
    /// portable resources. This is the product path used by the CLI.
    pub fn compose_project(
        &self,
        project_root: impl AsRef<std::path::Path>,
    ) -> Result<EffectiveEnvironment> {
        let project = resolve_project_resources(project_root)?;
        let mut resources = project.resources;
        resources.extend(self.package_resources(&self.store.package_ids()?)?);
        resources.sort_by_key(|resource| resource.identity());
        Ok(EffectiveEnvironment {
            root: project.root,
            resources,
        })
    }

    /// Package-only composition remains available for isolated library and
    /// conformance tests. It is not a separate product concept: callers that
    /// have a project should use `compose_project`.
    pub fn compose(&self, packages: &[PackageId]) -> Result<EffectiveEnvironment> {
        let resources = self.package_resources(packages)?;
        Ok(EffectiveEnvironment {
            root: self.store.home().root().to_path_buf(),
            resources,
        })
    }

    fn package_resources(&self, packages: &[PackageId]) -> Result<Vec<Resource>> {
        let mut resources = Vec::new();
        for id in packages {
            let package = self.store.package(id)?;
            resources.extend(package_resources_at(&package.id, &package.root)?);
        }
        resources.sort_by_key(|resource| resource.identity());
        Ok(resources)
    }
}

/// Discovers a package's capabilities from a directory on disk.
///
/// Shared with acquisition, which needs the same reading *before* a package
/// is installed in order to decide trust. Deliberately the same code path, so
/// what an operator authorizes cannot drift from what the Engine later
/// composes.
pub fn package_resources_at(id: &PackageId, root: &std::path::Path) -> Result<Vec<Resource>> {
    let mut resources = Vec::new();
    let skills_root = root.join("skills");
    if skills_root.is_dir() {
        for path in crate::project::files_named(&skills_root, "SKILL.md")? {
            let payload = crate::project::read_file(&path)?;
            resources.push(Resource::from_package(
                id.clone(),
                root.to_path_buf(),
                Capability {
                    kind: CapabilityKind::AgentSkill,
                    representation: Representation::Standard,
                    path,
                    payload,
                },
            ));
        }
    }
    resources.extend(command_resources(id, root)?);
    resources.extend(instruction_resources(id, root)?);
    resources.extend(mcp_resources(id, root)?);
    resources.sort_by_key(|resource| resource.identity());
    Ok(resources)
}

/// Discovers a package's canonical Commands — flat `.md` files directly
/// under the package's conventional `commands/` directory. One `Resource`
/// per file; the file stem is the command's logical name (`review.md` →
/// `review`), mirroring the naming convention every harness examined
/// (ADR-025) derives the slash-command name from.
///
/// Deliberately flat and non-recursive: nested directories are a
/// vendor-naming concern (OpenCode `team/review`, Gemini `/gcp:sync`) that
/// an integration may resolve at delivery; the canonical v0 model is one
/// file = one command. Discovery is also symlink-safe by construction —
/// symlinked entries are skipped, not followed, matching
/// `files_named`'s rule. The canonical file bytes are the payload:
/// **nothing is parsed, frontmatter or otherwise, at discovery time**, so
/// a Skill and a Command are never conflated here or anywhere downstream.
fn command_resources(id: &PackageId, package_root: &std::path::Path) -> Result<Vec<Resource>> {
    let mut resources = Vec::new();
    for path in command_files(package_root)? {
        let payload = crate::project::read_file(&path)?;
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .expect("command_files validates a utf-8 stem")
            .to_owned();
        resources.push(Resource::from_package_named(
            id.clone(),
            package_root.to_path_buf(),
            Capability {
                kind: CapabilityKind::Command,
                representation: Representation::Standard,
                path,
                payload,
            },
            name,
        ));
    }
    Ok(resources)
}

/// The canonical command files of a package — the one shared authority for
/// "which files are canonical Commands", used by Engine discovery and by
/// every integration that must translate or reference them identically. See
/// [`command_resources`] for the rules.
pub fn command_files(package_root: &std::path::Path) -> Result<Vec<std::path::PathBuf>> {
    let commands_root = package_root.join("commands");
    if !commands_root.is_dir() {
        return Ok(Vec::new());
    }
    let mut entries = std::fs::read_dir(&commands_root)
        .map_err(|source| crate::UzeError::Read {
            path: commands_root.clone(),
            source,
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|source| crate::UzeError::Read {
            path: commands_root.clone(),
            source,
        })?;
    entries.sort_by_key(|entry| entry.file_name());
    let mut files = Vec::new();
    for entry in entries {
        let path = entry.path();
        let metadata =
            std::fs::symlink_metadata(&path).map_err(|source| crate::UzeError::Read {
                path: path.clone(),
                source,
            })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if !stem
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        files.push(path);
    }
    Ok(files)
}

/// Discovers a package's optional root-level `AGENTS.md` — the same
/// standard convention `resolve_project` already recognizes at project
/// scope (see `project::discover_instructions`), read here at package scope
/// instead. A package does not ship a whole project instructions file; it
/// ships the portable content a project's own `AGENTS.md` later composes,
/// one delimited region per contributing package.
fn instruction_resources(id: &PackageId, package_root: &std::path::Path) -> Result<Vec<Resource>> {
    let path = package_root.join("AGENTS.md");
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let payload = crate::project::read_file(&path)?;
    Ok(vec![Resource::from_package(
        id.clone(),
        package_root.to_path_buf(),
        Capability {
            kind: CapabilityKind::Instruction,
            representation: Representation::Standard,
            path,
            payload,
        },
    )])
}

/// Discovers a package's optional root-level `mcp.json` (Agent Plugins 1.0
/// shape: `{"mcpServers": {"<name>": {"command", "args", ...}}}`) into one
/// `Resource` per declared server. A package declaring more than one server
/// produces distinct named resources while preserving the original
/// `mcp.json` bytes only once in the Store.
///
/// This module reads the standard, never a harness. Which harnesses already
/// consume that shape is evidence recorded in ADR-007, not a fact the Engine
/// needs or holds.
fn mcp_resources(id: &PackageId, package_root: &std::path::Path) -> Result<Vec<Resource>> {
    let manifest_path = package_root.join("mcp.json");
    if !manifest_path.is_file() {
        return Ok(Vec::new());
    }
    let payload = crate::project::read_file(&manifest_path)?;
    let manifest: serde_json::Value =
        serde_json::from_slice(&payload).map_err(|source| UzeError::Json {
            path: manifest_path.clone(),
            source,
        })?;
    let servers = manifest
        .get("mcpServers")
        .and_then(serde_json::Value::as_object);
    let Some(servers) = servers else {
        return Ok(Vec::new());
    };
    let mut entries: Vec<(&String, &serde_json::Value)> = servers.iter().collect();
    entries.sort_by_key(|(name, _)| name.as_str());
    entries
        .into_iter()
        .map(|(name, config)| {
            let payload = serde_json::to_vec(config)
                .expect("mcp server config re-serialization is infallible");
            Ok(Resource::from_package_named(
                id.clone(),
                package_root.to_path_buf(),
                Capability {
                    kind: CapabilityKind::Mcp,
                    representation: Representation::Standard,
                    path: manifest_path.clone(),
                    payload,
                },
                name.to_owned(),
            ))
        })
        .collect()
}

#[cfg(test)]
mod command_discovery_tests {
    use super::*;
    use std::fs;

    fn temp(label: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "uze-command-files-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn discovers_only_flat_valid_markdown_commands_sorted() {
        let root = temp("discover");
        let pkg = root.join("pkg");
        fs::create_dir_all(pkg.join("commands/nested")).unwrap();
        fs::write(pkg.join("commands/review.md"), "a").unwrap();
        fs::write(pkg.join("commands/commit.md"), "b").unwrap();
        fs::write(pkg.join("commands/not-valid name.md"), "c").unwrap();
        fs::write(pkg.join("commands/readme.txt"), "d").unwrap();
        fs::write(pkg.join("commands/nested/deep.md"), "e").unwrap();
        let files = command_files(&pkg).unwrap();
        let names: Vec<String> = files
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["commit.md".to_owned(), "review.md".to_owned()]);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_command_entries_are_never_followed() {
        use std::os::unix::fs::symlink;
        let root = temp("symlink");
        let pkg = root.join("pkg");
        fs::create_dir_all(pkg.join("commands")).unwrap();
        let outside = root.join("outside.md");
        fs::write(&outside, "x").unwrap();
        symlink(&outside, pkg.join("commands/evil.md")).unwrap();
        let files = command_files(&pkg).unwrap();
        assert!(
            files.is_empty(),
            "a symlink into a command file is not a canonical command"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn absent_commands_directory_is_empty() {
        let root = temp("absent");
        let pkg = root.join("pkg");
        fs::create_dir_all(&pkg).unwrap();
        assert!(command_files(&pkg).unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }
}
