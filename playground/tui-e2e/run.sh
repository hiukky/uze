#!/usr/bin/env bash
# Builds uze, drops it into a disposable container and drives the
# workspace TUI through the worktree-recovery flow. Needs Docker.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"

cargo build --manifest-path "$root/Cargo.toml" --bin uze
context="$(mktemp -d)"
trap 'rm -rf "$context"' EXIT
cp "$here/Dockerfile" "$here/drive.py" "$context/"
cp "$root/target/debug/uze" "$context/uze"

docker build -q -t uze-tui-e2e "$context" >/dev/null
docker run --rm -t uze-tui-e2e python3 /drive.py "${1:-all}"
