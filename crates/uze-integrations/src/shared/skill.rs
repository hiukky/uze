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

use std::{fs, path::Path};

use uze_core::{Result, UzeError, skill::SkillInvocationPolicy};

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
    let stripped = text.strip_prefix("\u{feff}").unwrap_or(text);
    let Some(rest) = stripped.strip_prefix("---\n") else {
        return (None, text.to_owned());
    };
    let Some(end) = rest.find("\n---\n") else {
        return (None, text.to_owned());
    };
    let head = &rest[..end];
    let body = &rest[end + "\n---\n".len()..];
    let mut description = None;
    for line in head.lines() {
        if let Some((key, value)) = line.split_once(':')
            && key.trim() == "description"
        {
            description = Some(value.trim().to_owned());
        }
    }
    (description, body.to_owned())
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
    frontmatter_key_value(bytes, "user-invocable").is_some_and(|value| value == "false")
}

fn frontmatter_is_true(bytes: &[u8], key: &str) -> bool {
    frontmatter_key_value(bytes, key).is_some_and(|value| value == "true")
}

/// Reads one top-level frontmatter value (e.g. `name`) from canonical
/// bytes, so a generated wrapper can preserve the canonically-declared
/// identity instead of inventing one. `None` for absent/malformed
/// frontmatter or non-UTF8 payload — never a guess.
pub fn frontmatter_value(bytes: &[u8], key: &str) -> Option<String> {
    frontmatter_key_value(bytes, key)
}

fn frontmatter_key_value(bytes: &[u8], key: &str) -> Option<String> {
    let text = is_utf8(bytes)?;
    let stripped = text.strip_prefix("\u{feff}").unwrap_or(text);
    let rest = stripped.strip_prefix("---\n")?;
    let end = rest.find("\n---\n")?;
    let head = &rest[..end];
    head.lines().find_map(|line| {
        let (candidate, value) = line.split_once(':')?;
        (candidate.trim() == key).then(|| value.trim().to_owned())
    })
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
    if dir.exists() {
        fs::remove_dir_all(dir).map_err(|source| UzeError::Write {
            path: dir.to_path_buf(),
            source,
        })?;
    }
    fs::create_dir_all(dir).map_err(|source| UzeError::Write {
        path: dir.to_path_buf(),
        source,
    })?;
    let (description, body) = parse_skill_body(canonical_bytes);
    let mut document = String::from("---\n");
    document.push_str(&format!("name: {label}\n"));
    if let Some(description) = description {
        let escaped = escape_yaml_double_quoted(&description);
        document.push_str(&format!("description: \"{escaped}\"\n"));
    }
    if !policy.user {
        document.push_str("slash: false\n");
    }
    if !policy.model {
        document.push_str("metadata:\n  opencode/autoinvoke: false\n");
    }
    document.push_str("---\n");
    document.push_str(&body);
    fs::write(dir.join("SKILL.md"), document).map_err(|source| UzeError::Write {
        path: dir.join("SKILL.md"),
        source,
    })?;
    if !policy.model {
        let policy_file = dir.join("agents/openai.yaml");
        if policy_file.exists() {
            fs::remove_file(&policy_file).map_err(|source| UzeError::Write {
                path: policy_file.clone(),
                source,
            })?;
        }
        fs::create_dir_all(policy_file.parent().expect("policy file has a parent")).map_err(
            |source| UzeError::Write {
                path: policy_file
                    .parent()
                    .expect("policy file has a parent")
                    .to_path_buf(),
                source,
            },
        )?;
        fs::write(
            &policy_file,
            "policy:\n  allow_implicit_invocation: false\n",
        )
        .map_err(|source| UzeError::Write {
            path: policy_file,
            source,
        })?;
    }
    // Everything else in the canonical skill directory stays referenced.
    // An absent canonical directory (a Resource built without a real Store
    // path, in unit-test contexts) simply has no extras to reference.
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
        let entry_name = entry.file_name();
        if entry_name == "SKILL.md" {
            continue;
        }
        if entry_name == "agents" {
            // The canonical skill may ship its own `agents/` support files;
            // they stay canonical (and a canonical `agents/openai.yaml` is
            // the author's, never re-derived here).
            continue;
        }
        let source = entry.path();
        let target = dir.join(&entry_name);
        if !target.exists() && !target.is_symlink() {
            symlink(&source, &target)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn symlink(source: &Path, target: &Path) -> Result<()> {
    std::os::unix::fs::symlink(source, target).map_err(|source_error| UzeError::Write {
        path: target.to_path_buf(),
        source: source_error,
    })
}

#[cfg(not(unix))]
fn symlink(_source: &Path, target: &Path) -> Result<()> {
    Err(UzeError::UnsupportedRuntimeProjection(target.to_path_buf()))
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
