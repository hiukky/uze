//! Test-only process orchestration for the UZE Harness Conformance Lab.
//!
//! This crate deliberately knows nothing about UZE integrations, Docker
//! internals, or vendor output schemas. It starts a selected real executable
//! under a caller-specified disposable environment and reports process facts.

use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// Per-run proof values. A behavioral prompt never contains these values: the
/// Skill receives one through its materialized content and the MCP server
/// receives the other through its process environment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DynamicProof {
    pub skill: String,
    pub mcp: String,
}

impl DynamicProof {
    pub fn from_nonce(nonce: &str) -> Self {
        Self {
            skill: format!("UZE_E2E_SKILL_{nonce}"),
            mcp: format!("UZE_E2E_MCP_{nonce}"),
        }
    }
}

/// Distinguishes the layer that was actually proved. A model failure can never
/// rewrite attachment/discovery evidence into an incompatibility claim.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvidenceState {
    Unverified,
    AttachmentVerified,
    DiscoveryVerified,
    LocalBehaviorVerified,
    VendorBehaviorVerified,
    Failed,
    BlockedByEnvironment,
    TimedOut,
    ModelFailure,
    HarnessFailure,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConformanceEvidence {
    pub attachment: EvidenceState,
    pub discovery: EvidenceState,
    pub behavior: EvidenceState,
    pub reason: Option<String>,
}

impl ConformanceEvidence {
    pub fn fresh() -> Self {
        Self {
            attachment: EvidenceState::Unverified,
            discovery: EvidenceState::Unverified,
            behavior: EvidenceState::Unverified,
            reason: None,
        }
    }
}

/// Copies the one portable multi-capability fixture into a per-run directory.
/// The source package remains immutable; only the Skill proof placeholder is
/// replaced. The MCP token is supplied as a process environment value.
pub fn materialize_fixture(
    source: &Path,
    destination: &Path,
    proof: &DynamicProof,
) -> Result<(), HarnessRunError> {
    if destination.exists() {
        return Err(HarnessRunError::Materialize(format!(
            "fixture destination already exists: {}",
            destination.display()
        )));
    }
    copy_fixture_tree(source, destination, proof)
}

fn copy_fixture_tree(
    source: &Path,
    destination: &Path,
    proof: &DynamicProof,
) -> Result<(), HarnessRunError> {
    fs::create_dir_all(destination)
        .map_err(|error| HarnessRunError::Materialize(error.to_string()))?;
    let entries =
        fs::read_dir(source).map_err(|error| HarnessRunError::Materialize(error.to_string()))?;
    for entry in entries {
        let entry = entry.map_err(|error| HarnessRunError::Materialize(error.to_string()))?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path)
            .map_err(|error| HarnessRunError::Materialize(error.to_string()))?;
        if metadata.file_type().is_dir() {
            copy_fixture_tree(&source_path, &destination_path, proof)?;
        } else if metadata.file_type().is_symlink() {
            let target = fs::read_link(&source_path)
                .map_err(|error| HarnessRunError::Materialize(error.to_string()))?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(target, &destination_path)
                .map_err(|error| HarnessRunError::Materialize(error.to_string()))?;
            #[cfg(not(unix))]
            return Err(HarnessRunError::Materialize(
                "fixture symlink materialization requires a platform-specific implementation"
                    .to_owned(),
            ));
        } else {
            let bytes = fs::read(&source_path)
                .map_err(|error| HarnessRunError::Materialize(error.to_string()))?;
            let bytes =
                if source_path.file_name().and_then(|name| name.to_str()) == Some("SKILL.md") {
                    String::from_utf8(bytes)
                        .map_err(|error| HarnessRunError::Materialize(error.to_string()))?
                        .replace("__UZE_SKILL_PROOF__", &proof.skill)
                        .into_bytes()
                } else {
                    bytes
                };
            fs::write(&destination_path, bytes)
                .map_err(|error| HarnessRunError::Materialize(error.to_string()))?;
            fs::set_permissions(&destination_path, metadata.permissions())
                .map_err(|error| HarnessRunError::Materialize(error.to_string()))?;
        }
    }
    Ok(())
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

    #[test]
    fn materialized_fixture_has_dynamic_skill_proof_and_immutable_source() {
        let root = temporary_directory("fixture");
        let source = root.join("source");
        let skill = source.join("skills/proof/SKILL.md");
        fs::create_dir_all(skill.parent().unwrap()).unwrap();
        fs::write(&skill, "proof: __UZE_SKILL_PROOF__").unwrap();
        let proof = DynamicProof::from_nonce("nonce-42");
        let destination = root.join("destination");

        materialize_fixture(&source, &destination, &proof).unwrap();

        assert_eq!(
            fs::read_to_string(destination.join("skills/proof/SKILL.md")).unwrap(),
            "proof: UZE_E2E_SKILL_nonce-42"
        );
        assert_eq!(
            fs::read_to_string(&skill).unwrap(),
            "proof: __UZE_SKILL_PROOF__"
        );
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
    fn fresh_evidence_does_not_claim_behavior() {
        assert_eq!(
            ConformanceEvidence::fresh(),
            ConformanceEvidence {
                attachment: EvidenceState::Unverified,
                discovery: EvidenceState::Unverified,
                behavior: EvidenceState::Unverified,
                reason: None,
            }
        );
    }
}
