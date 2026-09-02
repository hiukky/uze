# Compile portable hooks into the delivered artifact

Status: Accepted

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
`hooks/exec`, invoked as `exec <plugin-root> <event> <effect> <handler>…`.
It reads the harness's payload from stdin, exposes the hook context as
environment, runs the handlers, and answers in that harness's dialect. Its
arguments carry everything the group needs beyond the matcher the harness
already applied, so the native entry is readable and reproducible on its
own; the file itself is a constant per harness, byte-identical for every
package. OpenCode, which has no command hooks, receives a generated plugin
that is the same runtime with the package's groups as data.

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

Ordering, first-deny-wins, per-handler timeout and fail-closed are compiled
into the wrapper as constants, because no harness provides them. The
wrapper's own dependency (`jq`) follows the same rule: a `deny` group whose
wrapper cannot parse the payload denies.

The portable tool vocabulary gains fields. `uze-core` owns the alias set and
the portable fields each alias guarantees; each integration owns that
harness's native tool names and the native input field every portable field
is read from. Matchers, wrappers and the OpenCode plugin's alias table are
all generated from it. `native:<name>` bypasses the table: the handler gets
`HOOK_TOOL_NATIVE` and `HOOK_INPUT` and nothing else.

`uze hook-exec` stays, as the executable reference the wrapper templates are
tested against with shared fixtures, and as the delivery route where no
template applies (a platform the POSIX wrapper does not cover). A hook
delivered that way is reported as adapted with the reason, never as native.

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
template yet and takes the fallback route; and the semantics now live in
templates rather than one Rust function. The last is contained by generating
them from one vocabulary, by an equivalence test that runs every fixture
payload through both routes, and by the conformance Lab proving each
vocabulary row against the real harness.

The handler contract is a breaking change for anything written against
ADR-033's stdin JSON. The project is pre-1.0 and ships no compatibility
layer: fixtures, examples and documentation are rewritten, and the fallback
route speaks the new contract too, so there is exactly one contract.

Source change: openspec/changes/native-first-hooks/
