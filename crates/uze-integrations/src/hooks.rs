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

/// How a delivered hook is invoked by the harness. The native route runs
/// the generated wrapper; the fallback runs the packager's own runtime
/// where no wrapper template applies (see [`hook_delivery`]).
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
    // The native timeout is a backstop for the whole group; the sum of the
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
    entry.insert(
        "hooks".to_owned(),
        serde_json::Value::Array(vec![serde_json::Value::Object(invoked)]),
    );
    serde_json::Value::Object(entry)
}

/// One group's delivery: the native entry, the wrapper it needs on disk,
/// and — when the native route did not apply — why the packager runtime is
/// carrying it instead.
pub(crate) struct HookDelivery {
    pub entry: serde_json::Value,
    pub wrapper: Option<PathBuf>,
    pub adapted_reason: Option<String>,
}

/// Chooses how one group reaches a harness and renders it.
///
/// Native-first: the generated wrapper, vendored beside the delivery, with
/// nothing of the packager on the execution path. The packager's own
/// runtime stays the fallback for a platform the POSIX `sh` template does
/// not cover — the delivery still speaks the same contract, and the route
/// is reported rather than hidden.
pub(crate) fn hook_delivery(
    uze_home: &UzeHome,
    target: &str,
    adapter_id: &str,
    hook: &PortableHook,
    package_root: &Path,
    wrapper: Option<PathBuf>,
    exec_form: bool,
) -> HookDelivery {
    match wrapper.filter(|_| cfg!(unix)) {
        Some(wrapper) => {
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
            HookDelivery {
                entry: group_entry(target, hook, &invocation),
                wrapper: Some(wrapper),
                adapted_reason: None,
            }
        }
        None => {
            let _ = uze_home;
            let executable = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("uze"));
            let invocation = HookInvocation::Line(dispatcher_command(
                &executable,
                adapter_id,
                hook.event,
                hook.effect,
                package_root,
                &hook.handlers,
            ));
            HookDelivery {
                entry: group_entry(target, hook, &invocation),
                wrapper: None,
                adapted_reason: Some(
                    "no wrapper template covers this platform; the packager runtime carries the \
                     hook with the same contract"
                        .to_owned(),
                ),
            }
        }
    }
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
/// carrying the translated matcher and the wrapper invocation.
/// Deterministic per package.
///
/// The root is the hook map itself, never a `hooks` wrapper: the vendor
/// reads every root key as one named hook, so a wrapper registers a single
/// hook called `hooks` whose "events" are our ids — and no handler ever
/// runs (AGY 1.1.24: `plugin validate` reports 1 hook processed instead of
/// one per group, and the loader fires nothing).
pub(crate) fn agy_hook_document(
    hooks: &[&PortableHook],
    wrapper: &Path,
    package_root: &Path,
) -> String {
    let mut named = serde_json::Map::new();
    for hook in hooks {
        let invocation = HookInvocation::Line(wrapper_command_line(wrapper, hook, package_root));
        let entry = group_entry("antigravity", hook, &invocation);
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
    let WrapperDialect {
        harness,
        tool_filter,
        input_filter,
        cwd_filter,
        deny_document,
        allow_document,
    } = dialect;
    Some(format!(
        r#"#!/bin/sh
# hooks/exec — generated from hooks.json, one per harness. The harness runs
# this; it runs the author's handlers. The handlers never see a harness
# payload and never write harness JSON: the context arrives as HOOK_*
# environment and the decision leaves as an exit code (0 allow, 3 deny with
# the reason on stderr; anything else is a failure that follows the group's
# effect).
#
#   usage: exec <plugin-root> <event> <effect> <handler>...
#     event   pre_tool_use | post_tool_use | stop
#     effect  observe | allow | ask | deny
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
  exit 2                                          # the harness's block signal
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

# --- the handlers, in order; the first denial stops the rest --------------
for handler in "$@"; do
  reason=$("$handler" 2>&1 >/dev/null); status=$?
  case $status in
    0) ;;
    3) deny_native "${{reason:-$handler denied the operation}}" ;;
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
    // the harness.
    if fs::read_to_string(path).is_ok_and(|current| current == source) {
        return Ok(());
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

/// Removes a shared wrapper once no hook receipt of this integration is
/// left to use it. A wrapper still referenced by another group's entry is
/// kept: it is one file serving every package.
pub(crate) fn prune_shared_wrapper(uze_home: &UzeHome, integration_id: &str, target: &str) {
    let still_used = uze_core::state::receipts(uze_home, None).is_ok_and(|ledger| {
        ledger.iter().any(|(_, receipt)| {
            receipt.integration == integration_id
                && matches!(
                    receipt.artifact,
                    uze_core::integration::ManagedArtifact::HookConfigEntry { .. }
                )
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

/// The native command an entry runs: the wrapper, the package root, the
/// group's event and effect, then the author's handlers with
/// `${PLUGIN_ROOT}` already resolved. Everything harness-specific is
/// decided here, at generation time.
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
        arguments.push(
            handler
                .command
                .replace("${PLUGIN_ROOT}", &package_root.display().to_string()),
        );
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
    // The wrapper is the other half of the delivery: an entry pointing at a
    // missing or edited wrapper is drift, not a match.
    if let Some((target, path)) = wrapper {
        match fs::read_to_string(path) {
            Err(_) => {
                return AttachmentInspection {
                    state: AttachmentState::Missing,
                    reason: "the generated hook wrapper is absent".to_owned(),
                };
            }
            Ok(current) => {
                if wrapper_source(target).is_none_or(|expected| expected != current) {
                    return AttachmentInspection {
                        state: AttachmentState::Drifted,
                        reason: "the generated hook wrapper does not match what UZE writes"
                            .to_owned(),
                    };
                }
            }
        }
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
#[allow(clippy::too_many_arguments)]
pub(crate) fn hook_exposure_plan(
    uze_home: &UzeHome,
    resource: &Resource,
    capabilities: &HookCapabilities,
    config_file: PathBuf,
    target: &str,
    adapter_id: &str,
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
    let mut adapted_reason = None;
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
            let delivery = hook_delivery(
                uze_home,
                target,
                adapter_id,
                &hook,
                package_root,
                Some(shared_wrapper_path(uze_home, target)),
                exec_form,
            );
            adapted_reason = delivery.adapted_reason;
            ExposureMechanism::ManagedHookConfig {
                config_file,
                entry_name: hook_entry_name(resource, &hook),
                event: Some(hook.event),
                expected: serde_json::to_string(&delivery.entry).expect("hook entry serializes"),
                wrapper: delivery.wrapper,
            }
        }
    };
    // The route the delivery actually took is part of the verdict: a hook
    // the packager runtime carries is honestly Adaptable, never Native.
    let route = match (compatibility.route, &adapted_reason) {
        (CompatibilityRoute::Native, Some(_)) => CompatibilityRoute::Adaptable,
        (route, _) => route,
    };
    let evidence = match (&compatibility.reason, &adapted_reason) {
        (Some(reason), _) => format!("{evidence} Compatibility: {reason}"),
        (None, Some(adapted)) => format!("{evidence} Delivery: {adapted}."),
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

/// The matched tool as a handler sees it: the portable alias (when this
/// harness's vocabulary knows the native name) and the alias's portable
/// fields, each read from the native input field the vocabulary names. A
/// tool the table does not bind carries its native name and nothing else.
fn matched_tool(target: &str, native: &str, input: &serde_json::Value) -> HookTool {
    let binding = vocabulary(target).binding_for_native(native);
    HookTool {
        portable: binding.map(|entry| entry.alias.to_owned()),
        native: native.to_owned(),
        fields: binding
            .into_iter()
            .flat_map(|entry| entry.fields.iter())
            .filter_map(|(portable, native_field)| {
                input
                    .get(native_field)
                    .map(|value| ((*portable).to_owned(), scalar(value)))
            })
            .collect(),
    }
}

/// A native input field rendered for the environment: a string verbatim,
/// anything else as its JSON text, so a handler always reads a value and
/// never a Rust debug rendering.
fn scalar(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

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
    let input = native
        .get(input_key)
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let tool = match (tool_name, event) {
        (Some(name), HookEvent::PreToolUse | HookEvent::PostToolUse) => {
            Some(matched_tool(target, name, &input))
        }
        _ => None,
    };
    Ok(HookCommandInput {
        harness: target.to_owned(),
        event: event.abi_name().to_owned(),
        tool,
        input,
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
    let input = native
        .get("toolCall")
        .and_then(serde_json::Value::as_object)
        .and_then(|call| call.get("args"))
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let tool = match (tool_name, event) {
        (Some(name), HookEvent::PreToolUse | HookEvent::PostToolUse) => {
            Some(matched_tool("antigravity", name, &input))
        }
        _ => None,
    };
    Ok(HookCommandInput {
        harness: "antigravity".to_owned(),
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

    /// A group's native invocation through the generated wrapper, which is
    /// what every entry below carries.
    fn invocation(hook: &PortableHook) -> HookInvocation {
        HookInvocation::Line(wrapper_command_line(
            Path::new("/state/hooks/exec"),
            hook,
            Path::new("/pkg"),
        ))
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
            tool_name("claude", &HookMatcher::Native("Write".into())),
            "Write"
        );
        assert!(
            matched_tool("claude", "SomeVendorOnlyTool", &serde_json::json!({"x": 1}))
                .fields
                .is_empty()
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
        let entry = group_entry("claude", &hook, &invocation(&hook));
        assert_eq!(entry["matcher"], "Bash|Write");
        assert_eq!(entry["hooks"][0]["type"], "command");
        assert_eq!(
            entry["hooks"][0]["timeout"], 11,
            "sum of handler timeouts plus 1s grace"
        );
        hook.matchers = Vec::new();
        let entry = group_entry("claude", &hook, &invocation(&hook));
        assert!(
            entry.get("matcher").is_none(),
            "no matcher key for a match-all group"
        );
    }

    #[test]
    fn agy_document_is_named_per_group_and_deterministic() {
        let document = agy_hook_document(
            &[&hook()],
            Path::new("/state/hooks/exec"),
            Path::new("/pkg"),
        );
        let value: serde_json::Value = serde_json::from_str(&document).unwrap();
        assert_eq!(
            value["protect-env"]["PreToolUse"][0]["matcher"],
            "run_command|Write"
        );
        assert!(
            value.get("hooks").is_none(),
            "a `hooks` wrapper would register one dead hook named `hooks`"
        );
        assert!(
            document.contains("'/state/hooks/exec' '/pkg' 'pre_tool_use' 'deny'"),
            "the entry runs the vendored wrapper with the group's own arguments: {document}"
        );
        assert!(
            !document.contains("hook-exec"),
            "nothing on the execution path may be the packager"
        );
        let again = agy_hook_document(
            &[&hook()],
            Path::new("/state/hooks/exec"),
            Path::new("/pkg"),
        );
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
        assert_eq!(entries[0]["hooks"][0]["timeout"], 21);
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
                .replace("\"timeout\":11", "\"timeout\":99"),
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
    fn stop_payloads_carry_no_tool_and_a_matched_tool_carries_its_portable_fields() {
        let stop = serde_json::json!({"stop_hook_active": true});
        let input = claude_normalize_input(&stop, HookEvent::Stop).unwrap();
        assert!(input.tool.is_none());

        let call = serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": "cat .env"},
            "cwd": "/repo",
        });
        let input = claude_normalize_input(&call, HookEvent::PreToolUse).unwrap();
        let environment: std::collections::BTreeMap<String, String> =
            input.environment().into_iter().collect();
        assert_eq!(environment["HOOK_HARNESS"], "claude");
        assert_eq!(environment["HOOK_TOOL"], "shell");
        assert_eq!(environment["HOOK_TOOL_NATIVE"], "Bash");
        assert_eq!(environment["HOOK_COMMAND"], "cat .env");
        assert_eq!(environment["HOOK_CWD"], "/repo");
        assert_eq!(environment["HOOK_INPUT"], r#"{"command":"cat .env"}"#);

        let raw = serde_json::json!({
            "tool_name": "SomeVendorOnlyTool",
            "tool_input": {"anything": 1},
        });
        let environment: std::collections::BTreeMap<String, String> =
            claude_normalize_input(&raw, HookEvent::PreToolUse)
                .unwrap()
                .environment()
                .into_iter()
                .collect();
        assert_eq!(environment["HOOK_TOOL"], "");
        assert_eq!(environment["HOOK_TOOL_NATIVE"], "SomeVendorOnlyTool");
        assert!(!environment.contains_key("HOOK_COMMAND"));
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

/// The generated wrapper against real `sh`: the same cases the reference
/// runtime answers, run through the file a harness would actually execute.
#[cfg(all(test, unix))]
mod wrapper_tests {
    use super::*;
    use std::{
        collections::BTreeMap,
        os::unix::fs::PermissionsExt,
        process::{Command, Stdio},
    };
    use uze_core::hook::{CommandHandlerType, HookDecision, HookEvent};

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
        root
    }

    fn write_script(path: &Path, body: &str) {
        fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn group(root: &Path, effect: HookEffect, handlers: &[&str]) -> PortableHook {
        PortableHook {
            id: "protect-env".into(),
            event: HookEvent::PreToolUse,
            matchers: vec![HookMatcher::Portable("shell".into())],
            handlers: handlers
                .iter()
                .map(|name| CommandHook {
                    handler_type: CommandHandlerType::Command,
                    command: format!("${{PLUGIN_ROOT}}/scripts/{name}"),
                    timeout: 10,
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
            let hook = group(&root, HookEffect::Deny, &["guard", "audit"]);
            let answer = run_wrapper(target, &root, &hook, &payload(target, "cat .env"), None);
            assert_eq!(answer.exit, 2, "{target}: a denial uses the block signal");
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
            let hook = group(&root, HookEffect::Deny, &["guard", "audit"]);
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
            let closed = group(&root, HookEffect::Deny, &["absent"]);
            let answer = run_wrapper(target, &root, &closed, &payload(target, "ls"), None);
            assert_eq!(answer.exit, 2, "{target}: a deny group fails closed");
            assert!(answer.stderr.contains("handler failed"));

            let open = group(&root, HookEffect::Observe, &["absent"]);
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
            let closed = group(&root, HookEffect::Deny, &["guard"]);
            let answer = run_wrapper(
                target,
                &root,
                &closed,
                &payload(target, "ls"),
                Some("/nonexistent/jq"),
            );
            assert_eq!(answer.exit, 2, "{target}: a deny group denies without jq");
            assert!(answer.stderr.contains("jq is not installed"));

            let open = group(&root, HookEffect::Observe, &["guard"]);
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
        let hook = group(&root, HookEffect::Observe, &["probe"]);
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

    /// The reference runtime and the generated wrapper are two renderings of
    /// one contract: for the same payload they must answer identically.
    #[test]
    fn the_wrapper_and_the_reference_runtime_answer_alike() {
        let render: BTreeMap<&str, fn(&_, HookEvent) -> _> = [
            (
                "claude",
                claude_render_output as fn(&_, HookEvent) -> std::result::Result<_, String>,
            ),
            ("codex", codex_render_output),
            ("antigravity", antigravity_render_output),
        ]
        .into_iter()
        .collect();
        let normalize: BTreeMap<&str, fn(&_, HookEvent) -> _> = [
            (
                "claude",
                claude_normalize_input as fn(&_, HookEvent) -> std::result::Result<_, String>,
            ),
            ("codex", codex_normalize_input),
            ("antigravity", antigravity_normalize_input),
        ]
        .into_iter()
        .collect();

        for target in TARGETS {
            for (command, effect, handlers) in [
                ("cat .env", HookEffect::Deny, &["guard", "audit"][..]),
                ("ls -la", HookEffect::Deny, &["guard", "audit"][..]),
                ("ls", HookEffect::Deny, &["absent"][..]),
                ("ls", HookEffect::Observe, &["absent"][..]),
            ] {
                let root = package(&format!("equiv-{target}"));
                let hook = group(&root, effect, handlers);
                let raw = payload(target, command);
                let through_wrapper = run_wrapper(target, &root, &hook, &raw, None);

                let native: serde_json::Value = serde_json::from_str(&raw).unwrap();
                let input = normalize[target](&native, HookEvent::PreToolUse).unwrap();
                let outcome = uze_core::hook::dispatch_handlers(&hook, &input, &root).unwrap();
                let reference = render[target](&outcome, HookEvent::PreToolUse).unwrap();

                assert_eq!(
                    through_wrapper.exit, reference.exit_code,
                    "{target}/{command}: the two routes must agree on the exit code"
                );
                assert_eq!(
                    through_wrapper.stdout.trim().is_empty(),
                    reference.stdout.is_none(),
                    "{target}/{command}: the two routes must agree on whether a document is written"
                );
                if let Some(expected) = &reference.stdout {
                    let expected: serde_json::Value = serde_json::from_slice(expected).unwrap();
                    let actual: serde_json::Value =
                        serde_json::from_str(through_wrapper.stdout.trim()).unwrap();
                    let decision = |document: &serde_json::Value| {
                        if target == "antigravity" {
                            document["decision"].clone()
                        } else {
                            document["hookSpecificOutput"]["permissionDecision"].clone()
                        }
                    };
                    assert_eq!(
                        decision(&expected),
                        decision(&actual),
                        "{target}/{command}: the two routes must agree on the decision"
                    );
                }
                assert_eq!(
                    outcome.decision == Some(HookDecision::Deny),
                    through_wrapper.exit == 2,
                    "{target}/{command}: a denial is a denial on both routes"
                );
                let _ = fs::remove_dir_all(root);
            }
        }
    }
}
