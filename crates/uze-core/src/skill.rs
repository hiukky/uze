//! Canonical Skill invocation policy — the vendor-neutral declaration of
//! *who may invoke a Skill*.
//!
//! The canonical model has one capability kind (`AgentSkill`) and one
//! portable semantic dimension: invocation policy. Whether a Skill is
//! background knowledge the model may auto-select, an explicit action only
//! the user triggers, or both is a property of **invocation**, never of the
//! physical resource type (ADR-030). Vendor resource taxonomy (Claude
//! commands, OpenCode commands, Codex explicit-only skills) is a projection
//! concern owned by integrations — it never appears here.
//!
//! The `invoke:` frontmatter block is deliberately minimal and extensible
//! without becoming a policy DSL:
//!
//! ```markdown
//! ---
//! name: review
//! description: Review the current changes
//! invoke:
//!   model: false
//!   user: true
//! ---
//! ```
//!
//! Semantics per combination:
//!
//! | `model` | `user` | Meaning |
//! |---------|--------|---------|
//! | `true`  | `true`  | Normal interactive/discoverable Skill (default) |
//! | `true`  | `false` | Background/model-only capability |
//! | `false` | `true`  | Explicit user action (the thing previously called `Command`) |
//! | `false` | `false` | Invalid — nobody can invoke the Skill; integrations must not project it |
//!
//! Parsing is *intentionally* not a general YAML parser: the Store stays
//! byte-preserving, and this module only extracts the two booleans integrations
//! need for routing. Unknown `invoke:` sub-keys and any other frontmatter
//! field are left untouched and uninterpreted (section 6 — no universal Skill
//! schema). A block that cannot be understood (a non-boolean `model`/`user`
//! value) is treated as the invalid combination so it can never silently
//! degrade into a model-visible or user-visible default.

use serde::Serialize;

/// The canonical invocation policy of a Skill: who may invoke it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct SkillInvocationPolicy {
    /// The model may discover and auto-select this Skill.
    pub model: bool,
    /// The user may invoke this Skill explicitly (vendor-native syntax).
    pub user: bool,
}

impl SkillInvocationPolicy {
    /// The canonical default for a Skill that declares no `invoke:` block —
    /// existing `SKILL.md` files without one behave exactly as before.
    pub const MODEL_AND_USER: Self = Self {
        model: true,
        user: true,
    };

    /// Background/model-only capability.
    pub const MODEL_ONLY: Self = Self {
        model: true,
        user: false,
    };

    /// Explicit user action — the combination previously modeled as a
    /// canonical `Command`.
    pub const USER_ONLY: Self = Self {
        model: false,
        user: true,
    };

    /// Declared but meaningless: nobody can invoke the Skill.
    pub const INVALID: Self = Self {
        model: false,
        user: false,
    };

    pub fn is_default(&self) -> bool {
        self.model && self.user
    }

    pub fn is_invalid(&self) -> bool {
        !self.model && !self.user
    }
}

impl Default for SkillInvocationPolicy {
    fn default() -> Self {
        Self::MODEL_AND_USER
    }
}

/// Extracts a Skill's `invoke:` frontmatter policy from its canonical bytes.
///
/// Returns:
///
/// - `None` when the payload carries no `invoke:` block (or is not UTF-8 /
///   has no parseable frontmatter) — the canonical default
///   [`SkillInvocationPolicy::MODEL_AND_USER`] applies and existing Skills
///   keep their exact prior behavior;
/// - `Some(policy)` for an understood block, including the invalid
///   `model: false, user: false` combination, so consumers can refuse to
///   project it instead of silently changing the author's intent.
///
/// Only lowercase `true`/`false` literals are recognized (the same strict
/// literal rule the rest of UZE's frontmatter handling uses). Inside the
/// block, missing `model`/`user` keys default to `true` — the safe canonical
/// default — and unknown sub-keys are ignored.
pub fn parse_skill_invocation(bytes: &[u8]) -> Option<SkillInvocationPolicy> {
    let text = std::str::from_utf8(bytes).ok()?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let rest = text.strip_prefix("---\n")?;
    let end = rest.find("\n---\n")?;
    let head = &rest[..end];

    let mut model = true;
    let mut user = true;
    let mut invalid = false;
    let mut saw_invoke = false;
    let mut lines = head.lines().peekable();
    while let Some(line) = lines.next() {
        let Some((key, _value)) = line.split_once(':') else {
            continue;
        };
        if key.trim() != "invoke" {
            continue;
        }
        saw_invoke = true;
        // An unindented line ends the block (it is a top-level key).
        for nested in lines.by_ref() {
            if !nested.starts_with(char::is_whitespace) {
                break;
            }
            let Some((nested_key, nested_value)) = nested.split_once(':') else {
                continue;
            };
            match nested_key.trim() {
                "model" => match parse_bool(nested_value) {
                    Some(value) => model = value,
                    None => invalid = true,
                },
                "user" => match parse_bool(nested_value) {
                    Some(value) => user = value,
                    None => invalid = true,
                },
                _ => {}
            }
        }
        break;
    }
    if !saw_invoke {
        return None;
    }
    if invalid {
        return Some(SkillInvocationPolicy::INVALID);
    }
    Some(SkillInvocationPolicy { model, user })
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_invoke_block_is_none_and_defaults_to_model_and_user() {
        assert_eq!(
            parse_skill_invocation(b"---\nname: review\n---\nbody\n"),
            None
        );
        assert_eq!(parse_skill_invocation(b"no frontmatter at all\n"), None);
        assert_eq!(
            SkillInvocationPolicy::default(),
            SkillInvocationPolicy::MODEL_AND_USER
        );
    }

    #[test]
    fn user_only_is_parsed() {
        let bytes = b"---\nname: review\ndescription: review\ninvoke:\n  model: false\n  user: true\n---\nbody\n";
        assert_eq!(
            parse_skill_invocation(bytes),
            Some(SkillInvocationPolicy::USER_ONLY)
        );
    }

    #[test]
    fn model_only_is_parsed() {
        let bytes = b"---\ninvoke:\n  model: true\n  user: false\n---\n";
        assert_eq!(
            parse_skill_invocation(bytes),
            Some(SkillInvocationPolicy::MODEL_ONLY)
        );
    }

    #[test]
    fn explicit_model_and_user_is_parsed() {
        let bytes = b"---\ninvoke:\n  model: true\n  user: true\n---\n";
        assert_eq!(
            parse_skill_invocation(bytes),
            Some(SkillInvocationPolicy::MODEL_AND_USER)
        );
    }

    #[test]
    fn invalid_combination_is_kept_explicit_never_defaulted() {
        let bytes = b"---\ninvoke:\n  model: false\n  user: false\n---\n";
        assert_eq!(
            parse_skill_invocation(bytes),
            Some(SkillInvocationPolicy::INVALID)
        );
        assert!(SkillInvocationPolicy::INVALID.is_invalid());
    }

    #[test]
    fn missing_keys_inside_the_block_default_to_true() {
        let bytes = b"---\ninvoke:\n  model: false\n---\n";
        assert_eq!(
            parse_skill_invocation(bytes),
            Some(SkillInvocationPolicy::USER_ONLY)
        );
    }

    #[test]
    fn non_boolean_value_marks_the_block_invalid() {
        let bytes = b"---\ninvoke:\n  model: maybe\n  user: true\n---\n";
        assert_eq!(
            parse_skill_invocation(bytes),
            Some(SkillInvocationPolicy::INVALID)
        );
        let bytes = b"---\ninvoke:\n  model: false\n  user: yes\n---\n";
        assert_eq!(
            parse_skill_invocation(bytes),
            Some(SkillInvocationPolicy::INVALID)
        );
    }

    #[test]
    fn unknown_invoke_subkeys_are_ignored() {
        let bytes = b"---\ninvoke:\n  model: false\n  user: true\n  agents: all\n---\n";
        assert_eq!(
            parse_skill_invocation(bytes),
            Some(SkillInvocationPolicy::USER_ONLY)
        );
    }

    #[test]
    fn a_top_level_key_ends_the_invoke_block() {
        let bytes = b"---\ninvoke:\n  model: false\nname: review\n---\n";
        assert_eq!(
            parse_skill_invocation(bytes),
            Some(SkillInvocationPolicy::USER_ONLY)
        );
    }

    #[test]
    fn non_utf8_payload_is_none() {
        assert_eq!(parse_skill_invocation(b"\xff\xfe\x00"), None);
    }

    #[test]
    fn bom_prefixed_frontmatter_is_tolerated() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(b"---\ninvoke:\n  model: false\n  user: true\n---\n");
        assert_eq!(
            parse_skill_invocation(&bytes),
            Some(SkillInvocationPolicy::USER_ONLY)
        );
    }
}
