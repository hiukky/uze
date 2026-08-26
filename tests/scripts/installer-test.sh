#!/bin/sh
# Offline fixture test for install.sh.
#
# Serves synthetic release artifacts (fake `uze` binaries, real SHA-256
# sums, corrupt checksums) over localhost HTTP and exercises the installer:
# glibc and musl detection, pinned versions, checksum-mismatch refusal,
# and unsupported platform fail-closed paths. Zero network access required.
#
# Usage: sh tests/scripts/installer-test.sh

set -eu

repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
installer="$repo_root/install.sh"

need() {
  command -v "$1" >/dev/null 2>&1 || { printf 'missing: %s\n' "$1" >&2; exit 1; }
}
need python3
need curl
need tar
need sha256sum

work="$(mktemp -d "${TMPDIR:-/tmp}/uze-installer-test.XXXXXX")"
server_pid=""
cleanup() {
  if [ -n "$server_pid" ]; then
    kill "$server_pid" 2>/dev/null || true
  fi
  rm -rf -- "$work"
}
trap cleanup EXIT HUP INT TERM

# --- fixture tree --------------------------------------------------------------
site="$work/site"
latest="$site/latest/download"
pinned="$site/download/v9.9.9"
bad="$site/bad/latest/download"
mkdir -p "$latest" "$pinned" "$bad"

make_fake_bin() { # $1=fake dir  $2=version string printed by `uze --version`
  mkdir -p "$1"
  printf '#!/bin/sh\nprintf "%%s\\n" "%s"\n' "uze $2" > "$1/uze"
  chmod +x "$1/uze"
}

mk_tarball() { # $1=dest dir  $2=target triple  $3=fake bin dir
  (cd "$3" && tar -czf "$1/uze-$2.tar.gz" uze)
}

# Names are generated above (`uze-*.tar.gz`), so the glob can never match a
# leading dash that GNU sha256sum would misread as an option.
# shellcheck disable=SC2035
mk_sums() { (cd "$1" && sha256sum *.tar.gz > SHASUMS256.txt); }

make_fake_bin "$work/glibc/fake-bin" "9.9.9-glibc"
make_fake_bin "$work/musl/fake-bin" "9.9.9-musl"
make_fake_bin "$work/pinned/fake-bin" "9.9.9-pinned"
make_fake_bin "$work/corrupt/fake-bin" "9.9.9-corrupt"

mk_tarball "$latest" x86_64-unknown-linux-gnu "$work/glibc/fake-bin"
mk_tarball "$latest" x86_64-unknown-linux-musl "$work/musl/fake-bin"
mk_tarball "$pinned" x86_64-unknown-linux-gnu "$work/pinned/fake-bin"
mk_tarball "$bad" x86_64-unknown-linux-gnu "$work/corrupt/fake-bin"
mk_sums "$latest"
mk_sums "$pinned"
mk_sums "$bad"

# Corrupt every checksum of the "bad" site: the installer must refuse.
sed -i 's/^/00/' "$bad/SHASUMS256.txt"

# --- local server ---------------------------------------------------------------
# `-u`: the startup line must reach the log file immediately, or the port
# handshake below would stall on Python's block-buffered redirected stderr.
python3 -u -m http.server 0 --bind 127.0.0.1 --directory "$site" \
  >"$work/server.out" 2>&1 &
server_pid=$!

port=""
i=0
while [ "$i" -lt 50 ]; do
  port="$(sed -n 's/.*port \([0-9][0-9]*\).*/\1/p' "$work/server.out" | tail -n 1)"
  [ -n "$port" ] && break
  i=$((i + 1))
  sleep 0.1
done
if [ -z "$port" ]; then
  printf 'http server failed to start\n' >&2
  cat "$work/server.out" >&2
  exit 1
fi
base="http://127.0.0.1:${port}"

# --- assertions -------------------------------------------------------------------
pass=0
fail=0
check() { # $1=description  $2=result (0 = pass)
  if [ "$2" -eq 0 ]; then
    pass=$((pass + 1))
    printf 'ok   - %s\n' "$1"
  else
    fail=$((fail + 1))
    printf 'FAIL - %s\n' "$1" >&2
  fi
}

run_installer() { # $1=log file; rest = KEY=VALUE environment overrides
  log="$1"
  shift
  env "$@" sh "$installer" >"$log" 2>&1 || return $?
}

fake_uname() { # $1=fake bin dir  $2=os  $3=arch
  mkdir -p "$1"
  # `$1`/`$2`/`$3` here are written into the generated fake uname script.
  # shellcheck disable=SC2016
  printf '#!/bin/sh\ncase "$1" in\n  -m) echo %s ;;\n  *) echo %s ;;\nesac\n' "$3" "$2" > "$1/uname"
  chmod +x "$1/uname"
}

# Syntax door check.
sh -n "$installer"
check "install.sh parses cleanly under /bin/sh" $?

# Default (glibc, latest) happy path.
run_installer "$work/out1.log" UZE_BASE_URL="$base" UZE_BIN_DIR="$work/bin1"
check "glibc/latest install succeeds" $?
"$work/bin1/uze" --version | grep -q "9.9.9-glibc"
check "installed binary is the glibc artifact" $?
grep -q "latest/download" "$work/out1.log"
check "latest release URL shape is used" $?

# musl detection via a fake `ldd` earlier on PATH.
make_fake_bin "$work/musl-bin" "unused"
printf '#!/bin/sh\necho "musl libc (x86_64) Version 1.2.5"\n' > "$work/musl-bin/ldd"
chmod +x "$work/musl-bin/ldd"
run_installer "$work/out2.log" UZE_BASE_URL="$base" UZE_BIN_DIR="$work/bin2" \
  PATH="$work/musl-bin:$PATH"
check "musl install succeeds" $?
"$work/bin2/uze" --version | grep -q "9.9.9-musl"
check "installed binary is the musl artifact" $?
grep -q "unknown-linux-musl" "$work/out2.log"
check "musl target triple is selected" $?

# Pinned version resolves the /v<version>/ path.
run_installer "$work/out3.log" UZE_BASE_URL="$base" UZE_BIN_DIR="$work/bin3" \
  UZE_VERSION="9.9.9"
check "pinned-version install succeeds" $?
"$work/bin3/uze" --version | grep -q "9.9.9-pinned"
check "installed binary is the pinned artifact" $?
grep -q "download/v9.9.9/" "$work/out3.log"
check "pinned release URL shape is used" $?

# Checksum mismatch must fail closed and leave nothing installed.
if run_installer "$work/out4.log" UZE_BASE_URL="$base/bad" UZE_BIN_DIR="$work/bin4"; then
  check "checksum mismatch is refused" 1
else
  check "checksum mismatch is refused" 0
fi
grep -q "checksum mismatch" "$work/out4.log"
check "checksum mismatch is diagnosed" $?
if [ -e "$work/bin4/uze" ]; then
  check "no binary installed on mismatch" 1
else
  check "no binary installed on mismatch" 0
fi

# Unsupported architecture fails closed.
fake_uname "$work/arch-bin" "Linux" "mips"
if run_installer "$work/out5.log" UZE_BASE_URL="$base" UZE_BIN_DIR="$work/bin5" \
  PATH="$work/arch-bin:$PATH"; then
  check "unsupported architecture is refused" 1
else
  check "unsupported architecture is refused" 0
fi
grep -q "unsupported architecture: mips" "$work/out5.log"
check "unsupported architecture is diagnosed" $?

# Unsupported OS fails closed.
fake_uname "$work/os-bin" "Darwin" "x86_64"
if run_installer "$work/out6.log" UZE_BASE_URL="$base" UZE_BIN_DIR="$work/bin6" \
  PATH="$work/os-bin:$PATH"; then
  check "unsupported OS is refused" 1
else
  check "unsupported OS is refused" 0
fi
grep -q "unsupported OS: Darwin" "$work/out6.log"
check "unsupported OS is diagnosed" $?

printf '\n%d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]