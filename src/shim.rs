//! PATH shim entry point.
//!
//! This is the part of `uze` that runs when the binary is invoked under a
//! shim symlink name (`~/.uze/shims/<name>`, e.g. `claude`, `codex`,
//! `opencode`) rather than as `uze` itself — see
//! `UzeApplication::ensure_runtime_shim`, which is what creates that
//! symlink at `~/.uze/shims/<name>` as an ordinary part of
//! `uze setup <harness>`, for whichever integrations opt in. Which names
//! count is the registry's answer, never this file's.
//!
//! Deliberately thin, and deliberately generic: every vendor-specific
//! decision comes from `IntegrationPort::runtime_contribution`. This file
//! only detects the invocation, resolves the real binary, asks the matching
//! integration what to add, and `exec`s — no `UzeApplication`, no Store
//! scan, no marketplace refresh, no network. Those are exactly the costs
//! kept out of this hot path.
//!
//! `RUNTIME INFRASTRUCTURE`, not `CONTEXT DELIVERY POLICY` — this file
//! neither knows nor cares whether runtime projection ever replaces the
//! existing persistent `CLAUDE.md` bridge; it only launches
//! whatever the integration decided.

use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
};

use uze_core::{
    UzeHome,
    harness_runtime::{self, HarnessRuntimeContribution, RuntimeContext},
};
use uze_integrations::registry::IntegrationRegistry;

/// `None` when this process was not invoked through one of the registry's
/// shim names — the ordinary `uze <subcommand>` path in `main()` continues
/// unchanged, including a direct `uze` invocation.
pub fn detect() -> Option<String> {
    let argv0 = env::args_os().next()?;
    let name = Path::new(&argv0).file_name()?.to_str()?.to_owned();
    let home = UzeHome::from_env().ok()?;
    let registry = IntegrationRegistry::builtin(&home).ok()?;
    registry
        .shim_names()
        .contains(&name.as_str())
        .then_some(name)
}

/// Diverges. On success this replaces the process image (`exec`) and never
/// returns to `main()`; on an unrecoverable failure (no real executable
/// found at all — nothing left to fail open *to*) it prints one line to
/// stderr and exits non-zero. Every other failure mode falls open to a
/// plain launch of the real binary — see the inline handling below.
pub fn run(shim_name: &str) -> ! {
    let original_args: Vec<OsString> = env::args_os().skip(1).collect();

    let home = match UzeHome::from_env() {
        Ok(home) => home,
        // `$HOME` itself is missing — there is no reliable `shims_dir` to
        // exclude, so a bare PATH search here could resolve back to this
        // very shim. This is the one case where "just try anyway" is less
        // safe than a clear, immediate error.
        Err(error) => die(&format!(
            "cannot resolve UZE_HOME/HOME ({error}); refusing to guess at a real `{shim_name}` \
             to avoid a possible shim loop"
        )),
    };

    let bypass = env::var_os("UZE_BYPASS").is_some_and(|value| value != "0");

    // Reaching this line at all already means the shim symlink exists —
    // that is the entire opt-in signal (see
    // `IntegrationPort::supports_runtime_integration`'s doc comment). No
    // separate enabled/disabled state to read.
    let registry = match IntegrationRegistry::builtin(&home) {
        Ok(registry) => registry,
        Err(error) => die(&format!(
            "cannot compose the integration registry ({error}); refusing to guess at a real \
             `{shim_name}` to avoid a possible shim loop"
        )),
    };
    let integration = registry.by_shim_name(shim_name);

    // Resolve the real binary under the invoked name first, falling back to
    // any alternate names the integration declares (e.g. OpenCode's v2
    // installer names its binary `opencode2`, not `opencode`) — this is what
    // lets the shim dispatch to a differently-named real executable without
    // a physical alias file ever being created outside `$UZE_HOME`.
    let mut candidates = vec![shim_name];
    if let Some(integration) = &integration {
        candidates.extend(integration.runtime_executable_aliases());
    }
    let executable = match harness_runtime::resolve_real_executable(&candidates, &home.shims_dir())
    {
        Some(path) => path,
        None => die(&format!(
            "no real `{shim_name}` executable found on PATH outside {} — is it installed?",
            home.shims_dir().display()
        )),
    };

    if bypass {
        exec_or_die(
            &executable,
            &original_args,
            &HarnessRuntimeContribution::passthrough(),
            shim_name,
        );
    }

    let contribution = match &integration {
        Some(integration) => {
            let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            integration.runtime_contribution(&RuntimeContext {
                cwd: &cwd,
                home: &home,
            })
        }
        None => HarnessRuntimeContribution::passthrough(),
    };

    if let Some(note) = &contribution.note {
        eprintln!(
            "uze: runtime projection unavailable ({note}); launching {shim_name} without \
             portable context."
        );
    }

    exec_or_die(&executable, &original_args, &contribution, shim_name);
}

/// Exact argv passthrough: `contribution.extra_args` are prepended before
/// the caller's original argv (argv[1..] as this process received it,
/// untouched — never reparsed). Environment additions are applied on top of
/// the inherited environment; nothing is cleared. On Unix this replaces the
/// process image via `exec`, so the real binary inherits this process's
/// stdin/stdout/stderr, controlling terminal, and pid directly — PTY,
/// signals (Ctrl+C), and exit code all fall out of that for free, which is
/// exactly why `exec` is used instead of spawn-and-wait.
///
/// `UZE_SHIM_NAME` is stamped unconditionally (even under `UZE_BYPASS`,
/// which only skips `contribution` — this is identity bookkeeping, not
/// runtime projection): the persistent terminal workspace (`uze-terminal`)
/// reads it back from the launched process's live environment to recognize
/// an agent pane, since a harness is free to overwrite its own `comm` (e.g.
/// Claude Code sets its process title to its version string) in a way that
/// erases the name a person actually typed.
fn exec_or_die(
    executable: &Path,
    original_args: &[OsString],
    contribution: &HarnessRuntimeContribution,
    shim_name: &str,
) -> ! {
    let mut command = std::process::Command::new(executable);
    command.args(&contribution.extra_args);
    command.args(original_args);
    command.env("UZE_SHIM_NAME", shim_name);
    for (key, value) in &contribution.extra_env {
        command.env(key, value);
    }
    run_replacing_process(command, executable)
}

#[cfg(unix)]
fn run_replacing_process(mut command: std::process::Command, executable: &Path) -> ! {
    use std::os::unix::process::CommandExt;
    // `exec` only returns on failure.
    let error = command.exec();
    die(&format!(
        "failed to exec `{}`: {error}",
        executable.display()
    ));
}

/// Non-Unix fallback: `exec`-style process replacement has no equivalent in
/// `std` there, so this spawns and waits, forwarding the exit code. Not the
/// primary, empirically-verified path — Windows/WSL is explicitly deferred;
/// this only keeps the shim from being
/// Unix-only at compile time.
#[cfg(not(unix))]
fn run_replacing_process(mut command: std::process::Command, executable: &Path) -> ! {
    match command.status() {
        Ok(status) => std::process::exit(status.code().unwrap_or(1)),
        Err(error) => die(&format!(
            "failed to launch `{}`: {error}",
            executable.display()
        )),
    }
}

fn die(message: &str) -> ! {
    eprintln!("uze: {message}");
    std::process::exit(127);
}
