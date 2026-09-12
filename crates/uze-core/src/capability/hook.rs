//! Vendor-neutral portable Hook manifest and command ABI (ADR-033,
//! ADR-040): what a package may declare, what a handler is promised, and
//! how a group's semantics are assessed against one harness's capabilities.
//!
//! Nothing here runs a hook. The generated wrapper an integration vendors
//! beside the delivery is the only implementation of the ABI — the harness
//! runs that, never UZE — so this module owns the vocabulary and the
//! assessment, and the wrapper owns the execution.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use serde::{Deserialize, Serialize};

use crate::{
    error::{Result, UzeError},
    router::CompatibilityRoute,
};

pub const HOOKS_FILE_NAME: &str = "hooks.json";
pub const DEFAULT_TIMEOUT_SECONDS: u16 = 30;
pub const MAX_TIMEOUT_SECONDS: u16 = 300;
/// The system program a generated shell wrapper needs to read the harness's
/// payload. Named here so a diagnostic can check for it without knowing how
/// any particular wrapper is written.
pub const WRAPPER_DEPENDENCY: &str = "jq";

/// The handler's decision channel is its exit code: `0` allows, this one
/// denies with the reason on stderr, and every other code (or a timeout, or
/// a handler that cannot start) is a failure whose outcome follows the
/// group's declared effect. The generated wrapper is the one implementation
/// of that rule, and reads this constant rather than spelling it again.
pub const DENY_EXIT_CODE: i32 = 3;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum HookEvent {
    PreToolUse,
    PostToolUse,
    Stop,
}

impl HookEvent {
    pub const fn abi_name(self) -> &'static str {
        match self {
            Self::PreToolUse => "pre_tool_use",
            Self::PostToolUse => "post_tool_use",
            Self::Stop => "stop",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEffect {
    #[default]
    Observe,
    Allow,
    Ask,
    Deny,
    Transform,
}

impl HookEffect {
    pub const fn abi_name(self) -> &'static str {
        match self {
            Self::Observe => "observe",
            Self::Allow => "allow",
            Self::Ask => "ask",
            Self::Deny => "deny",
            Self::Transform => "transform",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HookManifest {
    pub hooks: BTreeMap<HookEvent, Vec<HookGroup>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HookGroup {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub matcher: Option<String>,
    #[serde(default)]
    pub effect: HookEffect,
    pub hooks: Vec<CommandHook>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CommandHook {
    #[serde(rename = "type")]
    pub handler_type: CommandHandlerType,
    pub command: String,
    #[serde(default = "default_timeout")]
    pub timeout: u16,
}

const fn default_timeout() -> u16 {
    DEFAULT_TIMEOUT_SECONDS
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CommandHandlerType {
    Command,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PortableHook {
    pub id: String,
    pub event: HookEvent,
    pub matchers: Vec<HookMatcher>,
    pub handlers: Vec<CommandHook>,
    pub effect: HookEffect,
    pub order: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "name", rename_all = "snake_case")]
pub enum HookMatcher {
    Portable(String),
    Native(String),
}

impl HookMatcher {
    pub fn parse(token: &str) -> std::result::Result<Self, String> {
        let token = token.trim();
        if let Some(native) = token.strip_prefix("native:") {
            if native.trim().is_empty() {
                return Err("native matcher cannot be empty".to_owned());
            }
            return Ok(Self::Native(native.trim().to_owned()));
        }
        if portable_tool_aliases().contains(&token) {
            return Ok(Self::Portable(token.to_owned()));
        }
        Err(format!(
            "unknown portable tool alias `{token}`; use native:<tool> for an explicit harness name"
        ))
    }
}

/// One portable tool alias and the portable fields a matched handler is
/// guaranteed to receive, whichever harness delivered the hook. The field
/// names are the vocabulary's own — never a harness's — and each becomes a
/// `HOOK_<FIELD>` environment variable (see [`hook_field_variable`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToolAlias {
    pub alias: &'static str,
    pub fields: &'static [&'static str],
}

/// The portable tool vocabulary: the single source of the alias set and of
/// what each alias promises a handler. Which native tool and which native
/// input field an alias reads from is a harness fact and therefore lives
/// with the harness (see [`HarnessToolVocabulary`]), not here — this crate
/// names no vendor.
pub fn portable_tool_vocabulary() -> &'static [ToolAlias] {
    &[
        ToolAlias {
            alias: "shell",
            fields: &["command"],
        },
        ToolAlias {
            alias: "file.read",
            fields: &["path"],
        },
        ToolAlias {
            alias: "file.write",
            fields: &["path"],
        },
        ToolAlias {
            alias: "file.edit",
            fields: &["path"],
        },
        ToolAlias {
            alias: "search.files",
            fields: &["query"],
        },
        ToolAlias {
            alias: "search.web",
            fields: &["query"],
        },
        ToolAlias {
            alias: "agent.spawn",
            fields: &[],
        },
        ToolAlias {
            alias: "agent.message",
            fields: &[],
        },
    ]
}

pub fn portable_tool_aliases() -> &'static [&'static str] {
    static ALIASES: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    ALIASES.get_or_init(|| {
        portable_tool_vocabulary()
            .iter()
            .map(|entry| entry.alias)
            .collect()
    })
}

/// The portable fields one alias guarantees; empty for an alias the
/// vocabulary carries without a portable payload, and for `native:` tools.
pub fn alias_fields(alias: &str) -> &'static [&'static str] {
    portable_tool_vocabulary()
        .iter()
        .find(|entry| entry.alias == alias)
        .map_or(&[][..], |entry| entry.fields)
}

/// The environment variable one portable field is delivered in:
/// `command` -> `HOOK_COMMAND`, `path` -> `HOOK_PATH`.
pub fn hook_field_variable(field: &str) -> String {
    format!(
        "HOOK_{}",
        field.to_ascii_uppercase().replace(['.', '-'], "_")
    )
}

/// One harness's binding of a portable alias: the native tool the alias is
/// matched as, any further native tool names that normalize back to it, and
/// the native input field each portable field is read from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToolBinding {
    pub alias: &'static str,
    /// `None` when the harness exposes no tool for this alias. The matcher
    /// then falls back to the alias literal, which matches nothing — an
    /// honest no-op rather than a fabricated tool name.
    pub native_tool: Option<&'static str>,
    /// Additional native names that resolve to this alias when a payload is
    /// normalized (a vendor renaming its shell tool, an older build). Never
    /// emitted into a matcher.
    pub also_matches: &'static [&'static str],
    /// `(portable field, native input field)` pairs, one per field the
    /// alias promises.
    pub fields: &'static [(&'static str, &'static str)],
}

/// One harness's whole binding table. Supplied by that harness's
/// integration — this crate owns the shape, the integration owns the names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HarnessToolVocabulary {
    pub bindings: &'static [ToolBinding],
}

impl HarnessToolVocabulary {
    pub fn binding(&self, alias: &str) -> Option<&'static ToolBinding> {
        self.bindings.iter().find(|entry| entry.alias == alias)
    }

    /// The alias a native tool name normalizes to, or `None` when this
    /// harness's table does not know the tool — the handler then receives
    /// `HOOK_TOOL_NATIVE` and `HOOK_INPUT` alone.
    pub fn binding_for_native(&self, native: &str) -> Option<&'static ToolBinding> {
        self.bindings
            .iter()
            .find(|entry| entry.native_tool == Some(native) || entry.also_matches.contains(&native))
    }

    /// Every native tool name that resolves to an alias, paired with its
    /// binding — the order the generated wrapper's dispatch table follows.
    pub fn native_names(&self) -> Vec<(&'static str, &'static ToolBinding)> {
        self.bindings
            .iter()
            .flat_map(|entry| {
                entry
                    .native_tool
                    .into_iter()
                    .chain(entry.also_matches.iter().copied())
                    .map(move |native| (native, entry))
            })
            .collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct HookCompatibility {
    pub route: CompatibilityRoute,
    pub reason: Option<String>,
    pub artifacts: Vec<String>,
}

/// An integration's declaration of the hook semantics it can preserve. This
/// lives in Core because it is vocabulary, not vendor knowledge; each vendor
/// integration supplies concrete values.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HookCapabilities {
    pub events: BTreeSet<HookEvent>,
    pub effects: BTreeSet<HookEffect>,
    pub supports_native_matchers: bool,
    pub supports_input_transform: bool,
    pub executes_handlers_in_order: bool,
    pub artifacts: Vec<String>,
}

/// Calculates compatibility over the actual semantic axes. `Native` is
/// reserved for a target that preserves every declared effect and ordering;
/// a generated bridge is `Adaptable` even when it faithfully executes it.
pub fn assess(
    hook: &PortableHook,
    capabilities: &HookCapabilities,
    bridged: bool,
) -> HookCompatibility {
    let reason = if !capabilities.events.contains(&hook.event) {
        Some(format!(
            "the target has no `{}` semantic event",
            hook.event.abi_name()
        ))
    } else if !capabilities.effects.contains(&hook.effect) {
        Some(format!(
            "the target cannot preserve `{}` hook effect",
            effect_name(hook.effect)
        ))
    } else if hook
        .matchers
        .iter()
        .any(|matcher| matches!(matcher, HookMatcher::Native(_)))
        && !capabilities.supports_native_matchers
    {
        Some("the target cannot safely apply an explicit native tool matcher".to_owned())
    } else if hook.effect == HookEffect::Transform && !capabilities.supports_input_transform {
        Some("the target cannot safely transform pre-tool input".to_owned())
    } else if !capabilities.executes_handlers_in_order && hook.handlers.len() > 1 {
        Some("the target cannot preserve ordered multi-handler execution".to_owned())
    } else {
        None
    };
    let route = match reason {
        Some(_) if hook.effect == HookEffect::Deny || hook.effect == HookEffect::Ask => {
            CompatibilityRoute::Unsupported
        }
        Some(_) => CompatibilityRoute::Degraded,
        None if bridged => CompatibilityRoute::Adaptable,
        None => CompatibilityRoute::Native,
    };
    HookCompatibility {
        route,
        reason,
        artifacts: capabilities.artifacts.clone(),
    }
}

fn effect_name(effect: HookEffect) -> &'static str {
    match effect {
        HookEffect::Observe => "observe",
        HookEffect::Allow => "allow",
        HookEffect::Ask => "ask",
        HookEffect::Deny => "deny",
        HookEffect::Transform => "transform",
    }
}

/// Parses and validates one package/project `hooks.json`. The returned order
/// is deterministic: semantic event order then source group order.
pub fn parse_manifest(path: &Path, bytes: &[u8]) -> Result<Vec<PortableHook>> {
    let manifest: HookManifest =
        serde_json::from_slice(bytes).map_err(|source| UzeError::Json {
            path: path.to_path_buf(),
            source,
        })?;
    let mut seen = std::collections::BTreeSet::new();
    let mut hooks = Vec::new();
    for (event, groups) in manifest.hooks {
        for (index, group) in groups.into_iter().enumerate() {
            let id = group
                .id
                .unwrap_or_else(|| format!("{}-{index}", event.abi_name()));
            if id.trim().is_empty()
                || !id
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
            {
                return invalid(
                    path,
                    "hook id must contain only ASCII letters, digits, `.`, `_`, or `-`",
                );
            }
            if !seen.insert(id.clone()) {
                return invalid(path, &format!("duplicate hook id `{id}`"));
            }
            if group.hooks.is_empty() {
                return invalid(path, &format!("hook `{id}` has no handlers"));
            }
            if matches!(group.effect, HookEffect::Transform) && event != HookEvent::PreToolUse {
                return invalid(
                    path,
                    &format!("hook `{id}` transforms input but is not PreToolUse"),
                );
            }
            let has_matcher = group.matcher.is_some();
            let matchers = match group.matcher {
                Some(matcher) => matcher
                    .split('|')
                    .map(HookMatcher::parse)
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(|reason| UzeError::InvalidHookManifest {
                        path: path.to_path_buf(),
                        reason,
                    })?,
                None => Vec::new(),
            };
            if has_matcher && matchers.is_empty() {
                return invalid(path, &format!("hook `{id}` has an empty matcher"));
            }
            for handler in &group.hooks {
                if handler.command.trim().is_empty() {
                    return invalid(path, &format!("hook `{id}` has an empty command"));
                }
                if !(1..=MAX_TIMEOUT_SECONDS).contains(&handler.timeout) {
                    return invalid(
                        path,
                        &format!(
                            "hook `{id}` timeout must be between 1 and {MAX_TIMEOUT_SECONDS} seconds"
                        ),
                    );
                }
            }
            hooks.push(PortableHook {
                id,
                event,
                matchers,
                handlers: group.hooks,
                effect: group.effect,
                order: hooks.len(),
            });
        }
    }
    Ok(hooks)
}

fn invalid<T>(path: &Path, reason: &str) -> Result<T> {
    Err(UzeError::InvalidHookManifest {
        path: path.to_path_buf(),
        reason: reason.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn parses_ordered_portable_groups_and_defaults_timeout() {
        let hooks = parse_manifest(Path::new("hooks.json"), br#"{"hooks":{"PreToolUse":[{"id":"protect-env","matcher":"shell|file.write|native:Write","hooks":[{"type":"command","command":"${PLUGIN_ROOT}/check"},{"type":"command","command":"second","timeout":10}]}]}}"#).unwrap();
        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0].id, "protect-env");
        assert_eq!(hooks[0].handlers[0].timeout, DEFAULT_TIMEOUT_SECONDS);
        assert_eq!(
            hooks[0].matchers,
            vec![
                HookMatcher::Portable("shell".into()),
                HookMatcher::Portable("file.write".into()),
                HookMatcher::Native("Write".into())
            ]
        );
    }

    #[test]
    fn rejects_unknown_alias_empty_commands_and_unsafe_transform() {
        for source in [
            br#"{"hooks":{"PreToolUse":[{"matcher":"invented","hooks":[{"type":"command","command":"ok"}]}]}}"#.as_slice(),
            br#"{"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":" "}]}]}}"#.as_slice(),
            br#"{"hooks":{"Stop":[{"effect":"transform","hooks":[{"type":"command","command":"ok"}]}]}}"#.as_slice(),
        ] { assert!(matches!(parse_manifest(Path::new("hooks.json"), source), Err(UzeError::InvalidHookManifest { .. }))); }
    }

    #[test]
    fn rejects_duplicate_ids_and_out_of_range_timeout() {
        let bytes = br#"{"hooks":{"PreToolUse":[{"id":"same","hooks":[{"type":"command","command":"ok","timeout":0}]},{"id":"same","hooks":[{"type":"command","command":"ok"}]}]}}"#;
        assert!(matches!(
            parse_manifest(Path::new("hooks.json"), bytes),
            Err(UzeError::InvalidHookManifest { .. })
        ));
    }

    #[test]
    fn the_vocabulary_is_the_single_source_of_the_alias_set_and_its_fields() {
        let aliases: Vec<&str> = portable_tool_vocabulary()
            .iter()
            .map(|entry| entry.alias)
            .collect();
        assert_eq!(aliases, portable_tool_aliases().to_vec());
        assert_eq!(alias_fields("shell"), ["command"]);
        assert_eq!(alias_fields("file.write"), ["path"]);
        assert_eq!(alias_fields("agent.spawn"), [] as [&str; 0]);
        assert_eq!(alias_fields("native:Write"), [] as [&str; 0]);
        assert_eq!(hook_field_variable("command"), "HOOK_COMMAND");
        assert_eq!(hook_field_variable("path"), "HOOK_PATH");
    }

    #[test]
    fn compatibility_does_not_equate_stop_with_a_tool_callback() {
        let hook = PortableHook {
            id: "review-stop".into(),
            event: HookEvent::Stop,
            matchers: Vec::new(),
            handlers: vec![CommandHook {
                handler_type: CommandHandlerType::Command,
                command: "check".into(),
                timeout: 1,
            }],
            effect: HookEffect::Observe,
            order: 0,
        };
        let compatibility = assess(
            &hook,
            &HookCapabilities {
                events: [HookEvent::PreToolUse, HookEvent::PostToolUse]
                    .into_iter()
                    .collect(),
                effects: [HookEffect::Observe].into_iter().collect(),
                executes_handlers_in_order: true,
                ..HookCapabilities::default()
            },
            true,
        );
        assert_eq!(compatibility.route, CompatibilityRoute::Degraded);
        assert!(
            compatibility
                .reason
                .unwrap()
                .contains("no `stop` semantic event")
        );
    }

    #[test]
    fn a_security_effect_that_cannot_be_enforced_is_unsupported() {
        let hook = PortableHook {
            id: "protect".into(),
            event: HookEvent::PreToolUse,
            matchers: Vec::new(),
            handlers: vec![CommandHook {
                handler_type: CommandHandlerType::Command,
                command: "check".into(),
                timeout: 1,
            }],
            effect: HookEffect::Deny,
            order: 0,
        };
        let compatibility = assess(&hook, &HookCapabilities::default(), false);
        assert_eq!(compatibility.route, CompatibilityRoute::Unsupported);
    }
}
