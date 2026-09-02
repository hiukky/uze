"""Driving a harness TUI, without knowing which one it is.

Every vertical hand-rolled the same sequence: spawn under a recorder, wait
for a prompt marker, sleep out the warmup, type a character at a time,
accumulate several reads because a repaint splits a name across frames,
strip ANSI, snapshot. Four copies of it drifted in the details that matter
— how long to accumulate, whether to strip before matching — which is how
two verticals ended up matching a heading instead of the content under it.
"""

import time

import pexpect

from shared import common
from shared.common import ansi_strip, make_screen, make_waiter


class Tui:
    """One live harness TUI."""

    def __init__(self, cfg, cmd, tag):
        self.cfg = cfg
        self.tag = tag
        self.child = pexpect.spawn(
            cmd[0], cmd[1:], encoding="utf-8", codec_errors="replace", timeout=300
        )
        self.child.setwinsize(50, 160)
        try:
            self.child.logfile_read = common.CastRecorder(cfg.outdir, tag)
        except Exception:
            pass
        self.screen = make_screen(self.child)
        self.wait_for = make_waiter(self.screen)
        self._snapshots = 0

    def until(self, markers, tries=8):
        """Waits for any of `markers` and returns the plain text that
        satisfied it.

        The text has to come back from here: waiting consumes the reads, so
        a `collect()` afterwards sees an empty screen. Every vertical
        learned that separately; the driver owns it now.
        """
        _, plain, matched = self.wait_for(
            list(markers), tries=tries, stop_on_death=True
        )
        return plain, matched

    def ready(self, markers, tries=16):
        """Waits for any of `markers`, returning the plain screen text."""
        _, plain, matched = self.wait_for(
            list(markers), tries=tries, stop_on_death=True
        )
        self.snapshot("ready", plain)
        return plain, matched

    def type(self, text, per_char=0.08):
        """Types as a person does. Harness prompts drop input pasted in one
        write — a measured behaviour, not superstition."""
        for character in text:
            self.child.send(character)
            time.sleep(per_char)

    def submit(self):
        self.child.send("\r")

    def collect(self, reads=8, gap=1.5, size=400_000):
        """Accumulates several reads into one plain-text view.

        One read is not a screen: a TUI repaints by region, so a name can
        arrive split across frames. Matching a single read is how a check
        starts depending on timing.
        """
        raw = ""
        for _ in range(reads):
            time.sleep(gap)
            try:
                raw += self.child.read_nonblocking(size=size, timeout=3)
            except Exception:
                break
        return ansi_strip(raw)

    def ask(self, prompt, reads=8):
        """Sends a prompt and returns what the turn produced."""
        self.type(prompt)
        self.submit()
        return self.collect(reads=reads)

    def snapshot(self, name, text):
        self._snapshots += 1
        path = f"{self.cfg.outdir}/{self._snapshots:02d}_{self.tag}-{name}.raw"
        with open(path, "w") as handle:
            handle.write(text)
        return path

    def close(self):
        try:
            self.child.close(force=True)
        except Exception:
            pass

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()
