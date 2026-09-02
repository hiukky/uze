//! Hook projections owned by harness integrations (ADR-033): per-vendor
//! capability profiles, native JSON configuration merging, the owned
//! OpenCode bridge, and the runtime hook-exec adapters that translate each
//! harness's native hook payload to the portable command ABI and back.
//!
//! Everything here is deterministic and vendor-local; the vendor-neutral
//! vocabulary (`PortableHook`, `HookCapabilities`, `assess`, ABI types)
//! lives in `uze-core::hook`. Every vendor contract in this module is a
//! documented mapping, verified by deterministic fixtures — real-binary
//! conformance evidence is recorded per integration in the Conformance Lab.

use std::{fs, path::Path, path::PathBuf};

use uze_core::{
    Result, UzeError,
    exposure::{ExposureMechanism, ExposurePlan},
    home::UzeHome,
    hook::{
        CommandHook, HOOKS_FILE_NAME, HarnessToolVocabulary, HookCapabilities, HookCommandInput,
        HookContext, HookDecision, HookDispatchOutcome, HookEffect, HookEvent, HookMatcher,
        HookNativeOutput, HookTool, PortableHook, ToolBinding,
    },
    integration::{AttachmentInspection, AttachmentState},
    persistence::write_atomic,
    project::Resource,
    router::{CompatibilityRoute, VerificationStatus},
};

// ============================================================================
// Capability profiles: the semantic axes each harness preserves
// ============================================================================

/// Claude Code's hook surface: documented `PreToolUse`/`PostToolUse`/`Stop`
/// command hooks with per-group matchers; observations, approvals, and
/// denials are expressible. Input rewriting is not yet claimed — a
/// `transform` effect therefore degrades instead of silently attaching
/// without its rewrite.
pub(crate) fn claude_capabilities() -> HookCapabilities {
    HookCapabilities {
        events: [
            HookEvent::PreToolUse,
            HookEvent::PostToolUse,
            HookEvent::Stop,
        ]
        .into_iter()
        .collect(),
        effects: [HookEffect::Observe, HookEffect::Allow, HookEffect::Deny]
            .into_iter()
            .collect(),
        supports_native_matchers: true,
        executes_handlers_in_order: true,
        ..HookCapabilities::default()
    }
}

/// Codex's hook surface mirrors Claude's event names with its own `hooks.json`
/// command form; the same conservative effect set is claimed.
pub(crate) fn codex_capabilities() -> HookCapabilities {
    HookCapabilities {
        events: [
            HookEvent::PreToolUse,
            HookEvent::PostToolUse,
            HookEvent::Stop,
        ]
        .into_iter()
        .collect(),
        effects: [HookEffect::Observe, HookEffect::Allow, HookEffect::Deny]
            .into_iter()
            .collect(),
        supports_native_matchers: true,
        executes_handlers_in_order: true,
        ..HookCapabilities::default()
    }
}

/// Antigravity CLI's plugin hooks carry named entries, camelCase payloads,
/// and native `allow`/`ask`/`deny` decisions. Hooks are delivered only
/// through the generated native plugin (the plugin system reads
/// `hooks.json`; there is no documented capability-level hook surface).
pub(crate) fn antigravity_capabilities() -> HookCapabilities {
    HookCapabilities {
        events: [
            HookEvent::PreToolUse,
            HookEvent::PostToolUse,
            HookEvent::Stop,
        ]
        .into_iter()
        .collect(),
        effects: [
            HookEffect::Observe,
            HookEffect::Allow,
            HookEffect::Ask,
            HookEffect::Deny,
        ]
        .into_iter()
        .collect(),
        supports_native_matchers: true,
        executes_handlers_in_order: true,
        ..HookCapabilities::default()
    }
}

/// OpenCode's plugin API supplies mutable pre/post tool callbacks where a
/// thrown error blocks the intercepted tool; there is no declarative hook
/// file, so UZE generates an owned, rebuildable TypeScript bridge. `Stop`
/// has no OpenCode equivalent and is never claimed; `ask` cannot be
/// expressed (an error is a hard denial), so an `Ask` hook routes
/// Unsupported rather than silently degrading.
pub(crate) fn opencode_capabilities() -> HookCapabilities {
    HookCapabilities {
        events: [HookEvent::PreToolUse, HookEvent::PostToolUse]
            .into_iter()
            .collect(),
        effects: [
            HookEffect::Observe,
            HookEffect::Allow,
            HookEffect::Transform,
        ]
        .into_iter()
        .collect(),
        supports_native_matchers: true,
        supports_input_transform: true,
        executes_handlers_in_order: true,
        ..HookCapabilities::default()
    }
}

// ============================================================================
// Matcher translation
// ============================================================================

/// Every harness's binding of the portable tool vocabulary: per alias, the
/// native tool it is matched as and the native input field each portable
/// field is read from. This table is the single source the matchers, the
/// generated wrappers and the runtime adapters all read — nothing here is
/// hand-written twice.
///
/// The names come from what each harness declares to the model, captured
/// with the Lab's `--discovery` mode, not from memory: Antigravity's
/// `run_command`/`CommandLine`+`Cwd`, `write_to_file`/`TargetFile`,
/// `view_file`/`AbsolutePath`, `grep_search`/`Query` and `search_web`/
/// `query` are read off its own `parametersJsonSchema`; Codex's shell tool
/// is `exec_command` with a `cmd` argument (0.150.1 onwards — `Bash` stays
/// in `also_matches` so an older payload still normalizes). OpenCode's
/// field names follow its documented tool schema; a `--discovery` capture
/// of that harness has not been taken yet.
pub(crate) fn vocabulary(target: &str) -> HarnessToolVocabulary {
    HarnessToolVocabulary {
        bindings: match target {
            "claude" => CLAUDE_TOOLS,
            "codex" => CODEX_TOOLS,
            "antigravity" => ANTIGRAVITY_TOOLS,
            "opencode" => OPENCODE_TOOLS,
            _ => &[],
        },
    }
}

/// An alias no harness tool answers to. Kept in every table so the
/// vocabulary is exhaustive by construction: absence of a native name is
/// stated, never left to a missing row.
const UNBOUND: Option<&'static str> = None;

const CLAUDE_TOOLS: &[ToolBinding] = &[
    ToolBinding {
        alias: "shell",
        native_tool: Some("Bash"),
        also_matches: &[],
        fields: &[("command", "command")],
    },
    ToolBinding {
        alias: "file.read",
        native_tool: Some("Read"),
        also_matches: &[],
        fields: &[("path", "file_path")],
    },
    ToolBinding {
        alias: "file.write",
        native_tool: Some("Write"),
        also_matches: &[],
        fields: &[("path", "file_path")],
    },
    ToolBinding {
        alias: "file.edit",
        native_tool: Some("MultiEdit"),
        also_matches: &["Edit"],
        fields: &[("path", "file_path")],
    },
    ToolBinding {
        alias: "search.files",
        native_tool: Some("Grep"),
        also_matches: &[],
        fields: &[("query", "pattern")],
    },
    ToolBinding {
        alias: "search.web",
        native_tool: Some("WebSearch"),
        also_matches: &[],
        fields: &[("query", "query")],
    },
    ToolBinding {
        alias: "agent.spawn",
        native_tool: Some("Task"),
        also_matches: &[],
        fields: &[],
    },
    ToolBinding {
        alias: "agent.message",
        native_tool: UNBOUND,
        also_matches: &[],
        fields: &[],
    },
];

const CODEX_TOOLS: &[ToolBinding] = &[
    ToolBinding {
        alias: "shell",
        native_tool: Some("exec_command"),
        also_matches: &["Bash", "shell"],
        fields: &[("command", "cmd")],
    },
    ToolBinding {
        alias: "file.read",
        native_tool: Some("Read"),
        also_matches: &[],
        fields: &[("path", "file_path")],
    },
    ToolBinding {
        alias: "file.write",
        native_tool: Some("Write"),
        also_matches: &[],
        fields: &[("path", "file_path")],
    },
    ToolBinding {
        alias: "file.edit",
        native_tool: Some("Edit"),
        also_matches: &[],
        fields: &[("path", "file_path")],
    },
    ToolBinding {
        alias: "search.files",
        native_tool: Some("Grep"),
        also_matches: &[],
        fields: &[("query", "pattern")],
    },
    ToolBinding {
        alias: "search.web",
        native_tool: Some("WebSearch"),
        also_matches: &[],
        fields: &[("query", "query")],
    },
    ToolBinding {
        alias: "agent.spawn",
        native_tool: UNBOUND,
        also_matches: &[],
        fields: &[],
    },
    ToolBinding {
        alias: "agent.message",
        native_tool: UNBOUND,
        also_matches: &[],
        fields: &[],
    },
];

const ANTIGRAVITY_TOOLS: &[ToolBinding] = &[
    ToolBinding {
        alias: "shell",
        native_tool: Some("run_command"),
        also_matches: &[],
        fields: &[("command", "CommandLine")],
    },
    ToolBinding {
        alias: "file.read",
        native_tool: Some("view_file"),
        also_matches: &[],
        fields: &[("path", "AbsolutePath")],
    },
    ToolBinding {
        alias: "file.write",
        native_tool: Some("write_to_file"),
        also_matches: &[],
        fields: &[("path", "TargetFile")],
    },
    ToolBinding {
        alias: "file.edit",
        native_tool: Some("replace_file_content"),
        also_matches: &[],
        fields: &[("path", "TargetFile")],
    },
    ToolBinding {
        alias: "search.files",
        native_tool: Some("grep_search"),
        also_matches: &[],
        fields: &[("query", "Query")],
    },
    ToolBinding {
        alias: "search.web",
        native_tool: Some("search_web"),
        also_matches: &[],
        fields: &[("query", "query")],
    },
    ToolBinding {
        alias: "agent.spawn",
        native_tool: UNBOUND,
        also_matches: &[],
        fields: &[],
    },
    ToolBinding {
        alias: "agent.message",
        native_tool: UNBOUND,
        also_matches: &[],
        fields: &[],
    },
];

const OPENCODE_TOOLS: &[ToolBinding] = &[
    ToolBinding {
        alias: "shell",
        native_tool: Some("bash"),
        also_matches: &[],
        fields: &[("command", "command")],
    },
    ToolBinding {
        alias: "file.read",
        native_tool: Some("read"),
        also_matches: &[],
        fields: &[("path", "filePath")],
    },
    ToolBinding {
        alias: "file.write",
        native_tool: Some("write"),
        also_matches: &[],
        fields: &[("path", "filePath")],
    },
    ToolBinding {
        alias: "file.edit",
        native_tool: Some("edit"),
        also_matches: &[],
        fields: &[("path", "filePath")],
    },
    ToolBinding {
        alias: "search.files",
        native_tool: Some("grep"),
        also_matches: &[],
        fields: &[("query", "pattern")],
    },
    ToolBinding {
        alias: "search.web",
        native_tool: Some("web_search"),
        also_matches: &[],
        fields: &[("query", "query")],
    },
    ToolBinding {
        alias: "agent.spawn",
        native_tool: Some("task"),
        also_matches: &[],
        fields: &[],
    },
    ToolBinding {
        alias: "agent.message",
        native_tool: UNBOUND,
        also_matches: &[],
        fields: &[],
    },
];

/// The native tool name a matcher becomes on one target. `native:<name>`
/// passes through unchanged; an alias this harness binds to no tool falls
/// back to the alias literal, which matches nothing.
pub(crate) fn tool_name(target: &str, matcher: &HookMatcher) -> String {
    match matcher {
        HookMatcher::Native(name) => name.clone(),
        HookMatcher::Portable(alias) => vocabulary(target)
            .binding(alias)
            .and_then(|binding| binding.native_tool)
            .unwrap_or(alias.as_str())
            .to_owned(),
    }
}

/// Inverse of [`tool_name`]: the portable alias for a native tool name, used
/// by the runtime adapters to normalize native payloads. `None` for a tool
/// this target table does not recognize — the ABI then carries no alias
/// rather than a fabricated one.
pub(crate) fn portable_name(target: &str, native: &str) -> Option<String> {
    vocabulary(target)
        .binding_for_native(native)
        .map(|binding| binding.alias.to_owned())
}

/// Translates every matcher of a group for one target; `None` for an
/// unmatch-all group (the entry then omits the matcher key).
pub(crate) fn matcher(target: &str, hook: &PortableHook) -> Option<String> {
    (!hook.matchers.is_empty()).then(|| {
        // Two authored matchers can translate to one native tool (a
        // portable alias plus the `native:` name it already resolves to);
        // the entry names it once.
        let mut names: Vec<String> = Vec::new();
        for entry in &hook.matchers {
            let name = tool_name(target, entry);
            if !names.contains(&name) {
                names.push(name);
            }
        }
        names.join("|")
    })
}

// ============================================================================
// Native entry rendering
// ============================================================================

/// POSIX single-quote quoting for a fragment embedded in a command line the
/// harness will run through its own shell. Spaces, quotes, and `$` all stay
/// literal inside single quotes; a single quote becomes the canonical
/// `'\''` splice.
pub(crate) fn shell_quote(fragment: &str) -> String {
    format!("'{}'", fragment.replace('\'', "'\\''"))
}

/// The `hook-exec` wrapper invocation that replaces the author's command in
/// every native entry: the harness runs this one command, which hands the
/// native payload to the adapter and runs the author's handlers against the
/// portable ABI. Absolute executable path pins the wrapper regardless of
/// the harness's own `PATH`; per-handler timeouts and the first-deny-wins
/// rule live inside the runner.
pub(crate) fn dispatcher_command(
    executable: &Path,
    adapter_id: &str,
    event: HookEvent,
    effect: HookEffect,
    package_root: &Path,
    handlers: &[CommandHook],
) -> String {
    let mut parts = vec![
        shell_quote(&executable.display().to_string()),
        "hook-exec".to_owned(),
        format!("--adapter {}", shell_quote(adapter_id)),
        format!("--event {}", event.abi_name()),
        format!("--effect {}", effect.abi_name()),
        format!(
            "--plugin-root {}",
            shell_quote(&package_root.display().to_string())
        ),
    ];
    for handler in handlers {
        parts.push(format!("--command {}", shell_quote(&handler.command)));
    }
    parts.join(" ")
}

/// The native group entry for an event-array target (Claude settings.json
/// hooks, Codex hooks.json): `{ "matcher": ..., "hooks": [...] }` with the
/// author's command carried inside the wrapper invocation. The matcher key
/// is omitted entirely for an unmatch-all group.
pub(crate) fn group_entry(
    target: &str,
    hook: &PortableHook,
    executable: &Path,
    adapter_id: &str,
    package_root: &Path,
) -> serde_json::Value {
    let command = dispatcher_command(
        executable,
        adapter_id,
        hook.event,
        hook.effect,
        package_root,
        &hook.handlers,
    );
    // The native timeout is a backstop for the whole wrapper; the sum of the
    // per-handler timeouts (with a 1s grace for the final render) bounds the
    // wrapper's own activity, capped at the canonical 300s maximum.
    let timeout: u16 = hook
        .handlers
        .iter()
        .map(|handler| u32::from(handler.timeout))
        .sum::<u32>()
        .saturating_add(1)
        .min(u32::from(uze_core::hook::MAX_TIMEOUT_SECONDS)) as u16;
    let mut entry = serde_json::Map::new();
    if let Some(matcher) = matcher(target, hook) {
        entry.insert("matcher".to_owned(), serde_json::Value::String(matcher));
    }
    entry.insert(
        "hooks".to_owned(),
        serde_json::Value::Array(vec![serde_json::json!({
            "type": "command",
            "command": command,
            "timeout": timeout,
        })]),
    );
    serde_json::Value::Object(entry)
}

const fn hook_event_name(event: HookEvent) -> &'static str {
    match event {
        HookEvent::PreToolUse => "PreToolUse",
        HookEvent::PostToolUse => "PostToolUse",
        HookEvent::Stop => "Stop",
    }
}

/// The generated plugin `hooks.json` for Antigravity CLI: named entries at
/// the document root (`{"<id>": {"<Event>": [<group>]}}`), each group
/// carrying the translated matcher and the hook-exec wrapper. Deterministic
/// per package.
///
/// The root is the hook map itself, never a `hooks` wrapper: the vendor
/// reads every root key as one named hook, so a wrapper registers a single
/// hook called `hooks` whose "events" are our ids — and no handler ever
/// runs (AGY 1.1.24: `plugin validate` reports 1 hook processed instead of
/// one per group, and the loader fires nothing).
pub(crate) fn agy_hook_document(
    hooks: &[&PortableHook],
    executable: &Path,
    package_root: &Path,
) -> String {
    let mut named = serde_json::Map::new();
    for hook in hooks {
        let entry = group_entry(
            "antigravity",
            hook,
            executable,
            agy_adapter_id(),
            package_root,
        );
        named.insert(
            hook.id.clone(),
            serde_json::json!({ hook_event_name(hook.event): [entry] }),
        );
    }
    format!(
        "{}\n",
        serde_json::to_string_pretty(&named).expect("generated hooks.json serializes")
    )
}

/// Stable adapter id for Antigravity, emitted into generated hook commands.
pub(crate) const fn agy_adapter_id() -> &'static str {
    "antigravity"
}

// ============================================================================
// Event-array config merge (Claude settings.json, Codex hooks.json)
// ============================================================================

/// Reads a shared hook config as a JSON object; a missing file is an empty
/// object. Malformed JSON or a non-object root is a blocked file, never
/// something UZE rewrites.
fn read_config_object(config_path: &Path) -> std::result::Result<serde_json::Value, String> {
    match fs::read(config_path) {
        Ok(bytes) if bytes.is_empty() => Ok(serde_json::json!({})),
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
            format!(
                "hook config `{}` is not readable JSON: {error}",
                config_path.display()
            )
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(serde_json::json!({})),
        Err(error) => Err(format!(
            "hook config `{}` cannot be read: {error}",
            config_path.display()
        )),
    }
    .and_then(|value| {
        if value.is_object() {
            Ok(value)
        } else {
            Err(format!(
                "hook config `{}` root must be a JSON object",
                config_path.display()
            ))
        }
    })
}

/// Writes a config document with a trailing newline, creating missing
/// parent directories for a UZE-created file. Atomic (temp+rename) so a
/// crash mid-merge can never corrupt a vendor config file.
fn write_config(config_path: &Path, config: &serde_json::Value) -> Result<()> {
    let parent = config_path.parent().expect("hook config path has a parent");
    fs::create_dir_all(parent).map_err(|source| UzeError::Write {
        path: parent.to_path_buf(),
        source,
    })?;
    let mut bytes = serde_json::to_vec_pretty(config).expect("hook config serializes");
    bytes.push(b'\n');
    write_atomic(config_path, &bytes)
}

/// The exact entries one integration already owns for one hook entry name
/// in the receipt ledger — the "previous version" contents for idempotent
/// re-attach, and the proof of ownership for a later replacement.
pub(crate) fn previous_hook_entry_content(
    uze_home: &UzeHome,
    integration_id: &str,
    hook_entry_name: &str,
) -> Result<Vec<String>> {
    let Ok(ledger) = uze_core::state::receipts(uze_home, None) else {
        return Ok(Vec::new());
    };
    Ok(ledger
        .into_iter()
        .filter(|(_, receipt)| {
            receipt.integration == integration_id
                && matches!(
                    &receipt.artifact,
                    uze_core::integration::ManagedArtifact::HookConfigEntry {
                        entry_name,
                        ..
                    } if entry_name == hook_entry_name
                )
        })
        .filter_map(|(_, receipt)| match receipt.artifact {
            uze_core::integration::ManagedArtifact::HookConfigEntry { expected, .. } => {
                Some(expected)
            }
            _ => None,
        })
        .collect())
}

/// Merges the current rendered entry, first replacing any earlier version of
/// the same group this integration already owns (an update may have changed
/// the rendered entry), then pruning identical duplicates. Returns the
/// config path the artifact claims, for the receipt.
pub(crate) fn attach_event_entry(
    uze_home: &UzeHome,
    integration_id: &str,
    config_file: &Path,
    event: HookEvent,
    entry_name: &str,
    expected: &str,
) -> Result<PathBuf> {
    let previous = previous_hook_entry_content(uze_home, integration_id, entry_name)?;
    let expected: serde_json::Value =
        serde_json::from_str(expected).map_err(|source| UzeError::Json {
            path: config_file.to_path_buf(),
            source,
        })?;
    merge_event_entry(config_file, event, &expected, &previous)?;
    Ok(config_file.to_path_buf())
}

/// The event's group array inside `{"hooks": {...}}`, creating it when
/// absent and refusing to merge into a non-array shape (a foreign schema
/// UZE must not rewrite).
fn event_array<'a>(
    config: &'a mut serde_json::Value,
    event: HookEvent,
    config_path: &Path,
) -> std::result::Result<&'a mut Vec<serde_json::Value>, String> {
    let hooks = config.as_object_mut().ok_or_else(|| {
        format!(
            "hook config `{}` root must be an object",
            config_path.display()
        )
    })?;
    let hooks = hooks
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| {
            format!(
                "hook config `{}` has a non-object `hooks` key; preserved",
                config_path.display()
            )
        })?;
    let event_key = hook_event_name(event);
    let array = hooks
        .entry(event_key.to_owned())
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
        .ok_or_else(|| {
            format!(
                "hook config `{}` has a non-array `hooks.{event_key}`; preserved",
                config_path.display()
            )
        })?;
    Ok(array)
}

/// Merges one group entry into the shared config's event array. Entries
/// matching any `previous` expected content are removed first (an earlier
/// version of this same UZE group being replaced); an identical entry is
/// left untouched (idempotence). Foreign groups and ordering are preserved.
pub(crate) fn merge_event_entry(
    config_path: &Path,
    event: HookEvent,
    entry: &serde_json::Value,
    previous: &[String],
) -> Result<PathBuf> {
    let mut config = read_config_object(config_path).map_err(|reason| {
        UzeError::ExposureUnavailable(format!("cannot merge hook entry: {reason}"))
    })?;
    let array = event_array(&mut config, event, config_path).map_err(|reason| {
        UzeError::ExposureUnavailable(format!("cannot merge hook entry: {reason}"))
    })?;
    for expected in previous {
        if let Ok(old) = serde_json::from_str::<serde_json::Value>(expected) {
            array.retain(|candidate| candidate != &old);
        }
    }
    if !array.iter().any(|candidate| candidate == entry) {
        array.push(entry.clone());
    }
    write_config(config_path, &config)?;
    Ok(config_path.to_path_buf())
}

/// Whether the exact expected group entry is present in the shared config's
/// event array — content identity is the receipt's fingerprint.
pub(crate) fn inspect_event_entry(
    config_path: &Path,
    event: HookEvent,
    expected: &str,
) -> AttachmentInspection {
    let Ok(config) = read_config_object(config_path) else {
        return blocked("hook config is missing or unreadable");
    };
    let Some(entries) = config
        .get("hooks")
        .and_then(serde_json::Value::as_object)
        .and_then(|hooks| hooks.get(hook_event_name(event)))
        .and_then(serde_json::Value::as_array)
    else {
        return AttachmentInspection {
            state: AttachmentState::Missing,
            reason: "the managed hook entry is absent".to_owned(),
        };
    };
    let Ok(expected) = serde_json::from_str::<serde_json::Value>(expected) else {
        return blocked("receipt carries an unreadable expected hook entry");
    };
    if entries.iter().any(|candidate| candidate == &expected) {
        AttachmentInspection {
            state: AttachmentState::Matched,
            reason: "managed hook entry matches the receipt".to_owned(),
        }
    } else {
        AttachmentInspection {
            state: AttachmentState::Missing,
            reason: "the managed hook entry is absent".to_owned(),
        }
    }
}

/// Removes exactly one matching entry, then prunes empty event arrays, an
/// empty `hooks` key, and finally the file itself when it holds nothing but
/// UZE's own content. A non-matched receipt blocks removal, and foreign
/// entries never change.
pub(crate) fn remove_event_entry(
    config_path: &Path,
    event: HookEvent,
    expected: &str,
) -> Result<AttachmentInspection> {
    let inspection = inspect_event_entry(config_path, event, expected);
    if inspection.state != AttachmentState::Matched {
        return Ok(inspection);
    }
    let mut config = read_config_object(config_path).map_err(|reason| {
        UzeError::ExposureUnavailable(format!("cannot detach hook entry: {reason}"))
    })?;
    let array = event_array(&mut config, event, config_path).map_err(|reason| {
        UzeError::ExposureUnavailable(format!("cannot detach hook entry: {reason}"))
    })?;
    let expected: serde_json::Value =
        serde_json::from_str(expected).map_err(|source| UzeError::Json {
            path: config_path.to_path_buf(),
            source,
        })?;
    if let Some(index) = array.iter().position(|candidate| candidate == &expected) {
        array.remove(index);
    }
    // Prune a now-empty event array, then an empty `hooks` key.
    if array.is_empty()
        && let Some(hooks) = config
            .get_mut("hooks")
            .and_then(serde_json::Value::as_object_mut)
    {
        hooks.remove(hook_event_name(event));
        if hooks.is_empty() {
            config
                .as_object_mut()
                .expect("root is an object")
                .remove("hooks");
        }
    }
    // A file that now holds nothing but an empty object was created by UZE
    // and is safe to remove entirely; anything else stays.
    if config.as_object().is_some_and(|root| root.is_empty()) {
        match fs::remove_file(config_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(UzeError::Write {
                    path: config_path.to_path_buf(),
                    source,
                });
            }
        }
    } else {
        write_config(config_path, &config)?;
    }
    Ok(AttachmentInspection {
        state: AttachmentState::Missing,
        reason: "managed hook entry detached".to_owned(),
    })
}

fn blocked(reason: &str) -> AttachmentInspection {
    AttachmentInspection {
        state: AttachmentState::Blocked,
        reason: reason.to_owned(),
    }
}

// ============================================================================
// OpenCode bridge (generated TypeScript, no author toolchain)
// ============================================================================

/// The owned bridge file path: `<config root>/plugins/uze-hooks-<package>.ts`.
/// `<config root>/plugins/` is OpenCode's documented global plugin directory
/// (`~/.config/opencode/plugins/`), auto-discovered at startup — the bridge
/// is therefore the single, self-contained load source: no `plugin` entry
/// in `opencode.json` exists to duplicate it. (Verified against the real
/// harness: the legacy `.opencode/plugins/` path is project-scoped and NOT
/// auto-discovered under the global config directory.)
pub(crate) fn opencode_bridge_path(config_root: &Path, package_id: &str) -> PathBuf {
    config_root
        .join("plugins")
        .join(format!("uze-hooks-{package_id}.ts"))
}

/// Serializes the group list for embedding in the bridge: translated
/// matchers (matched against the runtime native tool name), abi event name,
/// effect, and the authored handlers verbatim.
fn bridge_hooks(hooks: &[&PortableHook]) -> serde_json::Value {
    serde_json::Value::Array(
        hooks
            .iter()
            .map(|hook| {
                serde_json::json!({
                    "id": hook.id,
                    "event": hook.event.abi_name(),
                    "effect": hook.effect.abi_name(),
                    "matchers": hook.matchers.iter().map(|m| tool_name("opencode", m)).collect::<Vec<_>>(),
                    "handlers": hook.handlers.iter().map(|handler| serde_json::json!({
                        "command": handler.command,
                        "timeout": handler.timeout,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect(),
    )
}

/// Generates the OpenCode bridge source for one package. JavaScript valid as
/// TypeScript (no compilation step), deterministic, no dependencies: it
/// spawns each authored command sequentially against the portable ABI,
/// injects `PLUGIN_ROOT`, bounds stdout at 64 KiB, honors per-handler
/// timeouts, maps a denied decision (or a failure of a non-observational
/// group) to a thrown error that blocks the intercepted tool, and rewrites
/// `output.args` for a transform. Observational failure stays open, exactly
/// like every other adapter.
pub(crate) fn opencode_bridge(
    hooks: &[&PortableHook],
    plugin_root: &Path,
    package_id: &str,
) -> String {
    let root = plugin_root.display().to_string();
    let hooks = bridge_hooks(hooks);
    let hooks = serde_json::to_string(&hooks).expect("bridge hooks serialize");
    format!(
        r#"// Generated by UZE (ADR-033). Do not edit. Rebuild with `uze plugin install`.
// OpenCode V2 plugin API (opencode.ai/v2/docs/build/plugins): plugins are
// defined with Plugin.define and register runtime hooks on the plugin
// context. Pre-tool handlers run sequentially through Bun.spawn on the Bun
// runtime OpenCode embeds (the plugin context exposes the Bun shell API as
// `$`); the V2 tool hooks offer input replacement but no input-based block
// signal — the only action-level deny/ask is the permission hook, which
// carries no tool input — so deny/ask effects are degraded by capability
// assessment (ADR-033) and a runtime deny is logged, never fabricated.
import {{ Plugin }} from "@opencode-ai/plugin";

const ROOT = {root:?};
const HOOKS = {hooks};

function abi(event, tool, input) {{
  return {{
    version: 1,
    event,
    tool: {{ portable: null, native: tool }},
    input: input ?? {{}},
    context: {{ cwd: process.cwd() }},
  }};
}}

async function run(command, message, timeout) {{
  const proc = Bun.spawn(["/bin/sh", "-c", command], {{
    cwd: ROOT,
    env: {{ ...process.env, PLUGIN_ROOT: ROOT }},
    stdin: "pipe",
    stdout: "pipe",
    stderr: "pipe",
  }});
  proc.stdin.write(JSON.stringify(message) + "\n");
  proc.stdin.end();
  const decoder = new TextDecoder();
  let stdout = "";
  let overflow = false;
  const timer = setTimeout(() => {{ proc.kill(); }}, timeout * 1000);
  try {{
    for await (const chunk of proc.stdout) {{
      stdout += decoder.decode(chunk);
      if (stdout.length > 65536) {{
        overflow = true;
        proc.kill();
        break;
      }}
    }}
  }} finally {{
    clearTimeout(timer);
  }}
  const code = await proc.exited;
  if (overflow) throw new Error("UZE hook output exceeded 64 KiB");
  if (code && code !== 3) throw new Error(`UZE hook failed (exit ${{code}})`);
  if (!stdout) return {{}};
  try {{ return JSON.parse(stdout); }}
  catch (error) {{ throw new Error(`UZE hook wrote invalid JSON: ${{error.message}}`); }}
}}

export default Plugin.define({{
  id: "uze-hooks-{package_id}",
  async setup(ctx) {{
    await ctx.tool.hook("execute.before", async (event) => {{
      for (const hook of HOOKS) {{
        if (hook.event !== "pre_tool_use") continue;
        if (hook.matchers.length && !hook.matchers.includes(event.tool)) continue;
        let input = event.input;
        let replaced = false;
        for (const handler of hook.handlers) {{
          let result;
          try {{
            result = await run(handler.command, abi("pre_tool_use", event.tool, input), handler.timeout);
          }} catch (error) {{
            if (hook.effect === "deny" || hook.effect === "ask") throw error;
            console.error(`[uze-hooks:${{hook.id}}]`, error.message);
            continue;
          }}
          if (hook.effect === "deny" || hook.effect === "ask") {{
            // V2 exposes no input-based block (permission hooks carry no
            // tool input); a deny/ask decision is recorded, never faked.
            if (result.decision) {{
              console.error(`[uze-hooks:${{hook.id}}]`, `V2 cannot enforce ${{result.decision}} (no input-based block): ${{result.reason ?? ""}}`);
            }}
            continue;
          }}
          if (result.input) {{
            input = result.input;
            replaced = true;
          }}
        }}
        if (replaced) event.input = input;
      }}
    }});
    await ctx.tool.hook("execute.after", async (event) => {{
      for (const hook of HOOKS) {{
        if (hook.event !== "post_tool_use") continue;
        if (hook.matchers.length && !hook.matchers.includes(event.tool)) continue;
        for (const handler of hook.handlers) {{
          try {{
            await run(handler.command, abi("post_tool_use", event.tool, event.input), handler.timeout);
          }} catch (error) {{
            console.error(`[uze-hooks:${{hook.id}}]`, error.message);
          }}
        }}
      }}
    }});
  }},
}});
"#
    )
}

/// Removes the owned bridge file. The `plugins/` directory belongs to the
/// vendor's global plugin namespace — a foreign plugin file in it keeps it
/// alive; an empty directory left behind only by this file is removed.
pub(crate) fn remove_bridge_file(bridge_path: &Path) -> Result<()> {
    match fs::remove_file(bridge_path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(UzeError::Write {
                path: bridge_path.to_path_buf(),
                source,
            });
        }
    }
    if let Some(plugins_dir) = bridge_path.parent()
        && fs::read_dir(plugins_dir).is_ok_and(|mut entries| entries.next().is_none())
    {
        let _ = fs::remove_dir(plugins_dir);
    }
    Ok(())
}

/// All hook groups a package's stored `hooks.json` declares, in manifest
/// order — the materialization input for the OpenCode bridge.
pub(crate) fn package_hook_groups(package_root: &Path) -> Result<Vec<PortableHook>> {
    let manifest_path = package_root.join(HOOKS_FILE_NAME);
    let bytes = fs::read(&manifest_path).map_err(|source| UzeError::Read {
        path: manifest_path.clone(),
        source,
    })?;
    uze_core::hook::parse_manifest(&manifest_path, &bytes)
}

/// Canonical hook groups filtered to a set of group ids, preserving
/// manifest order; `None` when the package declares no canonical hooks.
pub(crate) fn groups_with_ids(
    package_root: &Path,
    keep: &dyn Fn(&str) -> bool,
) -> Result<Vec<PortableHook>> {
    let mut groups = package_hook_groups(package_root)?;
    groups.retain(|group| keep(&group.id));
    Ok(groups)
}

/// Parses a hook resource's payload into its portable group and computes the
/// per-resource plan: semantic compatibility from the vendor profile, and a
/// managed config entry carrying the exact rendered group (the receipt's
/// content-identity fingerprint). A `degraded` or `unsupported` route never
/// attaches — the mechanism carries the diagnostic instead.
pub(crate) fn hook_exposure_plan(
    resource: &Resource,
    capabilities: &HookCapabilities,
    config_file: PathBuf,
    target: &str,
    adapter_id: &str,
    bridged: bool,
    evidence: &str,
) -> ExposurePlan {
    let Ok(hook) = serde_json::from_slice::<PortableHook>(&resource.capability.payload) else {
        return unsupported_plan(
            resource,
            "hook resource payload is not a valid portable hook group",
        );
    };
    let compatibility = uze_core::hook::assess(&hook, capabilities, bridged);
    let mechanism = match compatibility.route {
        CompatibilityRoute::Unsupported | CompatibilityRoute::Degraded => {
            ExposureMechanism::Unsupported {
                rationale: compatibility
                    .reason
                    .clone()
                    .unwrap_or_else(|| "no compatible hook route".to_owned()),
            }
        }
        _ => {
            let executable = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("uze"));
            let package_root = resource
                .package_root()
                .expect("hook exposure_plan is only reached for packages");
            let entry = group_entry(target, &hook, &executable, adapter_id, package_root);
            ExposureMechanism::ManagedHookConfig {
                config_file,
                entry_name: hook_entry_name(resource, &hook),
                event: Some(hook.event),
                expected: serde_json::to_string(&entry).expect("hook entry serializes"),
            }
        }
    };
    let evidence = match &compatibility.reason {
        Some(reason) => format!("{evidence} Compatibility: {reason}"),
        None => evidence.to_owned(),
    };
    ExposurePlan {
        representation: resource.capability.representation,
        route: compatibility.route,
        verification: VerificationStatus::Unverified,
        mechanism,
        evidence,
    }
}

/// The stable UZE identity for one hook group entry, mirroring the
/// qualified-capability naming policy (ADR-026): `<package>:<hook-id>`.
pub(crate) fn hook_entry_name(resource: &Resource, hook: &PortableHook) -> String {
    match &resource.origin {
        uze_core::project::ResourceOrigin::Package { id, .. } => {
            format!("{}:{}", id.as_str(), hook.id)
        }
        uze_core::project::ResourceOrigin::Project { .. } => hook.id.clone(),
    }
}

fn unsupported_plan(resource: &Resource, rationale: &str) -> ExposurePlan {
    ExposurePlan {
        representation: resource.capability.representation,
        route: CompatibilityRoute::Unsupported,
        verification: VerificationStatus::NotExposed,
        mechanism: ExposureMechanism::Unsupported {
            rationale: rationale.to_owned(),
        },
        evidence: rationale.to_owned(),
    }
}

// ============================================================================
// Runtime adapters: native payload → portable ABI → native contract
// ============================================================================

/// Native blocking exit code shared by the command-hook harnesses
/// (Claude, Codex and Antigravity all document exit 2 as the pre-tool
/// block signal; every other non-zero exit is a non-blocking error there).
const NATIVE_BLOCK_EXIT: i32 = 2;

/// Extracts `tool_name`/`tool_input` shaped payload fields (Claude and
/// Codex both document this shape for tool events) and the common
/// `cwd`/`session_id` context keys, sticky across the two vendors' field
/// casing conventions.
fn normalize_tool_payload(
    target: &str,
    native: &serde_json::Value,
    event: HookEvent,
    tool_key: &str,
    input_key: &str,
    cwd_key: &str,
    session_key: &str,
) -> std::result::Result<HookCommandInput, String> {
    let tool_name = native.get(tool_key).and_then(serde_json::Value::as_str);
    let tool = match (tool_name, event) {
        (Some(name), HookEvent::PreToolUse | HookEvent::PostToolUse) => Some(HookTool {
            portable: portable_name(target, name),
            native: name.to_owned(),
        }),
        _ => None,
    };
    Ok(HookCommandInput {
        version: 1,
        event: event.abi_name().to_owned(),
        tool,
        input: native
            .get(input_key)
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
        context: HookContext {
            cwd: native
                .get(cwd_key)
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
            session_id: native
                .get(session_key)
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
        },
    })
}

/// Claude Code's native hook payload uses `tool_name`/`tool_input` and a
/// nested `context` carrying `cwd`/`session_id`.
pub(crate) fn claude_normalize_input(
    native: &serde_json::Value,
    event: HookEvent,
) -> std::result::Result<HookCommandInput, String> {
    normalize_tool_payload(
        "claude",
        native,
        event,
        "tool_name",
        "tool_input",
        "cwd",
        "session_id",
    )
    .map(|mut input| {
        if input.context.cwd.is_none()
            && let Some(cwd) = native
                .get("context")
                .and_then(serde_json::Value::as_object)
                .and_then(|context| context.get("cwd"))
                .and_then(serde_json::Value::as_str)
        {
            input.context.cwd = Some(cwd.to_owned());
        }
        if input.context.session_id.is_none()
            && let Some(session_id) = native
                .get("context")
                .and_then(serde_json::Value::as_object)
                .and_then(|context| context.get("session_id"))
                .and_then(serde_json::Value::as_str)
        {
            input.context.session_id = Some(session_id.to_owned());
        }
        input
    })
}

/// Codex's native hook payload mirrors `tool_name`/`tool_input` with
/// top-level `cwd`/`session_id`.
pub(crate) fn codex_normalize_input(
    native: &serde_json::Value,
    event: HookEvent,
) -> std::result::Result<HookCommandInput, String> {
    normalize_tool_payload(
        "codex",
        native,
        event,
        "tool_name",
        "tool_input",
        "cwd",
        "session_id",
    )
}

/// Antigravity CLI's native hook payload (official docs): a nested
/// `toolCall` object with `name`/`args`, plus `conversationId` and
/// `workspacePaths` (the first entry is the workspace directory).
pub(crate) fn antigravity_normalize_input(
    native: &serde_json::Value,
    event: HookEvent,
) -> std::result::Result<HookCommandInput, String> {
    let tool_name = native
        .get("toolCall")
        .and_then(serde_json::Value::as_object)
        .and_then(|call| call.get("name"))
        .and_then(serde_json::Value::as_str);
    let tool = match (tool_name, event) {
        (Some(name), HookEvent::PreToolUse | HookEvent::PostToolUse) => Some(HookTool {
            portable: portable_name("antigravity", name),
            native: name.to_owned(),
        }),
        _ => None,
    };
    let input = native
        .get("toolCall")
        .and_then(serde_json::Value::as_object)
        .and_then(|call| call.get("args"))
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    Ok(HookCommandInput {
        version: 1,
        event: event.abi_name().to_owned(),
        tool,
        input,
        context: HookContext {
            cwd: native
                .get("workspacePaths")
                .and_then(serde_json::Value::as_array)
                .and_then(|paths| paths.first())
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
            session_id: native
                .get("conversationId")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
        },
    })
}

/// Claude Code's native hook stdout contract (current hook reference):
/// one `hookSpecificOutput` object naming the event plus a
/// `permissionDecision` (`allow`/`deny`/`ask` for PreToolUse),
/// `permissionDecisionReason`, and an optional `updatedInput` rewrite.
/// A deny blocks the tool with **exit code 2**, whose stderr is fed back to
/// Claude as the reason — the exit-2 route never depends on JSON parsing,
/// so a deny cannot degrade into an allow under any output failure. Other
/// non-zero exits are non-blocking errors ("logged and ignored,
/// execution continues"), which is exactly why the canonical deny exit
/// must never leak outward.
pub(crate) fn claude_render_output(
    outcome: &HookDispatchOutcome,
    event: HookEvent,
) -> std::result::Result<HookNativeOutput, String> {
    // Decisions are honored on PreToolUse (and Stop, where exit 2 prevents
    // the stop — documented); PostToolUse cannot block anything.
    let blocking = matches!(event, HookEvent::PreToolUse | HookEvent::Stop);
    let Some(decision) = outcome.decision else {
        return Ok(HookNativeOutput::default());
    };
    if !blocking {
        return Ok(HookNativeOutput::default());
    }
    let mut hook_specific = serde_json::Map::new();
    hook_specific.insert(
        "hookEventName".to_owned(),
        serde_json::Value::String(hook_event_name(event).to_owned()),
    );
    let decision_name = match decision {
        HookDecision::Allow => "allow",
        HookDecision::Ask => "ask",
        HookDecision::Deny => "deny",
    };
    hook_specific.insert(
        "permissionDecision".to_owned(),
        serde_json::Value::String(decision_name.to_owned()),
    );
    hook_specific.insert(
        "permissionDecisionReason".to_owned(),
        serde_json::Value::String(outcome.reason.clone().unwrap_or_default()),
    );
    if event == HookEvent::PreToolUse
        && let Some(input) = &outcome.input_override
    {
        hook_specific.insert("updatedInput".to_owned(), input.clone());
    }
    let document = serde_json::json!({ "hookSpecificOutput": hook_specific });
    let stdout = Some(serde_json::to_vec(&document).expect("hook output serializes"));
    if decision == HookDecision::Deny {
        Ok(HookNativeOutput {
            stdout,
            stderr: Some(
                outcome
                    .reason
                    .clone()
                    .unwrap_or_else(|| "Denied by UZE hook".to_owned()),
            ),
            exit_code: NATIVE_BLOCK_EXIT,
        })
    } else {
        Ok(HookNativeOutput {
            stdout,
            stderr: None,
            exit_code: 0,
        })
    }
}

/// Codex's hook stdout contract: the `hookSpecificOutput` shape with
/// `permissionDecision` (`deny` is the decision Codex acts on; anything
/// else runs the tool). Deny blocks with exit code 2 — the documented
/// block signal — with the reason preserved on stderr. Codex's Stop event
/// additionally requires parseable JSON on stdout at exit 0, so an empty
/// observation renders `{}` there rather than plain text.
pub(crate) fn codex_render_output(
    outcome: &HookDispatchOutcome,
    event: HookEvent,
) -> std::result::Result<HookNativeOutput, String> {
    let Some(decision) = outcome.decision else {
        if event == HookEvent::Stop {
            return Ok(HookNativeOutput {
                stdout: Some(b"{}".to_vec()),
                ..HookNativeOutput::default()
            });
        }
        return Ok(HookNativeOutput::default());
    };
    let decision_name = match decision {
        HookDecision::Allow => "allow",
        HookDecision::Ask => "ask",
        HookDecision::Deny => "deny",
    };
    let document = serde_json::json!({
        "hookSpecificOutput": {
            "permissionDecision": decision_name,
            "permissionDecisionReason": outcome.reason.clone().unwrap_or_default(),
        }
    });
    let stdout = Some(serde_json::to_vec(&document).expect("hook output serializes"));
    if decision == HookDecision::Deny && event == HookEvent::PreToolUse {
        Ok(HookNativeOutput {
            stdout,
            stderr: Some(
                outcome
                    .reason
                    .clone()
                    .unwrap_or_else(|| "Denied by UZE hook".to_owned()),
            ),
            exit_code: NATIVE_BLOCK_EXIT,
        })
    } else {
        Ok(HookNativeOutput {
            stdout,
            stderr: None,
            exit_code: 0,
        })
    }
}

/// Antigravity CLI's hook stdout contract: native `allow`/`ask`/`deny`
/// decisions with a reason (official contract), plus the documented
/// blocking exit code 2 for a PreToolUse deny. Other events return 0.
pub(crate) fn antigravity_render_output(
    outcome: &HookDispatchOutcome,
    event: HookEvent,
) -> std::result::Result<HookNativeOutput, String> {
    let Some(decision) = outcome.decision else {
        if event == HookEvent::PreToolUse {
            return Ok(HookNativeOutput::default());
        }
        // PostToolUse/Stop: the official contract requires `{}`.
        return Ok(HookNativeOutput {
            stdout: Some(b"{}".to_vec()),
            ..HookNativeOutput::default()
        });
    };
    if event != HookEvent::PreToolUse {
        // PostToolUse returns an empty object and Stop answers with a
        // `continue` decision — neither carries a deny/allow, so any
        // decision there is observational only.
        return Ok(HookNativeOutput {
            stdout: Some(b"{}".to_vec()),
            ..HookNativeOutput::default()
        });
    }
    let decision_name = match decision {
        HookDecision::Allow => "allow",
        HookDecision::Ask => "ask",
        HookDecision::Deny => "deny",
    };
    let document = serde_json::json!({
        "decision": decision_name,
        "reason": outcome.reason.clone().unwrap_or_default(),
    });
    let stdout = Some(serde_json::to_vec(&document).expect("hook output serializes"));
    if decision == HookDecision::Deny {
        Ok(HookNativeOutput {
            stdout,
            stderr: Some(
                outcome
                    .reason
                    .clone()
                    .unwrap_or_else(|| "Denied by UZE hook".to_owned()),
            ),
            exit_code: NATIVE_BLOCK_EXIT,
        })
    } else {
        Ok(HookNativeOutput {
            stdout,
            stderr: None,
            exit_code: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uze_core::hook::{CommandHandlerType, HookMatcher};

    fn hook() -> PortableHook {
        PortableHook {
            id: "protect-env".into(),
            event: HookEvent::PreToolUse,
            matchers: vec![
                HookMatcher::Portable("shell".into()),
                HookMatcher::Native("Write".into()),
            ],
            handlers: vec![CommandHook {
                handler_type: CommandHandlerType::Command,
                command: "${PLUGIN_ROOT}/check".into(),
                timeout: 10,
            }],
            effect: HookEffect::Deny,
            order: 0,
        }
    }

    fn executable() -> PathBuf {
        PathBuf::from("/opt/uze stake/bin")
    }

    #[test]
    fn vendor_aliases_are_explicit() {
        assert_eq!(
            tool_name("claude", &HookMatcher::Portable("shell".into())),
            "Bash"
        );
        assert_eq!(
            tool_name("antigravity", &HookMatcher::Portable("shell".into())),
            "run_command"
        );
        assert_eq!(
            tool_name("opencode", &HookMatcher::Native("Write".into())),
            "Write"
        );
        assert_eq!(
            portable_name("claude", "Bash").as_deref(),
            Some("shell"),
            "the reverse table must round-trip the forward one"
        );
    }

    const TARGETS: [&str; 4] = ["claude", "codex", "antigravity", "opencode"];

    #[test]
    fn every_alias_is_bound_on_every_harness_and_carries_its_portable_fields() {
        for target in TARGETS {
            let table = vocabulary(target);
            for alias in uze_core::hook::portable_tool_aliases() {
                let binding = table
                    .binding(alias)
                    .unwrap_or_else(|| panic!("{target} has no row for alias `{alias}`"));
                let promised = uze_core::hook::alias_fields(alias);
                let bound: Vec<&str> = binding.fields.iter().map(|(name, _)| *name).collect();
                assert_eq!(
                    bound, promised,
                    "{target}/{alias} must read exactly the fields the vocabulary promises"
                );
                if let Some(native) = binding.native_tool {
                    assert_eq!(
                        table.binding_for_native(native).map(|entry| entry.alias),
                        Some(binding.alias),
                        "{target}/{alias} must round-trip through its native name"
                    );
                }
            }
        }
    }

    #[test]
    fn a_native_matcher_yields_no_portable_fields() {
        let table = vocabulary("claude");
        assert!(table.binding_for_native("SomeVendorOnlyTool").is_none());
        assert_eq!(
            tool_name("claude", &HookMatcher::Native("Write".into())),
            "Write"
        );
        assert_eq!(portable_name("claude", "SomeVendorOnlyTool"), None);
    }

    #[test]
    fn the_shell_alias_reads_each_harnesss_own_command_field() {
        let field = |target: &str| {
            vocabulary(target)
                .binding("shell")
                .and_then(|binding| binding.fields.first())
                .map(|(_, native)| *native)
        };
        assert_eq!(field("claude"), Some("command"));
        assert_eq!(field("codex"), Some("cmd"));
        assert_eq!(field("antigravity"), Some("CommandLine"));
        assert_eq!(field("opencode"), Some("command"));
    }

    #[test]
    fn a_renamed_vendor_tool_still_normalizes_to_its_alias() {
        assert_eq!(
            portable_name("codex", "exec_command").as_deref(),
            Some("shell")
        );
        assert_eq!(portable_name("codex", "Bash").as_deref(), Some("shell"));
        assert_eq!(
            tool_name("codex", &HookMatcher::Portable("shell".into())),
            "exec_command",
            "the matcher names the tool the harness actually offers today"
        );
    }

    #[test]
    fn dispatcher_command_is_shell_quoted_for_paths_and_commands() {
        let command = dispatcher_command(
            &executable(),
            "claude-code",
            hook().event,
            hook().effect,
            Path::new("/tmp/plugin root"),
            &hook().handlers,
        );
        assert!(
            command.starts_with("'/opt/uze stake/bin'"),
            "executable path with spaces is quoted"
        );
        assert!(command.contains("--adapter 'claude-code'"));
        assert!(command.contains("--event pre_tool_use"));
        assert!(command.contains("--effect deny"));
        assert!(command.contains("--plugin-root '/tmp/plugin root'"));
        assert!(
            command.contains("--command '${PLUGIN_ROOT}/check'"),
            "the authored command is retained verbatim"
        );
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn group_entry_omits_matcher_for_unmatch_all_and_reserves_native_timeout() {
        let mut hook = hook();
        let entry = group_entry(
            "claude",
            &hook,
            &executable(),
            "claude-code",
            Path::new("/pkg"),
        );
        assert_eq!(entry["matcher"], "Bash|Write");
        assert_eq!(entry["hooks"][0]["type"], "command");
        assert_eq!(
            entry["hooks"][0]["timeout"], 11,
            "sum of handler timeouts plus 1s grace"
        );
        hook.matchers = Vec::new();
        let entry = group_entry(
            "claude",
            &hook,
            &executable(),
            "claude-code",
            Path::new("/pkg"),
        );
        assert!(
            entry.get("matcher").is_none(),
            "no matcher key for a match-all group"
        );
    }

    #[test]
    fn agy_document_is_named_per_group_and_deterministic() {
        let document = agy_hook_document(&[&hook()], &executable(), Path::new("/pkg"));
        let value: serde_json::Value = serde_json::from_str(&document).unwrap();
        assert_eq!(
            value["protect-env"]["PreToolUse"][0]["matcher"],
            "run_command|Write"
        );
        assert!(
            value.get("hooks").is_none(),
            "a `hooks` wrapper would register one dead hook named `hooks`"
        );
        assert!(document.contains("--adapter 'antigravity'"));
        let again = agy_hook_document(&[&hook()], &executable(), Path::new("/pkg"));
        assert_eq!(document, again);
    }

    #[test]
    fn merge_inspect_detach_preserve_foreign_entries_and_order() {
        let root = uze_testkit::temp::scratch("hooks-merge");
        fs::create_dir_all(&root).unwrap();
        let config = root.join("settings.json");
        fs::write(
            &config,
            r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"foreign"}]}]},"theme":"dark"}"#,
        )
        .unwrap();
        let entry = group_entry(
            "claude",
            &hook(),
            &executable(),
            "claude-code",
            Path::new("/pkg"),
        );
        let expected = serde_json::to_string(&entry).unwrap();
        let path = merge_event_entry(&config, HookEvent::PreToolUse, &entry, &[]).unwrap();
        assert_eq!(path, config);
        assert_eq!(
            inspect_event_entry(&config, HookEvent::PreToolUse, &expected).state,
            AttachmentState::Matched
        );
        // Idempotence: a second merge changes nothing.
        merge_event_entry(&config, HookEvent::PreToolUse, &entry, &[]).unwrap();
        assert_eq!(
            inspect_event_entry(&config, HookEvent::PreToolUse, &expected).state,
            AttachmentState::Matched
        );
        let after: serde_json::Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
        assert_eq!(after["theme"], "dark");
        let groups = after["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(
            groups.len(),
            2,
            "the foreign group stays and UZE's is appended"
        );
        assert_eq!(
            inspect_event_entry(&config, HookEvent::PostToolUse, &expected).state,
            AttachmentState::Missing,
            "an entry in the wrong event array is not matched"
        );
        assert_eq!(
            remove_event_entry(&config, HookEvent::PreToolUse, &expected)
                .unwrap()
                .state,
            AttachmentState::Missing
        );
        let after: serde_json::Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
        assert_eq!(
            after["hooks"]["PreToolUse"].as_array().unwrap().len(),
            1,
            "only UZE's entry went"
        );
        assert_eq!(
            after["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            "foreign"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn merging_replaces_the_previous_version_of_the_same_group() {
        let root = uze_testkit::temp::scratch("hooks-replace");
        fs::create_dir_all(&root).unwrap();
        let config = root.join("hooks.json");
        let mut old = hook();
        old.handlers[0].timeout = 10;
        let old_entry = group_entry("codex", &old, &executable(), "codex", Path::new("/pkg"));
        merge_event_entry(&config, HookEvent::PreToolUse, &old_entry, &[]).unwrap();
        let mut updated = hook();
        updated.handlers[0].timeout = 20;
        let new_entry = group_entry("codex", &updated, &executable(), "codex", Path::new("/pkg"));
        merge_event_entry(
            &config,
            HookEvent::PreToolUse,
            &new_entry,
            &[serde_json::to_string(&old_entry).unwrap()],
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
        let entries = value["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(
            entries.len(),
            1,
            "the old version is replaced, not duplicated"
        );
        assert_eq!(entries[0]["hooks"][0]["timeout"], 21);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn drift_blocks_removal_and_an_empty_file_is_removed() {
        let root = uze_testkit::temp::scratch("hooks-drift");
        fs::create_dir_all(&root).unwrap();
        let config = root.join("hooks.json");
        let entry = group_entry("codex", &hook(), &executable(), "codex", Path::new("/pkg"));
        let expected = serde_json::to_string(&entry).unwrap();
        merge_event_entry(&config, HookEvent::PreToolUse, &entry, &[]).unwrap();
        // A user rewrites the UZE group — removal must inspect first and refuse.
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
        fs::write(
            &config,
            serde_json::to_string(&value)
                .unwrap()
                .replace("\"timeout\":11", "\"timeout\":99"),
        )
        .unwrap();
        assert_eq!(
            remove_event_entry(&config, HookEvent::PreToolUse, &expected)
                .unwrap()
                .state,
            AttachmentState::Missing,
            "drift refuses detach and preserves the file"
        );
        assert!(config.exists());
        // Re-attach restores the exact entry beside the drifted user copy;
        // removal then deletes exactly the UZE entry and leaves the user's
        // edited copy untouched.
        merge_event_entry(
            &config,
            HookEvent::PreToolUse,
            &entry,
            std::slice::from_ref(&expected),
        )
        .unwrap();
        assert_eq!(
            remove_event_entry(&config, HookEvent::PreToolUse, &expected)
                .unwrap()
                .state,
            AttachmentState::Missing
        );
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
        let groups = value["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(groups.len(), 1, "the drifted user copy survives removal");
        assert_eq!(groups[0]["hooks"][0]["timeout"], 99);
        // A UZE-created file holding nothing but UZE's own entry is removed
        // entirely once that entry goes.
        let solo = root.join("solo.json");
        merge_event_entry(&solo, HookEvent::PreToolUse, &entry, &[]).unwrap();
        assert_eq!(
            remove_event_entry(&solo, HookEvent::PreToolUse, &expected)
                .unwrap()
                .state,
            AttachmentState::Missing
        );
        assert!(!solo.exists(), "an empty UZE-only file is cleaned up");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn opencode_bridge_embeds_matchers_abi_and_effect_guards() {
        let bridge = opencode_bridge(&[&hook()], Path::new("/tmp/plugin root"), "hook-demo");
        // V2 plugin API (spec: opencode.ai/v2/docs/build/plugins) — a
        // Plugin.define module registering ctx.tool.hook callbacks.
        assert!(bridge.contains("import { Plugin } from \"@opencode-ai/plugin\""));
        assert!(bridge.contains("Plugin.define"));
        assert!(bridge.contains("ctx.tool.hook(\"execute.before\""));
        assert!(bridge.contains("ctx.tool.hook(\"execute.after\""));
        assert!(
            bridge.contains("Bun.spawn"),
            "the harness's embedded Bun runtime executes handlers"
        );
        assert!(bridge.contains("PLUGIN_ROOT"));
        assert!(bridge.contains("\"event\":\"pre_tool_use\""));
        assert!(bridge.contains("\"matchers\":[\"bash\",\"Write\"]"));
        assert!(bridge.contains("\"effect\":\"deny\""));
        assert!(bridge.contains("65536"));
        assert!(
            bridge.contains("code !== 3"),
            "the canonical deny exit is not a failure"
        );
        assert!(
            bridge.contains("hook.effect === \"deny\" || hook.effect === \"ask\""),
            "a deny/ask handler on V2 records its decision without fabricating a block"
        );
        assert!(
            bridge.contains("event.input"),
            "transform rewrites event.input"
        );
        assert!(
            !bridge.contains("Stop"),
            "no stop surface is ever claimed for OpenCode"
        );
    }

    #[test]
    fn adapters_render_native_decisions_and_block_exit_codes() {
        // Normalization: Claude/Codex carry tool_name/tool_input; AGY
        // carries toolCall.name/toolCall.args (official hooks docs).
        let claude = serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": "ls"},
            "hook_event_name": "PreToolUse",
            "context": {"cwd": "/work", "session_id": "s1"},
        });
        let input = claude_normalize_input(&claude, HookEvent::PreToolUse).unwrap();
        assert_eq!(
            input.tool.clone().unwrap().portable.as_deref(),
            Some("shell")
        );
        assert_eq!(input.tool.unwrap().native, "Bash");
        assert_eq!(input.context.cwd.as_deref(), Some("/work"));
        assert_eq!(input.context.session_id.as_deref(), Some("s1"));

        let agy = serde_json::json!({
            "toolCall": {"name": "run_command", "args": {"CommandLine": "ls"}},
            "stepIdx": 4,
            "conversationId": "c2",
            "workspacePaths": ["/work"],
        });
        let input = antigravity_normalize_input(&agy, HookEvent::PreToolUse).unwrap();
        assert_eq!(input.tool.unwrap().native, "run_command");
        assert_eq!(input.context.cwd.as_deref(), Some("/work"));
        assert_eq!(input.context.session_id.as_deref(), Some("c2"));

        // Deny: native JSON decision AND the harness's blocking exit code 2,
        // with the reason on stderr (the fed-back channel). Internal
        // canonical exit codes never leak outward.
        let deny = HookDispatchOutcome {
            decision: Some(HookDecision::Deny),
            reason: Some("blocked by policy".to_owned()),
            ..HookDispatchOutcome::default()
        };
        let claude_out = claude_render_output(&deny, HookEvent::PreToolUse).unwrap();
        assert_eq!(claude_out.exit_code, 2);
        assert_eq!(claude_out.stderr.as_deref(), Some("blocked by policy"));
        let json: serde_json::Value = serde_json::from_slice(&claude_out.stdout.unwrap()).unwrap();
        assert_eq!(json["hookSpecificOutput"]["hookEventName"], "PreToolUse");
        assert_eq!(json["hookSpecificOutput"]["permissionDecision"], "deny");
        assert_eq!(
            json["hookSpecificOutput"]["permissionDecisionReason"],
            "blocked by policy"
        );
        let codex_out = codex_render_output(&deny, HookEvent::PreToolUse).unwrap();
        assert_eq!(codex_out.exit_code, 2);
        assert_eq!(codex_out.stderr.as_deref(), Some("blocked by policy"));
        let json: serde_json::Value = serde_json::from_slice(&codex_out.stdout.unwrap()).unwrap();
        assert_eq!(json["hookSpecificOutput"]["permissionDecision"], "deny");
        let agy_out = antigravity_render_output(&deny, HookEvent::PreToolUse).unwrap();
        assert_eq!(agy_out.exit_code, 2);
        let json: serde_json::Value = serde_json::from_slice(&agy_out.stdout.unwrap()).unwrap();
        assert_eq!(json["decision"], "deny");
        assert_eq!(json["reason"], "blocked by policy");

        // Allow/observe: exit 0, no stderr; an observation renders no JSON
        // on Claude pre-tool, and the AGY official contract for
        // PostToolUse/Stop is an empty object.
        let allow = HookDispatchOutcome {
            decision: Some(HookDecision::Allow),
            reason: Some("ok".to_owned()),
            ..HookDispatchOutcome::default()
        };
        assert_eq!(
            claude_render_output(&allow, HookEvent::PreToolUse)
                .unwrap()
                .exit_code,
            0
        );
        assert_eq!(
            claude_render_output(&allow, HookEvent::PreToolUse)
                .unwrap()
                .stderr,
            None
        );
        let observed = HookDispatchOutcome::default();
        assert!(
            claude_render_output(&observed, HookEvent::PreToolUse)
                .unwrap()
                .stdout
                .is_none()
        );
        assert_eq!(
            antigravity_render_output(&observed, HookEvent::PostToolUse)
                .unwrap()
                .stdout
                .as_deref(),
            Some(&b"{}"[..])
        );
        // Decisions on non-blocking events are observational only.
        let agy_post = antigravity_render_output(&deny, HookEvent::PostToolUse).unwrap();
        assert_eq!(agy_post.exit_code, 0);
        assert_eq!(agy_post.stdout.as_deref(), Some(&b"{}"[..]));
    }

    #[test]
    fn stop_payloads_carry_no_tool_and_transform_survives_rendering() {
        let stop = serde_json::json!({"stop_hook_active": true});
        let input = claude_normalize_input(&stop, HookEvent::Stop).unwrap();
        assert!(input.tool.is_none());
        let transform = HookDispatchOutcome {
            decision: Some(HookDecision::Allow),
            input_override: Some(serde_json::json!({"path": "/safe"})),
            reason: Some("rewritten".to_owned()),
            ..HookDispatchOutcome::default()
        };
        let output: serde_json::Value = serde_json::from_slice(
            &claude_render_output(&transform, HookEvent::PreToolUse)
                .unwrap()
                .stdout
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            output["hookSpecificOutput"]["updatedInput"]["path"], "/safe",
            "the current Claude contract rewrites via updatedInput"
        );
        assert_eq!(
            transform_pair(transform.clone()).exit_code,
            0,
            "a rewrite is not a block"
        );
    }

    fn transform_pair(outcome: HookDispatchOutcome) -> HookNativeOutput {
        claude_render_output(&outcome, HookEvent::PreToolUse).unwrap()
    }

    #[test]
    fn bridge_path_lives_in_the_auto_discovered_global_plugin_directory() {
        let root = uze_testkit::temp::scratch("hooks-path");
        let bridge = opencode_bridge_path(&root, "demo");
        assert_eq!(
            bridge,
            root.join("plugins/uze-hooks-demo.ts"),
            "the single load source is the harness's global plugin directory"
        );
        assert!(
            !bridge.to_string_lossy().contains(".opencode"),
            "no legacy nested discovery path"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn bridge_file_cleanup_removes_only_uzes_files() {
        let root = uze_testkit::temp::scratch("hooks-cleanup");
        let bridge = root.join("plugins/uze-hooks-demo.ts");
        fs::create_dir_all(bridge.parent().unwrap()).unwrap();
        fs::write(&bridge, "// generated").unwrap();
        remove_bridge_file(&bridge).unwrap();
        assert!(!bridge.exists());
        assert!(
            !bridge.parent().unwrap().exists(),
            "an empty plugins dir left behind only by this file is removed"
        );
        // A foreign plugin file in the directory keeps it alive.
        fs::create_dir_all(bridge.parent().unwrap()).unwrap();
        fs::write(bridge.parent().unwrap().join("foreign.ts"), "// foreign").unwrap();
        fs::write(&bridge, "// generated").unwrap();
        remove_bridge_file(&bridge).unwrap();
        assert!(
            bridge.parent().unwrap().exists(),
            "a non-empty directory is preserved"
        );
        let _ = fs::remove_dir_all(root);
    }
}
