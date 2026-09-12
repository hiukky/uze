//! Hook projections owned by harness integrations (ADR-033, ADR-040):
//! per-vendor capability profiles, native JSON configuration merging, the
//! generated `hooks/exec` wrapper each command-hook harness runs, and the
//! owned OpenCode bridge.
//!
//! The wrapper is the only implementation of the hook ABI: it reads that
//! harness's payload, runs the author's handlers and answers in that
//! harness's dialect, with no UZE binary anywhere on the execution path. A
//! platform it has no template for delivers no hook at all, and says so.
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
        CommandHook, HOOKS_FILE_NAME, HarnessToolVocabulary, HookCapabilities, HookEffect,
        HookEvent, HookMatcher, PortableHook, ToolBinding,
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

/// OpenCode's plugin API supplies pre/post tool callbacks that see the tool
/// input but cannot block it; there is no declarative hook file, so UZE
/// generates an owned, rebuildable plugin instead. `Stop` has no OpenCode
/// equivalent and is never claimed. `deny`/`ask` live only on
/// `permission.evaluate`, which carries the action and its resources rather
/// than the tool input, so they are Unsupported until the Lab proves
/// otherwise. `transform` needs a channel for the handler to answer on,
/// which the exit-code contract does not have.
pub(crate) fn opencode_capabilities() -> HookCapabilities {
    HookCapabilities {
        events: [HookEvent::PreToolUse, HookEvent::PostToolUse]
            .into_iter()
            .collect(),
        effects: [HookEffect::Observe, HookEffect::Allow]
            .into_iter()
            .collect(),
        supports_native_matchers: true,
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
        also_matches: &["Bash"],
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

/// Every native tool name one matcher intercepts on a target. `native:<name>`
/// passes through unchanged; a portable alias yields every tool this harness
/// binds it to, because a vendor that renames its shell tool keeps answering
/// to the old name for a while and a hook must intercept both. An alias the
/// harness binds to no tool falls back to the alias literal, which matches
/// nothing — an honest no-op rather than a fabricated tool name.
pub(crate) fn tool_names(target: &str, matcher: &HookMatcher) -> Vec<String> {
    match matcher {
        HookMatcher::Native(name) => vec![name.clone()],
        HookMatcher::Portable(alias) => match vocabulary(target).binding(alias) {
            Some(binding) => {
                let names: Vec<String> = binding
                    .native_tool
                    .into_iter()
                    .chain(binding.also_matches.iter().copied())
                    .map(str::to_owned)
                    .collect();
                if names.is_empty() {
                    vec![alias.clone()]
                } else {
                    names
                }
            }
            None => vec![alias.clone()],
        },
    }
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
            for name in tool_names(target, entry) {
                if !names.contains(&name) {
                    names.push(name);
                }
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

/// How a delivered hook is invoked by the harness: the generated wrapper,
/// in the form that harness's own entry takes (see [`hook_delivery`]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum HookInvocation {
    /// `command` plus `args`: the harness starts the wrapper directly, with
    /// no shell to quote for.
    Exec { command: String, args: Vec<String> },
    /// One shell line, for a harness whose hook entry carries only a
    /// command string.
    Line(String),
}

/// The native group entry for an event-array target (Claude settings.json
/// hooks, Codex hooks.json): `{ "matcher": ..., "hooks": [...] }` carrying
/// one invocation. The matcher key is omitted entirely for an unmatch-all
/// group.
pub(crate) fn group_entry(
    target: &str,
    hook: &PortableHook,
    invocation: &HookInvocation,
) -> serde_json::Value {
    let mut entry = serde_json::Map::new();
    if let Some(matcher) = matcher(target, hook) {
        entry.insert("matcher".to_owned(), serde_json::Value::String(matcher));
    }
    entry.insert(
        "hooks".to_owned(),
        serde_json::Value::Array(vec![handler_entry(hook, invocation)]),
    );
    serde_json::Value::Object(entry)
}

/// One handler object — `{type, command[, args], timeout}` — the shape both
/// the grouped and the flat event forms are built from.
///
/// The native timeout is a backstop for the whole group, and it must never
/// be the bound that fires first: each handler can take its own deadline
/// plus the second the wrapper waits between `TERM` and `KILL`, so the sum
/// of those (with one more second for the final render) bounds the
/// wrapper's own activity, capped at the canonical 300s maximum.
fn handler_entry(hook: &PortableHook, invocation: &HookInvocation) -> serde_json::Value {
    let timeout: u16 = hook
        .handlers
        .iter()
        .map(|handler| u32::from(handler.timeout) + 1)
        .sum::<u32>()
        .saturating_add(1)
        .min(u32::from(uze_core::hook::MAX_TIMEOUT_SECONDS)) as u16;
    let mut invoked = serde_json::Map::new();
    invoked.insert("type".to_owned(), serde_json::json!("command"));
    match invocation {
        HookInvocation::Exec { command, args } => {
            invoked.insert("command".to_owned(), serde_json::json!(command));
            invoked.insert("args".to_owned(), serde_json::json!(args));
        }
        HookInvocation::Line(line) => {
            invoked.insert("command".to_owned(), serde_json::json!(line));
        }
    }
    invoked.insert("timeout".to_owned(), serde_json::json!(timeout));
    serde_json::Value::Object(invoked)
}

/// One group's delivery: the native entry and the wrapper it needs on disk.
pub(crate) struct HookDelivery {
    pub entry: serde_json::Value,
    pub wrapper: PathBuf,
}

/// Why a platform the `sh` template does not cover receives no hook at all.
/// There is one implementation of the contract and it is the wrapper: a
/// delivery that cannot write one has nothing honest to attach, so the hook
/// is reported Unsupported rather than carried by something else.
pub(crate) const NO_WRAPPER_TEMPLATE: &str =
    "no wrapper template for this platform, so the hook is not delivered";

/// Whether a wrapper can be written and run for this harness here.
fn deliverable(target: &str) -> bool {
    cfg!(unix) && wrapper_source(target).is_some()
}

/// Renders one group's delivery, or nothing when this platform has no
/// wrapper to deliver.
///
/// The generated wrapper is the only route: it is vendored beside the
/// delivery with nothing of the packager on the execution path. Where the
/// POSIX `sh` template does not reach, the answer is that the hook is not
/// delivered — see [`NO_WRAPPER_TEMPLATE`].
pub(crate) fn hook_delivery(
    target: &str,
    hook: &PortableHook,
    package_root: &Path,
    wrapper: Option<PathBuf>,
    exec_form: bool,
) -> Option<HookDelivery> {
    let wrapper = wrapper.filter(|_| deliverable(target))?;
    let arguments = wrapper_arguments(hook, package_root, &hook.handlers);
    let invocation = if exec_form {
        HookInvocation::Exec {
            command: wrapper.display().to_string(),
            args: arguments,
        }
    } else {
        HookInvocation::Line(
            std::iter::once(shell_quote(&wrapper.display().to_string()))
                .chain(arguments.iter().map(|argument| shell_quote(argument)))
                .collect::<Vec<_>>()
                .join(" "),
        )
    };
    Some(HookDelivery {
        entry: group_entry(target, hook, &invocation),
        wrapper,
    })
}

const fn hook_event_name(event: HookEvent) -> &'static str {
    match event {
        HookEvent::PreToolUse => "PreToolUse",
        HookEvent::PostToolUse => "PostToolUse",
        HookEvent::Stop => "Stop",
    }
}

/// Whether this harness expects an event's entry in the grouped form
/// (`{matcher, hooks: [...]}`) or as a flat list of handler objects.
///
/// The vendor's own customization docs (`agy-customizations/docs/hooks.md`)
/// split them: the tool events carry a matcher and are grouped, while
/// `PreInvocation`/`PostInvocation`/`Stop` are "flat (list of handler
/// objects directly)". A `Stop` written in the grouped form is parsed as
/// invalid and silently dropped — only `--log-file` shows the reason
/// ("command hook must specify 'command'"), and `agy plugin validate` says
/// nothing (antigravity-cli#925, 1.1.24).
const fn agy_event_is_grouped(event: HookEvent) -> bool {
    matches!(event, HookEvent::PreToolUse | HookEvent::PostToolUse)
}

/// One named hook as Antigravity CLI's shared `hooks.json` holds it: the
/// value under a root key, `{"<Event>": <entries>}` — grouped with the
/// translated matcher for a tool event, flat for `Stop` (the vendor parses
/// a grouped `Stop` as invalid and drops it silently, antigravity-cli#925).
///
/// The document root *is* the named-hook map: the vendor reads every root
/// key as one named hook, so a `hooks` wrapper key registers a single hook
/// called `hooks` whose "events" are our ids, and no handler ever runs
/// (1.1.24: `plugin validate` reports 1 hook processed instead of one per
/// group, and the loader fires nothing).
pub(crate) fn agy_named_entry(
    hook: &PortableHook,
    wrapper: &Path,
    package_root: &Path,
) -> serde_json::Value {
    let invocation = HookInvocation::Line(wrapper_command_line(wrapper, hook, package_root));
    let entries = if agy_event_is_grouped(hook.event) {
        vec![group_entry(ANTIGRAVITY_TARGET, hook, &invocation)]
    } else {
        vec![handler_entry(hook, &invocation)]
    };
    serde_json::json!({ hook_event_name(hook.event): entries })
}

/// Antigravity's shared `hooks.json` is a map of named hooks, not an event
/// array, so its merge is by *key*: this integration owns exactly the keys
/// it namespaces (`<package>:<group-id>`), and every other root key —
/// a hand-written hook, another tool's — keeps its value and its position
/// in the document. The file is re-emitted, not patched, so what a merge
/// does not preserve is formatting: indentation becomes two spaces and
/// whitespace between tokens is normalised.
pub(crate) fn merge_named_entry(
    config_path: &Path,
    entry_name: &str,
    entry: &serde_json::Value,
) -> Result<PathBuf> {
    let mut config = read_config_object(config_path).map_err(|reason| {
        UzeError::ExposureUnavailable(format!("cannot merge hook entry: {reason}"))
    })?;
    config
        .as_object_mut()
        .expect("read_config_object returns an object")
        .insert(entry_name.to_owned(), entry.clone());
    write_config(config_path, &config)?;
    Ok(config_path.to_path_buf())
}

/// Content identity for one named hook: the key must exist and hold exactly
/// what the receipt recorded, and the wrapper it names must be the one UZE
/// writes. Anything else is drift, and drift blocks removal.
pub(crate) fn inspect_named_entry(
    config_path: &Path,
    entry_name: &str,
    expected: &str,
    wrapper: Option<(&str, &Path)>,
) -> AttachmentInspection {
    if let Some(inspection) = inspect_wrapper(wrapper) {
        return inspection;
    }
    let Ok(config) = read_config_object(config_path) else {
        return blocked("hook config is missing or unreadable");
    };
    let Ok(expected) = serde_json::from_str::<serde_json::Value>(expected) else {
        return blocked("receipt carries an unreadable expected hook entry");
    };
    match config.get(entry_name) {
        Some(actual) if actual == &expected => AttachmentInspection {
            state: AttachmentState::Matched,
            reason: "managed hook entry matches the receipt".to_owned(),
        },
        Some(_) => AttachmentInspection {
            state: AttachmentState::Drifted,
            reason: "the managed hook entry differs from the receipt".to_owned(),
        },
        None => AttachmentInspection {
            state: AttachmentState::Missing,
            reason: "the managed hook entry is absent".to_owned(),
        },
    }
}

/// Removes exactly the one named key this receipt owns, and the file itself
/// only when nothing else is left in it. A non-matched receipt blocks
/// removal; a foreign named hook is never touched.
pub(crate) fn remove_named_entry(
    config_path: &Path,
    entry_name: &str,
    expected: &str,
    wrapper: Option<(&str, &Path)>,
) -> Result<AttachmentInspection> {
    let inspection = inspect_named_entry(config_path, entry_name, expected, wrapper);
    if inspection.state != AttachmentState::Matched {
        return Ok(inspection);
    }
    let mut config = read_config_object(config_path).map_err(|reason| {
        UzeError::ExposureUnavailable(format!("cannot detach hook entry: {reason}"))
    })?;
    config
        .as_object_mut()
        .expect("read_config_object returns an object")
        .remove(entry_name);
    // A file that now holds nothing was created by UZE and is safe to
    // remove entirely; anything else stays exactly as the user left it.
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

/// The vocabulary/dialect key for Antigravity CLI, shared by its matcher
/// translation, its generated wrapper and its runtime adapter.
pub(crate) const ANTIGRAVITY_TARGET: &str = "antigravity";

// ============================================================================
// Generated wrapper: hooks/exec
// ============================================================================

/// How one harness's payload is read and how its decision is written — the
/// only slots that differ between the generated `hooks/exec` wrappers.
struct WrapperDialect {
    /// The value the handler reads in `HOOK_HARNESS`.
    harness: &'static str,
    /// `jq` filter selecting the native tool name from the payload.
    tool_filter: &'static str,
    /// `jq` filter selecting the tool input object.
    input_filter: &'static str,
    /// `jq` filter selecting the workspace directory.
    cwd_filter: &'static str,
    /// The `sh` body that writes this harness's own denial on stdout, with
    /// `$1` already holding the reason as a JSON string literal.
    deny_document: &'static str,
    /// The `sh` body that writes what this harness expects when nothing is
    /// denied, with `$1` holding the ABI event name.
    allow_document: &'static str,
    /// The status the wrapper exits with after writing a denial. Claude and
    /// Codex document exit 2 as the block signal and read the decision only
    /// alongside it; Antigravity reads the decision from stdout and treats
    /// *any* non-zero exit as a failed hook — "pre-tool hook failed", the
    /// permission prompt, and the command runs anyway (measured on 1.1.24,
    /// `command_hook_executor.go`). So the code is a per-harness fact.
    deny_exit: &'static str,
}

fn wrapper_dialect(target: &str) -> Option<WrapperDialect> {
    match target {
        "claude" => Some(WrapperDialect {
            harness: "claude",
            tool_filter: ".tool_name // empty",
            input_filter: ".tool_input // {}",
            cwd_filter: ".cwd // .context.cwd // empty",
            // The event name is echoed back in `hookEventName`, which the
            // harness matches against the event it fired.
            deny_document: concat!(
                "case $HOOK_EVENT in\n",
                "    pre_tool_use) name=PreToolUse ;;\n",
                "    post_tool_use) name=PostToolUse ;;\n",
                "    *) name=Stop ;;\n",
                "  esac\n",
                "  printf '{\"hookSpecificOutput\":{\"hookEventName\":\"%s\",\"permissionDecision\":\"deny\",\"permissionDecisionReason\":%s}}' \"$name\" \"$reason_json\"",
            ),
            allow_document: ":",
            deny_exit: "2",
        }),
        "codex" => Some(WrapperDialect {
            harness: "codex",
            tool_filter: ".tool_name // empty",
            input_filter: ".tool_input // {}",
            cwd_filter: ".cwd // empty",
            deny_document: "printf '{\"hookSpecificOutput\":{\"permissionDecision\":\"deny\",\"permissionDecisionReason\":%s}}' \"$reason_json\"",
            // Stop is the one event whose stdout must parse as JSON even
            // when nothing was decided.
            allow_document: "[ \"$HOOK_EVENT\" = stop ] && printf '{}'",
            deny_exit: "2",
        }),
        "antigravity" => Some(WrapperDialect {
            harness: "antigravity",
            tool_filter: ".toolCall.name // empty",
            input_filter: ".toolCall.args // {}",
            cwd_filter: ".workspacePaths[0] // empty",
            deny_document: "printf '{\"decision\":\"deny\",\"reason\":%s}' \"$reason_json\"",
            // Only the pre-tool event carries a decision; the others answer
            // with the empty object the vendor's contract requires.
            allow_document: "[ \"$HOOK_EVENT\" = pre_tool_use ] || printf '{}'",
            // The decision is the stdout document; a non-zero exit is a
            // failed hook here, not a block.
            deny_exit: "0",
        }),
        _ => None,
    }
}

/// The `case` arm list translating this harness's native tool names into
/// `HOOK_TOOL` and the matched alias's portable field variables, generated
/// from the one vocabulary the matchers are generated from.
fn wrapper_alias_table(target: &str) -> String {
    let mut arms = String::new();
    for (native, binding) in vocabulary(target).native_names() {
        let mut assignments = format!("HOOK_TOOL={};", binding.alias);
        for (portable, native_field) in binding.fields {
            let variable = uze_core::hook::hook_field_variable(portable);
            assignments.push_str(&format!(
                " {variable}=$(printf '%s' \"$HOOK_INPUT\" | \"$JQ\" -r '.{native_field} // empty');"
            ));
        }
        arms.push_str(&format!("    {native}) {assignments} ;;\n"));
    }
    arms
}

/// Every portable field variable any alias of this harness can set. They are
/// declared empty up front so an unmatched tool leaves a defined (and empty)
/// variable rather than tripping `set -u` in the handler.
fn wrapper_field_variables(target: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for binding in vocabulary(target).bindings {
        for (portable, _) in binding.fields {
            let variable = uze_core::hook::hook_field_variable(portable);
            if !names.contains(&variable) {
                names.push(variable);
            }
        }
    }
    names
}

/// The wrapper a harness actually executes at hook time: POSIX `sh`, one per
/// harness, byte-identical for every package. It reads the harness's payload
/// from stdin, exposes the hook context as `HOOK_*` environment, runs the
/// handlers sequentially, and answers in the harness's own dialect.
///
/// Ordering, first-deny-wins and fail-closed are compiled in here because no
/// harness provides them: a group's hooks may run in parallel, and a hook
/// that exits non-zero is non-blocking, so a `deny` guard that crashes would
/// otherwise let the tool through. `jq` is the wrapper's own dependency and
/// is guarded by the same rule.
///
/// Nothing in this file names the packager: the contract is the file, and
/// any tool that can write it can deliver a portable hook.
pub(crate) fn wrapper_source(target: &str) -> Option<String> {
    let dialect = wrapper_dialect(target)?;
    let fields = wrapper_field_variables(target);
    let field_defaults = fields
        .iter()
        .map(|name| format!("{name}="))
        .collect::<Vec<_>>()
        .join(" ");
    let field_exports = fields.join(" ");
    let aliases = wrapper_alias_table(target);
    let deny_exit_code = uze_core::hook::DENY_EXIT_CODE;
    let WrapperDialect {
        harness,
        tool_filter,
        input_filter,
        cwd_filter,
        deny_document,
        allow_document,
        deny_exit,
    } = dialect;
    Some(format!(
        r#"#!/bin/sh
# hooks/exec — generated from hooks.json, one per harness. The harness runs
# this; it runs the author's handlers. The handlers never see a harness
# payload and never write harness JSON: the context arrives as HOOK_*
# environment and the decision leaves as an exit code: 0 allows, while
# {deny_exit_code} denies and the reason is read from stderr. Anything else
# is a failure that follows the group's effect.
#
#   usage: exec <plugin-root> <event> <effect> <seconds>:<handler>...
#     event    pre_tool_use | post_tool_use | stop
#     effect   observe | allow | ask | deny
#     seconds  this handler's own deadline; past it the handler and
#              everything it started are stopped, and the group's effect
#              decides, exactly as for any other handler failure
set -u
PLUGIN_ROOT=$1
HOOK_EVENT=$2
effect=$3
shift 3
HOOK_HARNESS={harness}
export PLUGIN_ROOT HOOK_EVENT HOOK_HARNESS

# --- this harness's decision dialect ------------------------------------
deny_native() {{                                  # $1 reason, plain text
  printf '%s\n' "$1" >&2
  reason_json=$(json_string "$1")
  {deny_document}
  exit {deny_exit}                                # this harness's block signal
}}

allow_native() {{
  {allow_document}
}}

# fail-closed effects: a guard that cannot be evaluated denies
closed() {{ [ "$effect" = deny ] || [ "$effect" = ask ]; }}
fail() {{ closed && deny_native "$1"; printf '%s\n' "$1" >&2; allow_native; exit 0; }}

# jq escapes the reason once it is available; before that (its own absence
# is the only reason reported then) a literal with neither quote nor
# newline needs no escaping.
json_string() {{
  if [ -n "${{JQ_READY:-}}" ]; then
    printf '%s' "$1" | "$JQ" -Rsa .
  else
    printf '"%s"' "$1"
  fi
}}

# --- the harness's payload becomes the hook context ----------------------
JQ=${{HOOK_JQ:-jq}}
command -v "$JQ" >/dev/null 2>&1 || fail "hooks/exec: jq is not installed"
JQ_READY=1
payload=$(cat)
HOOK_TOOL_NATIVE=$(printf '%s' "$payload" | "$JQ" -r '{tool_filter}')
HOOK_CWD=$(printf '%s' "$payload" | "$JQ" -r '{cwd_filter}')
HOOK_INPUT=$(printf '%s' "$payload" | "$JQ" -c '{input_filter}')
HOOK_TOOL= {field_defaults}
case "$HOOK_TOOL_NATIVE" in                       # the portable vocabulary
{aliases}esac
export HOOK_TOOL HOOK_TOOL_NATIVE HOOK_CWD HOOK_INPUT {field_exports}

# --- one handler, under its own deadline ---------------------------------
# There is no portable `timeout(1)` (macOS ships none) and no job control in
# a script, so there is no process group to signal: the deadline is a
# sleeper this shell can cancel, and what it stops is the handler plus every
# process the handler started. That second part is not thoroughness — a
# child still holding the pipe keeps this shell waiting long past the
# deadline it just enforced.
family() {{                                       # $1 pid -> $1 and its issue
  snapshot=$(ps -A -o pid=,ppid= 2>/dev/null)
  all=$1 layer=$1
  while [ -n "$layer" ]; do
    layer=$(printf '%s\n' "$snapshot" | while read -r pid parent; do
      for one in $layer; do
        [ "$parent" = "$one" ] && printf '%s ' "$pid"
      done
    done)
    all="$all $layer"
  done
  printf '%s' "$all"
}}

# Where a handler's reason is collected: a file, never a pipe. Anything the
# handler starts inherits a pipe, and one that outlives its deadline would
# hold this shell open long past the deadline it just enforced. `set -C`
# refuses a path that already exists, so a planted file or symlink is never
# written through; with nowhere to write at all, the reason is dropped
# rather than the hook.
reasons=${{TMPDIR:-/tmp}}/hooks-exec.$$
(set -C; : > "$reasons") 2>/dev/null || reasons=/dev/null
discard_reasons() {{ [ "$reasons" = /dev/null ] || rm -f "$reasons"; }}
trap discard_reasons EXIT INT TERM

# $1 seconds, $2 command. Leaves what the handler wrote on stderr in
# $reasons and answers with its exit status — or 124, the conventional
# timeout status, when the deadline stopped it. A handler that exits 124 of
# its own accord therefore reads as a timeout; `timeout(1)` carries the
# same ambiguity.
guarded() {{
  (
    sh -c "$2" </dev/null >/dev/null 2>"$reasons" &
    child=$!
    (
      napper=
      trap '[ -n "$napper" ] && kill "$napper" 2>/dev/null; exit 0' TERM
      sleep "$1" & napper=$!
      wait "$napper" 2>/dev/null
      doomed=$(family "$child")
      for one in $doomed; do kill -TERM "$one" 2>/dev/null; done
      sleep 1                                     # then the ones that stayed
      for one in $doomed; do kill -KILL "$one" 2>/dev/null; done
    ) >/dev/null 2>&1 &
    watchdog=$!
    wait "$child"; code=$?
    kill -TERM "$watchdog" 2>/dev/null            # cancels the sleeper too
    case $code in
      137|143) exit 124 ;;
      *) exit "$code" ;;
    esac
  ) 2>/dev/null                                   # the shell's own job notices
}}

# --- the handlers, in order; the first denial stops the rest --------------
# A handler is a shell command line, run from the package root: the same
# contract the canonical manifest documents, so `sh scripts/check --strict`
# means here exactly what it means when a person types it.
cd "$PLUGIN_ROOT" 2>/dev/null || :
for entry in "$@"; do
  seconds=${{entry%%:*}}
  handler=${{entry#*:}}
  case $seconds in
    ''|*[!0-9]*) fail "malformed handler argument: $entry" ;;
  esac
  guarded "$seconds" "$handler"; status=$?
  [ "$status" = 0 ] && continue                   # allowed; on to the next
  reason=$(cat "$reasons" 2>/dev/null)
  case $status in
    {deny_exit_code}) deny_native "${{reason:-$handler denied the operation}}" ;;
    124) fail "handler timed out after ${{seconds}}s: $handler" ;;
    *) fail "handler failed (exit $status): $handler${{reason:+ — $reason}}" ;;
  esac
done
allow_native
exit 0
"#
    ))
}

/// The name of the wrapper inside its delivered artifact. `hooks/exec` on
/// every harness: one path an author or reviewer can look for.
pub(crate) const WRAPPER_RELATIVE_PATH: &str = "hooks/exec";

/// Where a harness whose hooks are merged into a shared config file keeps
/// its wrapper: one file per harness under UZE's own state, never in the
/// Store and never in the harness's own directories. Byte-identical for
/// every package, so one file serves them all.
pub(crate) fn shared_wrapper_path(uze_home: &UzeHome, target: &str) -> PathBuf {
    uze_home
        .state_dir()
        .join("attachments")
        .join(target)
        .join(WRAPPER_RELATIVE_PATH)
}

/// Writes (or refreshes) a generated wrapper, executable. Idempotent: the
/// content is a pure function of the harness.
pub(crate) fn materialize_wrapper(path: &Path, source: &str) -> Result<()> {
    // Rewriting an identical wrapper would replace a file a harness may be
    // executing right now, for no gain: the content is a pure function of
    // the harness. The executable bit is not part of that content, and
    // `write_atomic` publishes under the umask before the chmod lands — a
    // crash in between leaves the right bytes with the wrong mode, which
    // only a second chmod repairs.
    if fs::read_to_string(path).is_ok_and(|current| current == source) {
        return if is_executable(path) {
            Ok(())
        } else {
            make_executable(path)
        };
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| UzeError::Write {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    write_atomic(path, source.as_bytes())?;
    make_executable(path)
}

/// Whether the wrapper on disk can be run at all. A harness that cannot
/// execute it reports exit 126, which a `deny` group turns into a
/// permanent block — so this is drift, not a cosmetic difference. On a
/// platform without Unix modes there is no bit to lose.
fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        true
    }
}

fn make_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).map_err(|source| {
            UzeError::Write {
                path: path.to_path_buf(),
                source,
            }
        })?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// Removes a shared wrapper once no hook entry of this integration is left
/// to run it. A wrapper another group's entry still points at is kept: it
/// is one file serving every package.
///
/// "Left" is read from the harness's own config files, not from the receipt
/// ledger. The prune runs inside a detach, and the lifecycle only rewrites
/// the ledger once every detach of the removal has returned — so the
/// receipt being detached, and during `uze remove` each of its siblings, is
/// still listed there while its entry is already gone from the config.
pub(crate) fn prune_shared_wrapper(uze_home: &UzeHome, integration_id: &str, target: &str) {
    let still_used = uze_core::state::receipts(uze_home, None).is_ok_and(|ledger| {
        ledger.iter().any(|(_, receipt)| {
            receipt.integration == integration_id && entry_is_attached(receipt, target)
        })
    });
    if still_used {
        return;
    }
    let path = shared_wrapper_path(uze_home, target);
    let _ = fs::remove_file(&path);
    if let Some(parent) = path.parent() {
        let _ = fs::remove_dir(parent);
    }
}

/// Whether this receipt's hook entry is still in the harness's config file.
/// The wrapper is deliberately not part of the question: it is what the
/// prune is deciding about, so inspecting it would answer "nothing is
/// attached" for every entry the moment it went missing.
///
/// The target decides the file's shape — Antigravity keys its entries by
/// name, the other command-hook harnesses by event — and every receipt
/// records its event either way, so the shape cannot be read off the
/// receipt.
fn entry_is_attached(receipt: &uze_core::integration::AttachmentReceipt, target: &str) -> bool {
    let uze_core::integration::ManagedArtifact::HookConfigEntry {
        config_file,
        entry_name,
        event,
        expected,
        ..
    } = &receipt.artifact
    else {
        return false;
    };
    let inspection = if target == ANTIGRAVITY_TARGET {
        inspect_named_entry(config_file, entry_name, expected, None)
    } else {
        inspect_event_entry(config_file, *event, expected, None)
    };
    inspection.state == AttachmentState::Matched
}

/// The native command an entry runs: the wrapper, the package root, the
/// group's event and effect, then the author's handlers — each as
/// `<seconds>:<command>`, with its declared deadline and `${PLUGIN_ROOT}`
/// already resolved. Everything harness-specific is decided here, at
/// generation time, so the native entry reads as what will run.
pub(crate) fn wrapper_arguments(
    hook: &PortableHook,
    package_root: &Path,
    handlers: &[CommandHook],
) -> Vec<String> {
    let mut arguments = vec![
        package_root.display().to_string(),
        hook.event.abi_name().to_owned(),
        hook.effect.abi_name().to_owned(),
    ];
    for handler in handlers {
        arguments.push(format!(
            "{}:{}",
            handler.timeout,
            handler
                .command
                .replace("${PLUGIN_ROOT}", &package_root.display().to_string())
        ));
    }
    arguments
}

/// The same invocation as one shell line, for the harnesses whose hook
/// entry carries a command string rather than a command plus arguments.
pub(crate) fn wrapper_command_line(
    wrapper: &Path,
    hook: &PortableHook,
    package_root: &Path,
) -> String {
    let mut parts = vec![shell_quote(&wrapper.display().to_string())];
    for argument in wrapper_arguments(hook, package_root, &hook.handlers) {
        parts.push(shell_quote(&argument));
    }
    parts.join(" ")
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
    wrapper: Option<(&str, &Path)>,
) -> Result<PathBuf> {
    // The wrapper is what the harness will actually run, so it lands before
    // the entry that names it.
    if let Some((target, path)) = wrapper
        && let Some(source) = wrapper_source(target)
    {
        materialize_wrapper(path, &source)?;
    }
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
    wrapper: Option<(&str, &Path)>,
) -> AttachmentInspection {
    if let Some(inspection) = inspect_wrapper(wrapper) {
        return inspection;
    }
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
    wrapper: Option<(&str, &Path)>,
) -> Result<AttachmentInspection> {
    let inspection = inspect_event_entry(config_path, event, expected, wrapper);
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

/// The wrapper is the other half of every merged delivery: an entry
/// pointing at a missing or edited wrapper is drift, not a match. `None`
/// when the wrapper is what UZE writes (or when there is none to check).
fn inspect_wrapper(wrapper: Option<(&str, &Path)>) -> Option<AttachmentInspection> {
    let (target, path) = wrapper?;
    match fs::read_to_string(path) {
        Err(_) => Some(AttachmentInspection {
            state: AttachmentState::Missing,
            reason: "the generated hook wrapper is absent".to_owned(),
        }),
        Ok(current) if wrapper_source(target).is_none_or(|expected| expected != current) => {
            Some(AttachmentInspection {
                state: AttachmentState::Drifted,
                reason: "the generated hook wrapper does not match what UZE writes".to_owned(),
            })
        }
        Ok(_) if !is_executable(path) => Some(AttachmentInspection {
            state: AttachmentState::Drifted,
            reason: "the generated hook wrapper is not executable".to_owned(),
        }),
        Ok(_) => None,
    }
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

/// The delivered plugin's path: `<config root>/plugins/hooks-<package>.ts`.
/// `<config root>/plugins/` is OpenCode's documented global plugin directory
/// (`~/.config/opencode/plugins/`), auto-discovered at startup — the file is
/// therefore the single, self-contained load source: no `plugin` entry in
/// `opencode.json` exists to duplicate it. (Verified against the real
/// harness: the legacy `.opencode/plugins/` path is project-scoped and NOT
/// auto-discovered under the global config directory.)
pub(crate) fn opencode_bridge_path(config_root: &Path, package_id: &str) -> PathBuf {
    config_root
        .join("plugins")
        .join(format!("hooks-{package_id}.ts"))
}

/// The package's groups as data for the generated plugin: translated
/// matchers (matched against the runtime native tool name), abi event name,
/// effect, and the authored handlers with `${PLUGIN_ROOT}` resolved.
fn bridge_hooks(hooks: &[&PortableHook], package_root: &Path) -> serde_json::Value {
    serde_json::Value::Array(
        hooks
            .iter()
            .map(|hook| {
                serde_json::json!({
                    "id": hook.id,
                    "event": hook.event.abi_name(),
                    "effect": hook.effect.abi_name(),
                    "matchers": hook.matchers.iter().flat_map(|m| tool_names("opencode", m)).collect::<Vec<_>>(),
                    "handlers": hook.handlers.iter().map(|handler| serde_json::json!({
                        "command": handler.command.replace(
                            "${PLUGIN_ROOT}",
                            &package_root.display().to_string(),
                        ),
                        "timeout": handler.timeout,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect(),
    )
}

/// The alias table this harness's plugin reads, generated from the one
/// vocabulary: native tool name → portable alias plus the portable field
/// values, each read from that harness's own input field.
fn bridge_alias_table() -> String {
    let mut rows = Vec::new();
    for (native, binding) in vocabulary("opencode").native_names() {
        let fields = binding
            .fields
            .iter()
            .map(|(portable, native_field)| {
                format!(
                    "{}: String(input.{native_field} ?? \"\")",
                    uze_core::hook::hook_field_variable(portable)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        rows.push(format!(
            "  {native}: {{ tool: \"{}\", fields: (input) => ({{ {fields} }}) }},",
            binding.alias
        ));
    }
    rows.join("\n")
}

/// The generated OpenCode plugin for one package. OpenCode has no
/// declarative hook file, so the plugin *is* the wrapper: the same contract
/// the `sh` wrapper implements on the other harnesses, with this package's
/// groups as data. JavaScript valid as TypeScript (no build step), no
/// dependencies, deterministic — and naming nothing but the hook contract.
///
/// The V2 tool hooks see the tool input but cannot block, and the only
/// decision point (`permission.evaluate`) carries the action and its
/// resources rather than the tool input; deny/ask are therefore diagnosed
/// before attach and never fabricated here.
pub(crate) fn opencode_bridge(
    hooks: &[&PortableHook],
    plugin_root: &Path,
    package_id: &str,
) -> String {
    let root = plugin_root.display().to_string();
    let groups = serde_json::to_string(&bridge_hooks(hooks, plugin_root))
        .expect("generated groups serialize");
    let aliases = bridge_alias_table();
    let deny_exit_code = uze_core::hook::DENY_EXIT_CODE;
    format!(
        r#"// Generated from hooks.json — do not edit; regenerate instead.
// OpenCode V2 (opencode.ai/v2/docs/build/plugins) has no hooks.json: the
// plugin is both the registration and the runner. Only GROUPS changes
// between packages; everything below it is the same runtime every time.
//
// Handler contract: the hook context arrives as HOOK_* environment and the
// decision leaves as an exit code — 0 allows, {deny_exit_code} denies with
// the reason on stderr, anything else is a failure that follows the group's
// effect (fail-closed for deny/ask, fail-open for observe/allow). Each
// handler is bounded by the deadline its author declared.
import {{ Plugin }} from "@opencode-ai/plugin";

const ROOT = {root:?};
const GROUPS = {groups};

// native tool name -> portable alias and its portable fields
const ALIASES = {{
{aliases}
}};

const closed = (effect) => effect === "deny" || effect === "ask";

function environment(group, native, input) {{
  const alias = ALIASES[native];
  return {{
    ...process.env,
    PLUGIN_ROOT: ROOT,
    HOOK_HARNESS: "opencode",
    HOOK_EVENT: group.event,
    HOOK_TOOL: alias?.tool ?? "",
    HOOK_TOOL_NATIVE: native,
    HOOK_CWD: process.cwd(),
    HOOK_INPUT: JSON.stringify(input ?? {{}}),
    ...(alias ? alias.fields(input ?? {{}}) : {{}}),
  }};
}}

// One handler: null when it allowed, otherwise the reason it answered with.
async function handler(command, timeout, env) {{
  let proc;
  try {{
    proc = Bun.spawn(["/bin/sh", "-c", command], {{
      cwd: ROOT,
      env,
      stdin: "ignore",
      stdout: "ignore",
      stderr: "pipe",
    }});
  }} catch (error) {{
    return {{ failed: true, reason: `handler failed to start: ${{command}} — ${{error.message}}` }};
  }}
  // The author's deadline, enforced here for the same reason the sh
  // wrapper enforces it: nothing else will.
  let expired = false;
  const timer = setTimeout(() => {{ expired = true; proc.kill(); }}, timeout * 1000);
  let stderr = "";
  try {{
    stderr = (await new Response(proc.stderr).text()).trim();
  }} finally {{
    clearTimeout(timer);
  }}
  const code = await proc.exited;
  if (expired) return {{ failed: true, reason: `handler timed out after ${{timeout}}s: ${{command}}` }};
  if (code === 0) return null;
  if (code === {deny_exit_code}) return {{ failed: false, reason: stderr || `${{command}} denied the operation` }};
  return {{ failed: true, reason: `handler failed (exit ${{code}}): ${{command}}${{stderr ? " — " + stderr : ""}}` }};
}}

// Handlers in manifest order; the first denial stops the rest. A failure
// denies for a fail-closed group and is reported for the others.
async function run(group, native, input) {{
  const env = environment(group, native, input);
  for (const entry of group.handlers) {{
    const answer = await handler(entry.command, entry.timeout, env);
    if (answer === null) continue;
    if (answer.failed && !closed(group.effect)) {{
      console.error(`[hooks:${{group.id}}]`, answer.reason);
      continue;
    }}
    return answer.reason;
  }}
  return null;
}}

function matches(group, event, native) {{
  return (
    group.event === event &&
    (group.matchers.length === 0 || group.matchers.includes(native))
  );
}}

export default Plugin.define({{
  id: "hooks-{package_id}",
  async setup(ctx) {{
    await ctx.tool.hook("execute.before", async (event) => {{
      for (const group of GROUPS) {{
        if (!matches(group, "pre_tool_use", event.tool)) continue;
        const reason = await run(group, event.tool, event.input);
        if (reason) console.error(`[hooks:${{group.id}}]`, reason);
      }}
    }});
    await ctx.tool.hook("execute.after", async (event) => {{
      for (const group of GROUPS) {{
        if (!matches(group, "post_tool_use", event.tool)) continue;
        const reason = await run(group, event.tool, event.input);
        if (reason) console.error(`[hooks:${{group.id}}]`, reason);
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
#[allow(clippy::too_many_arguments)]
pub(crate) fn hook_exposure_plan(
    uze_home: &UzeHome,
    resource: &Resource,
    capabilities: &HookCapabilities,
    config_file: PathBuf,
    target: &str,
    exec_form: bool,
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
    let mut undeliverable = None;
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
            let package_root = resource
                .package_root()
                .expect("hook exposure_plan is only reached for packages");
            match hook_delivery(
                target,
                &hook,
                package_root,
                Some(shared_wrapper_path(uze_home, target)),
                exec_form,
            ) {
                Some(delivery) => ExposureMechanism::ManagedHookConfig {
                    config_file,
                    entry_name: hook_entry_name(resource, &hook),
                    event: hook.event,
                    expected: serde_json::to_string(&delivery.entry)
                        .expect("hook entry serializes"),
                    wrapper: delivery.wrapper,
                },
                None => {
                    undeliverable = Some(NO_WRAPPER_TEMPLATE);
                    ExposureMechanism::Unsupported {
                        rationale: NO_WRAPPER_TEMPLATE.to_owned(),
                    }
                }
            }
        }
    };
    // A semantic route the delivery cannot take is not a route: a harness
    // this platform has no wrapper for is Unsupported, and says why.
    let route = match undeliverable {
        Some(_) => CompatibilityRoute::Unsupported,
        None => compatibility.route,
    };
    let evidence = match (&compatibility.reason, undeliverable) {
        (Some(reason), _) => format!("{evidence} Compatibility: {reason}"),
        (None, Some(reason)) => format!("{evidence} Delivery: {reason}."),
        (None, None) => evidence.to_owned(),
    };
    ExposurePlan {
        representation: resource.capability.representation,
        route,
        verification: VerificationStatus::Unverified,
        mechanism,
        evidence,
    }
}

/// Antigravity's hook plan: the same assessment and the same wrapper as
/// every merged delivery, but the entry is one *named* value
/// (`{"<Event>": <entries>}`) rather than a member of an event array,
/// because this harness's shared `hooks.json` is a map of named hooks.
///
/// The wrapper lives under UZE's own state (`$UZE_HOME/state/attachments/
/// antigravity/hooks/exec`), not inside a plugin: a shared config file has
/// no plugin root to resolve against, and the harness runs a hook with its
/// cwd set to the directory holding `hooks.json`, so every path in the
/// entry is absolute.
pub(crate) fn antigravity_hook_exposure_plan(
    uze_home: &UzeHome,
    resource: &Resource,
    capabilities: &HookCapabilities,
    config_file: PathBuf,
) -> ExposurePlan {
    const EVIDENCE: &str = "Antigravity CLI reads named hooks from its shared `~/.gemini/config/hooks.json`: UZE merges one named entry per canonical hook (`<package>:<group-id>`, matcher and timeout preserved, grouped for the tool events and flat for Stop) whose command is the generated `hooks/exec` wrapper — the handlers run against the portable HOOK_* contract with no UZE binary on the execution path — and keeps that exact entry receipt-owned. The generated plugin carries no hooks.json: the harness never reads one from a plugin directory (Conformance Lab, `hooks > delivery`).";
    let Ok(hook) = serde_json::from_slice::<PortableHook>(&resource.capability.payload) else {
        return unsupported_plan(
            resource,
            "hook resource payload is not a valid portable hook group",
        );
    };
    let compatibility = uze_core::hook::assess(&hook, capabilities, false);
    let mut undeliverable = false;
    let mechanism = match compatibility.route {
        CompatibilityRoute::Unsupported | CompatibilityRoute::Degraded => {
            ExposureMechanism::Unsupported {
                rationale: compatibility
                    .reason
                    .clone()
                    .unwrap_or_else(|| "no compatible hook route".to_owned()),
            }
        }
        _ if !deliverable(ANTIGRAVITY_TARGET) => {
            undeliverable = true;
            ExposureMechanism::Unsupported {
                rationale: NO_WRAPPER_TEMPLATE.to_owned(),
            }
        }
        _ => {
            let package_root = resource
                .package_root()
                .expect("hook exposure_plan is only reached for packages");
            let wrapper = shared_wrapper_path(uze_home, ANTIGRAVITY_TARGET);
            let entry = agy_named_entry(&hook, &wrapper, package_root);
            ExposureMechanism::ManagedHookConfig {
                config_file,
                entry_name: hook_entry_name(resource, &hook),
                event: hook.event,
                expected: serde_json::to_string(&entry).expect("hook entry serializes"),
                wrapper,
            }
        }
    };
    let evidence = match (&compatibility.reason, undeliverable) {
        (Some(reason), _) => format!("{EVIDENCE} Compatibility: {reason}"),
        (None, true) => format!("{EVIDENCE} Delivery: {NO_WRAPPER_TEMPLATE}."),
        (None, false) => EVIDENCE.to_owned(),
    };
    ExposurePlan {
        representation: resource.capability.representation,
        route: if undeliverable {
            CompatibilityRoute::Unsupported
        } else {
            compatibility.route
        },
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

    /// A group's native invocation through the generated wrapper, which is
    /// what every entry below carries.
    fn invocation(hook: &PortableHook) -> HookInvocation {
        HookInvocation::Line(wrapper_command_line(
            Path::new("/state/hooks/exec"),
            hook,
            Path::new("/pkg"),
        ))
    }

    #[test]
    fn vendor_aliases_are_explicit() {
        assert_eq!(
            tool_names("claude", &HookMatcher::Portable("shell".into())),
            ["Bash"]
        );
        assert_eq!(
            tool_names("antigravity", &HookMatcher::Portable("shell".into())),
            ["run_command"]
        );
        assert_eq!(
            tool_names("opencode", &HookMatcher::Native("Write".into())),
            ["Write"]
        );
        assert_eq!(
            vocabulary("claude")
                .binding_for_native("Bash")
                .map(|binding| binding.alias),
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
            tool_names("claude", &HookMatcher::Native("Write".into())),
            ["Write"]
        );
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
        let alias = |native| {
            vocabulary("codex")
                .binding_for_native(native)
                .map(|binding| binding.alias)
        };
        assert_eq!(alias("exec_command"), Some("shell"));
        assert_eq!(alias("Bash"), Some("shell"));
        assert_eq!(
            tool_names("codex", &HookMatcher::Portable("shell".into())),
            ["exec_command", "Bash"],
            "the matcher intercepts every name this harness's shell tool answers to"
        );
    }

    /// A platform the `sh` template does not cover gets no hook, and the
    /// plan says so. The wrapper is the only implementation of the
    /// contract, so a delivery that cannot write one has nothing honest to
    /// attach — an entry running something else would be a hook the author
    /// never wrote.
    #[test]
    fn a_platform_without_a_wrapper_template_delivers_no_hook() {
        let delivered = hook_delivery(
            "claude",
            &hook(),
            Path::new("/pkg"),
            Some(PathBuf::from("/state/hooks/exec")),
            true,
        )
        .expect("a harness with a template delivers");
        assert_eq!(delivered.wrapper, Path::new("/state/hooks/exec"));
        assert_eq!(delivered.entry["hooks"][0]["command"], "/state/hooks/exec");

        assert!(
            hook_delivery("claude", &hook(), Path::new("/pkg"), None, true).is_none(),
            "no wrapper to run is no delivery"
        );
        assert!(
            hook_delivery(
                "a-harness-with-no-template",
                &hook(),
                Path::new("/pkg"),
                Some(PathBuf::from("/state/hooks/exec")),
                true,
            )
            .is_none(),
            "a harness the template generator does not cover delivers nothing"
        );
    }

    /// The same answer one level up: the exposure plan reports Unsupported
    /// with the reason, rather than an entry pointing at something that is
    /// not there.
    #[test]
    fn a_hook_that_cannot_be_delivered_is_reported_unsupported() {
        let home = UzeHome::at(Path::new("/tmp/uze-home"));
        let package = uze_testkit::temp::scratch("undeliverable");
        fs::create_dir_all(&package).unwrap();
        let resource = Resource::from_package(
            uze_core::store::PackageId::from_plugin_name("demo", &package.join("plugin.json"))
                .unwrap(),
            package.clone(),
            uze_core::capability::Capability {
                kind: uze_core::capability::CapabilityKind::Hook,
                representation: uze_core::capability::Representation::Standard,
                path: package.join(HOOKS_FILE_NAME),
                payload: serde_json::to_vec(&hook()).unwrap(),
            },
        );
        let plan = hook_exposure_plan(
            &home,
            &resource,
            &claude_capabilities(),
            PathBuf::from("/config/settings.json"),
            "a-harness-with-no-template",
            true,
            false,
            "evidence.",
        );
        assert_eq!(plan.route, CompatibilityRoute::Unsupported);
        assert!(
            matches!(
                &plan.mechanism,
                ExposureMechanism::Unsupported { rationale } if rationale == NO_WRAPPER_TEMPLATE
            ),
            "nothing is attached: {:?}",
            plan.mechanism
        );
        assert!(plan.evidence.contains(NO_WRAPPER_TEMPLATE));
        let _ = fs::remove_dir_all(package);
    }

    #[test]
    fn group_entry_omits_matcher_for_unmatch_all_and_reserves_native_timeout() {
        let mut hook = hook();
        let entry = group_entry("claude", &hook, &invocation(&hook));
        assert_eq!(entry["matcher"], "Bash|Write");
        assert_eq!(entry["hooks"][0]["type"], "command");
        assert_eq!(
            entry["hooks"][0]["timeout"], 12,
            "each handler's own bound plus its kill grace, plus 1s to render"
        );
        hook.matchers = Vec::new();
        let entry = group_entry("claude", &hook, &invocation(&hook));
        assert!(
            entry.get("matcher").is_none(),
            "no matcher key for a match-all group"
        );
    }

    /// The vendor's own docs split the two shapes: a tool event is grouped
    /// with a matcher, while `Stop` is "flat (list of handler objects
    /// directly)". A grouped `Stop` is parsed as invalid and silently
    /// dropped — `plugin validate` says nothing and only `--log-file`
    /// reports it (antigravity-cli#925, 1.1.24).
    #[test]
    fn a_stop_entry_is_flat_while_a_tool_event_stays_grouped() {
        let stop = PortableHook {
            id: "archive".into(),
            event: HookEvent::Stop,
            matchers: Vec::new(),
            effect: HookEffect::Observe,
            ..hook()
        };
        let value = serde_json::json!({
            "protect-env": agy_named_entry(
                &hook(),
                Path::new("/state/hooks/exec"),
                Path::new("/pkg"),
            ),
            "archive": agy_named_entry(
                &stop,
                Path::new("/state/hooks/exec"),
                Path::new("/pkg"),
            ),
        });

        let grouped = &value["protect-env"]["PreToolUse"][0];
        assert_eq!(grouped["matcher"], "run_command|Write");
        assert_eq!(grouped["hooks"][0]["type"], "command");

        let flat = &value["archive"]["Stop"][0];
        assert_eq!(
            flat["type"], "command",
            "a flat entry is the handler object itself: {flat}"
        );
        assert!(
            flat.get("hooks").is_none(),
            "a `hooks` group under Stop is dropped by the vendor's parser"
        );
        assert!(flat.get("matcher").is_none(), "Stop matches no tool");
        assert!(
            flat["command"]
                .as_str()
                .is_some_and(|command| command.contains("'stop' 'observe'")),
            "the flat entry still runs the wrapper with the group's arguments: {flat}"
        );
        assert_eq!(flat["timeout"], 12);
    }

    #[test]
    fn agy_named_entry_carries_the_wrapper_and_is_deterministic() {
        let entry = agy_named_entry(&hook(), Path::new("/state/hooks/exec"), Path::new("/pkg"));
        let document = serde_json::to_string(&entry).unwrap();
        assert_eq!(entry["PreToolUse"][0]["matcher"], "run_command|Write");
        assert!(
            entry.get("hooks").is_none(),
            "the named key holds the event map directly; a `hooks` wrapper is one dead hook"
        );
        assert!(
            document.contains("'/state/hooks/exec' '/pkg' 'pre_tool_use' 'deny'"),
            "the entry runs the shared wrapper with the group's own arguments: {document}"
        );
        assert!(
            !document.contains("hook-exec"),
            "nothing on the execution path may be the packager"
        );
        assert_eq!(
            entry,
            agy_named_entry(&hook(), Path::new("/state/hooks/exec"), Path::new("/pkg")),
        );
    }

    /// Antigravity's shared `hooks.json` is a map of *named* hooks, so UZE
    /// owns keys, not array members: a hand-written hook beside ours — and
    /// any unrelated key — must survive attach, inspect and detach untouched.
    #[test]
    fn a_named_merge_leaves_every_foreign_hook_intact() {
        let root = uze_testkit::temp::scratch("hooks-named-merge");
        fs::create_dir_all(&root).unwrap();
        let config = root.join("hooks.json");
        fs::write(
            &config,
            r#"{"my-own-guard":{"PreToolUse":[{"matcher":"run_command","hooks":[{"type":"command","command":"mine"}]}]},"notes":"kept"}"#,
        )
        .unwrap();
        let name = "pkg@market:protect-env";
        let entry = agy_named_entry(&hook(), Path::new("/state/hooks/exec"), Path::new("/pkg"));
        let expected = serde_json::to_string(&entry).unwrap();

        merge_named_entry(&config, name, &entry).unwrap();
        assert_eq!(
            inspect_named_entry(&config, name, &expected, None).state,
            AttachmentState::Matched
        );
        // Merging the same entry again changes nothing (idempotence).
        merge_named_entry(&config, name, &entry).unwrap();

        let after: serde_json::Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
        assert_eq!(
            after["my-own-guard"]["PreToolUse"][0]["hooks"][0]["command"],
            "mine"
        );
        assert_eq!(after["notes"], "kept");
        assert_eq!(after[name], entry);

        let detached = remove_named_entry(&config, name, &expected, None).unwrap();
        assert_eq!(detached.state, AttachmentState::Missing);
        let survivors: serde_json::Value =
            serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
        assert!(survivors.get(name).is_none(), "UZE's own key is gone");
        assert_eq!(
            survivors["my-own-guard"]["PreToolUse"][0]["hooks"][0]["command"], "mine",
            "the foreign named hook is byte-identical: {survivors}"
        );
        assert_eq!(survivors["notes"], "kept");
    }

    /// Drift blocks removal: an edited entry is never silently rewritten,
    /// and a file UZE cannot parse is never mutated at all.
    #[test]
    fn a_drifted_or_unreadable_named_entry_is_never_removed() {
        let root = uze_testkit::temp::scratch("hooks-named-drift");
        fs::create_dir_all(&root).unwrap();
        let config = root.join("hooks.json");
        let name = "pkg@market:protect-env";
        let entry = agy_named_entry(&hook(), Path::new("/state/hooks/exec"), Path::new("/pkg"));
        let expected = serde_json::to_string(&entry).unwrap();
        merge_named_entry(&config, name, &entry).unwrap();

        let mut edited: serde_json::Value =
            serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
        edited[name]["PreToolUse"][0]["matcher"] = serde_json::json!("something-else");
        fs::write(&config, serde_json::to_vec_pretty(&edited).unwrap()).unwrap();
        assert_eq!(
            inspect_named_entry(&config, name, &expected, None).state,
            AttachmentState::Drifted
        );
        assert_eq!(
            remove_named_entry(&config, name, &expected, None)
                .unwrap()
                .state,
            AttachmentState::Drifted,
            "a drifted entry is reported, never removed"
        );

        fs::write(&config, "{not json").unwrap();
        assert_eq!(
            inspect_named_entry(&config, name, &expected, None).state,
            AttachmentState::Blocked
        );
        assert_eq!(fs::read_to_string(&config).unwrap(), "{not json");
    }

    /// A file that held nothing but UZE's own entry was created by UZE and
    /// goes away with it; one holding anything else stays.
    #[test]
    fn a_named_config_that_uze_created_is_removed_with_its_last_entry() {
        let root = uze_testkit::temp::scratch("hooks-named-empty");
        fs::create_dir_all(&root).unwrap();
        let config = root.join("hooks.json");
        let name = "pkg@market:protect-env";
        let entry = agy_named_entry(&hook(), Path::new("/state/hooks/exec"), Path::new("/pkg"));
        let expected = serde_json::to_string(&entry).unwrap();
        merge_named_entry(&config, name, &entry).unwrap();
        remove_named_entry(&config, name, &expected, None).unwrap();
        assert!(!config.exists(), "UZE removes the file it alone created");
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
        let entry = group_entry("claude", &hook(), &invocation(&hook()));
        let expected = serde_json::to_string(&entry).unwrap();
        let path = merge_event_entry(&config, HookEvent::PreToolUse, &entry, &[]).unwrap();
        assert_eq!(path, config);
        assert_eq!(
            inspect_event_entry(&config, HookEvent::PreToolUse, &expected, None).state,
            AttachmentState::Matched
        );
        // Idempotence: a second merge changes nothing.
        merge_event_entry(&config, HookEvent::PreToolUse, &entry, &[]).unwrap();
        assert_eq!(
            inspect_event_entry(&config, HookEvent::PreToolUse, &expected, None).state,
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
            inspect_event_entry(&config, HookEvent::PostToolUse, &expected, None).state,
            AttachmentState::Missing,
            "an entry in the wrong event array is not matched"
        );
        assert_eq!(
            remove_event_entry(&config, HookEvent::PreToolUse, &expected, None)
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
        let old_entry = group_entry("codex", &old, &invocation(&old));
        merge_event_entry(&config, HookEvent::PreToolUse, &old_entry, &[]).unwrap();
        let mut updated = hook();
        updated.handlers[0].timeout = 20;
        let new_entry = group_entry("codex", &updated, &invocation(&updated));
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
        assert_eq!(entries[0]["hooks"][0]["timeout"], 22);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn drift_blocks_removal_and_an_empty_file_is_removed() {
        let root = uze_testkit::temp::scratch("hooks-drift");
        fs::create_dir_all(&root).unwrap();
        let config = root.join("hooks.json");
        let entry = group_entry("codex", &hook(), &invocation(&hook()));
        let expected = serde_json::to_string(&entry).unwrap();
        merge_event_entry(&config, HookEvent::PreToolUse, &entry, &[]).unwrap();
        // A user rewrites the UZE group — removal must inspect first and refuse.
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
        fs::write(
            &config,
            serde_json::to_string(&value)
                .unwrap()
                .replace("\"timeout\":12", "\"timeout\":99"),
        )
        .unwrap();
        assert_eq!(
            remove_event_entry(&config, HookEvent::PreToolUse, &expected, None)
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
            remove_event_entry(&config, HookEvent::PreToolUse, &expected, None)
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
            remove_event_entry(&solo, HookEvent::PreToolUse, &expected, None)
                .unwrap()
                .state,
            AttachmentState::Missing
        );
        assert!(!solo.exists(), "an empty UZE-only file is cleaned up");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_opencode_plugin_is_the_wrapper_with_the_packages_groups_as_data() {
        let plugin = opencode_bridge(&[&hook()], Path::new("/tmp/plugin root"), "hook-demo");
        // V2 plugin API (spec: opencode.ai/v2/docs/build/plugins) — a
        // Plugin.define module registering ctx.tool.hook callbacks.
        assert!(plugin.contains("import { Plugin } from \"@opencode-ai/plugin\""));
        assert!(plugin.contains("Plugin.define"));
        assert!(plugin.contains("id: \"hooks-hook-demo\""));
        assert!(plugin.contains("ctx.tool.hook(\"execute.before\""));
        assert!(plugin.contains("ctx.tool.hook(\"execute.after\""));
        assert!(
            plugin.contains("Bun.spawn"),
            "the harness's embedded Bun runtime executes handlers"
        );
        assert!(plugin.contains("\"event\":\"pre_tool_use\""));
        assert!(plugin.contains("\"matchers\":[\"bash\",\"Write\"]"));
        assert!(plugin.contains("\"effect\":\"deny\""));
        assert!(
            plugin.contains("bash: { tool: \"shell\", fields: (input) => ({ HOOK_COMMAND:"),
            "the alias table comes from the one vocabulary"
        );
        assert!(
            plugin.contains("code === 3"),
            "the decision channel is the exit code"
        );
        assert!(
            !plugin.contains("Stop"),
            "no stop surface is ever claimed for OpenCode"
        );
        assert!(
            !plugin.to_lowercase().contains("uze"),
            "nothing in the delivered artifact names the packager"
        );
        assert_eq!(
            plugin,
            opencode_bridge(&[&hook()], Path::new("/tmp/plugin root"), "hook-demo"),
            "generation is deterministic"
        );
    }

    #[test]
    fn bridge_path_lives_in_the_auto_discovered_global_plugin_directory() {
        let root = uze_testkit::temp::scratch("hooks-path");
        let bridge = opencode_bridge_path(&root, "demo");
        assert_eq!(
            bridge,
            root.join("plugins/hooks-demo.ts"),
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
        let bridge = root.join("plugins/hooks-demo.ts");
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

    /// A merge rewrites the whole document, so the order the user's own
    /// keys are in is UZE's to lose. A hand-organised `settings.json` must
    /// come back in the order it was written, with the merged key appended
    /// rather than sorted into the middle.
    #[test]
    fn a_merge_keeps_the_users_own_key_order() {
        let root = uze_testkit::temp::scratch("hooks-key-order");
        fs::create_dir_all(&root).unwrap();
        let config = root.join("settings.json");
        fs::write(
            &config,
            r#"{"zed":{"nested":1,"already":2},"model":"opus","apiKeyHelper":"~/bin/key"}"#,
        )
        .unwrap();

        let entry = group_entry("claude", &hook(), &invocation(&hook()));
        merge_event_entry(&config, HookEvent::PreToolUse, &entry, &[]).unwrap();

        let after = fs::read_to_string(&config).unwrap();
        let keys: Vec<&str> = after
            .lines()
            .filter_map(|line| line.strip_prefix("  \""))
            .filter_map(|line| line.split('"').next())
            .collect();
        assert_eq!(
            keys,
            ["zed", "model", "apiKeyHelper", "hooks"],
            "the user's keys keep their order and UZE's is appended: {after}"
        );
        assert!(
            after.contains("\"nested\": 1,"),
            "a nested foreign object keeps its own order too: {after}"
        );
        let _ = fs::remove_dir_all(root);
    }

    /// The executable bit is half the wrapper: `write_atomic` publishes
    /// under the umask and chmods afterwards, so a crash between the two
    /// leaves the right bytes unrunnable — exit 126, which a `deny` group
    /// turns into a permanent block.
    #[cfg(unix)]
    #[test]
    fn a_wrapper_that_lost_its_executable_bit_is_drift_and_is_repaired() {
        use std::os::unix::fs::PermissionsExt;

        let root = uze_testkit::temp::scratch("hooks-wrapper-mode");
        let wrapper = root.join("hooks").join("exec");
        let source = wrapper_source("claude").unwrap();
        materialize_wrapper(&wrapper, &source).unwrap();
        fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o644)).unwrap();

        assert_eq!(
            inspect_wrapper(Some(("claude", &wrapper))).map(|inspection| inspection.state),
            Some(AttachmentState::Drifted),
            "a wrapper the harness cannot execute is drift, not a match"
        );
        materialize_wrapper(&wrapper, &source).unwrap();
        assert_eq!(
            fs::metadata(&wrapper).unwrap().permissions().mode() & 0o777,
            0o755,
            "re-materializing repairs the mode even when the bytes match"
        );
        assert!(inspect_wrapper(Some(("claude", &wrapper))).is_none());
        assert_eq!(fs::read_to_string(&wrapper).unwrap(), source);
        let _ = fs::remove_dir_all(root);
    }

    fn hook_receipt(
        config: &Path,
        entry_name: &str,
        expected: &str,
        wrapper: &Path,
    ) -> uze_core::integration::AttachmentReceipt {
        uze_core::integration::AttachmentReceipt {
            package_id: "pkg@market".to_owned(),
            resource_identity: None,
            integration: ANTIGRAVITY_TARGET.to_owned(),
            strategy: "hook-config-entry".to_owned(),
            artifact: uze_core::integration::ManagedArtifact::HookConfigEntry {
                config_file: config.to_path_buf(),
                entry_name: entry_name.to_owned(),
                event: HookEvent::PreToolUse,
                expected: expected.to_owned(),
                wrapper: wrapper.to_path_buf(),
            },
        }
    }

    /// The shared wrapper outlives every entry but the last one. The prune
    /// runs inside a detach, while the ledger still lists the receipt being
    /// detached — so "still used" has to be read from the harness's config,
    /// not from the ledger, or the wrapper is never removed at all.
    #[test]
    fn the_last_detached_hook_entry_takes_the_shared_wrapper_with_it() {
        let root = uze_testkit::temp::scratch("hooks-prune");
        fs::create_dir_all(&root).unwrap();
        let home = UzeHome::at(root.join("home"));
        let config = root.join("hooks.json");
        let wrapper = shared_wrapper_path(&home, ANTIGRAVITY_TARGET);
        materialize_wrapper(&wrapper, &wrapper_source(ANTIGRAVITY_TARGET).unwrap()).unwrap();

        let entry = agy_named_entry(&hook(), &wrapper, Path::new("/pkg"));
        let expected = serde_json::to_string(&entry).unwrap();
        let names = ["pkg@market:protect-env", "other@market:protect-env"];
        for name in names {
            merge_named_entry(&config, name, &entry).unwrap();
            uze_core::state::record_receipt(
                &home,
                name.to_owned(),
                hook_receipt(&config, name, &expected, &wrapper),
            )
            .unwrap();
        }

        remove_named_entry(&config, names[0], &expected, None).unwrap();
        prune_shared_wrapper(&home, ANTIGRAVITY_TARGET, ANTIGRAVITY_TARGET);
        assert!(
            wrapper.exists(),
            "a wrapper another entry still runs is kept"
        );

        remove_named_entry(&config, names[1], &expected, None).unwrap();
        prune_shared_wrapper(&home, ANTIGRAVITY_TARGET, ANTIGRAVITY_TARGET);
        assert!(
            !wrapper.exists(),
            "the last detached entry takes the shared wrapper with it"
        );
        let _ = fs::remove_dir_all(root);
    }
}

/// The generated wrapper against real `sh`: the same cases the reference
/// runtime answers, run through the file a harness would actually execute.
#[cfg(all(test, unix))]
mod wrapper_tests {
    use super::*;
    use std::{
        os::unix::fs::PermissionsExt,
        process::{Command, Stdio},
    };
    use uze_core::hook::{CommandHandlerType, HookEvent};

    const TARGETS: [&str; 3] = ["claude", "codex", "antigravity"];

    fn goldens_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("goldens")
    }

    /// A package whose handlers speak the portable contract: `guard` denies
    /// a command touching a secret, `audit` records what got through.
    fn package(label: &str) -> PathBuf {
        let root = uze_testkit::temp::scratch(label);
        let scripts = root.join("scripts");
        fs::create_dir_all(&scripts).unwrap();
        write_script(
            &scripts.join("guard"),
            "case \"$HOOK_COMMAND\" in\n  *.env*|*id_rsa*)\n    echo \"blocked: $HOOK_COMMAND (tool=$HOOK_TOOL cwd=$HOOK_CWD)\" >&2\n    exit 3 ;;\nesac\nexit 0",
        );
        write_script(
            &scripts.join("audit"),
            "printf '%s\\t%s\\n' \"$HOOK_HARNESS\" \"$HOOK_COMMAND\" >> \"$PLUGIN_ROOT/audit.log\"\nexit 0",
        );
        // A handler that never answers, and does it through a child of its
        // own — the shape a deadline has to survive: killing the shell that
        // started it leaves the child holding the pipe.
        write_script(&scripts.join("stall"), "sh -c 'sleep 30'\nexit 0");
        root
    }

    fn write_script(path: &Path, body: &str) {
        fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// A handler's command as the manifest carries it: a bare script name
    /// is shorthand for this package's own `scripts/<name>`, and anything
    /// else — an `sh` invocation, a flag, a relative path — is taken
    /// verbatim, because the ABI says a command is a shell command line.
    fn handler_command(spec: &str) -> String {
        if spec.contains(' ') || spec.contains('/') {
            spec.to_owned()
        } else {
            format!("${{PLUGIN_ROOT}}/scripts/{spec}")
        }
    }

    fn group(effect: HookEffect, handlers: &[&str]) -> PortableHook {
        group_at(HookEvent::PreToolUse, effect, handlers, 10)
    }

    fn group_at(
        event: HookEvent,
        effect: HookEffect,
        handlers: &[&str],
        timeout: u16,
    ) -> PortableHook {
        PortableHook {
            id: "protect-env".into(),
            event,
            matchers: vec![HookMatcher::Portable("shell".into())],
            handlers: handlers
                .iter()
                .map(|spec| CommandHook {
                    handler_type: CommandHandlerType::Command,
                    command: handler_command(spec),
                    timeout,
                })
                .collect(),
            effect,
            order: 0,
        }
    }

    struct Answer {
        exit: i32,
        stdout: String,
        stderr: String,
    }

    /// Runs the generated wrapper exactly as the harness does: the payload
    /// on stdin, the group's own arguments on the command line.
    fn run_wrapper(
        target: &str,
        root: &Path,
        hook: &PortableHook,
        payload: &str,
        jq: Option<&str>,
    ) -> Answer {
        let wrapper = root.join("hooks").join("exec");
        materialize_wrapper(&wrapper, &wrapper_source(target).unwrap()).unwrap();
        let mut command = Command::new(&wrapper);
        command.args(wrapper_arguments(hook, root, &hook.handlers));
        if let Some(jq) = jq {
            command.env("HOOK_JQ", jq);
        }
        // A sibling test forking while this file's write descriptor is
        // still open leaves the kernel reporting ETXTBSY for a moment; the
        // wrapper is on disk and complete, so the answer is to look again.
        let mut child = loop {
            match command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
            {
                Ok(child) => break child,
                Err(error) if error.raw_os_error() == Some(26) => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(error) => panic!("cannot start the generated wrapper: {error}"),
            }
        };
        use std::io::Write;
        // A wrapper that denies before reading stdin (a missing dependency)
        // closes the pipe first; that is an answer, not a test failure.
        let _ = child.stdin.take().unwrap().write_all(payload.as_bytes());
        let output = child.wait_with_output().unwrap();
        Answer {
            exit: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    /// A `stop` payload as each harness sends it: no tool at all, which is
    /// what the wrapper has to leave the handler seeing.
    fn stop_payload(target: &str) -> String {
        match target {
            "antigravity" => serde_json::json!({"workspacePaths": ["/repo"]}).to_string(),
            _ => serde_json::json!({"cwd": "/repo"}).to_string(),
        }
    }

    fn payload(target: &str, command: &str) -> String {
        match target {
            "antigravity" => serde_json::json!({
                "toolCall": {"name": "run_command", "args": {"CommandLine": command, "Cwd": "/repo"}},
                "workspacePaths": ["/repo"],
            })
            .to_string(),
            _ => serde_json::json!({
                "tool_name": if target == "codex" { "exec_command" } else { "Bash" },
                "tool_input": if target == "codex" {
                    serde_json::json!({"cmd": command})
                } else {
                    serde_json::json!({"command": command})
                },
                "cwd": "/repo",
            })
            .to_string(),
        }
    }

    /// What a denial exits with, per harness. Claude and Codex document
    /// exit 2 as the block signal; Antigravity reads the decision from
    /// stdout and logs any non-zero exit as a *failed* hook, so a denial
    /// there exits 0 (measured on 1.1.24).
    fn block_exit(target: &str) -> i32 {
        if target == ANTIGRAVITY_TARGET { 0 } else { 2 }
    }

    #[test]
    #[ignore = "regenerates the goldens; run with --ignored after changing the template"]
    fn regenerate_goldens() {
        for target in TARGETS {
            fs::create_dir_all(goldens_dir()).unwrap();
            fs::write(
                goldens_dir().join(format!("hooks-exec-{target}.sh")),
                wrapper_source(target).unwrap(),
            )
            .unwrap();
        }
    }

    #[test]
    fn the_wrapper_is_one_byte_identical_file_per_harness() {
        for target in TARGETS {
            let source = wrapper_source(target).expect("every command-hook harness has a wrapper");
            assert_eq!(
                source,
                wrapper_source(target).unwrap(),
                "{target}'s wrapper must be deterministic"
            );
            let golden = goldens_dir().join(format!("hooks-exec-{target}.sh"));
            assert_eq!(
                fs::read_to_string(&golden).unwrap_or_default(),
                source,
                "{} is out of date; regenerate it from wrapper_source",
                golden.display()
            );
            assert!(
                !source.to_lowercase().contains("uze"),
                "nothing in a delivered artifact may name the packager"
            );
        }
    }

    #[test]
    fn a_denial_is_relayed_in_each_harnesss_own_dialect() {
        for target in TARGETS {
            let root = package(&format!("wrapper-deny-{target}"));
            let hook = group(HookEffect::Deny, &["guard", "audit"]);
            let answer = run_wrapper(target, &root, &hook, &payload(target, "cat .env"), None);
            assert_eq!(
                answer.exit,
                block_exit(target),
                "{target}: a denial uses this harness's block signal"
            );
            assert!(
                answer.stderr.contains("blocked: cat .env"),
                "{target}: the reason reaches stderr"
            );
            let document: serde_json::Value = serde_json::from_str(answer.stdout.trim()).unwrap();
            let (decision, reason) = if target == "antigravity" {
                (&document["decision"], &document["reason"])
            } else {
                (
                    &document["hookSpecificOutput"]["permissionDecision"],
                    &document["hookSpecificOutput"]["permissionDecisionReason"],
                )
            };
            assert_eq!(*decision, "deny");
            assert!(reason.as_str().unwrap().contains("blocked: cat .env"));
            assert!(
                reason.as_str().unwrap().contains("tool=shell"),
                "{target}: the handler read the portable alias, not a native name"
            );
            assert!(
                !root.join("audit.log").exists(),
                "{target}: the denial stopped the second handler"
            );
            let _ = fs::remove_dir_all(root);
        }
    }

    #[test]
    fn an_allowance_lets_the_next_handler_run() {
        for target in TARGETS {
            let root = package(&format!("wrapper-allow-{target}"));
            let hook = group(HookEffect::Deny, &["guard", "audit"]);
            let answer = run_wrapper(target, &root, &hook, &payload(target, "ls -la"), None);
            assert_eq!(answer.exit, 0, "{target}: nothing was denied");
            assert_eq!(
                fs::read_to_string(root.join("audit.log")).unwrap(),
                format!("{target}\tls -la\n"),
                "{target}: the second handler ran and read the portable command"
            );
            let _ = fs::remove_dir_all(root);
        }
    }

    #[test]
    fn a_handler_that_cannot_run_follows_the_groups_effect() {
        for target in TARGETS {
            let root = package(&format!("wrapper-fail-{target}"));
            let closed = group(HookEffect::Deny, &["absent"]);
            let answer = run_wrapper(target, &root, &closed, &payload(target, "ls"), None);
            assert_eq!(
                answer.exit,
                block_exit(target),
                "{target}: a deny group fails closed"
            );
            assert!(answer.stderr.contains("handler failed"));

            let open = group(HookEffect::Observe, &["absent"]);
            let answer = run_wrapper(target, &root, &open, &payload(target, "ls"), None);
            assert_eq!(answer.exit, 0, "{target}: an observe group fails open");
            assert!(answer.stderr.contains("handler failed"));
            let _ = fs::remove_dir_all(root);
        }
    }

    #[test]
    fn a_missing_wrapper_dependency_follows_the_groups_effect() {
        for target in TARGETS {
            let root = package(&format!("wrapper-jq-{target}"));
            let closed = group(HookEffect::Deny, &["guard"]);
            let answer = run_wrapper(
                target,
                &root,
                &closed,
                &payload(target, "ls"),
                Some("/nonexistent/jq"),
            );
            assert_eq!(
                answer.exit,
                block_exit(target),
                "{target}: a deny group denies without jq"
            );
            assert!(answer.stderr.contains("jq is not installed"));

            let open = group(HookEffect::Observe, &["guard"]);
            let answer = run_wrapper(
                target,
                &root,
                &open,
                &payload(target, "ls"),
                Some("/nonexistent/jq"),
            );
            assert_eq!(answer.exit, 0, "{target}: an observe group proceeds");
            assert!(answer.stderr.contains("jq is not installed"));
            let _ = fs::remove_dir_all(root);
        }
    }

    #[test]
    fn a_native_tool_the_vocabulary_does_not_bind_carries_raw_input_only() {
        let root = uze_testkit::temp::scratch("wrapper-native");
        let scripts = root.join("scripts");
        fs::create_dir_all(&scripts).unwrap();
        write_script(
            &scripts.join("probe"),
            "printf '%s|%s|%s' \"$HOOK_TOOL\" \"$HOOK_TOOL_NATIVE\" \"$HOOK_INPUT\" \
             > \"$PLUGIN_ROOT/seen.txt\"\nexit 0",
        );
        let hook = group(HookEffect::Observe, &["probe"]);
        let payload = serde_json::json!({
            "tool_name": "SomeVendorOnlyTool",
            "tool_input": {"anything": "x"},
        })
        .to_string();
        let answer = run_wrapper("claude", &root, &hook, &payload, None);
        assert_eq!(answer.exit, 0);
        assert_eq!(
            fs::read_to_string(root.join("seen.txt")).unwrap(),
            r#"|SomeVendorOnlyTool|{"anything":"x"}"#
        );
        let _ = fs::remove_dir_all(root);
    }

    /// The author's `timeout` is the bound the handler actually gets. It is
    /// the only bound there is: the native entry's group timeout is the
    /// *harness's* backstop, which a harness is free to ignore, and a
    /// hanging `deny` handler would otherwise stall every matching tool
    /// call for as long as the handler felt like taking.
    #[test]
    fn a_handler_is_stopped_at_the_deadline_its_author_declared() {
        for target in TARGETS {
            let root = package(&format!("wrapper-timeout-{target}"));
            let closed = group_at(HookEvent::PreToolUse, HookEffect::Deny, &["stall"], 1);
            let started = std::time::Instant::now();
            let answer = run_wrapper(target, &root, &closed, &payload(target, "ls"), None);
            let elapsed = started.elapsed();
            assert!(
                elapsed < std::time::Duration::from_secs(20),
                "{target}: the 1s bound decided when to stop waiting, not the handler: {elapsed:?}"
            );
            assert_eq!(
                answer.exit,
                block_exit(target),
                "{target}: a deny group whose handler timed out blocks"
            );
            assert!(
                answer.stderr.contains("timed out after 1s"),
                "{target}: the reason names the author's own bound: {}",
                answer.stderr
            );

            let open = group_at(HookEvent::PreToolUse, HookEffect::Observe, &["stall"], 1);
            let answer = run_wrapper(target, &root, &open, &payload(target, "ls"), None);
            assert_eq!(answer.exit, 0, "{target}: an observe group proceeds");
            assert!(answer.stderr.contains("timed out after 1s"));
            let _ = fs::remove_dir_all(root);
        }
    }

    /// A `stop` payload carries no tool at all, and the wrapper has to
    /// leave the handler seeing that rather than a stale or invented one.
    #[test]
    fn a_stop_payload_leaves_the_handler_without_a_tool() {
        for target in TARGETS {
            let root = package(&format!("wrapper-stop-{target}"));
            write_script(
                &root.join("scripts").join("probe"),
                "printf '%s|%s|%s' \"$HOOK_EVENT\" \"$HOOK_TOOL\" \"$HOOK_TOOL_NATIVE\" \
                 > \"$PLUGIN_ROOT/seen.txt\"\nexit 0",
            );
            let hook = group_at(HookEvent::Stop, HookEffect::Observe, &["probe"], 10);
            let answer = run_wrapper(target, &root, &hook, &stop_payload(target), None);
            assert_eq!(answer.exit, 0, "{target}: a stop observation proceeds");
            assert_eq!(
                fs::read_to_string(root.join("seen.txt")).unwrap(),
                "stop||",
                "{target}: no tool is invented for a payload that carries none"
            );
            let _ = fs::remove_dir_all(root);
        }
    }

    // ========================================================================
    // The recorded answers
    // ========================================================================

    /// One fixture the wrapper is run against: a group, the payload it is
    /// fired with, and what the handlers do.
    struct Fixture {
        event: HookEvent,
        effect: HookEffect,
        handlers: &'static [&'static str],
        timeout: u16,
        /// The shell command the payload carries. `None` is a `stop`
        /// payload, which carries no tool.
        command: Option<&'static str>,
        /// Whether the recorded stderr is the wrapper's own words all the
        /// way. A handler that never started is reported with the system
        /// shell's diagnostic appended, and that wording is the platform's,
        /// so only the head of the line is recorded.
        wrapper_owns_the_whole_reason: bool,
    }

    /// Every fixture, in the order the recorded table holds them.
    fn fixtures() -> Vec<Fixture> {
        let case = |event, effect, handlers, command| Fixture {
            event,
            effect,
            handlers,
            timeout: 10,
            command,
            wrapper_owns_the_whole_reason: true,
        };
        let pre = HookEvent::PreToolUse;
        vec![
            case(pre, HookEffect::Deny, &["guard", "audit"], Some("cat .env")),
            case(pre, HookEffect::Deny, &["guard", "audit"], Some("ls -la")),
            Fixture {
                wrapper_owns_the_whole_reason: false,
                ..case(pre, HookEffect::Deny, &["absent"], Some("ls"))
            },
            Fixture {
                wrapper_owns_the_whole_reason: false,
                ..case(pre, HookEffect::Observe, &["absent"], Some("ls"))
            },
            // A command line, not an executable path: the shapes the
            // manifest documents and a bare-argv runner cannot start.
            case(
                pre,
                HookEffect::Deny,
                &["sh ${PLUGIN_ROOT}/scripts/guard --strict", "audit"],
                Some("cat .env"),
            ),
            case(
                pre,
                HookEffect::Deny,
                &["sh ${PLUGIN_ROOT}/scripts/guard --strict", "audit"],
                Some("ls -la"),
            ),
            case(pre, HookEffect::Deny, &["scripts/guard"], Some("cat .env")),
            case(
                pre,
                HookEffect::Deny,
                &["scripts/guard", "audit"],
                Some("ls -la"),
            ),
            case(
                HookEvent::PostToolUse,
                HookEffect::Observe,
                &["audit"],
                Some("ls"),
            ),
            case(HookEvent::Stop, HookEffect::Observe, &["audit"], None),
            // A handler that never answers is a handler failure like any
            // other: the deadline is its author's, and the group's effect
            // decides what that means.
            Fixture {
                timeout: 1,
                ..case(pre, HookEffect::Deny, &["stall"], Some("ls"))
            },
            Fixture {
                timeout: 1,
                ..case(pre, HookEffect::Observe, &["stall"], Some("ls"))
            },
        ]
    }

    fn answers_path() -> PathBuf {
        goldens_dir().join("hooks").join("wrapper-answers.json")
    }

    /// Runs one fixture through one harness's wrapper and records the
    /// answer, with the throwaway package root written back as the
    /// placeholder an author would have typed.
    fn recorded_answer(target: &str, index: usize, fixture: &Fixture) -> serde_json::Value {
        let root = package(&format!("recorded-{target}-{index}"));
        let hook = group_at(
            fixture.event,
            fixture.effect,
            fixture.handlers,
            fixture.timeout,
        );
        let raw = match fixture.command {
            Some(command) => payload(target, command),
            None => stop_payload(target),
        };
        let answer = run_wrapper(target, &root, &hook, &raw, None);
        let portable = |text: &str| {
            text.trim()
                .replace(&root.display().to_string(), "${PLUGIN_ROOT}")
        };
        let stdout = portable(&answer.stdout);
        let mut case = serde_json::Map::new();
        case.insert("harness".to_owned(), serde_json::json!(target));
        case.insert(
            "event".to_owned(),
            serde_json::json!(fixture.event.abi_name()),
        );
        case.insert(
            "effect".to_owned(),
            serde_json::json!(fixture.effect.abi_name()),
        );
        case.insert(
            "handlers".to_owned(),
            serde_json::json!(fixture.handlers.to_vec()),
        );
        case.insert("command".to_owned(), serde_json::json!(fixture.command));
        case.insert("exit".to_owned(), serde_json::json!(answer.exit));
        let document = if stdout.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_str(&stdout).expect("the wrapper answers with one JSON document")
        };
        case.insert(
            "stdout".to_owned(),
            if fixture.wrapper_owns_the_whole_reason {
                document
            } else {
                without_the_shells_own_words(document)
            },
        );
        let stderr = portable(&answer.stderr);
        if fixture.wrapper_owns_the_whole_reason {
            case.insert("stderr".to_owned(), serde_json::json!(stderr));
        } else {
            case.insert(
                "stderr_prefix".to_owned(),
                serde_json::json!(stderr.split(" — ").next().unwrap_or_default()),
            );
        }
        let _ = fs::remove_dir_all(root);
        serde_json::Value::Object(case)
    }

    /// A reason the system shell contributed the tail of, cut back to the
    /// wrapper's own words: `sh` says "not found" on one platform and "No
    /// such file or directory" on another, and neither is a contract.
    fn without_the_shells_own_words(value: serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::String(text) => serde_json::Value::String(
                text.split(" \u{2014} ")
                    .next()
                    .unwrap_or_default()
                    .to_owned(),
            ),
            serde_json::Value::Object(fields) => serde_json::Value::Object(
                fields
                    .into_iter()
                    .map(|(key, value)| (key, without_the_shells_own_words(value)))
                    .collect(),
            ),
            other => other,
        }
    }

    fn recorded_answers() -> Vec<serde_json::Value> {
        let fixtures = fixtures();
        let mut answers = Vec::new();
        for target in TARGETS {
            for (index, fixture) in fixtures.iter().enumerate() {
                answers.push(recorded_answer(target, index, fixture));
            }
        }
        answers
    }

    #[test]
    #[ignore = "rewrites the recorded answers; run with --ignored and review the diff"]
    fn regenerate_wrapper_answers() {
        let table = serde_json::json!({
            "about": "What the generated hooks/exec wrapper answers for every \
                      fixture, per harness: the native decision document, the \
                      exit status and the reason on stderr. Regenerate with \
                      `cargo test -p uze-integrations regenerate_wrapper_answers \
                      -- --ignored`; every changed line is a changed contract \
                      and belongs in the review.",
            "answers": recorded_answers(),
        });
        fs::create_dir_all(answers_path().parent().unwrap()).unwrap();
        fs::write(
            answers_path(),
            format!("{}\n", serde_json::to_string_pretty(&table).unwrap()),
        )
        .unwrap();
    }

    /// The wrapper is the only implementation of the contract, so what it
    /// answers is the contract — recorded once, per harness, per fixture.
    ///
    /// The recorded values were taken from the reference runtime this file
    /// used to be tested against (`uze hook-exec`, removed 2026-09-12): each
    /// pre-tool fixture below was proven to answer identically on both
    /// routes before the runtime was deleted. A golden that changes is a
    /// changed contract, and the diff is where that gets reviewed — never a
    /// regeneration folded into an unrelated change.
    #[test]
    fn the_wrapper_answers_every_fixture_as_recorded() {
        let table: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(answers_path()).unwrap_or_default())
                .expect("the recorded answers are readable");
        let expected = table["answers"]
            .as_array()
            .expect("the recorded answers are a list");
        let actual = recorded_answers();
        assert_eq!(
            expected.len(),
            actual.len(),
            "the fixture set changed; regenerate {}",
            answers_path().display()
        );
        for (recorded, answered) in expected.iter().zip(actual) {
            assert_eq!(
                *recorded, answered,
                "{}/{} answered differently than recorded",
                recorded["harness"], recorded["event"]
            );
        }
    }
}

/// The generated OpenCode plugin against the real Bun runtime, driven with a
/// V2-shaped plugin context. Skipped where Bun is absent: the plugin is a
/// delivered artifact for a harness that embeds Bun, and the goldens above
/// keep its bytes honest without it.
#[cfg(all(test, unix))]
mod opencode_runtime_tests {
    use super::*;
    use std::{os::unix::fs::PermissionsExt, process::Command};
    use uze_core::hook::{CommandHandlerType, HookEvent};

    fn bun_available() -> bool {
        Command::new("bun")
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
    }

    #[test]
    fn the_plugin_runs_the_handlers_on_the_harnesss_own_runtime() {
        if !bun_available() {
            eprintln!("bun is not installed; the OpenCode plugin runtime check is skipped");
            return;
        }
        let root = uze_testkit::temp::scratch("opencode-runtime");
        let scripts = root.join("scripts");
        fs::create_dir_all(&scripts).unwrap();
        for (name, body) in [
            (
                "guard",
                "case \"$HOOK_COMMAND\" in\n  *.env*)\n    echo \"blocked: $HOOK_COMMAND\" >&2\n    exit 3 ;;\nesac\nexit 0",
            ),
            (
                "audit",
                "printf '%s|%s|%s\\n' \"$HOOK_HARNESS\" \"$HOOK_TOOL\" \"$HOOK_COMMAND\" \
                 >> \"$PLUGIN_ROOT/audit.log\"\nexit 0",
            ),
        ] {
            let path = scripts.join(name);
            fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let hook = PortableHook {
            id: "protect-env".into(),
            event: HookEvent::PreToolUse,
            matchers: vec![HookMatcher::Portable("shell".into())],
            handlers: ["guard", "audit"]
                .into_iter()
                .map(|name| CommandHook {
                    handler_type: CommandHandlerType::Command,
                    command: format!("${{PLUGIN_ROOT}}/scripts/{name}"),
                    timeout: 10,
                })
                .collect(),
            effect: HookEffect::Observe,
            order: 0,
        };
        let plugin = root.join("hooks-demo.ts");
        fs::write(&plugin, opencode_bridge(&[&hook], &root, "demo")).unwrap();

        // The harness supplies this module; outside it, a stub that hands
        // the definition straight back is enough to drive the plugin.
        let stub = root
            .join("node_modules")
            .join("@opencode-ai")
            .join("plugin");
        fs::create_dir_all(&stub).unwrap();
        fs::write(
            stub.join("index.ts"),
            "export const Plugin = { define: (definition) => definition };\n",
        )
        .unwrap();

        fs::write(
            root.join("drive.ts"),
            r#"import plugin from "./hooks-demo.ts";
const hooks = {};
await plugin.setup({ tool: { hook: async (name, fn) => { hooks[name] = fn; } } });
const errors = [];
console.error = (...parts) => errors.push(parts.join(" "));
await hooks["execute.before"]({ tool: "bash", input: { command: "cat .env" } });
await hooks["execute.before"]({ tool: "bash", input: { command: "ls -la" } });
await hooks["execute.before"]({ tool: "read", input: { filePath: "/x" } });
console.log(JSON.stringify(errors));
"#,
        )
        .unwrap();

        let output = Command::new("bun")
            .arg("run")
            .arg("drive.ts")
            .current_dir(&root)
            .output()
            .expect("bun runs the driver");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "the plugin must load and run: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let reported: Vec<String> = serde_json::from_str(stdout.trim()).unwrap();
        assert!(
            reported
                .iter()
                .any(|line| line.contains("blocked: cat .env")),
            "the denial reason is reported: {reported:?}"
        );
        assert_eq!(
            fs::read_to_string(root.join("audit.log")).unwrap(),
            "opencode|shell|ls -la\n",
            "the second handler ran only for the allowed call, with the portable context"
        );
        let _ = fs::remove_dir_all(root);
    }
}
