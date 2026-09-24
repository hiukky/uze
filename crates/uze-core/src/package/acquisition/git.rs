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
    sync::mpsc,
    thread,
    time::Instant,
};

use super::forge::{self, Access, Transport};
use crate::{
    error::{Result, UzeError},
    subprocess::{kill_process_group, read_bounded, wait_with_timeout, with_process_group},
};

/// SSH that neither waits on a prompt nor offers a key to a host the
/// operator never accepted. A passphrase or host-key question would stop
/// the process on the terminal it shares with UZE, and an unknown host
/// collecting the operator's public keys could identify them from a host
/// named in somebody else's `agents.yaml`.
const SSH_COMMAND: &str = "ssh -o BatchMode=yes -o StrictHostKeyChecking=yes -o ConnectTimeout=15";

/// How this machine reaches the network, which a repository cannot
/// influence. libcurl reads `http_proxy` only in lowercase, the others in
/// both cases.
const NETWORK_ENVIRONMENT: &[&str] = &[
    "http_proxy",
    "https_proxy",
    "HTTPS_PROXY",
    "all_proxy",
    "ALL_PROXY",
    "no_proxy",
    "NO_PROXY",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "GIT_SSL_CAINFO",
    "GIT_SSL_CAPATH",
];

/// Where an operation says what it is doing, for whoever shows a person.
pub const STEP: &str = "uze::step";

/// The operator's network settings, with their per-URL scopes.
const NETWORK_KEYS: &str = r"^http\.(.+\.)?(proxy|sslcainfo|sslcapath)$";

/// The operator's credential settings, with their per-URL scopes.
const CREDENTIAL_KEYS: &str = r"^credential\.";

/// Wall-clock budget for any single Git invocation. A remote that never
/// answers must fail rather than hang a `uze add` forever.
const COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// Per-stream cap for a Git invocation's captured output. Every caller in
/// this module only ever reads small plumbing output (a ref name, a commit
/// hash, a short failure line) — this exists purely as a defense-in-depth
/// backstop against a hostile or misbehaving remote, not a real limit any
/// legitimate invocation should approach.
const GIT_OUTPUT_CAP: usize = 8 * 1024 * 1024;

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
///
/// Which userinfo is a secret depends on the transport, so this asks per
/// transport rather than refusing the `@` character:
///
/// - **`http`/`https`** — userinfo is only ever a credential there
///   (`user:token@host`, and a bare `token@host` for a forge that accepts
///   one), so any of it is refused.
/// - **SSH**, in either spelling (`ssh://git@host/repo`,
///   `git@host:org/repo`) — the userinfo is a *user name*, and the secret
///   is a key on disk that never appears in the URL. Refusing it made every
///   private repository unusable to protect against a secret that is not
///   there.
pub fn reject_inline_credentials(url: &str) -> Result<()> {
    let (scheme, rest) = match url.split_once("://") {
        Some((scheme, rest)) => (scheme, rest),
        // No scheme is `scp`-style, which is SSH.
        None => return Ok(()),
    };
    if !matches!(scheme, "http" | "https") {
        return Ok(());
    }
    // Only the authority carries userinfo, and only before the path.
    let authority = rest.split('/').next().unwrap_or_default();
    if authority.contains('@') {
        return Err(UzeError::CredentialBearingUrl);
    }
    Ok(())
}

/// Refuses a value Git would read as an option instead of as the URL or
/// reference it is meant to be.
///
/// The input is not only operator-typed: `git:` comes out of a project's
/// checked-in `agents.yaml` and out of `agents.lock`, so cloning a repository
/// and running `uze install` is a delivery path. A leading `-` turns the URL
/// into `--upload-pack=<cmd>` (a command Git spawns), `--template=<dir>` (a
/// directory Git copies into the new repository) or `--separate-git-dir=<p>`
/// (a write outside the scratch directory). The end-of-options markers at
/// each call site are the second half of the same rule; this half is what
/// holds for `git checkout`, which offers no such marker, and it refuses
/// before any process is spawned.
pub(super) fn reject_option_shaped(value: &str, what: &str) -> Result<()> {
    if value.starts_with('-') {
        return Err(UzeError::AcquisitionFailed(format!(
            "{what} `{value}` starts with `-`, which git reads as an option"
        )));
    }
    Ok(())
}

/// Clones `url` into `destination` and checks out `reference`, returning the
/// resolved commit.
///
/// `destination` must not exist; the caller owns it and its cleanup.
pub fn materialize(url: &str, reference: Option<&str>, destination: &Path) -> Result<String> {
    reject_inline_credentials(url)?;
    reject_option_shaped(url, "repository url")?;
    if let Some(reference) = reference {
        reject_option_shaped(reference, "reference")?;
    }

    // `--no-checkout` first, so nothing from the repository lands on disk
    // before the requested revision is chosen. Full history, not `--depth 1`:
    // a shallow clone cannot check out an arbitrary commit the caller pinned,
    // and correctness comes before the transfer saving.
    //
    // `--` is where `git clone [<options>] [--] <repo> [<dir>]` stops reading
    // options, so neither positional can be taken for one.
    through(url, None, |transport| {
        // A failed attempt may leave a partial clone behind, and the next
        // one needs the directory absent.
        let _ = fs::remove_dir_all(destination);
        run_as(
            Reach::of(transport),
            &[
                "clone",
                "--no-checkout",
                "--no-recurse-submodules",
                "--",
                &transport.url,
                &destination.to_string_lossy(),
            ],
            None,
        )?;
        holds_a_commit(destination, transport)
    })?;

    // Resolve to a commit *before* checking anything out. Detaching by SHA
    // removes every ambiguity a ref name carries — a branch that exists only
    // as a remote-tracking ref after clone, a tag and branch sharing a name,
    // or `HEAD` being read as a path — and it yields the resolved revision as
    // a side effect rather than as a second question.
    let commit = resolve_commit(reference, destination)?;
    // `git checkout` accepts no end-of-options marker (`--` there introduces
    // pathspecs), so the guard is the only thing standing between a value
    // Git printed and a value Git parses as an option.
    reject_option_shaped(&commit, "resolved commit")?;
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
        // `--end-of-options`, not `--`: in `rev-parse` the latter separates
        // revisions from *paths*, so it would stop the candidate being read
        // as a revision at all.
        if let Ok(output) = run(
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                "--end-of-options",
                candidate,
            ],
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

/// What one Git invocation may carry beyond the stripped environment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Reach {
    access: Access,
    plain_http: bool,
}

impl Reach {
    /// A repository on this disk, or a question about one: nothing to
    /// authenticate and no network to reach.
    pub(super) const LOCAL: Self = Self {
        access: Access::Local,
        plain_http: false,
    };

    pub(super) fn of(transport: &Transport) -> Self {
        Self {
            access: transport.access,
            plain_http: transport.plain_http(),
        }
    }
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
pub(super) fn run(arguments: &[&str], working_directory: Option<&Path>) -> Result<String> {
    run_as(Reach::LOCAL, arguments, working_directory)
}

/// [`run`], for an attempt that reaches the network.
///
/// Every attempt that leaves this machine also takes the operator's network
/// — proxy and certificate authority, from the environment and from their
/// Git config — because a company network is where a stripped environment
/// fails first. A credentialed one takes the operator's environment minus
/// every `GIT_*` variable, and their `credential.*` settings with the URL
/// scopes `gh auth setup-git` writes: a helper needs whatever its author
/// needed, and a whitelist would be a list of helpers that happen to work.
/// What stays excluded is *configuration* — aliases, filters, `includeIf`,
/// `core.sshCommand` — which is what the stripped environment is for.
///
/// Configuration is handed over through `GIT_CONFIG_COUNT`, never `-c`: an
/// inline helper can carry a secret, and argv is visible in `ps` and
/// recorded in the span below.
pub(super) fn run_as(
    reach: Reach,
    arguments: &[&str],
    working_directory: Option<&Path>,
) -> Result<String> {
    let span = tracing::info_span!(
        "acquisition.git",
        args = %arguments.join(" "),
        exit = tracing::field::Empty
    );
    let _entered = span.enter();
    let mut command = Command::new("git");
    match reach.access {
        Access::Local | Access::Anonymous => {
            command
                .env_clear()
                .env("PATH", std::env::var("PATH").unwrap_or_default());
        }
        Access::Credentialed => {
            without_git_environment(&mut command, &[]);
            // A helper that would open a browser or a dialog for a host a
            // project named is a login page somebody else chose.
            command.env("GCM_INTERACTIVE", "never");
        }
    }
    let protocols = if reach.plain_http {
        "file:https:ssh:git:http"
    } else {
        "file:https:ssh:git"
    };
    command
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "")
        .env("GIT_ALLOW_PROTOCOL", protocols);
    if reach.access != Access::Local {
        for key in NETWORK_ENVIRONMENT {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
    }
    let config = pushed_config(reach);
    command.env("GIT_CONFIG_COUNT", config.len().to_string());
    for (index, (key, value)) in config.iter().enumerate() {
        command
            .env(format!("GIT_CONFIG_KEY_{index}"), key)
            .env(format!("GIT_CONFIG_VALUE_{index}"), value);
    }
    command
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

    let mut child = with_process_group(command)
        .spawn()
        .map_err(|error| UzeError::AcquisitionFailed(format!("could not run `git`: {error}")))?;
    let Some(mut stdout) = child.stdout.take() else {
        return Err(UzeError::AcquisitionFailed(
            "captured git stdout was not piped".to_owned(),
        ));
    };
    let Some(mut stderr) = child.stderr.take() else {
        return Err(UzeError::AcquisitionFailed(
            "captured git stderr was not piped".to_owned(),
        ));
    };
    let deadline = Instant::now() + COMMAND_TIMEOUT;
    let timed_out_error = || {
        UzeError::AcquisitionFailed(format!(
            "`git {}` timed out",
            arguments.first().copied().unwrap_or("command")
        ))
    };
    // Readers report through a channel rather than a bare `wait_with_output`
    // call: `git` itself can write more than the OS pipe buffer before
    // exiting (a large `for-each-ref`/`log` listing, a verbose failure), and
    // nothing was draining the pipes while `wait_with_timeout` below polls
    // — `git`'s own write() would then block on the full buffer, so it never
    // reaches exit, and the stall gets misreported as a timeout rather than
    // what it actually is: backpressure from an unread pipe.
    let (stdout_tx, stdout_rx) = mpsc::channel();
    let (stderr_tx, stderr_rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = stdout_tx.send(read_bounded(&mut stdout, GIT_OUTPUT_CAP));
    });
    thread::spawn(move || {
        let _ = stderr_tx.send(read_bounded(&mut stderr, GIT_OUTPUT_CAP));
    });
    let (status, timed_out) = wait_with_timeout(&mut child, COMMAND_TIMEOUT).map_err(|error| {
        UzeError::AcquisitionFailed(format!("could not wait for `git`: {error}"))
    })?;
    span.record("exit", status.code().unwrap_or(-1));
    if timed_out {
        tracing::warn!("git timed out");
        return Err(timed_out_error());
    }
    // `git` itself exited, but the same reasoning as above applies once more
    // if it forked a helper (a credential helper, a submodule hook despite
    // `core.hooksPath=/dev/null`) that inherited and still holds a pipe
    // open: bound the wait for the readers by what remains of the deadline
    // instead of joining unconditionally.
    let remaining = deadline.saturating_duration_since(Instant::now());
    let (stdout_bytes, _) = match stdout_rx.recv_timeout(remaining) {
        Ok(result) => result,
        Err(_) => {
            kill_process_group(child.id());
            return Err(timed_out_error());
        }
    };
    let (stderr_bytes, _) = match stderr_rx.recv_timeout(remaining) {
        Ok(result) => result,
        Err(_) => {
            kill_process_group(child.id());
            return Err(timed_out_error());
        }
    };
    if !status.success() {
        // Git echoes the URL it was given. A rejected credential-bearing URL
        // never reaches this far, but a redirect or an embedded token in some
        // other position still must not survive into an error a user pastes
        // into an issue.
        let complaint = String::from_utf8_lossy(&stderr_bytes);
        let complaint = complaint.trim();
        if cannot_resolve_host(complaint) {
            return Err(UzeError::RepositoryOffline {
                detail: redact(complaint),
            });
        }
        if refused_for_access(complaint) {
            return Err(UzeError::RepositoryAccessRefused {
                detail: redact(complaint),
            });
        }
        return Err(UzeError::AcquisitionFailed(redact(&format!(
            "`git {}` failed: {}",
            arguments.first().copied().unwrap_or("command"),
            complaint
        ))));
    }
    Ok(String::from_utf8_lossy(&stdout_bytes).into_owned())
}

/// [`SSH_COMMAND`], sharing one connection per host for a minute when this
/// machine has a directory only this user can reach to keep its socket in.
///
/// An SSH handshake to a forge is a second or more of round trips, paid
/// again by every Git operation that opens one: a catalogue refresh over
/// three marketplaces on one host paid it three times. A master connection
/// kept for sixty seconds turns the second and later into a few
/// milliseconds. The socket is the operator's connection, so it lives where
/// nobody else can reach it — `$XDG_RUNTIME_DIR`, or a directory under the
/// temporary one that this user owns with no access for anybody else — and
/// when neither can be had, every operation opens its own.
fn ssh_command() -> String {
    match multiplexing_directory() {
        Some(directory) => format!(
            "{SSH_COMMAND} -o ControlMaster=auto -o ControlPersist=60 -o \"ControlPath={}/%C\"",
            directory.display()
        ),
        None => SSH_COMMAND.to_owned(),
    }
}

#[cfg(unix)]
fn multiplexing_directory() -> Option<PathBuf> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    // SAFETY: getuid cannot fail and touches no memory.
    let uid = unsafe { libc::getuid() };
    let directory = match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(runtime) => PathBuf::from(runtime).join("uze-ssh"),
        None => std::env::temp_dir().join(format!("uze-ssh-{uid}")),
    };
    let _ = fs::DirBuilder::new().mode(0o700).create(&directory);
    // Never followed: a link planted here would point the socket elsewhere.
    let metadata = fs::symlink_metadata(&directory).ok()?;
    let private =
        metadata.is_dir() && metadata.uid() == uid && metadata.permissions().mode() & 0o077 == 0;
    // A socket path has to fit `sun_path`, with room for the 40-character
    // hash `%C` expands to.
    let fits = directory.as_os_str().len() + 42 < 100;
    (private && fits && !directory.to_string_lossy().contains('"')).then_some(directory)
}

#[cfg(not(unix))]
fn multiplexing_directory() -> Option<PathBuf> {
    None
}

/// The configuration an attempt carries, beyond the stripped environment's.
fn pushed_config(reach: Reach) -> Vec<(String, String)> {
    let mut config = vec![("core.sshCommand".to_owned(), ssh_command())];
    if reach.access != Access::Local {
        config.extend(operator_config(NETWORK_KEYS));
    }
    if reach.access == Access::Credentialed {
        config.extend(operator_config(CREDENTIAL_KEYS));
        // After the operator's own, so no helper setting of theirs turns it
        // back on: a helper asks nobody anything for a host a project named.
        config.push(("credential.interactive".to_owned(), "false".to_owned()));
        // A redirect must not carry the helper's answer to another host.
        config.push(("http.followRedirects".to_owned(), "false".to_owned()));
    }
    config
}

/// Removes every `GIT_*` variable from `command`'s inherited environment,
/// except those named in `keep`.
fn without_git_environment(command: &mut Command, keep: &[&str]) {
    for (key, _) in std::env::vars_os() {
        let name = key.to_string_lossy();
        if name.starts_with("GIT_") && !keep.contains(&name.as_ref()) {
            command.env_remove(key);
        }
    }
}

/// The operator's own Git settings whose keys match `pattern`, in the order
/// Git reads them, across every scope and every `[include]`.
///
/// Read from a directory that is no repository, so nothing local answers,
/// and with the operator's environment minus `GIT_*` — except the three
/// that say which files their own Git reads (`GIT_CONFIG_NOSYSTEM`,
/// `GIT_CONFIG_GLOBAL`, `GIT_CONFIG_SYSTEM`), because what is read here has
/// to be what their Git would use. A path under
/// `~/` is expanded here: the attempt it is handed to may have no `HOME`.
///
/// Read once per process for each home it is asked under: a listing reads a
/// mirror's manifest with the access the mirror was last reached by, and a
/// `git config` spawn per read is what a budgeted command cannot pay.
fn operator_config(pattern: &str) -> Vec<(String, String)> {
    type Settings = Vec<(String, String)>;
    type Asked = (String, Vec<Option<std::ffi::OsString>>);
    static READ: std::sync::Mutex<Vec<(Asked, Settings)>> = std::sync::Mutex::new(Vec::new());
    let key = (
        pattern.to_owned(),
        [
            "HOME",
            "XDG_CONFIG_HOME",
            "GIT_CONFIG_NOSYSTEM",
            "GIT_CONFIG_GLOBAL",
            "GIT_CONFIG_SYSTEM",
        ]
        .iter()
        .map(std::env::var_os)
        .collect(),
    );
    if let Some((_, settings)) = READ
        .lock()
        .ok()
        .and_then(|read| read.iter().find(|(known, _)| *known == key).cloned())
    {
        return settings;
    }
    let settings = read_operator_config(pattern);
    if let Ok(mut read) = READ.lock() {
        read.push((key, settings.clone()));
    }
    settings
}

fn read_operator_config(pattern: &str) -> Vec<(String, String)> {
    const WHICH_FILES: &[&str] = &[
        "GIT_CONFIG_NOSYSTEM",
        "GIT_CONFIG_GLOBAL",
        "GIT_CONFIG_SYSTEM",
    ];
    // An empty directory as the repository, so no repository's own config
    // can answer — whatever encloses the temporary directory.
    let no_repository = std::env::temp_dir().join("uze-no-repository");
    let _ = fs::create_dir_all(&no_repository);
    let mut command = Command::new("git");
    without_git_environment(&mut command, WHICH_FILES);
    let Ok(output) = command
        .args(["config", "--includes", "-z", "--get-regexp", pattern])
        .env("GIT_DIR", &no_repository)
        .current_dir(&no_repository)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    let home = std::env::var_os("HOME").map(PathBuf::from);
    String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let (key, value) = entry.split_once('\n').unwrap_or((entry, ""));
            let value = match (value.strip_prefix("~/"), &home) {
                (Some(rest), Some(home))
                    if key.ends_with(".sslcainfo") || key.ends_with(".sslcapath") =>
                {
                    home.join(rest).to_string_lossy().into_owned()
                }
                _ => value.to_owned(),
            };
            (key.to_owned(), value)
        })
        .collect()
}

/// Reaches the repository `url` names by each of its transports in turn,
/// starting from `remembered` when it is one of them, and answers with the
/// first that works and the transport that did.
///
/// The HTTPS attempts share a host, so when one of them cannot resolve or
/// cannot connect to it, the other is skipped rather than paid for again —
/// but SSH is still asked, because the operator's `~/.ssh/config` may name
/// a host (`github-work`) that DNS has never heard of. The repository is
/// offline only when no transport resolved it. A failure to write locally
/// is not a question for another transport and is answered at once. When
/// none works, one error names every transport and what each said. A
/// repository with a single transport answers exactly as it always did.
pub(super) fn through<T>(
    url: &str,
    remembered: Option<&Transport>,
    mut attempt: impl FnMut(&Transport) -> Result<T>,
) -> Result<(T, Transport)> {
    let mut transports = forge::transports(url)?;
    if let Some(remembered) = remembered
        && let Some(position) = transports.iter().position(|t| t == remembered)
    {
        let first = transports.remove(position);
        transports.insert(0, first);
    }
    let identity = forge::canonical(url);
    let shown = forge::shown(&identity);
    let reach = |transport: &Transport| {
        tracing::info!(
            target: STEP,
            step = "reach",
            repository = %shown,
            via = transport.label()
        );
    };
    if let [only] = transports.as_slice() {
        reach(only);
        return attempt(only).map(|answer| (answer, only.clone()));
    }
    let mut failures = Vec::new();
    let mut https_unreachable: Option<&'static str> = None;
    let (mut unresolved, mut refused) = (0, 0);
    for transport in transports {
        let over_https = transport.url.contains("://") && !transport.url.starts_with("ssh://");
        if over_https && let Some(why) = https_unreachable {
            failures.push(format!("  {}: skipped, {why}", transport.label()));
            unresolved += usize::from(why == UNRESOLVED);
            continue;
        }
        // With no helper and no `.netrc`, HTTPS "with credentials" carries
        // none, and would only ask the anonymous question a second time.
        if over_https && transport.access == Access::Credentialed && !holds_https_credentials() {
            failures.push(format!(
                "  {}: skipped, no credential helper is configured",
                transport.label()
            ));
            continue;
        }
        reach(&transport);
        match attempt(&transport) {
            Ok(answer) => return Ok((answer, transport)),
            Err(error @ (UzeError::Write { .. } | UzeError::Read { .. })) => return Err(error),
            Err(error) => {
                let offline = matches!(error, UzeError::RepositoryOffline { .. });
                unresolved += usize::from(offline);
                refused += usize::from(matches!(error, UzeError::RepositoryAccessRefused { .. }));
                if over_https && offline {
                    https_unreachable = Some(UNRESOLVED);
                } else if over_https && cannot_connect(&error.to_string()) {
                    https_unreachable = Some("the host did not answer over HTTPS");
                }
                let reason = reason_of(&error, &transport);
                tracing::info!(
                    target: STEP,
                    step = "reach_failed",
                    repository = %shown,
                    via = transport.label(),
                    reason = %reason
                );
                failures.push(format!("  {}: {reason}", transport.label()));
            }
        }
    }
    let detail = format!("{identity}\n{}", failures.join("\n"));
    if unresolved == failures.len() {
        return Err(UzeError::RepositoryOffline { detail });
    }
    if refused > 0 {
        return Err(UzeError::RepositoryAccessRefused { detail });
    }
    Err(UzeError::AcquisitionFailed(detail))
}

const UNRESOLVED: &str = "the host does not resolve";

/// Whether an authenticated HTTPS attempt could carry anything an anonymous
/// one does not: a credential helper in the operator's config, or a
/// `.netrc` curl would read.
fn holds_https_credentials() -> bool {
    let helper = operator_config(CREDENTIAL_KEYS)
        .iter()
        .any(|(key, value)| key.ends_with(".helper") && !value.is_empty());
    let netrc = std::env::var_os("HOME")
        .map(PathBuf::from)
        .is_some_and(|home| home.join(".netrc").is_file() || home.join("_netrc").is_file());
    helper || netrc
}

/// Whether an attempt failed before any HTTP was spoken — a port that
/// refuses or never answers, or TLS that did not complete — which the other
/// HTTPS attempt, on the same host and port, would meet again.
fn cannot_connect(complaint: &str) -> bool {
    const UNREACHABLE: &[&str] = &[
        "failed to connect",
        "couldn't connect",
        "connection refused",
        "connection timed out",
        "timed out",
        "ssl",
        "tls",
    ];
    let lowered = complaint.to_lowercase();
    UNREACHABLE.iter().any(|phrase| lowered.contains(phrase))
}

/// Whether what an attempt brought back is a repository at all.
///
/// A forge that answers a login page with status 200 is read by Git as a
/// dumb HTTP server holding nothing, and the clone "succeeds" empty. That
/// is not an answer, and the next transport must be asked.
pub(super) fn holds_a_commit(directory: &Path, transport: &Transport) -> Result<()> {
    let any = run(
        &["for-each-ref", "--count=1", "--format=%(objectname)"],
        Some(directory),
    )?;
    if any.trim().is_empty() {
        return Err(UzeError::AcquisitionFailed(format!(
            "{} answered with no repository",
            redact(&transport.url)
        )));
    }
    Ok(())
}

/// The one line of a failed attempt a person needs: Git's own complaint
/// rather than its advice, and how to accept an SSH host the operator has
/// never connected to.
fn reason_of(error: &UzeError, transport: &Transport) -> String {
    let message = match error {
        UzeError::RepositoryAccessRefused { detail } | UzeError::RepositoryOffline { detail } => {
            detail.clone()
        }
        other => other.to_string(),
    };
    // Git's own words, not its advice: the line that says what went wrong
    // rather than "Please make sure you have the correct access rights" or
    // "Cloning into …", which it prints around every failure alike.
    let lines: Vec<&str> = message.lines().map(str::trim).collect();
    let line = lines
        .iter()
        .find(|line| {
            let lowered = line.to_lowercase();
            [
                "denied",
                "fatal:",
                "error:",
                "timed out",
                "could not resolve",
            ]
            .iter()
            .any(|marker| lowered.contains(marker))
        })
        .or_else(|| lines.iter().rev().find(|line| !line.is_empty()))
        .copied()
        .unwrap_or("failed");
    let line = line.rsplit_once("fatal: ").map_or(line, |(_, rest)| rest);
    if line.to_lowercase().contains("host key verification failed")
        && let Some(destination) = forge::ssh_destination(&transport.url)
    {
        return format!("{line} (accept the host once with `ssh -T {destination}`)");
    }
    line.to_owned()
}

/// Whether Git's complaint is about *reaching* the repository rather than
/// about what is in it.
///
/// Matched on Git's own words, which is a real cost: they are not a stable
/// interface and a translated Git says something else. The alternative is
/// worse — a private marketplace surfacing four lines of Git's advice to
/// somebody who only needs to know which marketplace it was and that it is
/// a question of access. A phrase that stops matching degrades to the
/// ordinary message, never to a wrong one.
fn refused_for_access(complaint: &str) -> bool {
    const ACCESS: &[&str] = &[
        "could not read from remote repository",
        "authentication failed",
        "permission denied",
        "repository not found",
        "access denied",
        "please make sure you have the correct access rights",
        "terminal prompts disabled",
    ];
    let lowered = complaint.to_lowercase();
    ACCESS.iter().any(|phrase| lowered.contains(phrase))
}

/// Whether Git's complaint is that the host has no address from here —
/// the one failure no other transport can do better on. Checked before
/// [`refused_for_access`], because `ssh` follows a resolution failure with
/// the same "could not read from remote repository" it prints for a
/// refused key.
fn cannot_resolve_host(complaint: &str) -> bool {
    const UNRESOLVED: &[&str] = &[
        "could not resolve host",
        "could not resolve hostname",
        "name or service not known",
        "temporary failure in name resolution",
        "nodename nor servname provided",
    ];
    let lowered = complaint.to_lowercase();
    UNRESOLVED.iter().any(|phrase| lowered.contains(phrase))
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

    /// A certificate authority the operator names under `~/` still reaches
    /// an attempt that has no `HOME`, and a credential helper keeps the URL
    /// scope `gh auth setup-git` writes it under.
    #[test]
    fn the_operators_network_and_credentials_are_read_with_their_scopes() {
        let home = uze_testkit::temp::scratch("operator-config");
        fs::write(
            home.join(".gitconfig"),
            "[http]\n\tsslCAInfo = ~/ca.pem\n\
             [http \"https://git.acme.io\"]\n\tproxy = http://proxy.acme.io:3128\n\
             [credential \"https://github.com\"]\n\thelper = !gh auth git-credential\n\
             [alias]\n\tco = checkout\n",
        )
        .unwrap();
        let mut environment = uze_testkit::env::scope();
        environment
            .set("HOME", &home)
            .set("XDG_CONFIG_HOME", home.join("xdg"))
            .set("GIT_CONFIG_NOSYSTEM", "1")
            .remove("GIT_CONFIG_GLOBAL")
            .remove("GIT_CONFIG_SYSTEM");

        let network = operator_config(NETWORK_KEYS);
        assert!(
            network.contains(&(
                "http.sslcainfo".to_owned(),
                home.join("ca.pem").to_string_lossy().into_owned()
            )),
            "{network:?}"
        );
        assert!(
            network.contains(&(
                "http.https://git.acme.io.proxy".to_owned(),
                "http://proxy.acme.io:3128".to_owned()
            )),
            "{network:?}"
        );
        let credentials = operator_config(CREDENTIAL_KEYS);
        assert_eq!(
            credentials,
            vec![(
                "credential.https://github.com.helper".to_owned(),
                "!gh auth git-credential".to_owned()
            )]
        );
        drop(environment);
        let _ = fs::remove_dir_all(home);
    }

    /// A helper is never allowed to ask anybody anything for a host a
    /// project named, whatever the operator configured.
    #[test]
    fn a_credentialed_attempt_never_lets_a_helper_prompt() {
        let home = uze_testkit::temp::scratch("no-interactive-helper");
        fs::write(
            home.join(".gitconfig"),
            "[credential]\n\thelper = manager\n\tinteractive = always\n",
        )
        .unwrap();
        let mut environment = uze_testkit::env::scope();
        environment
            .set("HOME", &home)
            .set("XDG_CONFIG_HOME", home.join("xdg"))
            .set("GIT_CONFIG_NOSYSTEM", "1")
            .remove("GIT_CONFIG_GLOBAL");
        let transport = Transport {
            url: "https://example.invalid/x".to_owned(),
            access: Access::Credentialed,
        };

        let config = pushed_config(Reach::of(&transport));

        let interactive: Vec<_> = config
            .iter()
            .filter(|(key, _)| key == "credential.interactive")
            .collect();
        assert_eq!(
            interactive.last().map(|(_, value)| value.as_str()),
            Some("false"),
            "{config:?}"
        );
        drop(environment);
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn inline_credentials_are_rejected_rather_than_sanitized() {
        for url in [
            "https://user:secret@example.com/repo.git",
            "https://token@example.com/repo.git",
            "http://token@example.com/repo.git",
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

    /// Over SSH the userinfo is a user name and the secret is a key on
    /// disk. Refusing `git@` there protected nothing and made every
    /// private repository unreachable.
    #[test]
    fn an_ssh_user_name_is_not_a_credential() {
        for url in [
            "git@example.com:org/repo.git",
            "ssh://git@example.com/org/repo.git",
            "ssh://example.com/org/repo.git",
        ] {
            assert!(reject_inline_credentials(url).is_ok(), "refused {url}");
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

    /// A `git:` line is project input, and a URL beginning with `-` is an
    /// option to Git — `--upload-pack=<cmd>` being the one that runs a
    /// command. The refusal has to happen before `git` is spawned, which is
    /// what a destination that is never created proves.
    #[test]
    fn an_option_shaped_url_or_reference_is_refused_before_git_is_spawned() {
        let root = uze_testkit::temp::scratch("option-shaped-url");
        let destination = root.join("checkout");

        for url in [
            "--upload-pack=touch /tmp/uze-injection",
            "--template=/tmp/uze-template",
            "-c",
        ] {
            assert!(
                matches!(
                    materialize(url, None, &destination),
                    Err(UzeError::AcquisitionFailed(_))
                ),
                "accepted {url}"
            );
            assert!(!destination.exists(), "{url} reached git");
        }

        assert!(
            matches!(
                materialize(
                    "file:///srv/repo.git",
                    Some("--output=/tmp/x"),
                    &destination
                ),
                Err(UzeError::AcquisitionFailed(_))
            ),
            "accepted an option-shaped reference"
        );
        assert!(
            !destination.exists(),
            "an option-shaped reference reached git"
        );

        let _ = std::fs::remove_dir_all(root);
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

    #[test]
    fn run_drains_output_larger_than_the_pipe_buffer_instead_of_deadlocking() {
        // Regression test: `wait_with_timeout`'s poll loop does not itself
        // read the child's stdout/stderr, so if nothing drains those pipes
        // concurrently, `git` blocks on its own `write()` once a ~64KB OS
        // pipe buffer fills — it never reaches exit, and the stall gets
        // misreported as a timeout. Forcing `for-each-ref` output well past
        // that size reproduces the exact shape of hang this was fixed for.
        //
        // This test resolves `git` by name (via `PATH`), unlike its
        // absolute-path siblings elsewhere in this crate's suite, so it
        // must serialize against `harness_runtime`'s tests, which
        // temporarily narrow process-global `PATH` — otherwise this can
        // race into a spurious `NotFound` with no connection visible from
        // either test's own code.
        let _env = uze_testkit::env::scope();
        let root = uze_testkit::temp::scratch("large-output");
        fs::create_dir_all(&root).unwrap();
        assert!(
            Command::new("git")
                .args(["init", "-q"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["-c", "user.email=t@t", "-c", "user.name=t"])
                .args(["commit", "-q", "--allow-empty", "-m", "root"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        let commit = String::from_utf8(
            Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(&root)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        let commit = commit.trim();
        // Written directly rather than via 5000 individual `git tag` calls:
        // this is setup for the test, not the behavior under test.
        let mut packed_refs = "# pack-refs with: peeled fully-peeled sorted\n".to_owned();
        for index in 0..5000 {
            packed_refs.push_str(&format!("{commit} refs/tags/tag-{index:05}\n"));
        }
        fs::write(root.join(".git/packed-refs"), packed_refs).unwrap();

        let started = Instant::now();
        let listing = run(
            &["for-each-ref", "--format=%(refname)", "refs/tags/"],
            Some(&root),
        )
        .expect("a large but well-formed listing must not be mistaken for a hang");
        assert!(
            started.elapsed() < COMMAND_TIMEOUT,
            "must finish well inside the timeout, not be saved only by eventually hitting it"
        );
        assert_eq!(listing.lines().count(), 5000, "no line may be lost");
        assert!(listing.contains("refs/tags/tag-04999"));

        let _ = fs::remove_dir_all(root);
    }
}
