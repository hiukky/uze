#!/bin/sh
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
HOOK_HARNESS=codex
export PLUGIN_ROOT HOOK_EVENT HOOK_HARNESS

# --- this harness's decision dialect ------------------------------------
deny_native() {                                  # $1 reason, plain text
  printf '%s\n' "$1" >&2
  reason_json=$(json_string "$1")
  printf '{"hookSpecificOutput":{"permissionDecision":"deny","permissionDecisionReason":%s}}' "$reason_json"
  exit 2                                          # the harness's block signal
}

allow_native() {
  [ "$HOOK_EVENT" = stop ] && printf '{}'
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
HOOK_CWD=$(printf '%s' "$payload" | "$JQ" -r '.cwd // empty')
HOOK_INPUT=$(printf '%s' "$payload" | "$JQ" -c '.tool_input // {}')
HOOK_TOOL= HOOK_COMMAND= HOOK_PATH= HOOK_QUERY=
case "$HOOK_TOOL_NATIVE" in                       # the portable vocabulary
    exec_command) HOOK_TOOL=shell; HOOK_COMMAND=$(printf '%s' "$HOOK_INPUT" | "$JQ" -r '.cmd // empty'); ;;
    Bash) HOOK_TOOL=shell; HOOK_COMMAND=$(printf '%s' "$HOOK_INPUT" | "$JQ" -r '.cmd // empty'); ;;
    shell) HOOK_TOOL=shell; HOOK_COMMAND=$(printf '%s' "$HOOK_INPUT" | "$JQ" -r '.cmd // empty'); ;;
    Read) HOOK_TOOL=file.read; HOOK_PATH=$(printf '%s' "$HOOK_INPUT" | "$JQ" -r '.file_path // empty'); ;;
    Write) HOOK_TOOL=file.write; HOOK_PATH=$(printf '%s' "$HOOK_INPUT" | "$JQ" -r '.file_path // empty'); ;;
    Edit) HOOK_TOOL=file.edit; HOOK_PATH=$(printf '%s' "$HOOK_INPUT" | "$JQ" -r '.file_path // empty'); ;;
    Grep) HOOK_TOOL=search.files; HOOK_QUERY=$(printf '%s' "$HOOK_INPUT" | "$JQ" -r '.pattern // empty'); ;;
    WebSearch) HOOK_TOOL=search.web; HOOK_QUERY=$(printf '%s' "$HOOK_INPUT" | "$JQ" -r '.query // empty'); ;;
esac
export HOOK_TOOL HOOK_TOOL_NATIVE HOOK_CWD HOOK_INPUT HOOK_COMMAND HOOK_PATH HOOK_QUERY

# --- the handlers, in order; the first denial stops the rest --------------
for handler in "$@"; do
  reason=$("$handler" 2>&1 >/dev/null); status=$?
  case $status in
    0) ;;
    3) deny_native "${reason:-$handler denied the operation}" ;;
    *) fail "handler failed (exit $status): $handler${reason:+ — $reason}" ;;
  esac
done
allow_native
exit 0
