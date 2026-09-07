"""Observation probe: what a second process in the same terminal actually does.

`continuity-second-process-ready` went red on three verticals and green on
one, and the three disagreed about what was on the screen — a colour picker
that never came back, a splash logo, and nothing at all. One message for
several different facts is not something to fix by guessing, so this drives
the exact shape the continuity contract drives — the scene's own prelude,
both launches through UZE's launcher — and then, instead of asserting,
prints what the terminal did second by second.

What it answers, per harness: does `quit` actually end the process; how long
the shell takes to reach the second launch; and what the second process
renders, so `rejoin` can wait for something that exists rather than for the
first run's onboarding, which never happens twice.

Pure evidence, zero assertions. Each vendor has a one-line entry point, so:

  python3 conformance/lab.py --harness codex --experiment codex/relaunch

`RELAUNCH_WATCH` (default 90) is how many seconds to keep reading after the
quit; `RELAUNCH_TURN=0` skips the first turn when only the exit matters.
"""

import importlib
import os
import time

from contract import continuity

WATCH_SECONDS = int(os.environ.get("RELAUNCH_WATCH", "90"))
TAKE_A_TURN = os.environ.get("RELAUNCH_TURN", "1") == "1"


def bindings_for(harness):
    """This harness's bindings, the way `lab.py` loads them."""
    module = importlib.import_module(f"harnesses.{harness}.bindings")
    for value in vars(module).values():
        if isinstance(value, type) and value.__name__.endswith("Bindings"):
            if value.__module__ == module.__name__:
                return value()
    raise RuntimeError(f"no bindings class in harnesses.{harness}.bindings")


def say(line):
    print(f"    {line}", flush=True)


def run(cfg, prov_ip):
    if os.environ.get("RELAUNCH_REJOIN") == "1":
        return probe_rejoin(cfg, prov_ip)
    bindings = bindings_for(cfg.harness)
    started = time.time()

    def elapsed():
        return f"{time.time() - started:6.1f}s"

    with bindings.relaunch_in(
        cfg, prov_ip, continuity.SLOT, continuity.prelude(bindings.launcher_name())
    ) as tui:
        plain, matched = bindings.prepare(tui)
        say(f"[{elapsed()}] first process ready={bool(matched)}")
        if not matched:
            say(f"screen was: {plain[-400:]!r}")
            return
        time.sleep(bindings.warmup)

        if TAKE_A_TURN:
            tui.type(f"{continuity.FIRST_MARKER}: remember this word")
            tui.submit()
            tui.collect(reads=4)
            say(f"[{elapsed()}] first turn sent")

        plain, ended = continuity._end_the_process(tui, bindings)
        say(f"[{elapsed()}] exit keys sent, first process ended={ended}")
        say(f"[{elapsed()}] reading for {WATCH_SECONDS}s")

        # One second per read, printed as it arrives: the question is *when*
        # each thing appears, and an accumulated dump at the end cannot say.
        deadline = time.time() + WATCH_SECONDS
        seen_end = ended
        while time.time() < deadline:
            text = tui.collect(reads=1, gap=1.0)
            if not text.strip():
                continue
            tail = " ".join(text.split())[-220:]
            say(f"[{elapsed()}] {tail}")
            if continuity.ENDED_MARKER in text and not seen_end:
                seen_end = True
                say(f"[{elapsed()}] ^^ the first process exited here")
        # A process that painted nothing may be alive and simply not
        # repainting: a screen read returns what arrived, and a TUI that
        # drew its frame into the alternate buffer before anyone was
        # reading draws nothing again until something provokes it.
        say(f"[{elapsed()}] child alive: {tui.child.isalive()} — nudging")
        for nudge, label in ((" \x7f", "space+backspace"), ("\x0c", "ctrl-l")):
            tui.child.send(nudge)
            text = tui.collect(reads=3, gap=1.0)
            say(f"[{elapsed()}] after {label}: {' '.join(text.split())[-300:]!r}")

        say(f"[{elapsed()}] done; first process exited: {seen_end}")
        tui.snapshot("relaunch-probe-tail", tui.collect(reads=2))


def probe_rejoin(cfg, prov_ip):
    """The same shape, but asking the question the contract asks: does
    `rejoin` see the second process reach its prompt, and how long did it
    take? `RELAUNCH_REJOIN=1` selects this over the raw dump."""
    bindings = bindings_for(cfg.harness)
    started = time.time()
    with bindings.relaunch_in(
        cfg, prov_ip, continuity.SLOT, continuity.prelude(bindings.launcher_name())
    ) as tui:
        plain, matched = bindings.prepare(tui)
        say(f"[{time.time() - started:6.1f}s] first ready={bool(matched)}")
        if not matched:
            say(f"screen was: {plain[-400:]!r}")
            return
        time.sleep(bindings.warmup)
        tui.type(f"{continuity.FIRST_MARKER}: remember this word")
        tui.submit()
        tui.collect(reads=4)
        _, ended = continuity._end_the_process(tui, bindings)
        say(f"[{time.time() - started:6.1f}s] first ended={ended}")
        plain, matched = bindings.rejoin(tui)
        say(f"[{time.time() - started:6.1f}s] rejoin matched={bool(matched)}")
        say("screen the second process left, in full:")
        for line in plain.splitlines():
            if line.strip():
                say(f"  | {line}")
        # What the whole wait accumulated, rendered as one screen: the
        # marker a single frame missed usually sits here.
        extra = tui.collect(reads=3)
        say("and what it drew next:")
        for line in extra.splitlines():
            if line.strip():
                say(f"  | {line}")
