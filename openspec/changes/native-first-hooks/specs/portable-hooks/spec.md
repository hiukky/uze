## ADDED Requirements

### Requirement: Delivered hooks run without the packager
The system SHALL deliver a package's portable hooks as a self-contained artifact in each harness's own hook form: the harness invokes a wrapper vendored inside the delivered artifact, never the `uze` binary, and the artifact SHALL contain no reference to the packager. A delivered hook SHALL keep working after the `uze` binary is removed from the machine, as long as the delivered artifact and the harness remain.

#### Scenario: Hook keeps working after UZE is uninstalled
- **WHEN** a package with a `deny` pre-tool hook was installed for Claude Code and the `uze` binary is subsequently removed from `PATH`
- **THEN** the harness still invokes the delivered wrapper for a matching tool call
- **AND** the handler's denial is relayed to the harness in its own contract and the tool does not run

#### Scenario: Native entry names only the delivered artifact
- **WHEN** the native hook configuration for a package is inspected on Claude Code, Codex or Antigravity
- **THEN** each entry's command resolves to a file inside the delivered artifact (`hooks/exec`) with the group's effect and the handler paths as arguments
- **AND** no entry contains the path of the `uze` executable

#### Scenario: OpenCode receives the runtime as a plugin
- **WHEN** a package with hooks is installed for OpenCode
- **THEN** the delivered artifact is a single generated plugin whose runtime is the same contract and whose package groups are data inside it
- **AND** the plugin runs the author's handlers with the same context and decision contract as the wrapper on the other harnesses

### Requirement: Handlers receive the hook context as environment and answer with exit codes
Command handlers SHALL receive the hook context as environment variables and SHALL NOT be required to parse any harness payload or emit any harness JSON. The wrapper SHALL set `HOOK_HARNESS`, `HOOK_EVENT`, `HOOK_TOOL` (portable alias, empty when the tool has none), `HOOK_TOOL_NATIVE`, `HOOK_CWD`, `HOOK_INPUT` (the tool input as JSON) and `PLUGIN_ROOT`, plus the portable fields of the matched alias. A handler's exit code SHALL be the decision: `0` allows, `3` denies with the reason read from stderr; any other exit code, a failure to start, or a timeout is a handler failure that follows the group's effect — fail-closed (a denial) for `deny` and `ask`, fail-open (the tool proceeds, the failure is reported) for `observe` and `allow`. Handlers of one group SHALL run sequentially in manifest order and the first denial SHALL stop the remaining handlers, regardless of how the harness schedules its own hooks.

#### Scenario: A shell handler reads the portable command
- **WHEN** a `shell`-matched pre-tool hook fires on any harness for a command line
- **THEN** the handler observes `HOOK_TOOL=shell`, `HOOK_COMMAND` equal to that command line, `HOOK_TOOL_NATIVE` equal to the harness's own tool name and `HOOK_CWD` equal to the workspace directory
- **AND** the same handler, unchanged, observes the same variables on every other harness that delivers the hook

#### Scenario: Denial by exit code is relayed natively
- **WHEN** a handler of a `deny` group exits with code 3 and writes a reason to stderr
- **THEN** the harness receives its own blocking decision carrying that reason and the tool does not run
- **AND** the remaining handlers of the group are not started

#### Scenario: A crashing deny handler still denies
- **WHEN** a handler of a `deny` group cannot start, exits with a code other than 0 or 3, or exceeds its timeout
- **THEN** the intercepted tool is denied and the reason names the failure
- **AND** a handler of an `observe` group failing the same way is reported and the tool proceeds

#### Scenario: Missing wrapper dependency follows the group's effect
- **WHEN** the wrapper's own dependency is not available on the machine (for the `sh` wrapper, `jq`)
- **THEN** a `deny` or `ask` group denies the intercepted tool with a reason naming the missing dependency
- **AND** an `observe` or `allow` group lets the tool proceed and reports the missing dependency

### Requirement: The portable tool vocabulary defines fields
The system SHALL maintain one vocabulary of portable tool aliases in which each alias names the portable fields it guarantees to handlers and, per harness, the native tool name and the native field each portable field is read from. Matchers, generated wrappers and compatibility assessment SHALL derive from this vocabulary alone. A tool matched through `native:<name>` SHALL yield only `HOOK_TOOL_NATIVE` and `HOOK_INPUT`, with `HOOK_TOOL` empty.

#### Scenario: Alias fields are the same on every harness
- **WHEN** the vocabulary defines `shell` with the portable field `command` and `file.write` with `path`
- **THEN** a handler matched on `shell` receives `HOOK_COMMAND` on every harness and a handler matched on `file.write` receives `HOOK_PATH` on every harness, each read from that harness's native field name

#### Scenario: Native escape hatch exposes raw input only
- **WHEN** a group matches `native:Write` on Claude Code
- **THEN** the handler receives `HOOK_TOOL_NATIVE=Write`, `HOOK_INPUT` with the raw tool input and an empty `HOOK_TOOL`
- **AND** no portable field variable is set

#### Scenario: Conformance proves each vocabulary row
- **WHEN** the conformance Lab runs a harness vertical
- **THEN** for every alias the harness delivers, a real tool call of that alias reaches a handler that asserts the guaranteed `HOOK_*` values
- **AND** an alias the harness cannot deliver is declared with a reason, never omitted

### Requirement: The packager runtime remains the reference and the fallback
The system SHALL keep the in-binary hook runtime (`uze hook-exec`) as the executable reference of the contract and as the delivery route where no wrapper template applies to the target platform or effect. Compatibility assessment SHALL report which route delivered each hook, and a hook delivered through the fallback SHALL carry the same context and decision contract.

#### Scenario: Platform without a wrapper template falls back
- **WHEN** a hook is delivered on a platform for which no wrapper template exists
- **THEN** the native entry invokes the packager runtime with the same context and decision contract
- **AND** the delivery is reported as the fallback route with the reason
