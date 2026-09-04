#!/usr/bin/env python3
"""Drives the uze workspace TUI through a pty and reads its screen back.

The flow a person actually performs, in a disposable container: create an
agent from the picker, let it commit in its slot, remove that slot by
hand, then click the "resume" the row grows and pick a harness. What it
asserts is the outcome — the task back on its own branch, its commit
present, and the dead row gone.

Run it with `./run.sh` from this directory (it builds the binary, copies it
into the image and runs this script); `./run.sh dump` stops after the first
frame, which is the fast way to see what the TUI is showing.

The harness the picker offers is a stand-in: a copy of `cat` named
`claude`, so the pane has a live foreground process by that name — which
is how a tab is recognized as an agent at all.
"""

import os
import pty
import select
import subprocess
import sys
import time

import pyte

COLUMNS, ROWS = 140, 42
PROJECT = "/work/project"


def sh(command, cwd=None, check=True):
    result = subprocess.run(
        command,
        cwd=cwd,
        shell=isinstance(command, str),
        capture_output=True,
        text=True,
        check=False,
    )
    if check and result.returncode != 0:
        raise SystemExit(
            f"{command}: {result.returncode}\n{result.stdout}\n{result.stderr}"
        )
    return result.stdout.strip()


class Tui:
    def __init__(self, cwd):
        self.screen = pyte.Screen(COLUMNS, ROWS)
        self.stream = pyte.ByteStream(self.screen)
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            os.chdir(cwd)
            os.environ["TERM"] = "xterm-256color"
            os.environ["COLUMNS"] = str(COLUMNS)
            os.environ["LINES"] = str(ROWS)
            os.execvp("uze", ["uze"])
        import fcntl
        import struct
        import termios

        fcntl.ioctl(
            self.fd, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLUMNS, 0, 0)
        )

    def pump(self, seconds=0.3):
        deadline = time.time() + seconds
        while time.time() < deadline:
            ready, _, _ = select.select([self.fd], [], [], 0.05)
            if not ready:
                continue
            try:
                data = os.read(self.fd, 65536)
            except OSError:
                return
            if not data:
                return
            self.stream.feed(data)

    def lines(self):
        return [self.screen.display[row].rstrip() for row in range(ROWS)]

    def dump(self, title=""):
        print(f"--- screen {title} ---")
        for index, line in enumerate(self.lines()):
            print(f"{index:>3}|{line}")
        print("--- end ---", flush=True)

    def find(self, needle, occurrence=0):
        """(column, row) of the start of `needle`, 0-based, or None."""
        seen = 0
        for row, line in enumerate(self.lines()):
            start = 0
            while True:
                column = line.find(needle, start)
                if column < 0:
                    break
                if seen == occurrence:
                    return column, row
                seen += 1
                start = column + 1
        return None

    def wait_for(self, needle, timeout=30, label=None):
        deadline = time.time() + timeout
        while time.time() < deadline:
            self.pump(0.3)
            found = self.find(needle)
            if found:
                return found
        self.dump(f"waiting for {label or needle!r}")
        raise SystemExit(f"timed out waiting for {needle!r}")

    def wait_until(self, predicate, timeout=30, label=""):
        deadline = time.time() + timeout
        while time.time() < deadline:
            self.pump(0.3)
            if predicate():
                return True
        self.dump(f"waiting for {label}")
        raise SystemExit(f"timed out waiting for {label}")

    def click(self, column, row):
        # SGR mouse, 1-based, button 0 press then release.
        os.write(self.fd, f"\x1b[<0;{column + 1};{row + 1}M".encode())
        self.pump(0.1)
        os.write(self.fd, f"\x1b[<0;{column + 1};{row + 1}m".encode())
        self.pump(0.4)

    def key(self, data):
        os.write(self.fd, data.encode())
        self.pump(0.3)


def worktrees():
    root = os.path.join(PROJECT, ".worktrees")
    if not os.path.isdir(root):
        return []
    return sorted(os.path.join(root, name) for name in os.listdir(root))


def main():
    os.makedirs(PROJECT, exist_ok=True)
    sh("git config --global user.email e2e@uze.test")
    sh("git config --global user.name e2e")
    sh("git config --global init.defaultBranch main")
    if not os.path.isdir(os.path.join(PROJECT, ".git")):
        sh("git init -q", cwd=PROJECT)
        with open(os.path.join(PROJECT, "README.md"), "w") as seed:
            seed.write("seed\n")
        sh("git add -A", cwd=PROJECT)
        sh("git commit -qm seed", cwd=PROJECT)

    tui = Tui(PROJECT)
    tui.pump(3.0)
    tui.dump("after launch")

    step = sys.argv[1] if len(sys.argv) > 1 else "all"
    if step == "dump":
        return

    # 1. Create an agent: the "✦" in the tab strip opens the picker.
    spark = tui.wait_for("✦", label="the new-agent button")
    tui.click(*spark)
    tui.pump(0.6)
    tui.dump("picker open")
    option = tui.find("Claude")
    if option is None:
        raise SystemExit("no harness offered in the picker")
    tui.click(option[0] + 1, option[1])
    tui.wait_until(lambda: len(worktrees()) == 1, label="the agent's worktree")
    slot = worktrees()[0]
    print(f"agent placed in {slot}", flush=True)
    tui.pump(2.0)
    tui.dump("agent running")

    # 2. The agent commits, and its checkout is removed from under it.
    with open(os.path.join(slot, "kept.rs"), "w") as kept:
        kept.write("fn kept() {}\n")
    sh("git add -A", cwd=slot)
    sh("git commit -qm 'work the agent did'", cwd=slot)
    branch = sh("git rev-parse --abbrev-ref HEAD", cwd=slot)
    print(f"committed on {branch}", flush=True)
    sh(f"rm -rf {slot}")

    # 3. The row says so, and offers the way back in.
    tui.wait_for("checkout removed", timeout=60, label="the row noticing")
    tui.dump("checkout removed")
    print(sh("git branch -v", cwd=PROJECT), flush=True)
    resume = tui.wait_for("resume", timeout=60, label="the resume affordance")
    print(f"resume at {resume}", flush=True)

    # 4. Click it, then pick the harness in the picker it opens.
    tui.click(resume[0] + 1, resume[1])
    tui.pump(0.8)
    tui.dump("picker over the tree")
    option = tui.find("Claude")
    if option is None:
        raise SystemExit("the resume opened no picker")
    # The leftmost cell of the row: the half that stands over the tree.
    tui.click(option[0] - 1, option[1])
    tui.wait_until(
        lambda: len(worktrees()) == 1, timeout=90, label="the task back in a slot"
    )
    tui.pump(3.0)
    tui.dump("after resume")

    revived = worktrees()[0]
    revived_branch = sh("git rev-parse --abbrev-ref HEAD", cwd=revived)
    kept = os.path.isfile(os.path.join(revived, "kept.rs"))
    rows = tui.lines()
    corpse = any("checkout removed" in row for row in rows)

    print("=== verdict ===")
    print(f"slot          : {revived}")
    print(f"branch        : {revived_branch} (wanted {branch})")
    print(f"commit kept   : {kept}")
    print(f"dead row left : {corpse}")
    ok = revived_branch == branch and kept and not corpse
    print("RESULT:", "PASS" if ok else "FAIL")
    if not ok:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
