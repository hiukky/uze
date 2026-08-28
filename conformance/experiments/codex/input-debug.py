"""Observation experiment: why does the codex TUI drop typed input in the
hooks phase (codex 0.150.1)?

Replicates the hooks-phase setup exactly (flow hook-plugin + toolcall
provider) and dumps the raw TUI stream on a timer while a unique marker is
typed — pure evidence, zero assertions. `DEBUG_CONTROL=1` runs the control
setup (flow mcp-plugin, static provider) so the two environments can be
compared byte-for-byte on the same machine/version.

Run:
  python3 conformance/lab.py --harness codex --experiment codex/input-debug
  DEBUG_CONTROL=1 python3 conformance/lab.py --harness codex --experiment codex/input-debug
"""

import json
import os
import time

import pexpect

from harnesses.codex.scenarios import (
    codex_container,
    drive_onboarding,
    make_screen,
)
from shared import common

MARK = "alpha bravo"
CONTROL = os.environ.get("DEBUG_CONTROL") == "1"


def run(cfg, prov_ip):
    label = "control" if CONTROL else "hooks-setup"
    plugins = "flow" if CONTROL else "flow hook-plugin"
    tool = os.environ.get("DEBUG_TOOL", "Bash")
    tool_args = os.environ.get("DEBUG_TOOL_ARGS", '{"command":"echo API secrets"}')
    if CONTROL:
        common.start_provider(cfg, "static")
    else:
        common.start_provider(
            cfg,
            "toolcall",
            {"TOOL_NAME": tool, "TOOL_ARGS": tool_args},
        )
    time.sleep(1)
    cmd = codex_container(
        cfg,
        prov_ip,
        "exec codex --dangerously-bypass-hook-trust" if not CONTROL else "exec codex",
        plugins=plugins,
    )
    child = pexpect.spawn(
        cmd[0], cmd[1:], encoding="utf-8", codec_errors="replace", timeout=300
    )
    child.setwinsize(50, 160)
    try:
        child.logfile_read = common.CastRecorder(cfg.outdir, f"debug-{label}")
    except Exception:
        pass
    screen = make_screen(child)

    log = open(f"{cfg.outdir}/input-debug-{label}.log", "w")

    def dump(tag, t, p):
        log.write(f"\n##### {tag} #####\nPLAIN: {p[-400:]}\nRAW: {t[-300:]!r}\n")
        log.flush()

    t, p = drive_onboarding(child)
    dump("after_onboarding", t, p)
    common.check(
        f"debug-{label}-onboarding",
        "Ask Codex to do anything" in p,
        "prompt reached after onboarding",
    )

    # 1:1 mirror of the canonical type_with_echo: same text, same pacing,
    # same Ctrl-U retries, per-try screen dumps to see what the check sees.
    text = "run the API check" if not CONTROL else "alpha bravo"
    echoed = False
    for attempt in range(10):
        if attempt > 0:
            child.send("\x15")
            time.sleep(0.5)
        for ch in text:
            child.send(ch)
            time.sleep(0.08)
        time.sleep(2.0)
        t, p = screen(1.5)
        dump(f"try{attempt}", t, p)
        if text.replace(" ", "") in p.replace(" ", ""):
            echoed = True
            break
    common.check(
        f"debug-{label}-typed-echo",
        echoed,
        "marker echoed at the prompt" if echoed else "never echoed across 10 tries",
    )
    child.send("\r")
    for i in range(10):
        time.sleep(2)
        t, p = screen(0.5)
        dump(f"t{i}", t, p)
    child.close(force=True)
    struct = common.provider_struct(cfg)
    with open(f"{cfg.outdir}/struct.json", "w") as f:
        json.dump(struct, f, indent=1)
    log.close()
    common.check(f"debug-{label}-observed", True, "observation stream recorded")
