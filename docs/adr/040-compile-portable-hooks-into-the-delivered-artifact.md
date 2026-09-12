# Compile portable hooks into the delivered artifact

Status: Accepted

Amended 2026-09-12: the in-binary runtime (`uze hook-exec`) is removed. The
generated wrapper is the sole route, a platform without a template is
Unsupported, and the per-handler timeout is the wrapper's to enforce. The
paragraphs below say so; nothing else about the decision changed.

Supersedes the ABI and dispatcher parts of
[033 — Adopt a canonical portable Hook capability](033-adopt-portable-hook-capability.md).
Everything else ADR-033 decided — `hooks.json` as the canonical manifest,
its events, matchers, effects and handler shape, the capability profiles and
the route vocabulary — stands unchanged.

## Context

ADR-033 put the vendor translation in the `uze` binary: every native hook
entry was a `uze hook-exec …` command line that read the harness's payload,
handed the handlers a normalized JSON object on stdin, read one JSON
decision back, and rendered the harness's own answer. It works, and its
conformance evidence is real. Its defect is where it lives.

A delivered hook only runs while `uze` is installed at the path baked into
the harness's configuration. Uninstalling or moving UZE turns a `deny` guard
into a blocked tool (fail-closed on a wrapper that no longer exists) and an
observational hook into a silent failure. The translation also happens at
every call, in a process the plugin does not own, so what a reviewer reads
in the harness's configuration says nothing about what will run.

The investigation of 2026-09-02 established the three command-hook contracts
precisely (stdin JSON in; a decision document plus an exit code out; only
exit 2 blocks on Claude and Codex, and every other non-zero exit runs the
tool; Claude runs a group's hooks in parallel) and established that OpenCode
has no command hooks at all. A local prototype then showed the alternative
answering all of them with one small generated shell script and no `uze` on
the execution path.

## Decision

The translation is compiled at install time into the delivered artifact.

Each command-hook harness receives one generated POSIX `sh` wrapper,
`hooks/exec`, invoked as
`exec <plugin-root> <event> <effect> <seconds>:<handler>…`.
It reads the harness's payload from stdin, exposes the hook context as
environment, runs the handlers, and answers in that harness's dialect. Its
arguments carry everything the group needs beyond the matcher the harness
already applied, so the native entry is readable and reproducible on its
own; the file itself is a constant per harness, byte-identical for every
package. OpenCode, which has no command hooks, receives a generated plugin
that is the same runtime with the package's groups as data.

`<plugin-root>` names the package's own root, not a delivered directory, so
the wrapper's location is free: it lives wherever the harness's entries can
name it. Claude, Codex and Antigravity therefore each keep one copy under
`$UZE_HOME/state/attachments/<harness>/hooks/exec`, referenced by absolute
path. Antigravity's entries used to be written into its generated plugin
with the wrapper vendored beside them; they are not, since 1.1.24 was
measured to read no `hooks.json` from a plugin directory (Conformance Lab,
`hooks > delivery`) — its hooks are merged into the shared
`~/.gemini/config/hooks.json` instead. A file the vendor never opens is not
a delivery; where the entry goes is a per-harness fact, and the compiled
artifact is unchanged by it.

The handler contract becomes environment in, exit code out. A handler
receives `HOOK_HARNESS`, `HOOK_EVENT`, `HOOK_TOOL`, `HOOK_TOOL_NATIVE`,
`HOOK_CWD`, `HOOK_INPUT`, `PLUGIN_ROOT` and the portable fields of the
matched alias (`HOOK_COMMAND`, `HOOK_PATH`, `HOOK_QUERY`). It answers with
`0` to allow or `3` to deny with the reason on stderr; any other exit, a
failure to start, or a timeout is a handler failure that follows the group's
effect — fail-closed for `deny` and `ask`, fail-open for `observe` and
`allow`. Environment variables are named parameters in every language an
author might reach for, and an exit code needs no parser; stdin JSON forced
a JSON parser into every handler, which is the burden the capability exists
to remove.

Ordering, first-deny-wins, the per-handler deadline and fail-closed are
compiled into the wrapper, because no harness provides them. The
wrapper's own dependency (`jq`) follows the same rule: a `deny` group whose
wrapper cannot parse the payload denies.

The portable tool vocabulary gains fields. `uze-core` owns the alias set and
the portable fields each alias guarantees; each integration owns that
harness's native tool names and the native input field every portable field
is read from. Matchers, wrappers and the OpenCode plugin's alias table are
all generated from it. `native:<name>` bypasses the table: the handler gets
`HOOK_TOOL_NATIVE` and `HOOK_INPUT` and nothing else.

There is one implementation of the contract, and it is the wrapper. The
in-binary runtime is gone: no `uze` appears on any hook's execution path,
and a platform the POSIX template does not cover delivers **no hook at
all** — reported Unsupported with that reason, never carried by something
else. A delivery that cannot write a wrapper has nothing honest to attach,
and an entry running a second implementation is a hook the author did not
write.

What the runtime was also the reference for is now data: the answer the
wrapper gives for every fixture — the native decision document, the exit
status and the reason — is recorded per harness under
`crates/uze-integrations/tests/goldens/hooks/`, taken from the runtime
before it was deleted. A golden that changes is a changed contract, and the
diff is where that is reviewed.

The per-handler `timeout` an author declares is the wrapper's to enforce,
since nothing else can: each handler is run under its own deadline, and past
it the handler and every process it started are stopped (`TERM`, then `KILL`)
and the group's effect decides, exactly as for any other handler failure. The
deadline rides beside its command in the native entry (`<seconds>:<command>`),
so what will run, and for how long, is readable there. The native entry's own
group timeout stays what it always was — the *harness's* backstop, sized so
it is never the bound that fires first.

Nothing in a delivered artifact names the packager. `HOOK_*`, `hooks/exec`,
`hooks-<package>.ts`, the comments — the convention must be usable by
another packager, or by hand.

`transform` cannot be expressed: rewriting the tool input needs a channel
for the handler to answer on, which an exit code is not. It is deferred to
its own change rather than bolted onto this one, and until then a
`transform` group degrades on every harness rather than attaching as a
silent observation.

Vendoring a copy of the `uze` binary inside each plugin was rejected:
self-contained, but megabytes per plugin and just as opaque. Generating one
runner per group was rejected: it replicates the semantics N times per
plugin and bloats the native configuration.

## Consequences

A delivered hook keeps working after `uze` is removed, and everything
harness-specific is decided at generation time and readable in the artifact.
The semantics that no harness provides are guaranteed once per harness
rather than argued about per group.

The cost is a runtime dependency (`jq`) the author's handler does not have,
guarded by effect and reported by `uze doctor`; a Windows machine has no
template yet and therefore no hooks; and the semantics now live in templates
rather than one Rust function. The last is contained by generating them from
one vocabulary, by the recorded answers above, and by the conformance Lab
proving each vocabulary row against the real harness. Enforcing a deadline
without job control costs a `ps` read on the timeout path and a file under
`$TMPDIR` for the handler's stderr — a pipe is inherited by whatever the
handler started, and the answer would then wait for *that*; a machine with
nowhere to write one loses the reason, not the hook.

The handler contract is a breaking change for anything written against
ADR-033's stdin JSON. The project is pre-1.0 and ships no compatibility
layer: fixtures, examples and documentation are rewritten, and there is
exactly one route, so there is exactly one contract.

Source change: openspec/changes/native-first-hooks/
