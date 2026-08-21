#!/usr/bin/env bash
# Build UZE in the current WSL distro and deploy only its release binary to a
# second, named WSL distro. The staging directory lives on the Windows mount,
# which is intentionally visible to both distributions.

set -euo pipefail

target_distro="${1:-Lab}"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
wsl_exe="${WSL_EXE:-/mnt/c/Windows/System32/wsl.exe}"

if [[ ! -x "$wsl_exe" ]]; then
  echo "error: WSL host executable not found at $wsl_exe" >&2
  echo "run this script from WSL, or set WSL_EXE to wsl.exe's path" >&2
  exit 1
fi
if ! command -v wslpath >/dev/null 2>&1 || [[ ! -d /mnt/c ]]; then
  echo "error: a mounted Windows filesystem and wslpath are required" >&2
  exit 1
fi

# Verify the target before compiling, so a typo never creates a staging copy.
if ! "$wsl_exe" -d "$target_distro" -- true >/dev/null 2>&1; then
  echo "error: WSL distro '$target_distro' is unavailable" >&2
  echo "available distros:" >&2
  "$wsl_exe" -l -q >&2 || true
  exit 1
fi

windows_temp="$(cd /mnt/c && cmd.exe /C 'echo %TEMP%' | tr -d '\r')"
shared_temp="$(wslpath -u "$windows_temp")"
stage_dir="$(mktemp -d "${shared_temp%/}/uze-wsl-deploy.XXXXXX")"
trap 'rm -rf -- "$stage_dir"' EXIT

cd "$repo_root"
echo "Building UZE release from $repo_root…"
cargo build --locked --release --bin uze

install -m 0755 target/release/uze "$stage_dir/uze"
# Both distributions mount the Windows staging directory at the same
# `/mnt/c/...` path. Keeping this Unix path avoids both the `\\wsl.localhost`
# conversion and the backslash re-parsing that `wsl.exe` applies to arguments.
quoted_artifact="'${stage_dir}/uze'"

echo "Installing UZE into WSL distro '$target_distro'…"
# `wsl.exe` forwards its current working directory. Invoke it from the
# Windows mount so it forwards `C:\…`, never this distro's UNC path.
target_command=$(cat <<'EOF'
set -euo pipefail
source_artifact=__UZE_STAGED_ARTIFACT__
destination="\$HOME/.local/bin/uze"
if [[ ! -f "\$source_artifact" ]]; then
  echo "error: staged artifact is unavailable in target distro: \$source_artifact" >&2
  exit 1
fi
mkdir -p "\$(dirname "\$destination")"
install -m 0755 "\$source_artifact" "\$destination"
"\$destination" --version
EOF
)
target_command="${target_command/__UZE_STAGED_ARTIFACT__/$quoted_artifact}"
pushd /mnt/c >/dev/null
"$wsl_exe" -d "$target_distro" -- bash -lc "$target_command"
popd >/dev/null

echo "Installed into $target_distro:~/.local/bin/uze"
