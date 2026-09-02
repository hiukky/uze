#!/bin/bash
# run_agy_obs.sh — run the REAL authenticated AGY through mitmproxy on the host.
# Observation only: passive forward proxy; sanitized log only; no raw flows.
# Usage: run_agy_obs.sh <logname> -- agy <args...>
set -euo pipefail
V=/tmp/mitm-venv
LOG="${1:?logname}"; shift
[ "$1" = "--" ] && shift
PORT=8082
LOGDIR="${AGY_OBS_DIR:-/tmp/agy-obs}"
mkdir -p "$LOGDIR"
LOG_PATH="$LOGDIR/$LOG.jsonl"
rm -f "$LOG_PATH"
export AGY_LOG_PATH="$LOG_PATH"
# Resolved from this script, not from one machine's checkout path.
ADDON="$(cd "$(dirname "$0")" && pwd)/mitmproxy/sanitizing_addon.py"

"$V/bin/mitmdump" --mode regular@$PORT \
  -s "$ADDON" \
  -q >/dev/null 2>&1 &
MITM=$!
trap 'kill $MITM 2>/dev/null' EXIT
sleep 1.2

export HTTPS_PROXY="http://127.0.0.1:$PORT"
export HTTP_PROXY="http://127.0.0.1:$PORT"
export ALL_PROXY="http://127.0.0.1:$PORT"
export SSL_CERT_FILE="$HOME/.mitmproxy/mitmproxy-ca-cert.pem"
unset NO_PROXY no_proxy

echo "==> running: $*"
set +e
timeout "${AGY_TIMEOUT:-120}" "$@" 2>&1 | tee "$LOGDIR/$LOG.stdout.txt"
RC=$?
set -e
sleep 0.5
echo "==> rc=$RC flows=$(wc -l < "$LOG_PATH" 2>/dev/null || echo 0)"
