#!/bin/sh
# uze — official Linux installer.
#
#   curl -fsSL https://hiukky.com/uze/install.sh | sh
#
# Downloads the prebuilt `uze` binary for this machine from GitHub
# Releases, verifies its SHA-256 checksum, and installs it into the user
# binary directory. Pure POSIX sh; Linux (x86_64 / aarch64) only for now.
#
# Environment overrides:
#   UZE_VERSION   Pin a release (e.g. 0.0.0-alpha.14); default: latest.
#   UZE_BASE_URL  Alternate download root (mirror or local test fixture).
#   UZE_BIN_DIR   Installation directory (default: $XDG_BIN_HOME or
#                 $HOME/.local/bin).

set -eu

DEFAULT_BASE_URL="https://github.com/hiukky/uze/releases"

say() { printf '%s\n' "$*"; }
die() { say "$*" >&2; exit 1; }

need() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

# --- prerequisites -----------------------------------------------------------
[ "$(uname -s)" = "Linux" ] ||
  die "unsupported OS: $(uname -s) (uze currently installs on Linux only)"
need curl
need tar
need sha256sum
need mktemp
need install

# --- platform -----------------------------------------------------------------
arch="$(uname -m)"
case "$arch" in
  x86_64 | amd64) target_arch="x86_64" ;;
  aarch64 | arm64) target_arch="aarch64" ;;
  *) die "unsupported architecture: $arch (supported: x86_64, aarch64)" ;;
esac

libc="gnu"
if command -v ldd >/dev/null 2>&1 && ldd --version 2>&1 | grep -qi musl; then
  libc="musl"
fi

target="${target_arch}-unknown-linux-${libc}"
archive="uze-${target}.tar.gz"
say "Installing uze for ${target}"

# --- download -----------------------------------------------------------------
base_url="${UZE_BASE_URL:-$DEFAULT_BASE_URL}"
if [ -n "${UZE_VERSION:-}" ]; then
  path="download/v${UZE_VERSION}"
else
  path="latest/download"
fi

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/uze-install.XXXXXX")"
trap 'rm -rf -- "$tmpdir"' EXIT HUP INT TERM

say "Downloading ${base_url}/${path}/${archive}"
curl -fsSL "${base_url}/${path}/${archive}" -o "${tmpdir}/${archive}"
curl -fsSL "${base_url}/${path}/SHASUMS256.txt" -o "${tmpdir}/SHASUMS256.txt"

# --- verification -------------------------------------------------------------
expected="$(grep -F "  ${archive}" "${tmpdir}/SHASUMS256.txt" | head -n 1 | cut -d' ' -f1)"
[ -n "$expected" ] || die "no checksum entry for ${archive} in SHASUMS256.txt"
actual="$(sha256sum "${tmpdir}/${archive}" | cut -d' ' -f1)"
[ "$actual" = "$expected" ] || die "checksum mismatch for ${archive} (expected ${expected}, got ${actual})"

# --- install ------------------------------------------------------------------
bin_dir="${UZE_BIN_DIR:-}"
if [ -z "$bin_dir" ]; then
  if [ -n "${XDG_BIN_HOME:-}" ]; then
    bin_dir="$XDG_BIN_HOME"
  else
    [ -n "${HOME:-}" ] || die "cannot determine an installation directory (set UZE_BIN_DIR)"
    bin_dir="${HOME}/.local/bin"
  fi
fi

mkdir -p "$bin_dir" || die "cannot create directory: $bin_dir"
tar -xzf "${tmpdir}/${archive}" -C "$tmpdir"
install -m 0755 "${tmpdir}/uze" "${bin_dir}/uze" || die "cannot install into: ${bin_dir}/uze"

version_output="$("${bin_dir}/uze" --version 2>&1)" ||
  die "installed binary failed to run: ${bin_dir}/uze"
say "Installed uze to ${bin_dir}/uze"
say "$version_output"
case ":$PATH:" in
  *":$bin_dir:"*) ;;
  *) say "note: $bin_dir is not on your PATH — add it with:"
     say "  export PATH=\"${bin_dir}:\$PATH\"" ;;
esac
say ""
say "Next: run 'uze setup' to detect and provision your harnesses, or 'uze' for the terminal UI."