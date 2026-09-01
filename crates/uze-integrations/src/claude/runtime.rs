//! `EXPERIMENTAL RUNTIME DELIVERY STRATEGY` for Claude Code — projecting a
//! project's `AGENTS.md`, `.agents/skills/`, and `.agents/agents/` into the
//! session via `--add-dir`, entirely outside the project's own working
//! tree. See `ClaudeIntegration::runtime_contribution`.
//!
//! Which project this is, and which of those resources it actually has,
//! is `uze_core::project_context`'s single answer — never an upward walk of
//! this module's own. Each resource projects independently: a project with
//! only `.agents/skills/` and no `AGENTS.md` still gets its Skills
//! delivered.

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use uze_core::{
    harness_runtime::{self, HarnessRuntimeContribution, RuntimeContext},
    project_context::{self, ProjectContext},
};

/// The vendor-documented (but undocumented-in-`--help`, empirically
/// confirmed) environment variable that makes
/// Claude Code treat a `--add-dir` directory's `CLAUDE.md` as loaded
/// instructions rather than only granting tool/file access to it.
pub(super) const RUNTIME_PROJECTION_ENV_VAR: &str = "CLAUDE_CODE_ADDITIONAL_DIRECTORIES_CLAUDE_MD";

/// `EXPERIMENTAL RUNTIME DELIVERY STRATEGY` — see `CONTEXT DELIVERY POLICY`
/// note on `runtime_contribution` below. This is intentionally not wired
/// into `exposure_plan`/`attach` (the persistent, project-root `CLAUDE.md`
/// bridge that `uze context reconcile` still owns — that remains the
/// `LEGACY/PERSISTENT CONTEXT DELIVERY STRATEGY` until an empirical
/// interactive comparison decides otherwise).
///
/// Builds (or refreshes) `$UZE_HOME/runtime/claude-code/projects/<id>/
/// CLAUDE.md` importing the current project's `AGENTS.md`, entirely outside
/// the project's own working tree. Returns `Ok(None)` when `ctx.cwd` is not
/// inside a project carrying any portable context at all — that is not an
/// error, it is the correct passthrough case. Every fallible step returns `Err`
/// with a short, human-readable reason instead of a typed `UzeError`,
/// because the only thing the caller (`runtime_contribution`) ever does
/// with it is fold it into a fail-open passthrough note.
pub(super) fn claude_runtime_projection(
    ctx: &RuntimeContext,
) -> std::result::Result<Option<PathBuf>, String> {
    let context = project_context::resolve(ctx.cwd);
    if !context.has_any() {
        return Ok(None);
    }
    let project_id = harness_runtime::project_id_for(&context.root);
    let runtime_dir = ctx.home.runtime_projection_dir("claude-code", &project_id);
    fs::create_dir_all(&runtime_dir).map_err(|error| error.to_string())?;

    project_instruction_projection(&context, &runtime_dir)?;
    for resource in project_context::AGENTS_DIRECTORY_RESOURCES {
        project_resource_projection(&context, &runtime_dir, resource)?;
    }

    Ok(Some(runtime_dir))
}

/// Writes (or clears) the projected `CLAUDE.md` that imports the project's
/// `AGENTS.md`. Independent of the `.agents/` projection below: a project
/// carrying only Skills still gets a runtime directory, it just gets one
/// with no instruction import in it.
fn project_instruction_projection(
    context: &ProjectContext,
    runtime_dir: &Path,
) -> std::result::Result<(), String> {
    let claude_md = runtime_dir.join("CLAUDE.md");
    let Some(agents_md) = context.agents_md.as_ref() else {
        // A project that dropped its AGENTS.md must stop importing the
        // path that used to hold it, rather than leave a dangling `@`
        // import Claude Code would report as a missing file every launch.
        match fs::remove_file(&claude_md) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
        return Ok(());
    };
    let desired = format!("@{}\n", agents_md.display());
    // Idempotent: two concurrent sessions on the same project compute the
    // same `project_id`, the same `runtime_dir`, and the same content —
    // there is nothing to reference-count or coordinate. A same-content
    // write is skipped entirely so a second session never even touches the
    // file the first one is using.
    let already_current = fs::read_to_string(&claude_md)
        .map(|current| current == desired)
        .unwrap_or(false);
    if !already_current {
        uze_core::persistence::write_atomic(&claude_md, desired.as_bytes())
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Codex, OpenCode, and Antigravity each read a project's `.agents/skills/`
/// (and, per the same convention, `.agents/agents/` for subagents) directly,
/// on their own (see `IntegrationPort::discovers_project_agents_directory`
/// for the vendor-doc citations); Claude Code has no such convention of its
/// own. What it does have, confirmed against its current behavior, is
/// automatic discovery of `.claude/skills/` *and* `.claude/agents/` inside
/// any `--add-dir` target — the same flag this module already passes for
/// `CLAUDE.md`, no extra flag or env var needed for either. So this mirrors
/// the project's `.agents/<resource>/` at `<runtime_dir>/.claude/<resource>`
/// as a single directory symlink per resource kind, refreshed idempotently:
/// Claude Code ends up discovering the same Skills and Subagents the other
/// three harnesses already do, without UZE ever writing into the project
/// itself.
fn project_resource_projection(
    context: &ProjectContext,
    runtime_dir: &Path,
    resource: &str,
) -> std::result::Result<(), String> {
    let project_source = context
        .agents_directory
        .as_ref()
        .map(|directory| directory.join(resource));
    let projected = runtime_dir.join(".claude").join(resource);

    let Some(project_source) = project_source.filter(|path| path.is_dir()) else {
        // Nothing to project — remove a stale link left over from a
        // project that used to have `.agents/<resource>/` but no longer
        // does, so a dangling reference never lingers silently. `NotFound`
        // is tolerated: a concurrent projection may have already removed
        // it between the `is_symlink` check and this call.
        if projected.is_symlink() {
            match fs::remove_file(&projected) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.to_string()),
            }
        }
        return Ok(());
    };

    let already_current = fs::read_link(&projected).is_ok_and(|target| target == project_source);
    if already_current {
        return Ok(());
    }

    let parent = projected
        .parent()
        .ok_or_else(|| format!("projected {resource} path has no parent directory"))?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;

    // Link into place through a unique temp name and an atomic `rename`,
    // never remove-then-symlink: two concurrent projections (a support
    // refresh racing a real shim launch, or two attached sessions) can both
    // pass the `already_current` check above and collide on `symlink`
    // (EEXIST), which degrades the whole contribution to passthrough — the
    // support popup then reports the harness as unavailable, and a real
    // launch drops `--add-dir` along with `AGENTS.md`. `rename` replaces
    // the previous link atomically and both writers converge on the same
    // target either way. Same nonce pattern `write_atomic` uses.
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_nanos();
    let temporary = parent.join(format!(".{}.{}.{nonce}.tmp", resource, std::process::id()));
    symlink_dir(&project_source, &temporary).inspect_err(|_| {
        let _ = fs::remove_file(&temporary);
    })?;
    if let Err(error) = fs::rename(&temporary, &projected) {
        let _ = fs::remove_file(&temporary);
        return Err(error.to_string());
    }
    Ok(())
}

#[cfg(unix)]
fn symlink_dir(source: &Path, target: &Path) -> std::result::Result<(), String> {
    std::os::unix::fs::symlink(source, target).map_err(|error| error.to_string())
}

#[cfg(not(unix))]
fn symlink_dir(source: &Path, target: &Path) -> std::result::Result<(), String> {
    Err(format!(
        "runtime project resource projection has no symlink support on this platform (would link {} -> {})",
        target.display(),
        source.display()
    ))
}

/// Pure read-only predicate backing `runtime_contribution_would_activate`:
/// whether the project carries any portable context is the only condition
/// deciding whether `claude_runtime_projection` produces a contribution —
/// the writes that follow are idempotent refreshes, not part of it. Kept
/// separate so a status computation (the agent-support popup) never
/// touches the projection directory, which is exactly where the
/// launch-path races lived.
pub(super) fn projection_would_activate(ctx: &RuntimeContext) -> bool {
    project_context::resolve(ctx.cwd).has_any()
}

/// Builds the `HarnessRuntimeContribution` from a `claude_runtime_projection`
/// outcome — the mapping shared by `ClaudeIntegration::runtime_contribution`.
pub(super) fn runtime_contribution(ctx: &RuntimeContext) -> HarnessRuntimeContribution {
    match claude_runtime_projection(ctx) {
        Ok(Some(runtime_dir)) => HarnessRuntimeContribution {
            extra_args: vec![OsString::from("--add-dir"), runtime_dir.into_os_string()],
            extra_env: vec![(
                OsString::from(RUNTIME_PROJECTION_ENV_VAR),
                OsString::from("1"),
            )],
            note: None,
        },
        Ok(None) => HarnessRuntimeContribution::passthrough(),
        Err(reason) => HarnessRuntimeContribution::passthrough_with_note(reason),
    }
}

#[cfg(test)]
mod runtime_projection_tests {
    use std::path::PathBuf;

    use uze_core::harness_runtime::{self, RuntimeContext};
    use uze_core::home::UzeHome;
    use uze_core::integration::IntegrationPort;

    use super::super::ClaudeIntegration;
    use super::RUNTIME_PROJECTION_ENV_VAR;

    fn scratch_dir(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "uze-claude-runtime-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn no_agents_md_is_pure_passthrough() {
        let root = scratch_dir("no-agents-md");
        // A `.git` boundary inside the scratch root: discovery must stop at
        // the first `.git` it finds walking up, so the test's outcome can
        // never depend on what else happens to live above the shared temp
        // dir (e.g. a stray `AGENTS.md` in `/tmp` from unrelated tooling).
        std::fs::create_dir_all(root.join(".git")).unwrap();
        let home = UzeHome::at(root.join("uze-home"));
        let ctx = RuntimeContext {
            cwd: &root,
            home: &home,
        };
        let contribution = ClaudeIntegration::new(root.join("claude-home"), home.clone())
            .runtime_contribution(&ctx);
        assert!(contribution.is_passthrough());
        assert!(contribution.note.is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn agents_md_projects_an_import_and_the_project_working_tree_stays_untouched() {
        let root = scratch_dir("with-agents-md");
        let project = root.join("project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("AGENTS.md"), "canary content\n").unwrap();
        let home = UzeHome::at(root.join("uze-home"));
        let ctx = RuntimeContext {
            cwd: &project,
            home: &home,
        };

        let contribution = ClaudeIntegration::new(root.join("claude-home"), home.clone())
            .runtime_contribution(&ctx);
        assert!(contribution.note.is_none(), "{:?}", contribution.note);
        assert_eq!(
            contribution.extra_env,
            vec![(
                std::ffi::OsString::from(RUNTIME_PROJECTION_ENV_VAR),
                std::ffi::OsString::from("1"),
            )]
        );
        assert_eq!(
            contribution.extra_args[0],
            std::ffi::OsString::from("--add-dir")
        );
        let runtime_dir = PathBuf::from(&contribution.extra_args[1]);
        assert!(runtime_dir.starts_with(home.runtime_dir()));

        let claude_md = std::fs::read_to_string(runtime_dir.join("CLAUDE.md")).unwrap();
        let canonical_agents_md = project.join("AGENTS.md").canonicalize().unwrap();
        assert_eq!(claude_md, format!("@{}\n", canonical_agents_md.display()));

        // The project's own working tree must never gain a file from this.
        let project_entries: Vec<_> = std::fs::read_dir(&project)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(project_entries, vec![std::ffi::OsString::from("AGENTS.md")]);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn repeated_projection_for_the_same_project_is_idempotent() {
        let root = scratch_dir("idempotent");
        let project = root.join("project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("AGENTS.md"), "v1\n").unwrap();
        let home = UzeHome::at(root.join("uze-home"));
        let ctx = RuntimeContext {
            cwd: &project,
            home: &home,
        };
        let integration = ClaudeIntegration::new(root.join("claude-home"), home.clone());

        let first = integration.runtime_contribution(&ctx);
        let second = integration.runtime_contribution(&ctx);
        assert_eq!(first, second, "same project must yield the same plan");

        let runtime_dir = PathBuf::from(&first.extra_args[1]);
        // Same project id both times: no second directory was created.
        let projects_dir = runtime_dir.parent().unwrap();
        let entries: Vec<_> = std::fs::read_dir(projects_dir).unwrap().collect();
        assert_eq!(entries.len(), 1);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn project_path_containing_spaces_is_handled_safely() {
        let root = scratch_dir("spaces");
        let project = root.join("a project with spaces");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("AGENTS.md"), "canary\n").unwrap();
        let home = UzeHome::at(root.join("uze-home"));
        let ctx = RuntimeContext {
            cwd: &project,
            home: &home,
        };

        let contribution = ClaudeIntegration::new(root.join("claude-home"), home.clone())
            .runtime_contribution(&ctx);
        assert!(contribution.note.is_none(), "{:?}", contribution.note);
        let runtime_dir = PathBuf::from(&contribution.extra_args[1]);
        let claude_md = std::fs::read_to_string(runtime_dir.join("CLAUDE.md")).unwrap();
        assert!(claude_md.contains("a project with spaces"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unwritable_runtime_dir_falls_open_to_passthrough_with_a_note() {
        let root = scratch_dir("unwritable");
        let project = root.join("project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("AGENTS.md"), "canary\n").unwrap();
        let home = UzeHome::at(root.join("uze-home"));

        // Occupy the exact path the projection needs as a *file* instead of
        // a directory, so `create_dir_all` fails deterministically without
        // needing real permission games.
        let agents_md = project.join("AGENTS.md").canonicalize().unwrap();
        let project_id = harness_runtime::project_id_for(agents_md.parent().unwrap());
        let blocked_path = home.runtime_projection_dir("claude-code", &project_id);
        std::fs::create_dir_all(blocked_path.parent().unwrap()).unwrap();
        std::fs::write(&blocked_path, b"not a directory").unwrap();

        let ctx = RuntimeContext {
            cwd: &project,
            home: &home,
        };
        let contribution = ClaudeIntegration::new(root.join("claude-home"), home.clone())
            .runtime_contribution(&ctx);
        assert!(contribution.is_passthrough());
        assert!(contribution.note.is_some(), "expected a fail-open note");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn project_skills_and_agents_are_symlinked_into_the_runtime_dir() {
        let root = scratch_dir("skills");
        let project = root.join("project");
        let skills_dir = project.join(".agents").join("skills").join("demo-skill");
        let agents_dir = project.join(".agents").join("agents");
        std::fs::create_dir_all(&skills_dir).unwrap();
        std::fs::create_dir_all(&agents_dir).unwrap();
        std::fs::write(skills_dir.join("SKILL.md"), "canary skill\n").unwrap();
        std::fs::write(agents_dir.join("reviewer.md"), "canary agent\n").unwrap();
        std::fs::write(project.join("AGENTS.md"), "canary\n").unwrap();
        let home = UzeHome::at(root.join("uze-home"));
        let ctx = RuntimeContext {
            cwd: &project,
            home: &home,
        };

        let contribution = ClaudeIntegration::new(root.join("claude-home"), home.clone())
            .runtime_contribution(&ctx);
        assert!(contribution.note.is_none(), "{:?}", contribution.note);
        let runtime_dir = PathBuf::from(&contribution.extra_args[1]);

        let projected_skills = runtime_dir.join(".claude").join("skills");
        assert!(projected_skills.is_symlink());
        assert_eq!(
            std::fs::read_link(&projected_skills).unwrap(),
            project
                .join(".agents")
                .join("skills")
                .canonicalize()
                .unwrap()
        );
        let skill_body =
            std::fs::read_to_string(projected_skills.join("demo-skill").join("SKILL.md")).unwrap();
        assert_eq!(skill_body, "canary skill\n");

        let projected_agents = runtime_dir.join(".claude").join("agents");
        assert!(projected_agents.is_symlink());
        assert_eq!(
            std::fs::read_link(&projected_agents).unwrap(),
            project
                .join(".agents")
                .join("agents")
                .canonicalize()
                .unwrap()
        );
        let agent_body = std::fs::read_to_string(projected_agents.join("reviewer.md")).unwrap();
        assert_eq!(agent_body, "canary agent\n");

        // The project's own working tree must never gain a file from this.
        let project_entries = std::fs::read_dir(&project)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            project_entries,
            [".agents", "AGENTS.md"]
                .into_iter()
                .map(std::ffi::OsString::from)
                .collect::<std::collections::BTreeSet<_>>()
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn no_project_agents_directory_projects_no_symlinks() {
        let root = scratch_dir("no-agents-dir");
        let project = root.join("project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("AGENTS.md"), "canary\n").unwrap();
        let home = UzeHome::at(root.join("uze-home"));
        let ctx = RuntimeContext {
            cwd: &project,
            home: &home,
        };

        let contribution = ClaudeIntegration::new(root.join("claude-home"), home.clone())
            .runtime_contribution(&ctx);
        assert!(contribution.note.is_none(), "{:?}", contribution.note);
        let runtime_dir = PathBuf::from(&contribution.extra_args[1]);
        assert!(!runtime_dir.join(".claude").join("skills").exists());
        assert!(!runtime_dir.join(".claude").join("agents").exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_removed_project_agents_directory_drops_stale_symlinks() {
        let root = scratch_dir("removed-agents-dir");
        let project = root.join("project");
        let skills_dir = project.join(".agents").join("skills").join("demo-skill");
        let agents_dir = project.join(".agents").join("agents");
        std::fs::create_dir_all(&skills_dir).unwrap();
        std::fs::create_dir_all(&agents_dir).unwrap();
        std::fs::write(skills_dir.join("SKILL.md"), "canary skill\n").unwrap();
        std::fs::write(agents_dir.join("reviewer.md"), "canary agent\n").unwrap();
        std::fs::write(project.join("AGENTS.md"), "canary\n").unwrap();
        let home = UzeHome::at(root.join("uze-home"));
        let ctx = RuntimeContext {
            cwd: &project,
            home: &home,
        };
        let integration = ClaudeIntegration::new(root.join("claude-home"), home.clone());

        let first = integration.runtime_contribution(&ctx);
        let runtime_dir = PathBuf::from(&first.extra_args[1]);
        let projected_skills = runtime_dir.join(".claude").join("skills");
        let projected_agents = runtime_dir.join(".claude").join("agents");
        assert!(projected_skills.is_symlink());
        assert!(projected_agents.is_symlink());

        std::fs::remove_dir_all(project.join(".agents")).unwrap();
        let second = integration.runtime_contribution(&ctx);
        assert!(second.note.is_none(), "{:?}", second.note);
        assert!(!projected_skills.exists() && !projected_skills.is_symlink());
        assert!(!projected_agents.exists() && !projected_agents.is_symlink());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn status_projection_predicate_is_read_only_and_agrees() {
        let root = scratch_dir("status-read-only");
        let project = root.join("project");
        let skills_dir = project.join(".agents").join("skills");
        std::fs::create_dir_all(&skills_dir).unwrap();
        // A `.git` boundary pins the resolved project root to `project`
        // itself, so nothing above the shared temp directory can change
        // what this test resolves.
        std::fs::create_dir_all(project.join(".git")).unwrap();
        std::fs::write(project.join("AGENTS.md"), "canary\n").unwrap();
        let home = UzeHome::at(root.join("uze-home"));
        let ctx = RuntimeContext {
            cwd: &project,
            home: &home,
        };
        let integration = ClaudeIntegration::new(root.join("claude-home"), home.clone());

        // The status predicate agrees with the real contribution's decision
        // for a healthy project...
        assert!(integration.runtime_contribution_would_activate(&ctx));

        // ...but a status *read* performs no writes: no projection
        // directory, no CLAUDE.md, no links — nothing the popup does may
        // ever touch the runtime tree (that was the launch-path race
        // source: status recomputation writing the projection). The
        // read-only assertion must come before the real contribution,
        // which is allowed — and expected — to write.
        let runtime_dir =
            home.runtime_projection_dir("claude-code", &harness_runtime::project_id_for(&project));
        assert!(!runtime_dir.exists());
        assert!(!integration.runtime_contribution(&ctx).is_passthrough());

        // Losing AGENTS.md does not switch the projection off: `.agents/`
        // is delivered on its own terms, so the Skills this project has
        // keep reaching the session. Only the instruction import goes away,
        // and the stale `CLAUDE.md` importing the removed file is cleared
        // rather than left behind as a dangling `@` import.
        std::fs::remove_file(project.join("AGENTS.md")).unwrap();
        assert!(integration.runtime_contribution_would_activate(&ctx));
        assert!(!integration.runtime_contribution(&ctx).is_passthrough());
        assert!(!runtime_dir.join("CLAUDE.md").exists());
        assert!(runtime_dir.join(".claude").join("skills").is_symlink());

        // Nothing portable left at all → inactive, matching the real
        // contribution's passthrough.
        std::fs::remove_dir_all(project.join(".agents")).unwrap();
        assert!(!integration.runtime_contribution_would_activate(&ctx));
        assert!(integration.runtime_contribution(&ctx).is_passthrough());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn concurrent_projection_calls_never_degrade_to_passthrough() {
        // The TUI refreshes agent support from a background thread on every
        // dropdown open and at attach, a real `claude` launch through the
        // shim projects at the same time, and a second attached session
        // adds a third concurrent writer — all racing this projection's
        // first-ever creation for a project. The old remove-then-symlink
        // sequence made the losers collide on `symlink` with EEXIST and
        // fall back to passthrough (popup shows "unavailable", launch loses
        // `--add-dir`). The barrier starts every racer from the raw,
        // unprojected state, which is exactly that window.
        let root = scratch_dir("concurrent-projection");
        let project = root.join("project");
        let skills_dir = project.join(".agents").join("skills").join("demo-skill");
        let agents_dir = project.join(".agents").join("agents");
        std::fs::create_dir_all(&skills_dir).unwrap();
        std::fs::create_dir_all(&agents_dir).unwrap();
        std::fs::write(skills_dir.join("SKILL.md"), "canary skill\n").unwrap();
        std::fs::write(agents_dir.join("reviewer.md"), "canary agent\n").unwrap();
        std::fs::write(project.join("AGENTS.md"), "canary\n").unwrap();
        let home = UzeHome::at(root.join("uze-home"));
        let ctx = RuntimeContext {
            cwd: &project,
            home: &home,
        };
        let integration = ClaudeIntegration::new(root.join("claude-home"), home.clone());

        let racers = 8;
        let barrier = std::sync::Barrier::new(racers);
        let contributions: Vec<_> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..racers)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        integration.runtime_contribution(&ctx)
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect()
        });

        for contribution in &contributions {
            assert!(
                !contribution.is_passthrough(),
                "a racing projection must never degrade to passthrough: {contribution:?}"
            );
            assert!(
                contribution.note.is_none(),
                "a racing projection must not fail open with a note: {contribution:?}"
            );
        }

        // Every racer converged on one consistent projection: both links
        // intact and pointing at the project, no temp names left behind.
        let runtime_dir = PathBuf::from(&contributions[0].extra_args[1]);
        let projected_skills = runtime_dir.join(".claude").join("skills");
        let projected_agents = runtime_dir.join(".claude").join("agents");
        let expected_skills = project
            .join(".agents")
            .join("skills")
            .canonicalize()
            .unwrap();
        let expected_agents = project
            .join(".agents")
            .join("agents")
            .canonicalize()
            .unwrap();
        assert_eq!(
            std::fs::read_link(&projected_skills).unwrap(),
            expected_skills
        );
        assert_eq!(
            std::fs::read_link(&projected_agents).unwrap(),
            expected_agents
        );
        let projected_entries: Vec<_> = std::fs::read_dir(runtime_dir.join(".claude"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        let mut projected_entries = projected_entries;
        projected_entries.sort();
        assert_eq!(
            projected_entries,
            ["agents", "skills"]
                .into_iter()
                .map(std::ffi::OsString::from)
                .collect::<Vec<_>>()
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
