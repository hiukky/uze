"""Tolerance/contract experiment: real Claude Code under a FAIL-CLOSED hook.

The canonical suite proves deny/allow/order with handlers that run. This
experiment proves the fail-closed contract (ADR-033): a *declared deny* hook
whose handler cannot run (missing script) must still deny — the intercepted
tool must never execute — because a safety hook that cannot be evaluated is
never weakened into a no-op.

Evidence = the real harness relay: the tool's output never reaches the
conversation (provider-side `struct.json`), the turn settles on the denied
surface, and the wrapper's failure reason is recorded.

Not part of the canonical suite yet: promotion requires 3 consecutive clean
runs (openspec/changes/extend-conformance-coverage).

Run: python3 conformance/lab.py --harness claude --experiment claude/fail-closed
"""

import json
import time

import pexpect

from harnesses.claude.scenarios import (
    claude_container,
    drive_onboarding,
    make_screen,
    make_waiter,
)
from shared import common


def run(cfg, prov_ip):
    common.start_provider(
        cfg,
        "toolcall",
        {"TOOL_NAME": "Bash", "TOOL_ARGS": '{"command":"echo API secrets"}'},
    )
    time.sleep(1)
    cmd = claude_container(cfg, prov_ip, "exec claude", plugins="flow hook-fail-plugin")
    child = pexpect.spawn(
        cmd[0], cmd[1:], encoding="utf-8", codec_errors="replace", timeout=300
    )
    child.setwinsize(50, 160)
    try:
        child.logfile_read = common.CastRecorder(cfg.outdir, "tui-fail-closed")
    except Exception:
        pass
    screen = make_screen(child)
    wait_for = make_waiter(screen)
    t, p, m = drive_onboarding(child)
    for ch in "run the API check":
        child.send(ch)
        time.sleep(0.06)
    child.send("\r")
    t3, p3, m3 = wait_for(
        [
            "UZE_CONFORMANCE_PASS",
            "blocked by protect-env",
            "Denied by UZE hook",
            "denied",
        ],
        tries=24,
        gap=2.5,
    )
    settled = m3 is not None and common.settle_and_quiet(screen)
    with open(f"{cfg.outdir}/fail_closed.raw", "w") as f:
        f.write(t3)
    struct = common.provider_struct(cfg)
    with open(f"{cfg.outdir}/fail_closed_struct.json", "w") as f:
        json.dump(struct, f, indent=1)
    common.check("fail-closed-turn-settled", m3 is not None, f"turn settled: {m3!r}")
    has_output = any(
        r.get("summary", {}).get("hook_markers", {}).get("plain output") for r in struct
    )
    common.check_absence(
        "fail-closed-tool-never-executed",
        not has_output,
        settled,
        "the intercepting tool never executed — fail-closed deny held"
        if not has_output
        else "the tool executed despite the fail-closed deny — contract broken",
    )
    child.send("\x03")
    time.sleep(0.6)
    child.send("\x03")
    child.close(force=True)
    common.check(
        "fail-closed-evidence-recorded",
        bool(struct),
        "provider-side evidence captured for the fail-closed turn",
    )
