#!/usr/bin/env python3
"""Live-follow the most recent Lab TUI recording.

`make lab-watch` runs this watcher: it waits for a recording to appear
under /tmp/harness-conformance (or the explicit LAB_WATCH path), streams
it to the terminal in real time as `script` writes it, retargets
automatically when a newer run starts, reports the run verdict once it
completes, and keeps waiting instead of exiting — Ctrl+C to leave.

Terminal mouse/focus reporting sequences (SGR `CSI < b;c;M/m`, X10
`CSI M ...`, focus `CSI I/O`, and the mode-sets that enable them) are
stripped from the display: they are input-side noise the recording can
pick up whenever the harness TUI enables mouse tracking, and re-emitting
them to a viewer's terminal renders as garbage.
"""
import glob
import json
import os
import re
import sys
import time

OUTDIR = "/tmp/harness-conformance"
MAX_HOLD = 64


class Scrubber:
    """Strips terminal mouse/focus reporting sequences from a PTY byte
    stream, tolerating sequences split across reads (held until complete,
    flushed when the recording stops growing)."""

    def __init__(self):
        self.pending = b""
        self.patterns = [
            re.compile(rb"\x1b\[<[0-9;]*[Mm]"),  # SGR mouse press/motion/release
            re.compile(rb"\x1b\[M..."),  # X10 mouse
            re.compile(rb"\x1b\[[IO]"),  # focus in/out
            re.compile(rb"\x1b\[\?100[0-9][hl]"),  # mouse/focus mode-sets
        ]

    def feed(self, data, flush=False):
        buf = self.pending + data
        if not flush:
            idx = buf.rfind(b"\x1b")
            if idx >= 0 and len(buf) - idx <= MAX_HOLD:
                tail = buf[idx:]
                if not any(p.match(tail) for p in self.patterns):
                    self.pending = tail
                    return self._strip(buf[:idx])
        self.pending = b""
        return self._strip(buf)

    def _strip(self, data):
        for p in self.patterns:
            data = p.sub(b"", data)
        return data


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
    scrubber = Scrubber()

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
                reported_run = None
                out = scrubber.feed(b"", flush=True)
                if out:
                    sys.stdout.buffer.write(out)
                    sys.stdout.buffer.flush()
            size = os.path.getsize(target)
            if size < offset:
                # The same path was recreated by a fresh run.
                offset = 0
                reported_run = None
            if size > offset:
                with open(target, "rb") as f:
                    f.seek(offset)
                    chunk = f.read()
                offset = size
                out = scrubber.feed(chunk)
                if out:
                    sys.stdout.buffer.write(out)
                    sys.stdout.buffer.flush()
            else:
                out = scrubber.feed(b"", flush=True)
                if out:
                    sys.stdout.buffer.write(out)
                    sys.stdout.buffer.flush()
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