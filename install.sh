#!/bin/sh
# uze — official Linux installer.
#
#   curl -fsSL https://uze.hiukky.com/i | sh
#
# Downloads the prebuilt `uze` binary for this machine from GitHub
# Releases, verifies its SHA-256 checksum, and installs it into the user
# binary directory. Pure POSIX sh; Linux (x86_64 / aarch64) only for now.
#
# Environment overrides:
#   UZE_VERSION   Pin a release (e.g. 0.0.0-alpha.1); default: latest.
#   UZE_BASE_URL  Alternate download root (mirror or local test fixture).
#   UZE_BIN_DIR   Installation directory (default: $XDG_BIN_HOME or
#                 $HOME/.local/bin).
#   NO_COLOR      Any value forces the plain, escape-free transcript.

set -eu

DEFAULT_BASE_URL="https://github.com/hiukky/uze/releases"

# --- presentation -------------------------------------------------------------
# The same rule `src/progress.rs` applies to the CLI itself (`color_enabled`),
# so the installer and the tool it installs never speak in two voices: colour
# and motion only for a real terminal, and a plain transcript everywhere else —
# a pipe, a CI log, `TERM=dumb`. Everything below degrades to one line per
# step, with no escape sequence in it at all.
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ] && [ "${TERM:-dumb}" != "dumb" ]; then
  interactive=1
else
  interactive=0
fi

if [ "$interactive" = 1 ]; then
  esc="$(printf '\033')"
  # The palette is `src/progress.rs`'s, by value: sage for what worked, amber
  # for what needs attention, red only for failure.
  BRIGHT="${esc}[1;38;2;242;240;234m"
  MUTED="${esc}[38;2;107;113;118m"
  # Bold grey: a section heading reads as structure, not as one more
  # muted label — `progress::HEADING`, same reasoning.
  HEADING="${esc}[1;38;2;107;113;118m"
  ACCENT="${esc}[38;2;143;209;158m"
  AMBER="${esc}[38;2;224;181;103m"
  DANGER="${esc}[38;2;224;118;95m"
  RESET="${esc}[0m"
  ERASE="${esc}[2K"
else
  BRIGHT=""; MUTED=""; HEADING=""; ACCENT=""; AMBER=""; DANGER=""; RESET=""; ERASE=""
fi

say() { printf '%s\n' "$*"; }
section() { printf '%s%s%s\n' "$HEADING" "$*" "$RESET"; }

# `uze --help` opens with three centred lines, and centres them in the
# width of its own command table (`print_root_help` in src/main.rs). The
# installer opens with the same block, so the first thing a new user sees
# is the first thing the tool itself will show them.
HELP_WIDTH=60
pad() {
  count=0
  while [ "$count" -lt "$1" ]; do
    printf ' '
    count=$((count + 1))
  done
}
centred() {
  length=${#1}
  if [ "$length" -lt "$HELP_WIDTH" ]; then
    pad $(((HELP_WIDTH - length) / 2))
  fi
  printf '%s' "$1"
}
# A dim continuation line under the step it belongs to — the same "│" gutter
# `uze setup` logs behind. It carries the download URL, which is also what the
# offline fixture test reads the resolved release path out of.
note() { printf '%s│%s %s\n' "$MUTED" "$RESET" "$*"; }
ok() { printf '%s✓%s %s\n' "$ACCENT" "$RESET" "$*"; }
warn() { printf '%s!%s %s\n' "$AMBER" "$RESET" "$*" >&2; }

spinner_pid=""

spinner_stop() {
  [ -n "$spinner_pid" ] || return 0
  # A plain positive pid: this is the installer's own direct child, never a
  # process group. `kill -<pid>` is what once took down a whole login session
  # (see AGENTS.md); nothing here has any reason to reach for it.
  kill "$spinner_pid" 2>/dev/null || true
  wait "$spinner_pid" 2>/dev/null || true
  spinner_pid=""
  printf '\r%s' "$ERASE"
}

spinner_start() {
  [ "$spinnable" = 1 ] || return 0
  (
    while :; do
      for frame in ⠋ ⠙ ⠹ ⠸ ⠼ ⠴ ⠦ ⠧ ⠇ ⠏; do
        printf '\r%s%s%s %s' "$ACCENT" "$frame" "$RESET" "$1"
        sleep 0.12
      done
    done
  ) &
  spinner_pid=$!
}

die() {
  spinner_stop
  printf '%s×%s %s\n' "$DANGER" "$RESET" "$*" >&2
  exit 1
}

# One unit of work, announced while it runs and settled when it ends.
#
#   step "<while it runs>" "<once it worked>" <command>…
#
# The command's own output is captured rather than printed: a curl progress
# meter or a tar warning interleaved with a spinner is unreadable, and on
# success there is nothing in it worth showing. On failure it is replayed,
# indented, under the failed step — which is where the checksum refusal and
# every network error surface.
step() {
  running="$1"
  settled="$2"
  shift 2
  rm -f "$settled_line"
  spinner_start "$running"
  # `$?` is read inside the `else`, where it is still the condition's own
  # status: a POSIX `if` whose condition fails and has no branch to run
  # reports zero, so reading it after `fi` would turn every failed step into
  # a successful exit.
  if "$@" >"$step_log" 2>&1; then
    spinner_stop
    # A step that ends up knowing something the caller could not — the
    # version the installed binary reports — says so by leaving its own
    # line behind.
    if [ -s "$settled_line" ]; then
      settled="$(cat "$settled_line")"
    fi
    ok "$settled"
    return 0
  else
    status=$?
  fi
  spinner_stop
  printf '%s×%s %s\n' "$DANGER" "$RESET" "$running" >&2
  if [ -s "$step_log" ]; then
    sed 's/^/  /' "$step_log" >&2
  fi
  exit "$status"
}

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

# --- workspace ----------------------------------------------------------------
tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/uze-install.XXXXXX")"
step_log="${tmpdir}/step.log"
settled_line="${tmpdir}/settled"
# The spinner is stopped here too: an interrupt mid-download would otherwise
# leave a detached frame loop writing over the shell prompt.
trap 'spinner_stop; rm -rf -- "$tmpdir"' EXIT HUP INT TERM

# Fractional sleep is a GNU/busybox extension, not POSIX. Asking once costs a
# tenth of a second and keeps a minimal `sleep` from turning the spinner into
# a ten-frames-per-second stutter, so the transcript stays quiet instead.
spinnable=0
if [ "$interactive" = 1 ] && sleep 0.1 2>/dev/null; then
  spinnable=1
fi

# --- download -----------------------------------------------------------------
base_url="${UZE_BASE_URL:-$DEFAULT_BASE_URL}"
if [ -n "${UZE_VERSION:-}" ]; then
  path="download/v${UZE_VERSION}"
else
  path="latest/download"
fi

# The version slot carries the target instead: which build this machine
# gets is the one fact the header can state before the download resolves
# what "latest" currently means.
printf '%s%s%s\n' "$BRIGHT" "$(centred UZE)" "$RESET"
printf '%s%s%s\n' "$MUTED" "$(centred "$target")" "$RESET"
printf '%s%s%s\n' "$MUTED" "$(centred 'Agent environment manager')" "$RESET"
say ""
note "${base_url}/${path}/${archive}"

fetch() {
  curl -fsSL "${base_url}/${path}/${archive}" -o "${tmpdir}/${archive}" &&
    curl -fsSL "${base_url}/${path}/SHASUMS256.txt" -o "${tmpdir}/SHASUMS256.txt"
}
step "Downloading ${archive}" "Downloaded ${archive}" fetch

# --- verification -------------------------------------------------------------
verify() {
  expected="$(grep -F "  ${archive}" "${tmpdir}/SHASUMS256.txt" | head -n 1 | cut -d' ' -f1)"
  [ -n "$expected" ] || {
    echo "no checksum entry for ${archive} in SHASUMS256.txt" >&2
    return 1
  }
  actual="$(sha256sum "${tmpdir}/${archive}" | cut -d' ' -f1)"
  [ "$actual" = "$expected" ] || {
    echo "checksum mismatch for ${archive} (expected ${expected}, got ${actual})" >&2
    return 1
  }
}
step "Verifying checksum" "Checksum verified" verify

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

place() {
  mkdir -p "$bin_dir" || {
    echo "cannot create directory: $bin_dir" >&2
    return 1
  }
  # Unpacking and placing are asked separately: chained with `&&` before a
  # shared `||`, an unpack that failed was reported as an install that
  # could not write — the one message the reader would have acted on was
  # the one thing that had not gone wrong.
  tar -xzf "${tmpdir}/${archive}" -C "$tmpdir" || {
    echo "cannot unpack: ${archive}" >&2
    return 1
  }
  install -m 0755 "${tmpdir}/uze" "${bin_dir}/uze" || {
    echo "cannot install into: ${bin_dir}/uze" >&2
    return 1
  }
}
step "Installing into ${bin_dir}" "Installed ${bin_dir}/uze" place

# The last thing every step has in common: the binary that came out of it
# answers. It settles on the version it reports, which is the one line
# worth keeping on screen out of all of this.
confirm() {
  version_output="$("${bin_dir}/uze" --version 2>&1)" || {
    echo "installed binary failed to run: ${bin_dir}/uze" >&2
    return 1
  }
  printf '%s\n' "$version_output" >"$settled_line"
}
step "Verifying the install" "Verified" confirm

case ":$PATH:" in
  *":$bin_dir:"*) ;;
  *)
    warn "$bin_dir is not on your PATH — add it with:"
    note "export PATH=\"${bin_dir}:\$PATH\""
    ;;
esac

# Two commands, laid out the way `uze --help` lays out its own — an
# installer that ends on a bare success line leaves the reader to guess
# what they just installed is for.
say ""
section "Next"
# Padded outside the colour, so the accent covers the command and nothing
# else — the gutter belongs to the row, not to the word before it.
next() {
  printf '  %s%s%s' "$ACCENT" "$1" "$RESET"
  pad $((13 - ${#1}))
  printf '%s\n' "$2"
}
next "uze setup" "Detect and provision your harnesses"
next "uze" "Open the terminal workspace"
