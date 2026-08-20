//! Test-only process orchestration for the UZE Harness Conformance Lab.
//!
//! This crate deliberately knows nothing about UZE integrations, Docker
//! internals, or vendor output schemas. It starts a selected real executable
//! under a caller-specified disposable environment and reports process facts.

use std::{
    collections::BTreeMap,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

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
}

impl std::fmt::Display for HarnessRunError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(message) => write!(formatter, "unable to spawn harness: {message}"),
            Self::Stdin(message) => write!(formatter, "unable to write harness stdin: {message}"),
            Self::Wait(message) => write!(formatter, "unable to wait for harness: {message}"),
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
}
