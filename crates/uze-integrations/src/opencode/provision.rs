//! OpenCode automatic provisioning and binary detection/aliasing.
//!
//! OpenCode's v2 installer names the binary `opencode2`; UZE's canonical
//! invocation stays `opencode` with no version suffix, so provisioning
//! installs or upgrades normally and then ensures that alias exists —
//! success is only reported once `opencode` itself resolves, not merely
//! `opencode2`.

use std::{fs, path::Path, path::PathBuf, process::Command};

use uze_core::{
    Result,
    integration::HarnessDetection,
    provisioning::{ProcessRunner, ProcessSpec, ProvisionAction, ProvisioningResult},
};

/// Resolves whichever name currently answers to `--version`: `opencode`
/// (the canonical alias, once created) first, then `opencode2` (the raw v2
/// binary name) as a fallback for a fresh v2 install that has no alias yet.
pub(super) fn resolve_opencode_binary() -> Option<(&'static str, HarnessDetection)> {
    let primary = detect_binary("opencode");
    if primary.present {
        return Some(("opencode", primary));
    }
    let fallback = detect_binary("opencode2");
    fallback.present.then_some(("opencode2", fallback))
}

pub(super) fn detect_binary(program: &str) -> HarnessDetection {
    let Ok(output) = Command::new(program).arg("--version").output() else {
        return HarnessDetection::default();
    };
    // `opencode --version` prints "opencode2 v0.0.0-beta-17823" — the
    // version trails, and comes with a `v` prefix intact.
    HarnessDetection {
        present: true,
        version: String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .last()
            .map(str::to_owned),
    }
}

pub(super) fn provision_opencode(
    runner: &dyn ProcessRunner,
    detect: impl Fn() -> HarnessDetection,
) -> Result<ProvisioningResult> {
    if !cfg!(unix) {
        return Ok(ProvisioningResult::blocked(
            "OpenCode automatic provisioning is currently supported on Unix and WSL only",
        ));
    }
    let method = "official-install-script";
    let resolved = resolve_opencode_binary();
    let before = resolved
        .as_ref()
        .map(|(_, detection)| detection.clone())
        .unwrap_or_default();
    let action = if before.present {
        ProvisionAction::Update
    } else {
        ProvisionAction::Install
    };
    let command = match &resolved {
        Some((which, _)) => ProcessSpec::new(*which, ["upgrade"]).with_inherited_output(),
        None => ProcessSpec::new(
            "sh",
            ["-c", "curl -fsSL https://opencode.ai/v2/install | bash"],
        )
        .with_inherited_output(),
    };
    let outcome = match runner.run(&command) {
        Ok(o) => o,
        Err(_) => {
            return Ok(ProvisioningResult::failed(
                action,
                method,
                "official installer could not be started",
            ));
        }
    };
    if !outcome.success {
        let reason = if outcome.timed_out {
            "official installer timed out"
        } else {
            "official installer exited unsuccessfully"
        };
        return Ok(ProvisioningResult::failed(action, method, reason));
    }
    if let Err(reason) = ensure_opencode_alias() {
        return Ok(ProvisioningResult::failed(action, method, reason));
    }
    let verified = runner.run(&ProcessSpec::new("opencode", ["--version"]));
    if !matches!(verified, Ok(o) if o.success) {
        return Ok(ProvisioningResult::failed(
            action,
            method,
            "installer finished but the `opencode` executable could not be verified",
        ));
    }
    Ok(ProvisioningResult::verified(action, method, detect()))
}

/// Resolves `name` to an absolute path via the shell's own `PATH` lookup —
/// the same mechanism the installer that put it there used.
pub(super) fn resolve_executable_path(name: &str) -> Option<PathBuf> {
    let output = Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {name}"))
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8(output.stdout).ok()?;
    let path = path.trim();
    (!path.is_empty()).then(|| PathBuf::from(path))
}

/// Creates or repairs a symlink at `alias_path` pointing to `target`.
/// Idempotent: a symlink already pointing at `target` is left untouched, a
/// stale symlink is replaced, and a real (non-symlink) file already
/// occupying `alias_path` is never touched — it isn't UZE's to overwrite.
#[cfg(unix)]
pub(super) fn ensure_symlink_alias(alias_path: &Path, target: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(alias_path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            if fs::read_link(alias_path)? == target {
                return Ok(());
            }
            fs::remove_file(alias_path)?;
            std::os::unix::fs::symlink(target, alias_path)
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if let Some(parent) = alias_path.parent() {
                fs::create_dir_all(parent)?;
            }
            std::os::unix::fs::symlink(target, alias_path)
        }
        Err(error) => Err(error),
    }
}

#[cfg(not(unix))]
pub(super) fn ensure_symlink_alias(_alias_path: &Path, _target: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "opencode alias creation is only supported on Unix",
    ))
}

/// Ensures `opencode` resolves to OpenCode v2's `opencode2` binary, since
/// UZE's canonical invocation never carries the version suffix. Tries both a
/// alias next to `opencode2` on `PATH` and one in `~/.local/bin`, and
/// succeeds if either lands — `provision`'s own final `opencode --version`
/// check is the actual proof this worked; this function's `Err` only short-
/// circuits that doomed check when neither location was writable.
pub(super) fn ensure_opencode_alias() -> std::result::Result<(), String> {
    if detect_binary("opencode").present {
        return Ok(());
    }
    let target = resolve_executable_path("opencode2")
        .ok_or_else(|| "opencode2 executable not found on PATH after install".to_owned())?;
    let mut aliased = false;
    if let Some(dir) = target.parent()
        && ensure_symlink_alias(&dir.join("opencode"), &target).is_ok()
    {
        aliased = true;
    }
    if let Some(home) = std::env::var_os("HOME")
        && ensure_symlink_alias(&PathBuf::from(home).join(".local/bin/opencode"), &target).is_ok()
    {
        aliased = true;
    }
    if aliased {
        Ok(())
    } else {
        Err(format!(
            "could not create an `opencode` alias for {}",
            target.display()
        ))
    }
}

#[cfg(test)]
mod provision_tests {
    use std::{fs, path::PathBuf};

    use uze_core::home::UzeHome;
    use uze_core::integration::IntegrationPort;
    use uze_core::provisioning::{ProcessResult, ProcessRunner, ProcessSpec};

    use super::super::OpenCodeIntegration;
    use super::{detect_binary, ensure_symlink_alias, resolve_executable_path};

    fn temp(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "uze-opencode-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[cfg(unix)]
    #[test]
    fn symlink_alias_is_created_repaired_and_leaves_foreign_files_alone() {
        use std::os::unix::fs::symlink;

        let root = temp("alias");
        fs::create_dir_all(&root).unwrap();
        let target = root.join("opencode2");
        fs::write(&target, "binary").unwrap();

        // Absent -> created.
        let alias = root.join("bin/opencode");
        ensure_symlink_alias(&alias, &target).unwrap();
        assert_eq!(fs::read_link(&alias).unwrap(), target);

        // Already correct -> left untouched (idempotent).
        ensure_symlink_alias(&alias, &target).unwrap();
        assert_eq!(fs::read_link(&alias).unwrap(), target);

        // Stale symlink (e.g. still pointing at a removed v1 binary) -> repaired.
        let stale_target = root.join("opencode-v1");
        fs::write(&stale_target, "old").unwrap();
        let stale_alias = root.join("bin2/opencode");
        fs::create_dir_all(stale_alias.parent().unwrap()).unwrap();
        symlink(&stale_target, &stale_alias).unwrap();
        ensure_symlink_alias(&stale_alias, &target).unwrap();
        assert_eq!(fs::read_link(&stale_alias).unwrap(), target);

        // A real (non-symlink) file already at the alias path is not UZE's
        // to overwrite.
        let foreign = root.join("bin3/opencode");
        fs::create_dir_all(foreign.parent().unwrap()).unwrap();
        fs::write(&foreign, "not managed by uze").unwrap();
        ensure_symlink_alias(&foreign, &target).unwrap();
        assert_eq!(fs::read_to_string(&foreign).unwrap(), "not managed by uze");

        fs::remove_dir_all(root).unwrap();
    }

    struct RecordingRunner {
        commands: std::sync::Mutex<Vec<ProcessSpec>>,
    }

    impl ProcessRunner for RecordingRunner {
        fn run(&self, spec: &ProcessSpec) -> uze_core::Result<ProcessResult> {
            self.commands.lock().unwrap().push(spec.clone());
            Ok(ProcessResult {
                success: true,
                timed_out: false,
            })
        }
    }

    /// `resolve_opencode_binary` genuinely shells out to whatever `opencode`/
    /// `opencode2` resolve to on the machine running the test, so — like
    /// `detect()` itself — there's no fake executable name to substitute
    /// (unlike the sibling integrations' `provision_cli`, which is
    /// parameterized). This asserts the dispatch logic is *consistent* with
    /// whatever `detect()` observes, rather than assuming one fixed starting
    /// state.
    #[test]
    fn provision_dispatches_install_or_update_consistently_with_detected_state() {
        if !cfg!(unix) {
            return;
        }
        // `provision`'s install/upgrade command is mocked below, so nothing
        // is genuinely installed — `ensure_opencode_alias` still resolves
        // `opencode`/`opencode2` for real. On a machine with neither on
        // PATH (a bare CI runner, unlike a dev box that has one installed)
        // that real resolution fails and `Verified` is unreachable no
        // matter what the mock records; skip rather than assert a status
        // this test has no way to produce here.
        if !detect_binary("opencode").present && resolve_executable_path("opencode2").is_none() {
            return;
        }
        let root = temp("provision");
        let integration = OpenCodeIntegration::new(
            root.join("agents"),
            root.join("config/opencode.json"),
            UzeHome::at(root.join("uze")),
        );
        let runner = RecordingRunner {
            commands: std::sync::Mutex::new(Vec::new()),
        };
        let before = integration.detect();
        let result = integration.provision(&runner).unwrap();
        assert_eq!(
            result.action,
            if before.present {
                uze_core::provisioning::ProvisionAction::Update
            } else {
                uze_core::provisioning::ProvisionAction::Install
            }
        );
        assert_eq!(
            result.status,
            uze_core::provisioning::ProvisionStatus::Verified
        );
        let commands = runner.commands.lock().unwrap();
        if before.present {
            assert_eq!(commands[0].arguments, ["upgrade"]);
        } else {
            assert_eq!(commands[0].program, "sh");
            assert!(commands[0].arguments[1].contains("opencode.ai/v2/install"));
        }
        assert_eq!(commands[1].program, "opencode");
        assert_eq!(commands[1].arguments, ["--version"]);
    }
}
