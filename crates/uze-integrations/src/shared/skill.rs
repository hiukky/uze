//! Shared Skill-frontmatter reading and generation for integrations that
//! cannot deliver the canonical `skills/<name>/SKILL.md` bytes as-is and
//! must translate them into a vendor representation (Claude's
//! `disable-model-invocation`/`user-invocable` fields, Codex's
//! `agents/openai.yaml` policy sidecar, OpenCode's
//! `metadata.opencode/*` controls).
//!
//! The canonical Skill model (ADR-030) is deliberately minimal: an optional
//! YAML-style frontmatter block whose only UZE-consumed fields are
//! `description` and the `invoke:` invocation policy, followed by the body.
//! Parsing here is best-effort and never fatal — a body that does not parse
//! as expected frontmatter is treated as body-only, which is always
//! deliverable. The canonical bytes themselves remain the Store's payload;
//! this module only reads them, and generated wrappers are always Derived
//! Artifacts under `$UZE_HOME`, never Store writes.
//!
//! The vendor fields below are deliberately NOT canonical UZE metadata:
//! they exist only to translate the canonical `invoke:` policy into each
//! harness's own surface (section 5 — no vendor field names in the
//! canonical model).
//!
//! The one shared-root wrapper this module also writes
//! ([`write_superset_skill_wrapper`]) is different by necessity: it lives
//! in a directory Codex and OpenCode consume *together*, so its bytes are
//! the superset of both vendors' encodings — never a canonical rewrite.

use std::{
    fs,
    path::{Path, PathBuf},
};

use uze_core::{
    Result, UzeError,
    capability::Resource,
    exposure::{ExposureMechanism, ExposurePlan},
    home::UzeHome,
    integration::{IntegrationPort, active_plugin_name, qualified_exposure_name_candidates},
    router::CompatibilityRoute,
    skill::SkillInvocationPolicy,
};

/// A Skill that nobody may invoke is never projected (ADR-030 §1).
pub(crate) fn invalid_policy_plan() -> ExposurePlan {
    ExposurePlan {
        route: CompatibilityRoute::Unsupported,
        mechanism: ExposureMechanism::Unsupported {
            rationale: "This Skill declares invoke.model: false and invoke.user: false — nobody can invoke it, so UZE never projects it. Fix the `invoke:` block in SKILL.md.".to_owned(),
        },
        evidence: "Invalid canonical invocation policy: a Skill that nobody may invoke is not a projectable capability (ADR-030 §1).".to_owned(),
    }
}

/// A Skill reaches `harness` through a managed user-scope attachment, which
/// exists only once `uze setup` has completed.
pub(crate) fn setup_pending_plan(harness: &str) -> ExposurePlan {
    ExposurePlan {
        route: CompatibilityRoute::Unsupported,
        mechanism: ExposureMechanism::Unsupported {
            rationale: format!(
                "{harness} has not completed `uze setup`; run `uze setup` so UZE can attach this Skill."
            ),
        },
        evidence: format!(
            "Skills reach {harness} through a managed user-scope attachment, which exists only once `uze setup` has completed."
        ),
    }
}

/// The physical entry name for `resource`: the one shared-root resolution
/// settled on, else the integration's own first candidate.
pub(crate) fn entry_name(integration: &dyn IntegrationPort, resource: &Resource) -> Option<String> {
    resource.resolved_exposure_name.clone().or_else(|| {
        integration
            .exposure_name_candidates(resource)
            .into_iter()
            .next()
    })
}

/// The stable namespaced invocation label (`flow:review`, ADR-026) a
/// generated wrapper declares as its `name`.
pub(crate) fn skill_label(uze_home: &UzeHome, resource: &Resource) -> Option<String> {
    qualified_exposure_name_candidates(resource, &active_plugin_name(uze_home, resource))
        .into_iter()
        .next()
}

/// Where `vendor` generates one Skill's wrapper, under UZE's own state and
/// never under the Store: `<attachments>/<vendor>/skills/<package>/<skill>`.
pub(crate) fn generated_skill_dir(
    uze_home: &UzeHome,
    vendor: &str,
    resource: &Resource,
) -> PathBuf {
    let package_id = resource
        .package_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown");
    let name = resource
        .logical_capability_name()
        .unwrap_or_else(|| resource.name());
    skill_wrapper_root(uze_home, vendor)
        .join(package_id)
        .join(name)
}

/// The root every one of `vendor`'s generated Skill wrappers sits under.
pub(crate) fn skill_wrapper_root(uze_home: &UzeHome, vendor: &str) -> PathBuf {
    crate::shared::path::attachment_root(uze_home, vendor).join("skills")
}

/// A generated SKILL.md: `name`, the canonical description re-quoted (never
/// raw-interpolated), the vendor's own `markers` as frontmatter lines, then
/// the canonical body verbatim.
pub fn render_skill_wrapper(name: &str, canonical_bytes: &[u8], markers: &[&str]) -> String {
    let (description, body) = parse_skill_body(canonical_bytes);
    let mut document = format!("---\nname: {name}\n");
    if let Some(description) = description {
        let escaped = escape_yaml_double_quoted(&description);
        document.push_str(&format!("description: \"{escaped}\"\n"));
    }
    for marker in markers {
        document.push_str(marker);
        document.push('\n');
    }
    document.push_str("---\n");
    document.push_str(&body);
    document
}

/// The two harnesses that read the shared `~/.agents/skills` root.
pub(crate) enum SharedRootReader {
    Codex,
    OpenCode,
}

/// When shared-root resolution reused another integration's wrapper for
/// `resource`, that wrapper must still carry `reader`'s own encoding of the
/// canonical policy — otherwise it would silently degrade (ADR-030 §25).
pub(crate) fn verify_reused_wrapper(
    resource: &Resource,
    wrapper: &Path,
    skills_dir: &Path,
    reader: SharedRootReader,
    integration_id: &str,
) -> Result<()> {
    let policy = resource.skill_invocation();
    let missing = if policy.is_invalid() {
        None
    } else {
        match reader {
            SharedRootReader::Codex => (!policy.model && !has_explicit_only_sidecar(wrapper)).then_some(
                "Codex needs agents/openai.yaml with policy.allow_implicit_invocation: false for a user-only Skill",
            ),
            SharedRootReader::OpenCode => {
                let bytes = fs::read(wrapper.join("SKILL.md")).unwrap_or_default();
                if !policy.model && !has_opencode_autoinvoke_false(&bytes) {
                    Some("OpenCode needs metadata.opencode/autoinvoke: false for a user-only Skill")
                } else if !policy.user && !has_slash_false(&bytes) {
                    Some("OpenCode needs slash: false for a model-only Skill")
                } else {
                    None
                }
            }
        }
    };
    let Some(requirement) = missing else {
        return Ok(());
    };
    let entry = resource
        .resolved_exposure_name
        .as_ref()
        .map_or_else(|| wrapper.to_path_buf(), |name| skills_dir.join(name));
    Err(crate::shared::projection::conflict(
        resource,
        &entry,
        wrapper,
        requirement,
        integration_id,
    ))
}

/// Splits a canonical SKILL.md into `(description, body)`:
///
/// - A leading `---` line, then key-value lines, then a closing `---` line
///   is frontmatter; only the `description` key is consumed, everything
///   else in the block is deliberately ignored (never reinterpreted, never
///   dropped from the canonical bytes — this function only *reads*).
///   Frontmatter that is malformed falls through to "body-only".
/// - Everything after the closing marker is the body, with surrounding
///   whitespace preserved exactly as shipped (no trim, no rewrite).
pub fn parse_skill_body(bytes: &[u8]) -> (Option<String>, String) {
    let Some(text) = is_utf8(bytes) else {
        return (None, String::new());
    };
    let Some((head, body)) = split_frontmatter(text.strip_prefix('\u{feff}').unwrap_or(text))
    else {
        return (None, text.to_owned());
    };
    let description = head
        .lines()
        .rev()
        .find_map(|line| key_value(line, "description"));
    (description.map(str::to_owned), body.to_owned())
}

/// Splits a document into its `---` frontmatter block and the body after
/// it; `None` when the document does not open with a closed block.
pub fn split_frontmatter(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix("---\n")?;
    let end = rest.find("\n---\n")?;
    Some((&rest[..end], &rest[end + "\n---\n".len()..]))
}

/// The first value `key` holds in a frontmatter block, trimmed.
pub fn head_value<'a>(head: &'a str, key: &str) -> Option<&'a str> {
    head.lines().find_map(|line| key_value(line, key))
}

fn key_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let (candidate, value) = line.split_once(':')?;
    (candidate.trim() == key).then(|| value.trim())
}

/// Escapes `value` as the contents of a YAML double-quoted scalar, so it can
/// be safely written as `"{escaped}"` in generated frontmatter regardless of
/// content — colons, embedded newlines, quotes, or text shaped like another
/// frontmatter key (e.g. `disable-model-invocation: false`) can never break
/// out of the quoted value or forge/duplicate a key. Only backslash, double
/// quote, and control characters need escaping inside a double-quoted YAML
/// scalar; everything else is passed through verbatim.
pub fn escape_yaml_double_quoted(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            other if (other as u32) < 0x20 => {
                escaped.push_str(&format!("\\x{:02x}", other as u32));
            }
            other => escaped.push(other),
        }
    }
    escaped
}

/// Whether the payload's own frontmatter carries `disable-model-invocation:
/// true` — the Claude marker that keeps a Skill explicit-only rather than
/// model-invocable. Recognized only when the trimmed value is exactly
/// `true`. Malformed or absent frontmatter, or a non-UTF8 payload, is
/// `false` — never a panic, never a guess.
pub fn has_disable_model_invocation(bytes: &[u8]) -> bool {
    frontmatter_is_true(bytes, "disable-model-invocation")
}

/// Whether the payload's own frontmatter carries `user-invocable: false` —
/// the Claude marker that hides a Skill from the `/` catalog while the
/// model may still load it automatically. Same strictness as
/// [`has_disable_model_invocation`].
pub fn has_user_invocable_false(bytes: &[u8]) -> bool {
    frontmatter_value(bytes, "user-invocable").is_some_and(|value| value == "false")
}

fn frontmatter_is_true(bytes: &[u8], key: &str) -> bool {
    frontmatter_value(bytes, key).is_some_and(|value| value == "true")
}

/// Reads one top-level frontmatter value (e.g. `name`) from canonical
/// bytes, so a generated wrapper can preserve the canonically-declared
/// identity instead of inventing one. `None` for absent/malformed
/// frontmatter or non-UTF8 payload — never a guess.
pub fn frontmatter_value(bytes: &[u8], key: &str) -> Option<String> {
    let text = is_utf8(bytes)?;
    let (head, _) = split_frontmatter(text.strip_prefix('\u{feff}').unwrap_or(text))?;
    head_value(head, key).map(str::to_owned)
}

/// Whether the payload declares OpenCode's user-only control —
/// `metadata` containing `opencode/autoinvoke: false` (the documented V2
/// syntax; real-world SKILL.md files use this exact shape). Line
/// substring-based: UZE only ever inspects its own generated wrappers or
/// author bytes, and a false positive here would only tighten reuse
/// verification.
pub fn has_opencode_autoinvoke_false(bytes: &[u8]) -> bool {
    let Some(text) = is_utf8(bytes) else {
        return false;
    };
    text.lines()
        .any(|line| line.trim() == "opencode/autoinvoke: false")
}

/// Whether the payload declares OpenCode's user-invocation suppression —
/// a trimmed `slash: false` line in frontmatter.
pub fn has_slash_false(bytes: &[u8]) -> bool {
    let Some(text) = is_utf8(bytes) else {
        return false;
    };
    text.lines().any(|line| line.trim() == "slash: false")
}

/// Whether `skill_dir` carries Codex's explicit-only policy sidecar:
/// `agents/openai.yaml` declaring `allow_implicit_invocation: false`.
pub fn has_explicit_only_sidecar(skill_dir: &Path) -> bool {
    fs::read_to_string(skill_dir.join("agents/openai.yaml")).is_ok_and(|policy| {
        policy
            .lines()
            .any(|line| line.trim() == "allow_implicit_invocation: false")
    })
}

fn is_utf8(bytes: &[u8]) -> Option<&str> {
    std::str::from_utf8(bytes).ok()
}

/// Writes one shared-root Skill wrapper directory — the superset
/// representation Codex and OpenCode both consume from their single shared
/// `~/.agents/skills` physical entry:
///
/// ```text
/// <wrapper>/
/// ├── SKILL.md          stable namespaced label as `name`, canonical
/// │                      description/body, plus OpenCode's own native
/// │                      invocation controls (`opencode/autoinvoke`,
/// │                      `slash`)
/// └── agents/
///     └── openai.yaml   Codex's explicit-only policy sidecar (model=false)
/// ```
///
/// Every integration that shares the root materializes this exact content
/// under its own `$UZE_HOME` wrapper directory, so whichever wrapper the
/// shared symlink ends up pointing at, every consumer finds its own
/// encoding — and the other integration's reuse verification passes instead
/// of degrading the canonical `invoke:` policy (ADR-030 §25). The harness
/// that does not own an encoding ignores it: Codex reads `name` and the
/// policy sidecar and ignores OpenCode's frontmatter fields (verified via
/// `codex debug prompt-input` against codex-cli 0.149.1); OpenCode derives
/// the skill id from the path and ignores unknown files. The canonical
/// bytes are never rewritten; anything else in the canonical skill
/// directory stays referenced, not copied. Idempotent and rebuilt
/// wholesale — the directory is entirely UZE-owned and non-authoritative
/// (ADR-013 §5).
pub fn write_superset_skill_wrapper(
    dir: &Path,
    canonical_dir: &Path,
    canonical_bytes: &[u8],
    label: &str,
    policy: &SkillInvocationPolicy,
) -> Result<()> {
    recreate_dir(dir)?;
    let mut markers = Vec::new();
    if !policy.user {
        markers.push("slash: false");
    }
    if !policy.model {
        markers.extend(["metadata:", "  opencode/autoinvoke: false"]);
    }
    write_file(
        &dir.join("SKILL.md"),
        render_skill_wrapper(label, canonical_bytes, &markers).as_bytes(),
    )?;
    if !policy.model {
        write_explicit_only_sidecar(dir)?;
    }
    // A canonical `agents/` directory stays out: the sidecar above is the
    // encoding this wrapper owns, and an author's own `agents/openai.yaml`
    // is never re-derived into it.
    link_extras(canonical_dir, dir, &["agents"])
}

/// Codex's official invocation-policy metadata: `agents/openai.yaml` beside
/// `SKILL.md`, with `policy.allow_implicit_invocation: false`. Per Codex's
/// Build skills documentation this makes the skill *not* implicitly
/// invocable by the model while explicit `$skill` invocation still works
/// (verified against codex-cli 0.149.0 via `codex debug prompt-input`).
pub(crate) const EXPLICIT_ONLY_POLICY_YAML: &str = "policy:\n  allow_implicit_invocation: false\n";

/// Writes Codex's explicit-only policy sidecar into `skill_dir`.
pub(crate) fn write_explicit_only_sidecar(skill_dir: &Path) -> Result<()> {
    let agents = skill_dir.join("agents");
    fs::create_dir_all(&agents).map_err(|source| UzeError::Write {
        path: agents.clone(),
        source,
    })?;
    write_file(
        &agents.join("openai.yaml"),
        EXPLICIT_ONLY_POLICY_YAML.as_bytes(),
    )
}

/// Replaces `dir` with an empty directory: a generated artifact is rebuilt
/// wholesale, never patched, because nothing in it is authoritative.
pub(crate) fn recreate_dir(dir: &Path) -> Result<()> {
    if dir.exists() {
        fs::remove_dir_all(dir).map_err(|source| UzeError::Write {
            path: dir.to_path_buf(),
            source,
        })?;
    }
    fs::create_dir_all(dir).map_err(|source| UzeError::Write {
        path: dir.to_path_buf(),
        source,
    })
}

pub(crate) fn write_file(path: &Path, content: &[u8]) -> Result<()> {
    fs::write(path, content).map_err(|source| UzeError::Write {
        path: path.to_path_buf(),
        source,
    })
}

/// Points `link` at `source`, repairing a link that points elsewhere and
/// refusing to replace anything that is not a link.
pub(crate) fn link_or_repair(link: &Path, source: &Path) -> Result<()> {
    match fs::symlink_metadata(link) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            let current = fs::read_link(link).map_err(|error| UzeError::Read {
                path: link.to_path_buf(),
                source: error,
            })?;
            if current == source {
                return Ok(());
            }
            fs::remove_file(link).map_err(|error| UzeError::Write {
                path: link.to_path_buf(),
                source: error,
            })?;
            uze_core::persistence::create_symlink(source, link)
        }
        Ok(_) => Err(UzeError::ManagedEntryConflict(link.to_path_buf())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            uze_core::persistence::create_symlink(source, link)
        }
        Err(error) => Err(UzeError::Read {
            path: link.to_path_buf(),
            source: error,
        }),
    }
}

/// Links every entry of a canonical skill directory except `SKILL.md` (and
/// the names in `skip`) into a generated skill directory, so a wrapper that
/// replaces `SKILL.md` never drops the scripts and references it names by
/// relative path. An entry already present is left as it is. An absent
/// canonical directory (a Resource built without a real Store path, in
/// unit-test contexts) has no extras to link.
pub fn link_extras(canonical_dir: &Path, target_dir: &Path, skip: &[&str]) -> Result<()> {
    let entries = match fs::read_dir(canonical_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(UzeError::Read {
                path: canonical_dir.to_path_buf(),
                source,
            });
        }
    };
    for entry in entries {
        let entry = entry.map_err(|source| UzeError::Read {
            path: canonical_dir.to_path_buf(),
            source,
        })?;
        let name = entry.file_name();
        if name == "SKILL.md" || skip.iter().any(|skipped| name == *skipped) {
            continue;
        }
        let target = target_dir.join(&name);
        if !target.exists() && !target.is_symlink() {
            uze_core::persistence::create_symlink(&entry.path(), &target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_description_and_preserves_body_verbatim() {
        let bytes = b"---\ndescription: Review code\nothers: ignored\n---\n\nBody line one.\nBody line two.\n";
        let (description, body) = parse_skill_body(bytes);
        assert_eq!(description.as_deref(), Some("Review code"));
        assert_eq!(body, "\nBody line one.\nBody line two.\n");
    }

    #[test]
    fn body_only_file_has_no_description() {
        let bytes = b"Just a prompt body.\n";
        let (description, body) = parse_skill_body(bytes);
        assert_eq!(description, None);
        assert_eq!(body, "Just a prompt body.\n");
    }

    #[test]
    fn malformed_frontmatter_falls_through_to_body_only() {
        let bytes = b"---\ndescription: broken\nno closing marker\n";
        let (description, body) = parse_skill_body(bytes);
        assert_eq!(description, None);
        assert_eq!(body, std::str::from_utf8(bytes).unwrap());
    }

    #[test]
    fn non_utf8_payload_degrades_to_empty_body() {
        let bytes = b"\xff\xfe\x00";
        let (description, body) = parse_skill_body(bytes);
        assert_eq!(description, None);
        assert_eq!(body, "");
    }

    #[test]
    fn escapes_backslash_quote_and_control_characters() {
        let escaped = escape_yaml_double_quoted("a \\ b \" c\nd\te");
        assert_eq!(escaped, "a \\\\ b \\\" c\\nd\\te");
    }

    #[test]
    fn escaped_description_cannot_forge_or_duplicate_the_marker() {
        let tricky = "Has: a colon, \"quotes\", a\nnewline, and disable-model-invocation: false";
        let escaped = escape_yaml_double_quoted(tricky);
        assert!(!escaped.contains('\n'));
        assert!(
            escaped
                .match_indices('"')
                .all(|(index, _)| index > 0 && escaped.as_bytes()[index - 1] == b'\\')
        );
        let frontmatter =
            format!("---\ndescription: \"{escaped}\"\ndisable-model-invocation: true\n---\nbody\n");
        // The description text may mention the marker name; it must never
        // become a second key. Exactly one frontmatter LINE declares it,
        // it stays `true`, and the embedded `false` text is trapped inside
        // the quoted description scalar.
        let marker_lines = frontmatter
            .lines()
            .filter(|line| line.trim_start().starts_with("disable-model-invocation:"))
            .collect::<Vec<_>>();
        assert_eq!(marker_lines.len(), 1, "{frontmatter}");
        assert_eq!(marker_lines[0].trim(), "disable-model-invocation: true");
        assert!(has_disable_model_invocation(frontmatter.as_bytes()));
    }

    #[test]
    fn detects_disable_model_invocation_true() {
        let bytes = b"---\ndescription: d\ndisable-model-invocation: true\n---\nbody\n";
        assert!(has_disable_model_invocation(bytes));
    }

    #[test]
    fn absent_marker_is_false() {
        let bytes = b"---\ndescription: d\n---\nbody\n";
        assert!(!has_disable_model_invocation(bytes));
    }

    #[test]
    fn marker_set_to_false_is_false() {
        let bytes = b"---\ndisable-model-invocation: false\n---\nbody\n";
        assert!(!has_disable_model_invocation(bytes));
    }

    #[test]
    fn malformed_frontmatter_marker_is_false() {
        let bytes = b"---\ndisable-model-invocation: true\nno closing marker\n";
        assert!(!has_disable_model_invocation(bytes));
    }

    #[test]
    fn non_utf8_marker_is_false() {
        assert!(!has_disable_model_invocation(b"\xff\xfe\x00"));
    }

    #[test]
    fn detects_user_invocable_false_and_only_false() {
        let bytes = b"---\nuser-invocable: false\n---\nbody\n";
        assert!(has_user_invocable_false(bytes));
        let bytes = b"---\nuser-invocable: true\n---\nbody\n";
        assert!(!has_user_invocable_false(bytes));
        let bytes = b"---\ndescription: d\n---\nbody\n";
        assert!(!has_user_invocable_false(bytes));
    }
}
