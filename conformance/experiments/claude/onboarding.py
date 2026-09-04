"""Probe: does the first-run drive still reach Claude Code's prompt?

Onboarding is the single point every check in the claude vertical depends
on, and the only part of it that a version bump silently rewrites: a
screen the drive fails to recognize is answered with Enter anyway, and the
folder-trust screen answers Enter with "No, exit" — the TUI quits and all
24 checks fail together, ten minutes into a gate run.

This is that first minute on its own: spawn the real TUI, drive the
dialogs, and say whether the prompt came up — the loop to iterate in when
a `VERSION DRIFT` line precedes a red vertical (2.1.260 painted the trust
screen with cursor-forward moves instead of spaces, which is why the
matching is space-insensitive).

Run: python3 conformance/lab.py --harness claude --experiment claude/onboarding
"""

import pexpect

from harnesses.claude.scenarios import claude_container, drive_onboarding
from shared import common


def run(cfg, prov_ip):
    cmd = claude_container(cfg, prov_ip, "exec claude")
    child = pexpect.spawn(
        cmd[0], cmd[1:], encoding="utf-8", codec_errors="replace", timeout=300
    )
    child.setwinsize(50, 160)
    child.logfile_read = common.CastRecorder(cfg.outdir, "onboarding")

    t, p, m = drive_onboarding(child)
    with open(f"{cfg.outdir}/onboarding.raw", "w") as f:
        f.write(t or "")
    joined = common.squash(p)
    reached = bool(t) and ("Opus" in joined or "APIUsageBilling" in joined)
    common.check(
        "onboarding-reached-prompt",
        reached,
        "the first-run dialogs were answered and the prompt came up"
        if reached
        else f"last marker={m!r} screen={p[-200:]!r}",
    )
    common.check(
        "onboarding-tui-alive",
        child.isalive(),
        "the TUI survived onboarding"
        if child.isalive()
        else "the TUI exited — a dialog was answered with the wrong option",
    )
    child.send("\x03")
    child.close(force=True)
