//! `EXPERIMENTAL RUNTIME DELIVERY STRATEGY` for Claude Code — projecting a
//! project's `AGENTS.md` into the session via `--add-dir`, entirely outside
//! the project's own working tree. See `ClaudeIntegration::runtime_contribution`.

use std::{ffi::OsString, fs, path::PathBuf};

use uze_core::harness_runtime::{self, HarnessRuntimeContribution, RuntimeContext};

/// The vendor-documented (but undocumented-in-`--help`, empirically
/// confirmed — see the Checkpoint report) environment variable that makes
/// Claude Code treat a `--add-dir` directory's `CLAUDE.md` as loaded
/// instructions rather than only granting tool/file access to it.
pub(super) const RUNTIME_PROJECTION_ENV_VAR: &str = "CLAUDE_CODE_ADDITIONAL_DIRECTORIES_CLAUDE_MD";

/// `EXPERIMENTAL RUNTIME DELIVERY STRATEGY` — see `CONTEXT DELIVERY POLICY`
/// note on `runtime_contribution` below. This is intentionally not wired
/// into `exposure_plan`/`attach` (the persistent, project-root `CLAUDE.md`
/// bridge that `uze context reconcile` still owns, see
/// `uze-application::application::BRIDGE_INTEGRATIONS` — that remains the
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
    Ok(Some(runtime_dir))
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
}
