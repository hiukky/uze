#!/bin/sh
# hooks/exec — generated from hooks.json, one per harness. The harness runs
# this; it runs the author's handlers. The handlers never see a harness
# payload and never write harness JSON: the context arrives as HOOK_*
# environment and the decision leaves as an exit code: 0 allows, while
# 3 denies and the reason is read from stderr. Anything else
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
HOOK_HARNESS=claude
export PLUGIN_ROOT HOOK_EVENT HOOK_HARNESS

# --- this harness's decision dialect ------------------------------------
deny_native() {                                  # $1 reason, plain text
  printf '%s\n' "$1" >&2
  reason_json=$(json_string "$1")
  case $HOOK_EVENT in
    pre_tool_use) name=PreToolUse ;;
    post_tool_use) name=PostToolUse ;;
    *) name=Stop ;;
  esac
  printf '{"hookSpecificOutput":{"hookEventName":"%s","permissionDecision":"deny","permissionDecisionReason":%s}}' "$name" "$reason_json"
  exit 2                                # this harness's block signal
}

allow_native() {
  :
}

# fail-closed effects: a guard that cannot be evaluated denies
closed() { [ "$effect" = deny ] || [ "$effect" = ask ]; }
fail() { closed && deny_native "$1"; printf '%s\n' "$1" >&2; allow_native; exit 0; }

# jq escapes the reason once it is available; before that (its own absence
# is the only reason reported then) a literal with neither quote nor
# newline needs no escaping.
json_string() {
  if [ -n "${JQ_READY:-}" ]; then
    printf '%s' "$1" | "$JQ" -Rsa .
  else
    printf '"%s"' "$1"
  fi
}

# --- the harness's payload becomes the hook context ----------------------
JQ=${HOOK_JQ:-jq}
command -v "$JQ" >/dev/null 2>&1 || fail "hooks/exec: jq is not installed"
JQ_READY=1
payload=$(cat)
HOOK_TOOL_NATIVE=$(printf '%s' "$payload" | "$JQ" -r '.tool_name // empty')
HOOK_CWD=$(printf '%s' "$payload" | "$JQ" -r '.cwd // .context.cwd // empty')
HOOK_INPUT=$(printf '%s' "$payload" | "$JQ" -c '.tool_input // {}')
HOOK_TOOL= HOOK_COMMAND= HOOK_PATH= HOOK_QUERY=
case "$HOOK_TOOL_NATIVE" in                       # the portable vocabulary
    Bash) HOOK_TOOL=shell; HOOK_COMMAND=$(printf '%s' "$HOOK_INPUT" | "$JQ" -r '.command // empty'); ;;
    Read) HOOK_TOOL=file.read; HOOK_PATH=$(printf '%s' "$HOOK_INPUT" | "$JQ" -r '.file_path // empty'); ;;
    Write) HOOK_TOOL=file.write; HOOK_PATH=$(printf '%s' "$HOOK_INPUT" | "$JQ" -r '.file_path // empty'); ;;
    MultiEdit) HOOK_TOOL=file.edit; HOOK_PATH=$(printf '%s' "$HOOK_INPUT" | "$JQ" -r '.file_path // empty'); ;;
    Edit) HOOK_TOOL=file.edit; HOOK_PATH=$(printf '%s' "$HOOK_INPUT" | "$JQ" -r '.file_path // empty'); ;;
    Grep) HOOK_TOOL=search.files; HOOK_QUERY=$(printf '%s' "$HOOK_INPUT" | "$JQ" -r '.pattern // empty'); ;;
    WebSearch) HOOK_TOOL=search.web; HOOK_QUERY=$(printf '%s' "$HOOK_INPUT" | "$JQ" -r '.query // empty'); ;;
    Task) HOOK_TOOL=agent.spawn; ;;
esac
export HOOK_TOOL HOOK_TOOL_NATIVE HOOK_CWD HOOK_INPUT HOOK_COMMAND HOOK_PATH HOOK_QUERY

# --- one handler, under its own deadline ---------------------------------
# There is no portable `timeout(1)` (macOS ships none) and no job control in
# a script, so there is no process group to signal: the deadline is a
# sleeper this shell can cancel, and what it stops is the handler plus every
# process the handler started. That second part is not thoroughness — a
# child still holding the pipe keeps this shell waiting long past the
# deadline it just enforced.
family() {                                       # $1 pid -> $1 and its issue
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
}

# Where a handler's reason is collected: a file, never a pipe. Anything the
# handler starts inherits a pipe, and one that outlives its deadline would
# hold this shell open long past the deadline it just enforced. `set -C`
# refuses a path that already exists, so a planted file or symlink is never
# written through; with nowhere to write at all, the reason is dropped
# rather than the hook.
reasons=${TMPDIR:-/tmp}/hooks-exec.$$
(set -C; : > "$reasons") 2>/dev/null || reasons=/dev/null
discard_reasons() { [ "$reasons" = /dev/null ] || rm -f "$reasons"; }
trap discard_reasons EXIT INT TERM

# $1 seconds, $2 command. Leaves what the handler wrote on stderr in
# $reasons and answers with its exit status — or 124, the conventional
# timeout status, when the deadline stopped it. A handler that exits 124 of
# its own accord therefore reads as a timeout; `timeout(1)` carries the
# same ambiguity.
guarded() {
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
}

# --- the handlers, in order; the first denial stops the rest --------------
# A handler is a shell command line, run from the package root: the same
# contract the canonical manifest documents, so `sh scripts/check --strict`
# means here exactly what it means when a person types it.
cd "$PLUGIN_ROOT" 2>/dev/null || :
for entry in "$@"; do
  seconds=${entry%%:*}
  handler=${entry#*:}
  case $seconds in
    ''|*[!0-9]*) fail "malformed handler argument: $entry" ;;
  esac
  guarded "$seconds" "$handler"; status=$?
  [ "$status" = 0 ] && continue                   # allowed; on to the next
  reason=$(cat "$reasons" 2>/dev/null)
  case $status in
    3) deny_native "${reason:-$handler denied the operation}" ;;
    124) fail "handler timed out after ${seconds}s: $handler" ;;
    *) fail "handler failed (exit $status): $handler${reason:+ — $reason}" ;;
  esac
done
allow_native
exit 0
