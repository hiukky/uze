//! Test-only orchestration for the UZE Harness Conformance Lab.
//!
//! The primitives below know nothing beyond process facts: [`run`] starts a
//! selected real executable under a caller-supplied disposable environment
//! and reports how it exited. Knowledge of specific harness CLIs is confined
//! to [`harness`], which is a declarative table, and the scenario logic in
//! [`scenario`] is generic over it. Nothing here knows Docker internals, and
//! no product-domain type ever references this crate.
//!
//! The Lab consumes the **single canonical fixture source** in
//! `tests/_fixtures` through `uze-testkit` (see [`compose_lab_package`]);
//! it never maintains a second canonical tree.

pub mod evidence;
pub mod harness;
pub mod scenario;

use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// Per-run proof values. The MCP server receives its proof through process
/// arguments (the one channel every delivery route persists intact). The
/// Skill proof is the canonical fixture's own token, replaced per run so
/// evidence is never attributable to stale bytes (see `compose_lab_package`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DynamicProof {
    pub skill: String,
    pub mcp: String,
}

impl DynamicProof {
    pub fn from_nonce(nonce: &str) -> Self {
        Self {
            skill: format!("UZE_CONFORMANCE_SKILL_{nonce}"),
            mcp: format!("UZE_CONFORMANCE_MCP_{nonce}"),
        }
    }
}

/// Which capabilities a materialized fixture keeps.
///
/// The skill and MCP behavioral probes are installed separately, and that is
/// deliberate. When both live in one installation the model is offered an MCP
/// tool whose whole documented purpose is "return the conformance proof
/// value" at the same moment it is asked to make a skill return a proof
/// token, and it routinely answers the skill prompt with the MCP tool's
/// value. Isolating the capability makes attribution structural: if the MCP
/// server is not installed at all, a skill proof in the output can only have
/// come from the skill. It replaces per-harness prompt patches that tried to
/// talk the model out of the ambiguity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixtureVariant {
    /// Both capabilities. Realistic, but never used to attribute a proof.
    Full,
    SkillOnly,
    McpOnly,
}

impl FixtureVariant {
    fn keeps_skill(self) -> bool {
        matches!(self, Self::Full | Self::SkillOnly)
    }

    fn keeps_mcp(self) -> bool {
        matches!(self, Self::Full | Self::McpOnly)
    }
}

/// How one disposable copy of the fixture is produced.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixtureSpec {
    pub variant: FixtureVariant,
    /// Absolute path of the built MCP fixture server, substituted for the
    /// manifest placeholder so the manifest never carries a build path.
    pub mcp_binary: PathBuf,
    pub proof: DynamicProof,
}

const MCP_MANIFESTS: [&str; 2] = ["mcp.json", ".mcp.json"];
const CODEX_ENVELOPE: &str = "plugin.json";

/// The canonical skill fixture's static proof token; `compose_lab_package`
/// replaces it with the materialization placeholder so each run's value is
/// per-run, never stale.
const CANONICAL_SKILL_PROOF_TOKEN: &str = "UZE_CONFORMANCE_SKILL_PROOF_20260820";

/// Builds the Lab's canonical multi-capability package **from the single
/// fixture source** (`tests/_fixtures`, via `uze-testkit`):
///
/// - canonical `skill-plugin` (the Skill, canonical bytes);
/// - canonical `mcp-plugin`'s manifests (the MCP server, placeholder intact);
/// - the foreign Codex-native envelope (vendor-native format).
///
/// The skill proof token is replaced with the materialization placeholder so
/// `materialize_fixture` can substitute a per-run value. If the canonical
/// token ever changes, this fails loudly instead of passing on stale bytes.
pub fn compose_lab_package(destination: &Path) -> Result<(), HarnessRunError> {
    let skill_source = uze_testkit::fixtures::canonical("skill-plugin");
    let mcp_source = uze_testkit::fixtures::canonical("mcp-plugin");
    let envelope_source = uze_testkit::fixtures::foreign("codex", "native-plugin");

    if destination.exists() {
        return Err(HarnessRunError::Materialize(format!(
            "composed package destination already exists: {}",
            destination.display()
        )));
    }
    fs::create_dir_all(destination).map_err(materialize_error)?;

    copy_tree(&skill_source, destination).map_err(materialize_error)?;
    copy_tree(&mcp_source, destination).map_err(materialize_error)?;
    // The Codex-native envelope references `./.mcp.json`; the canonical MCP
    // plugin ships `mcp.json` only, so the Lab composes the same manifest
    // under both names (vendor-format duplication, not fixture duplication).
    let mcp_bytes = fs::read(destination.join("mcp.json")).map_err(materialize_error)?;
    fs::write(destination.join(".mcp.json"), mcp_bytes).map_err(materialize_error)?;

    let envelope_dir = destination.join(".codex-plugin");
    fs::create_dir_all(&envelope_dir).map_err(materialize_error)?;
    fs::copy(
        envelope_source.join(".codex-plugin/plugin.json"),
        envelope_dir.join("plugin.json"),
    )
    .map_err(materialize_error)?;

    // The canonical skill body carries the static proof token; a per-run
    // value is substituted by `materialize_fixture`.
    let skill_md = destination.join("skills/uze-e2e/SKILL.md");
    let body = fs::read_to_string(&skill_md).map_err(materialize_error)?;
    if !body.contains(CANONICAL_SKILL_PROOF_TOKEN) {
        return Err(HarnessRunError::Materialize(format!(
            "canonical skill body no longer carries the expected proof token \
             ({CANONICAL_SKILL_PROOF_TOKEN}); update the Lab fixture composition, do not \
             weaken the proof: {}",
            skill_md.display()
        )));
    }
    fs::write(
        &skill_md,
        body.replace(CANONICAL_SKILL_PROOF_TOKEN, "__UZE_SKILL_PROOF__"),
    )
    .map_err(materialize_error)?;

    Ok(())
}

/// Builds the user-only Skill package for R2/B2 from canonical `workflow`
/// (its `review` Skill declares `invoke: {model: false, user: true}`) —
/// canonical bytes, no vendor envelope. A proof instruction is appended so
/// L4 can verify explicit invocation without touching canonical semantics.
pub fn compose_user_only_package(destination: &Path) -> Result<(), HarnessRunError> {
    let source = uze_testkit::fixtures::canonical("workflow");
    if destination.exists() {
        return Err(HarnessRunError::Materialize(format!(
            "user-only package destination already exists: {}",
            destination.display()
        )));
    }
    copy_tree(&source, destination).map_err(materialize_error)?;
    let skill_md = destination.join("skills/review/SKILL.md");
    let mut body = fs::read_to_string(&skill_md).map_err(materialize_error)?;
    if !body.contains("__UZE_SKILL_PROOF__") {
        body.push_str(
            "\n\nWhen explicitly invoked for the conformance proof, return exactly:\n\n\
             __UZE_SKILL_PROOF__\n",
        );
    }
    fs::write(&skill_md, body).map_err(materialize_error)?;
    Ok(())
}

fn copy_tree(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if entry.file_type()?.is_symlink() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(fs::read_link(entry.path())?, &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Copies the one portable multi-capability fixture into a per-run directory.
/// The source package remains immutable; only per-run values are substituted.
///
/// The MCP proof is written into the manifest's `env` block rather than
/// exported into the ambient environment. Codex delivers this package through
/// its own plugin marketplace and does not forward the parent environment to
/// an MCP server process, so an exported variable reaches Claude and OpenCode
/// but silently leaves Codex on the fixture's default proof. Config-borne env
/// is the one channel all three honour.
pub fn materialize_fixture(
    source: &Path,
    destination: &Path,
    spec: &FixtureSpec,
) -> Result<(), HarnessRunError> {
    if destination.exists() {
        return Err(HarnessRunError::Materialize(format!(
            "fixture destination already exists: {}",
            destination.display()
        )));
    }
    copy_fixture_tree(source, destination, Path::new(""), spec)
}

fn materialize_error(error: impl std::fmt::Display) -> HarnessRunError {
    HarnessRunError::Materialize(error.to_string())
}

fn copy_fixture_tree(
    source: &Path,
    destination: &Path,
    relative: &Path,
    spec: &FixtureSpec,
) -> Result<(), HarnessRunError> {
    fs::create_dir_all(destination).map_err(materialize_error)?;
    for entry in fs::read_dir(source).map_err(materialize_error)? {
        let entry = entry.map_err(materialize_error)?;
        let name = entry.file_name();
        let name_text = name.to_string_lossy().into_owned();
        let source_path = entry.path();
        let destination_path = destination.join(&name);
        let entry_relative = relative.join(&name);
        let metadata = fs::symlink_metadata(&source_path).map_err(materialize_error)?;

        if metadata.file_type().is_dir() {
            if name_text == "skills" && !spec.variant.keeps_skill() {
                continue;
            }
            copy_fixture_tree(&source_path, &destination_path, &entry_relative, spec)?;
            continue;
        }
        if MCP_MANIFESTS.contains(&name_text.as_str()) && !spec.variant.keeps_mcp() {
            continue;
        }
        if metadata.file_type().is_symlink() {
            let target = fs::read_link(&source_path).map_err(materialize_error)?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(target, &destination_path).map_err(materialize_error)?;
            #[cfg(not(unix))]
            return Err(HarnessRunError::Materialize(
                "fixture symlink materialization requires a platform-specific implementation"
                    .to_owned(),
            ));
            continue;
        }

        let bytes = fs::read(&source_path).map_err(materialize_error)?;
        let bytes = if name_text == "SKILL.md" {
            String::from_utf8(bytes)
                .map_err(materialize_error)?
                .replace("__UZE_SKILL_PROOF__", &spec.proof.skill)
                .into_bytes()
        } else if MCP_MANIFESTS.contains(&name_text.as_str()) {
            resolve_mcp_manifest(&bytes, spec)?
        } else if name_text == CODEX_ENVELOPE && relative.file_name().is_some() {
            // Only the `.codex-plugin/plugin.json` envelope, never the
            // package root manifest of the same name.
            prune_codex_envelope(&bytes, spec)?
        } else {
            bytes
        };
        fs::write(&destination_path, bytes).map_err(materialize_error)?;
        fs::set_permissions(&destination_path, metadata.permissions())
            .map_err(materialize_error)?;
    }
    Ok(())
}

/// Substitutes the server binary placeholder and declares the per-run MCP
/// proof on every server in the manifest.
///
/// The proof travels in `args`, not in the ambient environment and not only
/// in `env`. Exporting it reaches Claude and OpenCode but silently leaves
/// Codex on the fixture default, because Codex does not forward the parent
/// environment to an MCP server process. An `env` block has the mirror-image
/// problem: Codex copies the manifest verbatim, while UZE's vendor-config
/// writers record `environment` as an empty reference list and drop it.
/// `args` is the one channel every route persists intact.
fn resolve_mcp_manifest(bytes: &[u8], spec: &FixtureSpec) -> Result<Vec<u8>, HarnessRunError> {
    let text = String::from_utf8(bytes.to_vec())
        .map_err(materialize_error)?
        .replace(
            "__UZE_MCP_FIXTURE_BINARY__",
            &spec.mcp_binary.to_string_lossy(),
        );
    let mut manifest: serde_json::Value = serde_json::from_str(&text).map_err(materialize_error)?;
    let servers = manifest
        .get_mut("mcpServers")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| HarnessRunError::Materialize("mcp manifest has no mcpServers".to_owned()))?;
    for (_, server) in servers.iter_mut() {
        let table = server.as_object_mut().ok_or_else(|| {
            HarnessRunError::Materialize("mcp server entry is not an object".to_owned())
        })?;
        table.insert(
            "args".to_owned(),
            serde_json::json!(["--proof", spec.proof.mcp]),
        );
    }
    serde_json::to_vec_pretty(&manifest).map_err(materialize_error)
}

/// Drops envelope keys pointing at capabilities this variant removed, so a
/// capability-isolated copy never advertises a path that no longer exists.
fn prune_codex_envelope(bytes: &[u8], spec: &FixtureSpec) -> Result<Vec<u8>, HarnessRunError> {
    let mut envelope: serde_json::Value =
        serde_json::from_slice(bytes).map_err(materialize_error)?;
    let Some(table) = envelope.as_object_mut() else {
        return Ok(bytes.to_vec());
    };
    if !spec.variant.keeps_skill() {
        table.remove("skills");
    }
    if !spec.variant.keeps_mcp() {
        table.remove("mcpServers");
    }
    serde_json::to_vec_pretty(&envelope).map_err(materialize_error)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessRunSpec {
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    pub environment: BTreeMap<String, String>,
    pub home: PathBuf,
    pub uze_home: PathBuf,
    pub working_directory: PathBuf,
    pub stdin: Option<Vec<u8>>,
    pub timeout: Duration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessRunResult {
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub elapsed: Duration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HarnessRunError {
    Spawn(String),
    Stdin(String),
    Wait(String),
    Materialize(String),
}

impl std::fmt::Display for HarnessRunError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(message) => write!(formatter, "unable to spawn harness: {message}"),
            Self::Stdin(message) => write!(formatter, "unable to write harness stdin: {message}"),
            Self::Wait(message) => write!(formatter, "unable to wait for harness: {message}"),
            Self::Materialize(message) => {
                write!(formatter, "unable to materialize fixture: {message}")
            }
        }
    }
}

impl std::error::Error for HarnessRunError {}

pub fn run(spec: &HarnessRunSpec) -> Result<HarnessRunResult, HarnessRunError> {
    let started = Instant::now();
    let mut command = Command::new(&spec.executable);
    command
        .args(&spec.arguments)
        .current_dir(&spec.working_directory)
        .env_clear()
        .env("HOME", &spec.home)
        .env("UZE_HOME", &spec.uze_home)
        .envs(&spec.environment)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(
            spec.stdin
                .is_some()
                .then(Stdio::piped)
                .unwrap_or_else(Stdio::null),
        );
    let mut child = command
        .spawn()
        .map_err(|error| HarnessRunError::Spawn(error.to_string()))?;
    if let Some(input) = &spec.stdin {
        child
            .stdin
            .take()
            .ok_or_else(|| HarnessRunError::Stdin("stdin pipe was not available".to_owned()))?
            .write_all(input)
            .map_err(|error| HarnessRunError::Stdin(error.to_string()))?;
    }
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| HarnessRunError::Wait(error.to_string()))?
        {
            let output = child
                .wait_with_output()
                .map_err(|error| HarnessRunError::Wait(error.to_string()))?;
            return Ok(HarnessRunResult {
                exit_code: status.code(),
                timed_out: false,
                stdout: output.stdout,
                stderr: output.stderr,
                elapsed: started.elapsed(),
            });
        }
        if started.elapsed() >= spec.timeout {
            child
                .kill()
                .map_err(|error| HarnessRunError::Wait(error.to_string()))?;
            let output = child
                .wait_with_output()
                .map_err(|error| HarnessRunError::Wait(error.to_string()))?;
            return Ok(HarnessRunResult {
                exit_code: output.status.code(),
                timed_out: true,
                stdout: output.stdout,
                stderr: output.stderr,
                elapsed: started.elapsed(),
            });
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, fs};

    fn temporary_directory(label: &str) -> PathBuf {
        let path = env::temp_dir().join(format!("uze-conformance-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn run_clears_ambient_environment_and_sets_isolated_homes() {
        let root = temporary_directory("environment");
        let output = run(&HarnessRunSpec {
            executable: PathBuf::from("sh"),
            arguments: vec![
                "-c".to_owned(),
                "printf '%s|%s|%s' \"$HOME\" \"$UZE_HOME\" \"${UNRELATED-unset}\"".to_owned(),
            ],
            environment: BTreeMap::new(),
            home: root.join("home"),
            uze_home: root.join("uze"),
            working_directory: root.clone(),
            stdin: None,
            timeout: Duration::from_secs(1),
        })
        .unwrap();
        assert_eq!(output.exit_code, Some(0));
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            format!(
                "{}|{}|unset",
                root.join("home").display(),
                root.join("uze").display()
            )
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn run_reports_timeout_without_hanging_the_test_process() {
        let root = temporary_directory("timeout");
        let output = run(&HarnessRunSpec {
            executable: PathBuf::from("sh"),
            arguments: vec!["-c".to_owned(), "sleep 1".to_owned()],
            environment: BTreeMap::new(),
            home: root.join("home"),
            uze_home: root.join("uze"),
            working_directory: root.clone(),
            stdin: None,
            timeout: Duration::from_millis(20),
        })
        .unwrap();
        assert!(output.timed_out);
        let _ = fs::remove_dir_all(root);
    }

    fn write_fixture(root: &Path) -> PathBuf {
        let source = root.join("source");
        let skill = source.join("skills/proof/SKILL.md");
        fs::create_dir_all(skill.parent().unwrap()).unwrap();
        fs::write(&skill, "proof: __UZE_SKILL_PROOF__").unwrap();
        for name in MCP_MANIFESTS {
            fs::write(
                source.join(name),
                r#"{"mcpServers":{"conformance":{"command":"__UZE_MCP_FIXTURE_BINARY__","args":[]}}}"#,
            )
            .unwrap();
        }
        fs::create_dir_all(source.join(".codex-plugin")).unwrap();
        fs::write(
            source.join(".codex-plugin/plugin.json"),
            r#"{"name":"p","skills":"./skills/","mcpServers":"./.mcp.json"}"#,
        )
        .unwrap();
        source
    }

    fn spec(variant: FixtureVariant) -> FixtureSpec {
        FixtureSpec {
            variant,
            mcp_binary: PathBuf::from("/usr/local/bin/fixture"),
            proof: DynamicProof::from_nonce("nonce-42"),
        }
    }

    #[test]
    fn materialized_fixture_has_dynamic_skill_proof_and_immutable_source() {
        let root = temporary_directory("fixture");
        let source = write_fixture(&root);
        let destination = root.join("destination");

        materialize_fixture(&source, &destination, &spec(FixtureVariant::Full)).unwrap();

        assert_eq!(
            fs::read_to_string(destination.join("skills/proof/SKILL.md")).unwrap(),
            "proof: UZE_CONFORMANCE_SKILL_nonce-42"
        );
        assert_eq!(
            fs::read_to_string(source.join("skills/proof/SKILL.md")).unwrap(),
            "proof: __UZE_SKILL_PROOF__"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn materialized_mcp_manifest_declares_the_proof_and_the_resolved_binary() {
        let root = temporary_directory("mcp-manifest");
        let source = write_fixture(&root);
        let destination = root.join("destination");

        materialize_fixture(&source, &destination, &spec(FixtureVariant::Full)).unwrap();

        for name in MCP_MANIFESTS {
            let manifest: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(destination.join(name)).unwrap()).unwrap();
            let server = &manifest["mcpServers"]["conformance"];
            assert_eq!(server["command"], "/usr/local/bin/fixture");
            // Arguments, because that is the only channel every UZE delivery
            // route persists intact.
            assert_eq!(
                server["args"],
                serde_json::json!(["--proof", "UZE_CONFORMANCE_MCP_nonce-42"])
            );
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn skill_only_variant_removes_every_mcp_surface() {
        let root = temporary_directory("skill-only");
        let source = write_fixture(&root);
        let destination = root.join("destination");

        materialize_fixture(&source, &destination, &spec(FixtureVariant::SkillOnly)).unwrap();

        assert!(destination.join("skills/proof/SKILL.md").exists());
        for name in MCP_MANIFESTS {
            assert!(!destination.join(name).exists(), "{name} must be absent");
        }
        let envelope: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(destination.join(".codex-plugin/plugin.json")).unwrap(),
        )
        .unwrap();
        assert!(envelope.get("mcpServers").is_none());
        assert!(envelope.get("skills").is_some());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn mcp_only_variant_removes_every_skill_surface() {
        let root = temporary_directory("mcp-only");
        let source = write_fixture(&root);
        let destination = root.join("destination");

        materialize_fixture(&source, &destination, &spec(FixtureVariant::McpOnly)).unwrap();

        assert!(!destination.join("skills").exists());
        assert!(destination.join("mcp.json").exists());
        let envelope: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(destination.join(".codex-plugin/plugin.json")).unwrap(),
        )
        .unwrap();
        assert!(envelope.get("skills").is_none());
        assert!(envelope.get("mcpServers").is_some());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn dynamic_proof_keeps_skill_and_mcp_channels_distinct() {
        let proof = DynamicProof::from_nonce("same-run");
        assert_ne!(proof.skill, proof.mcp);
        assert!(proof.skill.contains("same-run"));
        assert!(proof.mcp.contains("same-run"));
    }

    #[test]
    fn composed_package_comes_from_the_single_fixture_source() {
        let root = temporary_directory("compose");
        let destination = root.join("package");
        compose_lab_package(&destination).unwrap();
        // Canonical bytes (renamed fixture tree, not a second copy).
        assert!(destination.join("skills/uze-e2e/SKILL.md").is_file());
        assert!(destination.join("plugin.json").is_file());
        assert!(destination.join("mcp.json").is_file());
        assert!(destination.join(".mcp.json").is_file());
        // The vendor-native envelope is the foreign fixture's.
        assert!(destination.join(".codex-plugin/plugin.json").is_file());
        // The canonical proof token was swapped for the per-run placeholder.
        let body = fs::read_to_string(destination.join("skills/uze-e2e/SKILL.md")).unwrap();
        assert!(
            body.contains("__UZE_SKILL_PROOF__"),
            "composed skill must carry the materialization placeholder: {body}"
        );
        // Materializing a second time substitutes the per-run proof.
        let run_dir = root.join("run");
        materialize_fixture(&destination, &run_dir, &spec(FixtureVariant::Full)).unwrap();
        let body = fs::read_to_string(run_dir.join("skills/uze-e2e/SKILL.md")).unwrap();
        assert!(
            body.contains("UZE_CONFORMANCE_SKILL_nonce-42"),
            "per-run skill proof must be substituted: {body}"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn user_only_package_is_canonical_workflow_with_no_envelope() {
        let root = temporary_directory("user-only");
        let destination = root.join("package");
        compose_user_only_package(&destination).unwrap();
        let body = fs::read_to_string(destination.join("skills/review/SKILL.md")).unwrap();
        assert!(
            body.contains("invoke:") && body.contains("model: false"),
            "workflow's review skill must be user-only: {body}"
        );
        assert!(!destination.join(".codex-plugin").exists());
        let _ = fs::remove_dir_all(root);
    }
}
