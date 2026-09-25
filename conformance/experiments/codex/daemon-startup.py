"""Observation experiment: what does a codex 0.157 interactive launch draw
while it starts its shared app-server daemon for the first time?

0.157.0 enables `daemon_auto_start` by default (openai/codex#47179). On a
home with no daemon package the TUI paints, leaves the alternate screen,
prints `Installing daemon from CLI version ...` on the primary one, then
comes back and only then opens the trust dialog. This records the raw
stream for a fixed window, answering the trust dialog with Enter, and
reports the order the landmarks arrived in — pure evidence, no assertions
beyond the ordering it observed.

`DAEMON_PREINSTALL=1` installs the package first with the vendor's own
`codex app-server daemon start` / `stop`, which is the state every launch
after a person's first one starts from.

Run:
  python3 conformance/lab.py --harness codex --experiment codex/daemon-startup
  DAEMON_PREINSTALL=1 python3 conformance/lab.py --harness codex --experiment codex/daemon-startup
"""

import os
import time

import pexpect

from harnesses.codex.scenarios import codex_container
from shared import common

PREINSTALL = os.environ.get("DAEMON_PREINSTALL") == "1"
LANDMARKS = (
    "Installing daemon",
    "Trust this folder?",
    "Ask Codex to do anything",
    "for shortcuts",
    "\x1b[?1049l",
    "\x1b[?1049h",
    "Error:",
)


def run(cfg, prov_ip):
    label = "preinstalled" if PREINSTALL else "first-launch"
    common.start_provider(cfg, "static")
    time.sleep(1)
    prelude = (
        "codex app-server daemon start >/dev/null 2>&1\n"
        "codex app-server daemon stop >/dev/null 2>&1\n"
        if PREINSTALL
        else ""
    )
    cmd = codex_container(cfg, prov_ip, f"{prelude}exec codex")
    child = pexpect.spawn(
        cmd[0], cmd[1:], encoding="utf-8", codec_errors="replace", timeout=300
    )
    child.setwinsize(50, 160)
    child.logfile_read = common.CastRecorder(cfg.outdir, f"daemon-{label}")

    raw = ""
    trusted = False
    deadline = time.time() + 45
    while time.time() < deadline:
        try:
            raw += child.read_nonblocking(65536, timeout=0.5)
        except pexpect.TIMEOUT:
            pass
        except pexpect.EOF:
            break
        if not trusted and "Trust this folder?" in common.ansi_strip(raw):
            time.sleep(0.5)
            child.send("\r")
            trusted = True

    order = sorted((raw.find(mark), mark) for mark in LANDMARKS if raw.find(mark) >= 0)
    with open(f"{cfg.outdir}/daemon-{label}.log", "w") as log:
        for pos, mark in order:
            log.write(f"{pos:>8} {mark!r}\n")
    for pos, mark in order:
        print(f"[{label}] {pos:>8} {mark!r}")
    common.check(
        f"daemon-{label}-no-install-banner",
        "Installing daemon" not in raw,
        "no install banner in the TUI stream",
    )
    common.check(
        f"daemon-{label}-single-screen-entry",
        raw.count("\x1b[?1049h") == 1,
        f"alternate screen entered {raw.count(chr(27) + '[?1049h')} time(s)",
    )
    try:
        child.terminate(force=True)
    except Exception:
        pass
