//! Vendor-neutral portable Hook manifest and command ABI (ADR-033).

use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use serde::{Deserialize, Serialize};

use crate::{
    error::{Result, UzeError},
    router::CompatibilityRoute,
    subprocess::{read_bounded, wait_with_timeout, with_process_group},
};

pub const HOOKS_FILE_NAME: &str = "hooks.json";
pub const DEFAULT_TIMEOUT_SECONDS: u16 = 30;
pub const MAX_TIMEOUT_SECONDS: u16 = 300;
pub const MAX_HANDLER_STDOUT_BYTES: usize = 64 * 1024;
pub const MAX_HANDLER_STDERR_BYTES: usize = 64 * 1024;

/// Canonical exit code a command handler may use to signal a hard deny when
/// its stdout is not a reliable channel. Distinct from `0` (no decision),
/// from JSON `{"decision":"deny"}` (decision via stdout), and from any other
/// non-zero exit (a run failure whose effect depends on the declared hook
/// effect — see `dispatch_handlers`).
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

    /// Inverse of [`Self::abi_name`], used by the runtime dispatcher to
    /// parse its `--event` argument without naming a harness.
    pub fn parse_abi(name: &str) -> Option<Self> {
        match name {
            "pre_tool_use" => Some(Self::PreToolUse),
            "post_tool_use" => Some(Self::PostToolUse),
            "stop" => Some(Self::Stop),
            _ => None,
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

    /// Inverse of [`Self::abi_name`], used by the runtime dispatcher to
    /// parse its `--effect` argument.
    pub fn parse_abi(name: &str) -> Option<Self> {
        match name {
            "observe" => Some(Self::Observe),
            "allow" => Some(Self::Allow),
            "ask" => Some(Self::Ask),
            "deny" => Some(Self::Deny),
            "transform" => Some(Self::Transform),
            _ => None,
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

pub fn portable_tool_aliases() -> &'static [&'static str] {
    &[
        "shell",
        "file.read",
        "file.write",
        "file.edit",
        "search.files",
        "search.web",
        "agent.spawn",
        "agent.message",
    ]
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HookCommandInput {
    pub version: u8,
    pub event: String,
    pub tool: Option<HookTool>,
    pub input: serde_json::Value,
    pub context: HookContext,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HookTool {
    pub portable: Option<String>,
    pub native: String,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct HookContext {
    pub cwd: Option<String>,
    pub session_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HookCommandOutput {
    #[serde(default)]
    pub decision: Option<HookDecision>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub input: Option<serde_json::Value>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HookDecision {
    Allow,
    Ask,
    Deny,
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

// ============================================================================
// Runtime dispatch (ADR-033 §ABI)
// ============================================================================

/// The normalized result of running one hook group's command handlers in
/// manifest order. `decision` is `None` for a pure observation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HookDispatchOutcome {
    pub decision: Option<HookDecision>,
    pub reason: Option<String>,
    /// A pre-tool input replacement requested by a `transform` handler.
    pub input_override: Option<serde_json::Value>,
    /// Non-null when at least one handler could not run to completion
    /// (launch failure, timeout, oversized output, non-zero exit) — the
    /// failure reason, preserved for diagnostics even when the declared
    /// effect forced a fail-closed decision.
    pub failure: Option<String>,
}

/// How the runtime wrapper answers one harness: the hook may render
/// vendor-native JSON on stdout, a reason on stderr (the channel Claude and
/// Codex feed back to the model on a blocked tool), and — crucially — the
/// harness's own blocking exit code. Internal canonical decisions
/// (e.g. the handler-level deny exit) never leak outward; each adapter
/// translates them into the native contract of its own harness.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HookNativeOutput {
    pub stdout: Option<Vec<u8>>,
    pub stderr: Option<String>,
    pub exit_code: i32,
}

/// Per-handler result, consumed by `dispatch_handlers`' aggregation.
struct HandlerResult {
    decision: Option<HookDecision>,
    reason: Option<String>,
    input_override: Option<serde_json::Value>,
    failure: Option<String>,
}

/// Runs one group's handlers sequentially and aggregates their decisions.
///
/// Contract (ADR-033): every handler receives the same normalized ABI input
/// on stdin and may answer with one bounded JSON object on stdout. Handlers
/// run in manifest order; the first deny stops later handlers. Launch
/// failure, timeout, oversized output, and a non-zero exit other than
/// [`DENY_EXIT_CODE`] are fail-open for observational (`Observe`/`Allow`)
/// hooks and fail-closed (a deny) for a declared `Deny`/`Ask`/`Transform`
/// pre-tool effect — a safety hook is never silently weakened into a
/// no-op observation.
pub fn dispatch_handlers(
    hook: &PortableHook,
    input: &HookCommandInput,
    package_root: &Path,
) -> Result<HookDispatchOutcome> {
    let mut outcome = HookDispatchOutcome::default();
    for handler in &hook.handlers {
        let result = run_handler(handler, input, package_root, hook.effect)?;
        if let Some(failure) = &result.failure {
            outcome.failure = Some(failure.clone());
        }
        match result.decision {
            // The first deny wins and stops later handlers (spec scenario
            // "First deny wins in deterministic order").
            Some(HookDecision::Deny) => {
                outcome.decision = Some(HookDecision::Deny);
                outcome.reason = result.reason;
                return Ok(outcome);
            }
            // A softer decision only fills an empty slot; a later deny still
            // overrides it by returning above.
            Some(decision) if outcome.decision.is_none() => {
                outcome.decision = Some(decision);
                outcome.reason = result.reason;
            }
            _ => {}
        }
        if outcome.input_override.is_none() {
            outcome.input_override = result.input_override;
        }
    }
    Ok(outcome)
}

/// Expands the canonical `${PLUGIN_ROOT}` placeholder to the package root,
/// so an authored command stays portable while the emitted projection can
/// pin the concrete store path. The variable is also injected as the
/// `PLUGIN_ROOT` environment variable for shell-level expansion.
fn expand_plugin_root(command: &str, package_root: &Path) -> String {
    command.replace("${PLUGIN_ROOT}", &package_root.to_string_lossy())
}

fn run_handler(
    handler: &CommandHook,
    input: &HookCommandInput,
    package_root: &Path,
    effect: HookEffect,
) -> Result<HandlerResult> {
    let command = expand_plugin_root(&handler.command, package_root);
    // The system shell is resolved without relying on `PATH`: other
    // components mutate `PATH` under their own guards, and a hook payload
    // must never be undeliverable just because a sibling test or a shim
    // reordered the environment. `/bin/sh` is the POSIX system shell on
    // every supported Unix; `sh` remains the PATH fallback.
    let shell = if cfg!(windows) {
        "cmd"
    } else if Path::new("/bin/sh").exists() {
        "/bin/sh"
    } else {
        "sh"
    };
    let mut invocation = Command::new(shell);
    if cfg!(windows) {
        invocation.arg("/C").arg(&command);
    } else {
        invocation.arg("-c").arg(&command);
    }
    let mut child = with_process_group(invocation)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir(package_root)
        .env("PLUGIN_ROOT", package_root)
        .spawn()
        .map_err(|source| UzeError::Process {
            program: command.clone(),
            source,
        })?;

    let mut stdin = child.stdin.take().expect("hook stdin was piped");
    writeln!(
        stdin,
        "{}",
        serde_json::to_string(input).expect("normalized hook input always serializes")
    )
    .ok();
    drop(stdin);

    let stdout = child.stdout.take().expect("hook stdout was piped");
    let stderr = child.stderr.take().expect("hook stderr was piped");
    // Readers run on threads so a chatty handler cannot block on a full
    // pipe while the wait loop is polling; `read_bounded` caps each stream.
    let stdout_reader = thread::spawn(move || read_bounded(stdout, MAX_HANDLER_STDOUT_BYTES));
    let stderr_reader = thread::spawn(move || read_bounded(stderr, MAX_HANDLER_STDERR_BYTES));

    let (status, timed_out) =
        wait_with_timeout(&mut child, Duration::from_secs(u64::from(handler.timeout))).map_err(
            |source| UzeError::Process {
                program: command.clone(),
                source,
            },
        )?;
    let (stdout_bytes, stdout_overflow) = if timed_out {
        // The handler was killed past its deadline; a descendant may still
        // hold the pipes open, so joining the readers could hang for the
        // descendant's whole lifetime. The timeout verdict already decided
        // the outcome — detach the readers and discard their output.
        drop(stdout_reader);
        drop(stderr_reader);
        (Vec::new(), false)
    } else {
        let bytes = stdout_reader.join().unwrap_or_else(|_| (Vec::new(), true));
        drop(stderr_reader);
        bytes
    };

    let mut decision = None;
    let mut reason = None;
    let mut input_override = None;

    // The canonical deny exit is a decision, not a failure: an author may
    // signal a hard deny through the exit channel when stdout is unreliable.
    let denied_by_exit = status.code() == Some(DENY_EXIT_CODE);
    if denied_by_exit {
        decision = Some(HookDecision::Deny);
        if let Ok(output) = serde_json::from_slice::<HookCommandOutput>(&stdout_bytes) {
            reason = output.reason;
        }
    }

    let failure = if timed_out {
        Some(format!("`{command}` timed out after {}s", handler.timeout))
    } else if stdout_overflow {
        Some(format!(
            "`{command}` stdout exceeded the {MAX_HANDLER_STDOUT_BYTES} byte cap"
        ))
    } else if !denied_by_exit && !stdout_bytes.is_empty() {
        match serde_json::from_slice::<HookCommandOutput>(&stdout_bytes) {
            Ok(output) => {
                decision = output.decision;
                reason = output.reason;
                input_override = output.input;
                None
            }
            Err(_) => Some(format!("`{command}` wrote invalid JSON on stdout")),
        }
    } else if !denied_by_exit {
        match status.code() {
            Some(0) => None,
            // A script without the executable bit: distinguish it from a
            // generic failure so the author gets an actionable diagnostic
            // (the canonical manifest runs commands through the shell, so
            // `chmod +x` or an explicit `sh` prefix fixes it).
            Some(126) => Some(format!(
                "`{command}` is not executable (exit 126); chmod +x the script or invoke it as `sh {command}`"
            )),
            Some(code) => Some(format!("`{command}` exited with code {code}")),
            None => Some(format!("`{command}` was terminated by a signal")),
        }
    } else {
        None
    };

    // Failure semantics depend on the declared effect: observational hooks
    // fail open (their purpose is diagnostic); a declared pre-tool
    // deny/ask/transform effect fails closed so the intercepted operation
    // cannot proceed on an unverifiable verdict (spec: "fail-open for
    // observational hooks but fail-closed for a declared pre-tool
    // deny/ask effect").
    if let Some(failure) = &failure
        && matches!(
            effect,
            HookEffect::Deny | HookEffect::Ask | HookEffect::Transform
        )
    {
        decision = Some(HookDecision::Deny);
        reason = Some(format!(
            "{} could not be evaluated: {failure}",
            match effect {
                HookEffect::Deny => "the deny hook",
                HookEffect::Ask => "the ask hook",
                _ => "the transform hook",
            }
        ));
    }

    Ok(HandlerResult {
        decision,
        reason,
        input_override,
        failure,
    })
}

/// A hook adapter translates one harness's native hook stdin/stdout contract
/// to and from the portable command ABI (ADR-033). The Core defines the
/// contract; each integration owns its vendor mapping table. `hook-exec`
/// resolves adapters through the integration registry by id, so no layer
/// above the integrations ever names a harness.
pub trait HookAdapterPort: Send + Sync {
    /// The adapter's stable identity — matches the owning integration's
    /// `IntegrationPort::id`, so `hook-exec` resolves one adapter per
    /// harness through the registry. Named distinctly from `id()` to avoid
    /// method ambiguity on types implementing both traits.
    fn adapter_id(&self) -> &'static str;

    /// Normalizes the harness's native hook payload (read from the wrapper
    /// command's stdin) into the portable ABI input every authored handler
    /// speaks. Fails fast on a payload shape the adapter does not
    /// understand; the wrapper turns that into a fail-open/closed decision
    /// per the group's declared effect.
    fn normalize_input(
        &self,
        native: &serde_json::Value,
        event: HookEvent,
    ) -> std::result::Result<HookCommandInput, String>;

    /// Renders the aggregated outcome back into the harness's native
    /// contract: stdout JSON, a stderr reason (the channel native hooks
    /// feed back on a blocked tool), and the harness's own blocking exit
    /// code — `0` for allow/observe, the native block code (2 on
    /// Claude/Codex/Antigravity tool use) for a deny. `None` stdout when
    /// the harness treats an empty stdout as allow/observe.
    fn render_output(
        &self,
        outcome: &HookDispatchOutcome,
        event: HookEvent,
    ) -> std::result::Result<HookNativeOutput, String>;
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
    fn command_abi_round_trips_allow_and_input() {
        let output: HookCommandOutput =
            serde_json::from_str(r#"{"decision":"allow","input":{"path":"x"}}"#).unwrap();
        assert_eq!(output.decision, Some(HookDecision::Allow));
        assert_eq!(output.input.unwrap()["path"], "x");
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

/// End-to-end dispatcher behavior against real `sh` scripts: ABI stdin,
/// bounded reads, per-handler timeouts, the canonical deny exit, and the
/// fail-open/fail-closed effect semantics (ADR-033 §ABI).
#[cfg(all(test, unix))]
mod dispatch_tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, time::Instant};

    fn temp(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        std::env::temp_dir().join(format!(
            "uze-hook-dispatch-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    fn script(root: &Path, name: &str, body: &str) -> String {
        fs::create_dir_all(root).unwrap();
        let path = root.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn input() -> HookCommandInput {
        HookCommandInput {
            version: 1,
            event: "pre_tool_use".to_owned(),
            tool: Some(HookTool {
                portable: Some("shell".to_owned()),
                native: "Bash".to_owned(),
            }),
            input: serde_json::json!({"command": "ls"}),
            context: HookContext {
                cwd: Some("/tmp".to_owned()),
                session_id: None,
            },
        }
    }

    fn hook(commands: Vec<&str>, effect: HookEffect) -> PortableHook {
        PortableHook {
            id: "dispatch".into(),
            event: HookEvent::PreToolUse,
            matchers: Vec::new(),
            handlers: commands
                .into_iter()
                .map(|command| CommandHook {
                    handler_type: CommandHandlerType::Command,
                    command: command.to_owned(),
                    timeout: DEFAULT_TIMEOUT_SECONDS,
                })
                .collect(),
            effect,
            order: 0,
        }
    }

    // The dispatch tests must not depend on `PATH`: a sibling test suite
    // mutates the process-global `PATH` (serialized only against itself),
    // so every handler body below uses shell builtins or `/bin/` paths.

    #[test]
    fn empty_stdout_is_an_observation() {
        let root = temp("observe");
        let script = script(&root, "observe.sh", ":");
        let outcome =
            dispatch_handlers(&hook(vec![&script], HookEffect::Observe), &input(), &root).unwrap();
        assert_eq!(outcome.decision, None);
        assert_eq!(outcome.failure, None);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn allow_deny_and_transform_decisions_are_parsed_from_stdout() {
        let root = temp("decisions");
        let allow = script(
            &root,
            "allow.sh",
            "read -r _; printf '{\"decision\":\"allow\",\"reason\":\"ok\"}'",
        );
        let deny = script(
            &root,
            "deny.sh",
            "read -r _; printf '{\"decision\":\"deny\",\"reason\":\"blocked\"}'",
        );
        let transform = script(
            &root,
            "transform.sh",
            "read -r _; printf '{\"decision\":\"allow\",\"input\":{\"path\":\"/safe\"}}'",
        );
        assert_eq!(
            dispatch_handlers(&hook(vec![&allow], HookEffect::Observe), &input(), &root)
                .unwrap()
                .decision,
            Some(HookDecision::Allow)
        );
        let denied =
            dispatch_handlers(&hook(vec![&deny], HookEffect::Observe), &input(), &root).unwrap();
        assert_eq!(denied.decision, Some(HookDecision::Deny));
        assert_eq!(denied.reason.as_deref(), Some("blocked"));
        assert_eq!(
            dispatch_handlers(
                &hook(vec![&transform], HookEffect::Transform),
                &input(),
                &root
            )
            .unwrap()
            .input_override,
            Some(serde_json::json!({"path": "/safe"}))
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn native_payload_round_trips_through_abi_stdin() {
        let root = temp("abi");
        let probe = script(
            &root,
            "probe.sh",
            "/bin/cat > \"${PLUGIN_ROOT}/abi-input.json\"",
        );
        let outcome =
            dispatch_handlers(&hook(vec![&probe], HookEffect::Observe), &input(), &root).unwrap();
        assert_eq!(outcome.decision, None);
        let seen: HookCommandInput =
            serde_json::from_slice(&fs::read(root.join("abi-input.json")).unwrap()).unwrap();
        assert_eq!(
            seen,
            input(),
            "the handler must receive the normalized ABI input verbatim"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn canonical_deny_exit_is_a_decision_not_a_failure() {
        let root = temp("deny-exit");
        let script = script(&root, "deny.sh", "read -r _; exit 3");
        let outcome =
            dispatch_handlers(&hook(vec![&script], HookEffect::Deny), &input(), &root).unwrap();
        assert_eq!(outcome.decision, Some(HookDecision::Deny));
        assert_eq!(outcome.failure, None);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn handlers_run_in_order_and_the_first_deny_stops_later_ones() {
        let root = temp("order");
        let order_file = root.join("order.txt");
        let first = script(
            &root,
            "first.sh",
            &format!("echo first >> \"{}\"", order_file.display()),
        );
        let deny = script(
            &root,
            "deny.sh",
            &format!(
                "echo deny >> \"{}\"; printf '{{\"decision\":\"deny\"}}'",
                order_file.display()
            ),
        );
        let second = script(
            &root,
            "second.sh",
            &format!("echo second >> \"{}\"", order_file.display()),
        );
        let hook = PortableHook {
            handlers: vec![
                CommandHook {
                    handler_type: CommandHandlerType::Command,
                    command: first,
                    timeout: 30,
                },
                CommandHook {
                    handler_type: CommandHandlerType::Command,
                    command: deny,
                    timeout: 30,
                },
                CommandHook {
                    handler_type: CommandHandlerType::Command,
                    command: second,
                    timeout: 30,
                },
            ],
            ..hook(vec![], HookEffect::Observe)
        };
        let outcome = dispatch_handlers(&hook, &input(), &root).unwrap();
        assert_eq!(outcome.decision, Some(HookDecision::Deny));
        let order = fs::read_to_string(&order_file).unwrap();
        assert_eq!(order, "first\ndeny\n", "the third handler must not run");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn observation_fails_open_but_a_declared_deny_effect_fails_closed() {
        let root = temp("fail");
        let script = script(&root, "fail.sh", "read -r _; exit 7");
        let observed =
            dispatch_handlers(&hook(vec![&script], HookEffect::Observe), &input(), &root).unwrap();
        assert_eq!(
            observed.decision, None,
            "observational hook failure stays open"
        );
        assert!(observed.failure.unwrap().contains("code 7"));
        let denied =
            dispatch_handlers(&hook(vec![&script], HookEffect::Deny), &input(), &root).unwrap();
        assert_eq!(denied.decision, Some(HookDecision::Deny));
        assert!(
            denied.reason.unwrap().contains("code 7"),
            "the fail-closed reason must carry the underlying failure"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn malformed_stdout_is_a_failure_not_a_decision() {
        let root = temp("bad-json");
        let script = script(&root, "bad.sh", "read -r _; printf 'not json'");
        let outcome =
            dispatch_handlers(&hook(vec![&script], HookEffect::Deny), &input(), &root).unwrap();
        assert_eq!(outcome.decision, Some(HookDecision::Deny));
        assert!(outcome.reason.unwrap().contains("invalid JSON"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn timeout_terminates_a_hung_handler_and_fails_closed_for_deny() {
        let root = temp("timeout");
        let script = script(&root, "spin.sh", "while :; do :; done");
        let hook = PortableHook {
            handlers: vec![CommandHook {
                handler_type: CommandHandlerType::Command,
                command: script,
                timeout: 1,
            }],
            ..hook(vec![], HookEffect::Deny)
        };
        let started = Instant::now();
        let outcome = dispatch_handlers(&hook, &input(), &root).unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "handler must be killed at its timeout"
        );
        assert_eq!(outcome.decision, Some(HookDecision::Deny));
        assert!(outcome.reason.unwrap().contains("timed out after 1s"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn oversized_stdout_is_capped_and_treated_as_a_failure() {
        let root = temp("oversize");
        let script = script(
            &root,
            "big.sh",
            "read -r _; i=0; while [ $i -lt 1200 ]; do printf 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'; i=$((i + 1)); done",
        );
        let outcome =
            dispatch_handlers(&hook(vec![&script], HookEffect::Observe), &input(), &root).unwrap();
        assert_eq!(outcome.decision, None);
        assert!(outcome.failure.unwrap().contains("byte cap"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn plugin_root_is_injected_as_env_and_as_a_command_placeholder() {
        let root = temp("root");
        fs::create_dir_all(&root).unwrap();
        let probe = root.join("probe.sh");
        fs::write(
            &probe,
            "printf '%s' \"$PLUGIN_ROOT\" > \"$PLUGIN_ROOT/seen.txt\"\n",
        )
        .unwrap();
        fs::set_permissions(&probe, fs::Permissions::from_mode(0o755)).unwrap();
        let outcome = dispatch_handlers(
            &hook(vec!["${PLUGIN_ROOT}/probe.sh"], HookEffect::Observe),
            &input(),
            &root,
        )
        .unwrap();
        assert_eq!(outcome.decision, None);
        assert_eq!(
            fs::read_to_string(root.join("seen.txt")).unwrap(),
            root.to_string_lossy()
        );
        let _ = fs::remove_dir_all(root);
    }
}
