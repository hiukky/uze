//! Core-owned Git materialization.
//!
//! The mechanism is Git and nothing else: a URL is a URL, so no host is named
//! here and no host API is called. Everything runs through the `git`
//! executable with a deliberately hostile-input posture.
//!
//! Acquisition never executes package code. It also takes care not to let the
//! *repository* execute anything on its behalf, which is a separate problem:
//! a clone can carry hooks and submodule declarations, and Git will honour
//! configuration it finds unless told not to.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use crate::error::{Result, UzeError};

/// Wall-clock budget for any single Git invocation. A remote that never
/// answers must fail rather than hang a `uze add` forever.
const COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// Upper bound on a materialized checkout. Crude on purpose: it exists so a
/// hostile or accidentally enormous repository cannot fill the disk, not to
/// enforce a package-size policy.
const MAX_MATERIALIZED_BYTES: u64 = 512 * 1024 * 1024;

/// Rejects a URL carrying inline credentials before it is ever used, logged
/// or persisted.
///
/// Storing a sanitized copy was the alternative and is worse: the secret
/// would still have existed in process memory, in argv, and in whatever Git
/// wrote to its own error output. Refusing the input keeps the secret from
/// entering UZE at all. Authenticated Git is a separate mechanism to design
/// deliberately, not a side effect of URL parsing.
pub fn reject_inline_credentials(url: &str) -> Result<()> {
    // Only the authority component can carry userinfo, and only before the
    // first `/` of the path. `scp`-style `user@host:path` is matched too.
    let authority = url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or_default();
    if authority.contains('@') {
        return Err(UzeError::CredentialBearingUrl);
    }
    Ok(())
}

/// Clones `url` into `destination` and checks out `reference`, returning the
/// resolved commit.
///
/// `destination` must not exist; the caller owns it and its cleanup.
pub fn materialize(url: &str, reference: Option<&str>, destination: &Path) -> Result<String> {
    reject_inline_credentials(url)?;

    // `--no-checkout` first, so nothing from the repository lands on disk
    // before the requested revision is chosen. Full history, not `--depth 1`:
    // a shallow clone cannot check out an arbitrary commit the caller pinned,
    // and correctness comes before the transfer saving.
    run(
        &[
            "clone",
            "--no-checkout",
            "--no-recurse-submodules",
            url,
            &destination.to_string_lossy(),
        ],
        None,
    )?;

    // Resolve to a commit *before* checking anything out. Detaching by SHA
    // removes every ambiguity a ref name carries — a branch that exists only
    // as a remote-tracking ref after clone, a tag and branch sharing a name,
    // or `HEAD` being read as a path — and it yields the resolved revision as
    // a side effect rather than as a second question.
    let commit = resolve_commit(reference, destination)?;
    run(&["checkout", "--detach", &commit], Some(destination))?;

    assert_within_size_budget(destination)?;

    // The repository's own metadata is not package content, and leaving it in
    // place would let a `.git` directory travel into the Store.
    let git_dir = destination.join(".git");
    if git_dir.exists() {
        fs::remove_dir_all(&git_dir).map_err(|source| UzeError::Write {
            path: git_dir,
            source,
        })?;
    }
    Ok(commit)
}

/// Turns a request into an immutable commit.
///
/// `None` means the repository's own default branch: after a clone, `HEAD`
/// already points at whatever the remote chose, so it is read rather than
/// guessed. A hardcoded `main` or `master` would fail on a repository using
/// neither, which is exactly the assumption this avoids.
///
/// A named reference is tried as given first, then as a tag, then as a
/// remote-tracking branch — a plain `clone` creates a local branch only for
/// the default, so `feature` exists solely as `origin/feature`.
fn resolve_commit(reference: Option<&str>, checkout: &Path) -> Result<String> {
    let candidates: Vec<String> = match reference {
        // A `--no-checkout` clone creates no local branch, so the local
        // `HEAD` may point at an unborn ref. The remote's own default is read
        // from the tracking ref the clone did create, and if the remote never
        // advertised one, from its sole branch. Never from a guessed name.
        None => {
            let mut candidates = vec!["refs/remotes/origin/HEAD^{commit}".to_owned()];
            if let Some(single) = sole_remote_branch(checkout) {
                candidates.push(single);
            }
            candidates
        }
        Some(reference) => vec![
            format!("{reference}^{{commit}}"),
            format!("refs/tags/{reference}^{{commit}}"),
            format!("refs/remotes/origin/{reference}^{{commit}}"),
        ],
    };
    for candidate in &candidates {
        if let Ok(output) = run(
            &["rev-parse", "--verify", "--quiet", candidate],
            Some(checkout),
        ) {
            let commit = output.trim().to_owned();
            if !commit.is_empty() {
                return Ok(commit);
            }
        }
    }
    Err(UzeError::AcquisitionFailed(format!(
        "no commit matches reference `{}`",
        reference.unwrap_or("HEAD")
    )))
}

/// The single remote branch, when a repository has exactly one.
///
/// Only consulted when the remote advertised no default. With more than one
/// branch and no advertised default there is no honest answer, so this
/// returns `None` and the caller reports that rather than picking.
fn sole_remote_branch(checkout: &Path) -> Option<String> {
    let listing = run(
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/remotes/origin/",
        ],
        Some(checkout),
    )
    .ok()?;
    let branches: Vec<&str> = listing
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && *line != "refs/remotes/origin/HEAD")
        .collect();
    match branches.as_slice() {
        [single] => Some(format!("{single}^{{commit}}")),
        _ => None,
    }
}

fn assert_within_size_budget(root: &Path) -> Result<()> {
    fn total(path: &Path, accumulated: &mut u64) -> Result<()> {
        for entry in fs::read_dir(path).map_err(|source| UzeError::Read {
            path: path.to_path_buf(),
            source,
        })? {
            let entry = entry.map_err(|source| UzeError::Read {
                path: path.to_path_buf(),
                source,
            })?;
            let metadata = entry.metadata().map_err(|source| UzeError::Read {
                path: entry.path(),
                source,
            })?;
            if metadata.is_dir() {
                total(&entry.path(), accumulated)?;
            } else {
                *accumulated += metadata.len();
            }
            if *accumulated > MAX_MATERIALIZED_BYTES {
                return Err(UzeError::AcquisitionFailed(format!(
                    "materialized repository exceeds {MAX_MATERIALIZED_BYTES} bytes"
                )));
            }
        }
        Ok(())
    }
    total(root, &mut 0)
}

/// Runs one `git` invocation under a deliberately minimal, non-interactive
/// environment.
///
/// Every flag here closes a way the repository or the ambient machine could
/// influence the run:
///
/// - `env_clear` plus an explicit `PATH`: no inherited Git configuration, no
///   proxy or credential helper picked up from the operator's shell.
/// - `GIT_CONFIG_NOSYSTEM` / `GIT_CONFIG_GLOBAL=/dev/null`: system and user
///   config cannot introduce a helper, alias or `filter` that runs a command.
/// - `core.hooksPath=/dev/null`: a repository's own hooks are never run.
/// - `GIT_TERMINAL_PROMPT=0` and empty `GIT_ASKPASS`: a private repository
///   fails immediately rather than blocking on a credential prompt.
/// - `protocol.file.allow=always`: needed so a local bare repository — the
///   only kind the deterministic tests use — remains reachable.
fn run(arguments: &[&str], working_directory: Option<&Path>) -> Result<String> {
    let mut command = Command::new("git");
    command
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "")
        .env("GIT_ALLOW_PROTOCOL", "file:https:ssh:git")
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(["-c", "protocol.file.allow=always"])
        .args(["-c", "submodule.recurse=false"])
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(directory) = working_directory {
        command.current_dir(directory);
    }

    let mut child = command
        .spawn()
        .map_err(|error| UzeError::AcquisitionFailed(format!("could not run `git`: {error}")))?;
    let deadline = std::time::Instant::now() + COMMAND_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            Ok(None) => {
                let _ = child.kill();
                return Err(UzeError::AcquisitionFailed(format!(
                    "`git {}` timed out",
                    arguments.first().copied().unwrap_or("command")
                )));
            }
            Err(error) => {
                return Err(UzeError::AcquisitionFailed(format!(
                    "could not wait for `git`: {error}"
                )));
            }
        }
    }
    let output = child.wait_with_output().map_err(|error| {
        UzeError::AcquisitionFailed(format!("could not read `git` output: {error}"))
    })?;
    if !output.status.success() {
        // Git echoes the URL it was given. A rejected credential-bearing URL
        // never reaches this far, but a redirect or an embedded token in some
        // other position still must not survive into an error a user pastes
        // into an issue.
        return Err(UzeError::AcquisitionFailed(redact(&format!(
            "`git {}` failed: {}",
            arguments.first().copied().unwrap_or("command"),
            String::from_utf8_lossy(&output.stderr).trim()
        ))));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Replaces anything shaped like inline credentials in a message.
pub fn redact(message: &str) -> String {
    let mut result = String::with_capacity(message.len());
    for token in message.split_inclusive(char::is_whitespace) {
        match (token.find("://"), token.find('@')) {
            (Some(scheme), Some(at)) if at > scheme => {
                result.push_str(&token[..scheme + 3]);
                result.push_str("<redacted>@");
                result.push_str(&token[at + 1..]);
            }
            _ => result.push_str(token),
        }
    }
    result
}

/// Resolves a package root inside a materialized checkout.
///
/// Validated both lexically and physically: the lexical check rejects a
/// traversal before touching the filesystem, and the physical check catches a
/// path that only escapes once symlinks are followed.
pub fn resolve_subdirectory(root: &Path, subdirectory: &Path) -> Result<PathBuf> {
    if subdirectory.is_absolute()
        || subdirectory
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(UzeError::PackageEscapesRoot {
            link: subdirectory.to_path_buf(),
            target: subdirectory.to_path_buf(),
        });
    }
    let candidate = root.join(subdirectory);
    if !candidate.is_dir() {
        return Err(UzeError::MissingPath(candidate));
    }
    let resolved = candidate.canonicalize().map_err(|source| UzeError::Read {
        path: candidate.clone(),
        source,
    })?;
    let root = root.canonicalize().map_err(|source| UzeError::Read {
        path: root.to_path_buf(),
        source,
    })?;
    if !resolved.starts_with(&root) {
        return Err(UzeError::PackageEscapesRoot {
            link: candidate,
            target: resolved,
        });
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_credentials_are_rejected_rather_than_sanitized() {
        for url in [
            "https://user:secret@example.com/repo.git",
            "https://token@example.com/repo.git",
            "git@example.com:org/repo.git",
        ] {
            assert!(
                matches!(
                    reject_inline_credentials(url),
                    Err(UzeError::CredentialBearingUrl)
                ),
                "accepted {url}"
            );
        }
    }

    #[test]
    fn an_ordinary_url_is_accepted() {
        for url in [
            "https://example.com/org/repo.git",
            "file:///srv/repos/repo.git",
            "https://example.com/org/repo@weird.git",
        ] {
            assert!(reject_inline_credentials(url).is_ok(), "rejected {url}");
        }
    }

    #[test]
    fn redaction_removes_userinfo_from_a_message() {
        let redacted = redact("failed to clone https://user:secret@example.com/repo.git now");
        assert!(!redacted.contains("secret"));
        assert!(redacted.contains("<redacted>@example.com/repo.git"));
    }

    #[test]
    fn a_traversing_subdirectory_is_rejected_before_touching_the_filesystem() {
        let root = Path::new("/nonexistent");
        for subdirectory in ["/etc", "../escape", "packages/../../escape"] {
            assert!(
                matches!(
                    resolve_subdirectory(root, Path::new(subdirectory)),
                    Err(UzeError::PackageEscapesRoot { .. })
                ),
                "accepted {subdirectory}"
            );
        }
    }
}
