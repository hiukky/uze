#!/usr/bin/env python3
"""Live-follow the most recent Lab TUI recording.

`make lab-watch` runs this watcher: it waits for a recording to appear
under /tmp/harness-conformance (or the explicit LAB_WATCH path), streams
it to the terminal in real time as `script` writes it, retargets
automatically when a newer run starts, reports the run verdict once it
completes, and keeps waiting instead of exiting — Ctrl+C to leave.
"""
import glob
import json
import os
import sys
import time

OUTDIR = "/tmp/harness-conformance"


def newest_run_dir():
    runs = sorted(glob.glob(f"{OUTDIR}/*/run*"), key=os.path.getmtime, reverse=True)
    return runs[0] if runs else None


def verdict_summary(run_dir):
    path = os.path.join(run_dir, "verdict.json")
    if not os.path.isfile(path):
        return None
    with open(path) as f:
        results = json.load(f)
    passed = sum(1 for r in results if r["pass"])
    adapted = sum(1 for r in results if r.get("kind") == "adapted")
    return f"{passed}/{len(results)} asserted PASS, {adapted} ADAPTED"


def main():
    fixed = os.environ.get("LAB_WATCH") or None
    target = None
    offset = 0
    reported_run = None
    waiting_printed = False

    while True:
        try:
            if fixed is not None:
                run_dir = os.path.dirname(fixed)
                candidate = fixed
            else:
                run_dir = newest_run_dir()
                candidate = os.path.join(run_dir, "tui.typescript") if run_dir else None
            if candidate is None or not os.path.isfile(candidate):
                if not waiting_printed:
                    if fixed is not None:
                        print(f"waiting for recording {fixed} to appear…", flush=True)
                    else:
                        print(
                            "no recording yet — start a run in another terminal, e.g. "
                            "make lab-run HARNESS=antigravity",
                            flush=True,
                        )
                    waiting_printed = True
                time.sleep(2)
                continue
            waiting_printed = False
            if candidate != target:
                print(f"following {candidate}", flush=True)
                target = candidate
                offset = 0
            size = os.path.getsize(target)
            if size < offset:
                # The same path was recreated by a fresh run.
                offset = 0
                reported_run = None
            if size > offset:
                with open(target, "rb") as f:
                    f.seek(offset)
                    sys.stdout.buffer.write(f.read())
                    sys.stdout.buffer.flush()
                offset = size
            if run_dir != reported_run:
                summary = verdict_summary(run_dir)
                if summary:
                    reported_run = run_dir
                    print(
                        f"\n{os.path.basename(run_dir)} finished: {summary}",
                        flush=True,
                    )
            time.sleep(0.4)
        except KeyboardInterrupt:
            print("\nbye", flush=True)
            break


if __name__ == "__main__":
    main()