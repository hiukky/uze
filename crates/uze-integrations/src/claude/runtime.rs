//! `EXPERIMENTAL RUNTIME DELIVERY STRATEGY` for Claude Code — projecting a
//! project's `AGENTS.md`, `.agents/skills/`, and `.agents/agents/` into the
//! session via `--add-dir`, entirely outside the project's own working
//! tree. See `ClaudeIntegration::runtime_contribution`.

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use uze_core::harness_runtime::{self, HarnessRuntimeContribution, RuntimeContext};

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
/// inside a project that has an `AGENTS.md` at all — that is not an error,
/// it is the correct passthrough case. Every fallible step returns `Err`
/// with a short, human-readable reason instead of a typed `UzeError`,
/// because the only thing the caller (`runtime_contribution`) ever does
/// with it is fold it into a fail-open passthrough note.
pub(super) fn claude_runtime_projection(
    ctx: &RuntimeContext,
) -> std::result::Result<Option<PathBuf>, String> {
    let Some(agents_md) = harness_runtime::discover_project_agents_md(ctx.cwd) else {
        return Ok(None);
    };
    let agents_md = agents_md
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let project_root = agents_md
        .parent()
        .ok_or_else(|| "AGENTS.md has no parent directory".to_owned())?;
    let project_id = harness_runtime::project_id_for(project_root);
    let runtime_dir = ctx.home.runtime_projection_dir("claude-code", &project_id);
    fs::create_dir_all(&runtime_dir).map_err(|error| error.to_string())?;

    let claude_md = runtime_dir.join("CLAUDE.md");
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

    project_resource_projection(project_root, &runtime_dir, "skills")?;
    project_resource_projection(project_root, &runtime_dir, "agents")?;

    Ok(Some(runtime_dir))
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
    project_root: &Path,
    runtime_dir: &Path,
    resource: &str,
) -> std::result::Result<(), String> {
    let project_source = project_root.join(".agents").join(resource);
    let projected = runtime_dir.join(".claude").join(resource);

    if !project_source.is_dir() {
        // Nothing to project — remove a stale link left over from a
        // project that used to have `.agents/<resource>/` but no longer
        // does, so a dangling reference never lingers silently.
        if projected.is_symlink() {
            fs::remove_file(&projected).map_err(|error| error.to_string())?;
        }
        return Ok(());
    }

    let already_current = fs::read_link(&projected).is_ok_and(|target| target == project_source);
    if already_current {
        return Ok(());
    }

    let parent = projected
        .parent()
        .ok_or_else(|| format!("projected {resource} path has no parent directory"))?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;

    match projected.symlink_metadata() {
        Ok(_) => fs::remove_file(&projected).map_err(|error| error.to_string())?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }

    symlink_dir(&project_source, &projected)
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
}
