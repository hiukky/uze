//! Claude Code Agent Skill exposure — the managed skills-dir plugin shim
//! (see ADR-006): a small owned manifest plus a `SKILL.md` reference,
//! symlinked once into `<claude_home>/skills/<name>`. Invocation policy is
//! translated into Claude's own SKILL.md frontmatter fields
//! (`disable-model-invocation`, `user-invocable`) — see ADR-030.

use std::{fs, path::Path};

use uze_core::{
    Result, UzeError,
    capability::Resource,
    exposure::{ExposureMechanism, ExposurePlan, ManagedArtifact},
    integration::IntegrationPort,
    router::CompatibilityRoute,
    skill::SkillInvocationPolicy,
    state,
};

use super::ClaudeIntegration;
use crate::shared::skill::{
    entry_name, frontmatter_value, invalid_policy_plan, link_extras, link_or_repair,
    render_skill_wrapper, setup_pending_plan, write_file,
};

impl ClaudeIntegration {
    /// Claude's shim is a one-skill plugin, so its proof of ownership is
    /// the plugin manifest beside the `SKILL.md` — the shim root is the
    /// vendor root itself, not a `skills` directory inside it.
    pub(super) fn cleanup_unused_wrapper(&self, shim_root: &Path) -> Result<()> {
        let managed_root = crate::shared::path::attachment_root(&self.uze_home, "claude");
        crate::shared::path::cleanup_unused_wrapper(
            shim_root,
            &managed_root,
            &self.skills_dir,
            &managed_root,
            &|shim| {
                let skill = shim.join("SKILL.md");
                shim.join(".claude-plugin/plugin.json").is_file()
                    && (skill.is_symlink() || skill.is_file())
            },
        )
    }

    pub(super) fn skill_exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        let policy = resource.skill_invocation();
        if policy.is_invalid() {
            return invalid_policy_plan();
        }
        if !state::is_installed(&self.uze_home, self.id()) {
            return setup_pending_plan("Claude Code");
        }
        let Some(entry_name) = entry_name(self, resource) else {
            return setup_pending_plan("Claude Code");
        };
        let shim_root = resource
            .resolved_artifact_target
            .clone()
            .unwrap_or_else(|| {
                crate::shared::path::attachment_root(&self.uze_home, "claude").join(&entry_name)
            });
        let mut evidence = String::from(
            "UZE materializes a small owned manifest shim (.claude-plugin/plugin.json plus a SKILL.md reference into the UZE store) and symlinks it once into <claude_home>/skills/. Claude auto-loads it on every future session with no --plugin-dir flag.",
        );
        if !policy.is_default() {
            evidence.push_str(
                " The canonical invoke policy is translated into Claude's own frontmatter (disable-model-invocation: true when model=false, user-invocable: false when user=false) without touching the canonical Store bytes.",
            );
        }
        // Every valid policy is carried by Claude's own frontmatter markers,
        // on a shim UZE generates rather than the Store bytes.
        ExposurePlan {
            route: CompatibilityRoute::Adaptable,
            mechanism: ExposureMechanism::Managed(ManagedArtifact::SymlinkReference {
                path: self.skills_dir.join(entry_name),
                target: shim_root,
            }),
            evidence,
        }
    }
}

/// Materializes the UZE-owned shim directory: the small plugin manifest
/// plus a `SKILL.md` that is either a symlink to the canonical Store bytes
/// (default model+user policy — byte-preserving) or a UZE-generated file
/// carrying the canonical name/description/body plus Claude's own
/// invocation markers (non-default policy — the canonical bytes stay in
/// the Store; this wrapper is a Derived Artifact, ADR-013 §5).
pub(super) fn materialize_shim(
    shim_root: &Path,
    canonical_skill_dir: &Path,
    entry_name: &str,
    namespace: Option<&str>,
    policy: &SkillInvocationPolicy,
) -> Result<()> {
    let plugin_dir = shim_root.join(".claude-plugin");
    fs::create_dir_all(&plugin_dir).map_err(|source| UzeError::Write {
        path: plugin_dir.clone(),
        source,
    })?;
    // The manifest plugin `name` is what Claude uses to namespace
    // components (plugins-reference: "This name is used for namespacing
    // components"), so it must be the *namespace* (`flow`) while the shim
    // directory carries the full stable label (`flow:review`). Together
    // with the skill's own frontmatter `name` (`review`) Claude exposes
    // `/flow:review` — never `/flow:flow:review` (ADR-026). A canonical
    // SKILL.md without a `name` field relies on Claude's directory-name
    // fallback for the skill name; packages should ship `name` frontmatter
    // (documented residual risk).
    let plugin_name = namespace.unwrap_or(entry_name);
    let manifest = serde_json::json!({
        "$schema": "https://anthropic.com/claude-code/plugin.schema.json",
        "name": plugin_name,
        "version": "0.1.0",
        "description": "UZE-managed skill, referencing the UZE store.",
        "skills": ["./"],
    });
    fs::write(
        plugin_dir.join("plugin.json"),
        serde_json::to_vec_pretty(&manifest).expect("plugin manifest serialization is infallible"),
    )
    .map_err(|source| UzeError::Write {
        path: plugin_dir.join("plugin.json"),
        source,
    })?;

    // Claude resolves a skill's relative scripts and references against the
    // shim, not against the Store, so they are linked beside its SKILL.md.
    link_extras(canonical_skill_dir, shim_root, &[".claude-plugin"])?;
    let skill_link = shim_root.join("SKILL.md");
    if policy.is_default() {
        let skill_source = canonical_skill_dir.join("SKILL.md");
        return link_or_repair(&skill_link, &skill_source);
    }
    // Non-default policy: the delivered SKILL.md must carry Claude's own
    // frontmatter markers, so it is materialized — never a symlink.
    let bytes = fs::read(canonical_skill_dir.join("SKILL.md")).map_err(|error| UzeError::Read {
        path: canonical_skill_dir.join("SKILL.md"),
        source: error,
    })?;
    let fallback_name = canonical_skill_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(entry_name);
    let document = claude_wrapper_skill_document(&bytes, policy, fallback_name);
    if skill_link.is_symlink() {
        fs::remove_file(&skill_link).map_err(|source| UzeError::Write {
            path: skill_link.clone(),
            source,
        })?;
    }
    write_file(&skill_link, document.as_bytes())
}

/// Renders the generated SKILL.md for one canonical Skill under a
/// non-default invocation policy: the canonical `name` and description are
/// preserved and Claude's own invocation markers injected. Used by both the
/// capability shim and the generated native package envelope.
pub(super) fn claude_wrapper_skill_document(
    canonical_bytes: &[u8],
    policy: &SkillInvocationPolicy,
    fallback_name: &str,
) -> String {
    let name =
        frontmatter_value(canonical_bytes, "name").unwrap_or_else(|| fallback_name.to_owned());
    let mut markers = Vec::new();
    if !policy.model {
        markers.push("disable-model-invocation: true");
    }
    if !policy.user {
        markers.push("user-invocable: false");
    }
    render_skill_wrapper(&name, canonical_bytes, &markers)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A skill's relative scripts must resolve from the shim Claude loads,
    /// whichever policy decides how its `SKILL.md` is delivered.
    #[test]
    fn the_shim_links_the_skills_supporting_files_beside_its_skill_md() {
        for (label, body) in [
            (
                "claude-shim-default",
                "---\nname: deploy\n---\nRun scripts/run.sh.\n",
            ),
            (
                "claude-shim-user-only",
                "---\nname: deploy\ninvoke:\n  model: false\n  user: true\n---\nRun scripts/run.sh.\n",
            ),
        ] {
            let root = uze_testkit::temp::scratch(label);
            let canonical = root.join("store/skills/deploy");
            fs::create_dir_all(canonical.join("scripts")).unwrap();
            fs::write(canonical.join("SKILL.md"), body).unwrap();
            fs::write(canonical.join("scripts/run.sh"), "#!/bin/sh\n").unwrap();
            let shim = root.join("shim");
            let policy = uze_core::skill::parse_skill_invocation(body.as_bytes())
                .unwrap_or(SkillInvocationPolicy::MODEL_AND_USER);
            materialize_shim(&shim, &canonical, "flow:deploy", Some("flow"), &policy).unwrap();
            assert!(shim.join("scripts").is_symlink(), "{label}");
            assert!(shim.join("scripts/run.sh").is_file(), "{label}");
            assert!(shim.join("SKILL.md").exists(), "{label}");
            let _ = fs::remove_dir_all(root);
        }
    }
}
