#!/usr/bin/env python3
"""journey — perform a product flow, then check the machine it left behind.

    journey list     [DIR]    what each journey proves, in reading order
    journey validate SPEC     parse it, resolve it, say what is wrong
    journey seed     SPEC     build the disposable world
    journey probe    SPEC     seed, open the app, leave it up to poke at
    journey run      SPEC     seed, perform every scene, check the machine

A journey never asks UZE whether UZE is happy: a `when` step performs the
flow, and every `then` check reads the filesystem, Git, the recorded task
state or the process table. Screen text appears only as `expect` — the gate
that a gesture landed — never as an assertion.
"""

from __future__ import annotations

import argparse
import glob as globlib
import hashlib
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path

import yaml

REPO = Path(__file__).resolve().parent.parent

# Outside the repository on purpose. UZE reads any path containing
# `.worktrees/<id>` as an isolated checkout of the repository above it
# (`isolated_checkout` is lexical), so a world nested under a checkout of
# this repo would open a space rooted at *this* repo rather than at the
# fixture project.
WORLDS = Path(os.environ.get("JOURNEY_WORLDS", "/tmp/uze-journeys"))
EVIDENCE = Path(
    os.environ.get("JOURNEY_EVIDENCE", Path(__file__).resolve().parent / ".evidence")
)

GREEN, RED, DIM, YELLOW, BOLD, OFF = (
    "\033[38;5;114m",
    "\033[38;5;167m",
    "\033[2m",
    "\033[38;5;179m",
    "\033[1m",
    "\033[0m",
)


def say(message: str) -> None:
    print(f"{GREEN}▸{OFF} {message}", flush=True)


def die(message: str, code: int = 1):
    print(f"journey: {message}", file=sys.stderr)
    raise SystemExit(code)


# ── the world ────────────────────────────────────────────────────────────

# A sandbox that could reach one of these is refused before any gesture.
REAL_ROOTS = [
    Path.home(),
    Path.home() / ".uze",
    Path.home() / ".claude",
    Path.home() / ".codex",
    Path.home() / ".agents",
    Path.home() / ".config" / "opencode",
]


@dataclass
class World:
    root: Path
    project: Path
    env: dict

    @property
    def home(self) -> Path:
        return self.root / "home"

    @property
    def uze_home(self) -> Path:
        return self.home / ".uze"

    def shell_env(self) -> dict:
        """Built, never inherited. An inherited variable is how a sandbox
        ends up talking to the developer's own running UZE."""
        return dict(self.env)

    def vars(self) -> dict:
        return {
            "world": str(self.root),
            "home": str(self.home),
            "uze_home": str(self.uze_home),
            "project": str(self.project),
            "repo": str(REPO),
        }


def standin_binary() -> Path:
    """The tool that writes the harness stand-ins — `uze-fake-harness` from
    `uze-testkit`."""
    if named := os.environ.get("JOURNEY_FAKE_HARNESS"):
        return Path(named)
    for candidate in (
        REPO / "target" / "debug" / "uze-fake-harness",
        REPO / "target" / "release" / "uze-fake-harness",
    ):
        if candidate.exists():
            return candidate
    die(
        "no uze-fake-harness binary: run `cargo build -p uze-testkit --bin uze-fake-harness`. "
        "The stand-ins come from uze-testkit so this tier and the Rust suites cannot come to "
        "disagree about what a harness does."
    )


def install_standins(root: Path, world_spec: dict) -> None:
    """Writes the standard stand-in set into the world's `bin`.

    Never hand-rolled here. A stand-in emulates the side effect UZE reads
    back — `agy plugin install` staging a byte copy UZE then reads as its
    ownership proof, the vendor marketplace state Claude and Codex answer
    from — and a second implementation of that drifting from the first is how
    two tiers disagree about a harness while both stay green. Real vendor
    behaviour is the conformance Lab's verdict; nothing here speaks to a
    model or a provider, and a journey that needs one was in the wrong tier.
    """
    result = subprocess.run(
        [
            str(standin_binary()),
            "--bin-dir",
            str(root / "bin"),
            "--home",
            str(root / "home"),
            "--state-dir",
            str(root / "standin-state"),
            # A journey launches these into panes, so a bare invocation has
            # to hold the terminal the way the real binary does.
            "--interactive",
            *(
                argument
                for name in world_spec.get("scripted_agents") or []
                for argument in ("--scripted-agent", name)
            ),
        ],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        die(f"uze-fake-harness failed: {result.stdout}{result.stderr}")


def guard(root: Path) -> None:
    root = root.resolve()
    for real in REAL_ROOTS:
        real = real.resolve()
        if root == real or real.is_relative_to(root):
            die(f"refusing to run: the sandbox at {root} would contain {real}")


def build_world(spec: dict, slug: str, binary: Path, keep: bool) -> World:
    root = WORLDS / slug
    guard(root)
    if root.exists() and not keep:
        shutil.rmtree(root)
    world_spec = spec.get("world", {})
    # Deliberately not `home/.uze`: UZE creates its own home on demand, and a
    # world that pre-creates it makes that unprovable.
    for part in ("home", "run", "bin", "projects"):
        (root / part).mkdir(parents=True, exist_ok=True)

    install_standins(root, world_spec)

    env = {
        "HOME": str(root / "home"),
        "UZE_HOME": str(root / "home" / ".uze"),
        "XDG_RUNTIME_DIR": str(root / "run"),
        "PATH": f"{root / 'bin'}:{binary.parent}:/usr/local/bin:/usr/bin:/bin",
        "TERM": "xterm-256color",
        "SHELL": "/bin/bash",
        "LANG": "C.UTF-8",
        "PS1": "journey $ ",
        "GIT_AUTHOR_NAME": "Ada Lovelace",
        "GIT_AUTHOR_EMAIL": "ada@journey.test",
        "GIT_COMMITTER_NAME": "Ada Lovelace",
        "GIT_COMMITTER_EMAIL": "ada@journey.test",
        "GIT_CONFIG_GLOBAL": str(root / "home" / ".gitconfig"),
    }
    (root / "home" / ".gitconfig").write_text(
        "[user]\n\tname = Ada Lovelace\n\temail = ada@journey.test\n"
        "[init]\n\tdefaultBranch = main\n[advice]\n\tdetachedHead = false\n"
    )

    # Anything a journey needs staged in its world that is not the project
    # itself — a marketplace to install from, a file a command reads.
    for relative, content in (world_spec.get("files") or {}).items():
        staged = root / relative
        staged.parent.mkdir(parents=True, exist_ok=True)
        staged.write_text(content)

    name = world_spec.get("project", "demo-app")
    project = root / "projects" / name
    if not project.exists():
        seed_project(project, world_spec, env)
    return World(root=root, project=project, env=env)


def seed_project(project: Path, world_spec: dict, env: dict) -> None:
    project.mkdir(parents=True)

    def run(*args: str) -> None:
        subprocess.run(args, cwd=project, env=env, check=True, capture_output=True)

    run("git", "init", "-q", "-b", "main")
    commits = world_spec.get("commits") or [
        {"message": "chore: initial commit", "files": {"README.md": "# demo-app\n"}},
        {
            "message": "feat(api): add the health endpoint",
            "files": {"src/main.rs": "fn main() {}\n"},
        },
        {
            "message": "docs: describe the request lifecycle",
            "files": {"docs/lifecycle.md": "# Request lifecycle\n"},
        },
    ]
    for commit in commits:
        for path, body in commit.get("files", {}).items():
            target = project / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(body)
        run("git", "add", "-A")
        run("git", "commit", "-q", "-m", commit["message"])
    if manifest := world_spec.get("manifest"):
        (project / "agents.yaml").write_text(manifest)
        run("git", "add", "-A")
        run("git", "commit", "-q", "-m", "chore: declare the worktree policy")


# ── the screen ───────────────────────────────────────────────────────────


@dataclass
class Screen:
    """One tmux session holding the app's pty. Coordinates are read from the
    frame on screen now — never typed in advance, because strips move."""

    session: str
    sidebar: int = 31

    def pane(self) -> str:
        return subprocess.run(
            ["tmux", "capture-pane", "-t", self.session, "-p"],
            capture_output=True,
            text=True,
        ).stdout

    def alive(self) -> bool:
        return (
            subprocess.run(
                ["tmux", "has-session", "-t", self.session], capture_output=True
            ).returncode
            == 0
        )

    def band(self, where: str) -> tuple[int, int]:
        return {"sidebar": (1, self.sidebar), "pane": (self.sidebar + 1, 9999)}.get(
            where, (1, 9999)
        )

    def find(self, text: str, where: str = "screen", occurrence="first"):
        low, high = self.band(where)
        rows = self.pane().splitlines()
        if where == "strip":
            rows = rows[:1]
        hits = []
        for row, line in enumerate(rows, 1):
            start = low - 1
            while (col := line.find(text, start, high)) != -1:
                hits.append((row, col + 1))
                start = col + 1
        if not hits:
            return (0, 0)
        if occurrence == "last":
            return hits[-1]
        if isinstance(occurrence, int):
            return hits[occurrence - 1] if 0 < occurrence <= len(hits) else (0, 0)
        return hits[0]

    def find_glyph(self, glyphs: str, where: str = "screen", occurrence="first"):
        low, high = self.band(where)
        rows = self.pane().splitlines()
        if where == "strip":
            rows = rows[:1]
        hits = []
        for row, line in enumerate(rows, 1):
            for col, char in enumerate(line[low - 1 : high], low):
                if char in glyphs:
                    hits.append((row, col))
        if not hits:
            return (0, 0)
        if occurrence == "last":
            return hits[-1]
        if isinstance(occurrence, int):
            return hits[occurrence - 1] if 0 < occurrence <= len(hits) else (0, 0)
        return hits[0]

    def shows(self, pattern: str, where: str = "screen") -> bool:
        haystack = (
            "".join(self.pane().splitlines()[:1]) if where == "strip" else self.pane()
        )
        return re.search(pattern, haystack) is not None

    def send(self, *args: str) -> None:
        subprocess.run(
            ["tmux", "send-keys", "-t", self.session, *args], capture_output=True
        )

    def key(self, name: str) -> None:
        self.send(name)

    def literal(self, text: str) -> None:
        self.send("-l", text)

    def mouse(self, col: int, row: int, button: int = 0) -> None:
        # SGR (1006) press/release into the pty: an app that enabled mouse
        # reporting cannot tell this from a hand.
        self.literal(f"\033[<{button};{col};{row}M")
        time.sleep(0.07)
        self.literal(f"\033[<{button};{col};{row}m")

    def type_text(self, text: str) -> None:
        for char in text:
            self.literal(char)
            time.sleep(0.03)

    def kill(self) -> None:
        subprocess.run(
            ["tmux", "kill-session", "-t", self.session], capture_output=True
        )


# ── performing ───────────────────────────────────────────────────────────


class Failed(Exception):
    """A gesture that did not happen, or a check that did not hold."""


GESTURES = ("open", "click", "rclick", "dclick", "type", "key", "shell", "wait")
AIMED = ("click", "rclick", "dclick")


@dataclass
class Runner:
    world: World
    binary: Path
    screen: Screen | None = None
    captures: dict = field(default_factory=dict)
    cast: Path | None = None
    title: str = "journey"

    def resolve(self, value):
        if isinstance(value, str):
            names = {**self.world.vars(), "uze": str(self.binary)}

            def swap(match):
                return names.get(match.group(1), match.group(0))

            return re.sub(r"\{([a-z_]+)\}", swap, value)
        if isinstance(value, list):
            return [self.resolve(item) for item in value]
        if isinstance(value, dict):
            return {key: self.resolve(item) for key, item in value.items()}
        return value

    # gestures ------------------------------------------------------------

    def perform(self, step: dict) -> None:
        action = next((name for name in GESTURES if name in step), None)
        if action is None:
            raise Failed(f"no gesture in step {step!r}")
        getattr(self, f"_{action}")(step)
        if expect := step.get("expect"):
            self.await_screen(
                expect,
                step.get("expect_in", "screen"),
                float(step.get("expect_timeout", 15)),
                step,
            )
        if (refuse := step.get("refuse")) and self.screen and self.screen.shows(refuse):
            raise Failed(f"screen shows what it must not ({refuse!r})")
        time.sleep(float(step.get("pause", 0.4)))

    def await_screen(
        self, pattern: str, where: str, timeout: float, step: dict
    ) -> None:
        deadline = time.time() + timeout
        while not self.screen.shows(pattern, where):
            if time.time() > deadline:
                raise Failed(f"{self.label(step)}: never showed {pattern!r}")
            time.sleep(0.3)

    @staticmethod
    def label(step: dict) -> str:
        return step.get("say") or next(
            (f"{name} {step[name]!r}" for name in GESTURES if name in step), "step"
        )

    def _open(self, step: dict) -> None:
        command = self.resolve(step["open"])
        cast = self.cast
        cwd = self.resolve(step.get("in", "{project}"))
        session = f"journey-{os.getpid()}"
        subprocess.run(["tmux", "kill-session", "-t", session], capture_output=True)
        # `env -i` rather than tmux's own `-e`: tmux sessions inherit the
        # tmux server's environment, and one inherited `UZE_PANE` makes the
        # app believe it is nested inside a pane of the developer's own
        # running workspace.
        launch = " ".join(
            ["env", "-i"]
            + [
                shlex.quote(f"{key}={value}")
                for key, value in self.world.shell_env().items()
            ]
            + [shlex.quote(command) if isinstance(command, str) else " ".join(command)]
        )
        # A cast is for a person to watch; it is never what proves a check.
        # Recorded inside the tmux pane, so reading the screen is unaffected.
        if cast:
            launch = (
                f"asciinema rec -q --overwrite -e TERM "
                f"-t {shlex.quote(self.title)} -c {shlex.quote(launch)} {shlex.quote(str(cast))}"
            )
        # The pane outlives the app on purpose. tmux tears a session down the
        # moment its command exits, and an app that refused to start would
        # then leave an empty capture — the one frame worth having.
        epilogue = (
            "; status=$?"
            "; printf '\\n[journey] the app exited with %s\\n' \"$status\""
            "; sleep 3600"
        )
        launch = "sh -c " + shlex.quote(launch + epilogue)
        subprocess.run(
            [
                "tmux",
                "new-session",
                "-d",
                "-s",
                session,
                "-x",
                str(step.get("cols", 150)),
                "-y",
                str(step.get("rows", 40)),
                "-c",
                cwd,
                launch,
            ],
            check=True,
            capture_output=True,
        )
        subprocess.run(
            ["tmux", "set-option", "-t", session, "status", "off"], capture_output=True
        )
        self.screen = Screen(session)
        time.sleep(1.5)

    def _target(self, step: dict) -> tuple[int, int]:
        where = step.get("in", "screen")
        occurrence = step.get("occurrence", "first")
        action = next(name for name in AIMED if name in step)
        if glyphs := step.get("glyph"):
            row, col = self.screen.find_glyph(glyphs, where, occurrence)
        else:
            row, col = self.screen.find(step[action], where, occurrence)
        if col <= 0:
            raise Failed(f"{self.label(step)}: target is not on screen")
        return row + int(step.get("row_offset", 0)), col + int(step.get("offset", 0))

    def _click(self, step: dict) -> None:
        row, col = self._target(step)
        self.screen.mouse(col, row)

    def _rclick(self, step: dict) -> None:
        row, col = self._target(step)
        self.screen.mouse(col, row, button=2)

    def _dclick(self, step: dict) -> None:
        row, col = self._target(step)
        self.screen.mouse(col, row)
        time.sleep(0.5)
        row, col = self._target(step)
        self.screen.mouse(col, row)
        time.sleep(0.11)
        self.screen.mouse(col, row)

    def _type(self, step: dict) -> None:
        if clear := step.get("clear"):
            for _ in range(32 if clear == "all" else int(clear)):
                self.screen.key("BSpace")
                time.sleep(0.03)
        self.screen.type_text(self.resolve(step["type"]))
        if step.get("submit", True):
            time.sleep(0.3)
            self.screen.key("Enter")

    def _key(self, step: dict) -> None:
        keys = step["key"] if isinstance(step["key"], list) else [step["key"]]
        for name in keys:
            self.screen.key(name)
            time.sleep(0.08)

    def _shell(self, step: dict) -> None:
        command = self.resolve(step["shell"])
        result = subprocess.run(
            command,
            shell=True,
            cwd=self.world.project,
            env=self.world.shell_env(),
            capture_output=True,
            text=True,
        )
        if result.returncode != 0 and step.get("check", True):
            raise Failed(
                f"{self.label(step)}: shell failed ({result.returncode})\n"
                f"{result.stdout}{result.stderr}"
            )

    def _wait(self, step: dict) -> None:
        until = self.resolve(step.get("until", ""))
        kind = step["wait"]
        deadline = time.time() + float(step.get("timeout", 60))
        while True:
            if kind == "shell":
                ok = (
                    subprocess.run(
                        until,
                        shell=True,
                        cwd=self.world.project,
                        env=self.world.shell_env(),
                        capture_output=True,
                    ).returncode
                    == 0
                )
            elif kind == "file":
                ok = bool(globlib.glob(until))
            elif kind == "screen":
                ok = self.screen.shows(until, step.get("in", "screen"))
            else:
                raise Failed(f"unknown wait kind {kind!r}")
            if ok:
                break
            if time.time() > deadline:
                raise Failed(
                    f"{self.label(step)}: waited, it never happened ({until!r})"
                )
            time.sleep(0.5)
        time.sleep(float(step.get("settle", 1.0)))


# ── checking the machine ─────────────────────────────────────────────────


def snapshot_tree(roots: list[str]) -> dict:
    """Every path under `roots`, with a digest of what it holds — file bytes,
    or a symlink's target. What "nothing was left behind" is measured
    against, and the reason it is a digest rather than a listing: an artifact
    that survived a removal with different content is still an orphan."""
    found = {}
    for root in roots:
        base = Path(root)
        if not base.exists():
            continue
        for path in sorted(base.rglob("*")):
            key = f"{base.name}/{path.relative_to(base)}"
            if path.is_symlink():
                found[key] = f"-> {os.readlink(path)}"
            elif path.is_dir():
                found[key] = "dir"
            else:
                try:
                    found[key] = hashlib.sha256(path.read_bytes()).hexdigest()[:16]
                except OSError as error:
                    found[key] = f"unreadable: {error}"
    return found


def resolve_json(document, path: str) -> list:
    """A dotted path into a JSON document, where `*` takes every value of an
    object or every element of a list. Small on purpose: a check reads a fact
    off a document UZE wrote, and a query language would invite asserting on
    a document's shape instead of on what it says."""
    nodes = [document]
    for part in [segment for segment in path.split(".") if segment]:
        next_nodes = []
        for node in nodes:
            if part == "*":
                if isinstance(node, dict):
                    next_nodes += list(node.values())
                elif isinstance(node, list):
                    next_nodes += node
            elif isinstance(node, dict) and part in node:
                next_nodes.append(node[part])
            elif isinstance(node, list) and part.isdigit() and int(part) < len(node):
                next_nodes.append(node[int(part)])
        nodes = next_nodes
    return nodes


VERBS = (
    "dir",
    "file",
    "link",
    "tree",
    "json",
    "git",
    "tasks",
    "process",
    "capture",
    "cmd",
)


class Checker:
    def __init__(self, runner: Runner):
        self.runner = runner
        self.world = runner.world

    def tasks(self) -> list:
        stores = sorted((self.world.uze_home / "state" / "tasks").glob("*.json"))
        out = []
        for store in stores:
            try:
                out += json.loads(store.read_text()).get("tasks", [])
            except json.JSONDecodeError:
                pass
        return sorted(out, key=lambda task: task.get("created_at_unix", 0))

    def git(self, where: Path, *args: str) -> str:
        return subprocess.run(
            ["git", *args],
            cwd=where,
            env=self.world.shell_env(),
            capture_output=True,
            text=True,
        ).stdout

    def check(self, spec: dict) -> tuple[bool, str]:
        verb = next((name for name in VERBS if name in spec), None)
        if verb is None:
            return False, f"no check verb in {spec!r}"
        return getattr(self, f"_{verb}")(self.runner.resolve(spec))

    # verbs ---------------------------------------------------------------

    def _dir(self, spec: dict) -> tuple[bool, str]:
        pattern = spec["dir"]
        found = sorted(path for path in globlib.glob(pattern) if Path(path).is_dir())
        names = sorted(Path(path).name for path in found)
        if "count" in spec and len(found) != spec["count"]:
            return (
                False,
                f"{pattern}: expected {spec['count']} directories, found {len(found)} {names}",
            )
        if spec.get("exists") is True and not found:
            return False, f"{pattern}: nothing there"
        if spec.get("exists") is False and found:
            return False, f"{pattern}: still there {names}"
        if remembered := spec.get("same_as"):
            before = self.runner.captures.get(remembered)
            if before is None:
                return False, f"no capture named {remembered!r}"
            if set(names) != set(before):
                return False, (
                    f"{pattern}: the set changed\n"
                    f"        was {sorted(before)}\n        now {names}"
                )
        return True, f"{pattern}: {names}"

    def _file(self, spec: dict) -> tuple[bool, str]:
        pattern = spec["file"]
        found = sorted(path for path in globlib.glob(pattern) if Path(path).is_file())
        if spec.get("exists") is False:
            return (not found), (
                f"{pattern}: still there" if found else f"{pattern}: absent"
            )
        if not found:
            return False, f"{pattern}: nothing there"
        if "count" in spec and len(found) != spec["count"]:
            return (
                False,
                f"{pattern}: expected {spec['count']} files, found {len(found)}",
            )
        if text := spec.get("contains"):
            missing = [
                path
                for path in found
                if text not in Path(path).read_text(errors="replace")
            ]
            if missing:
                return False, f"{pattern}: {text!r} not in {missing}"
        return True, f"{pattern}: {[Path(path).name for path in found]}"

    def _git(self, spec: dict) -> tuple[bool, str]:
        where = Path(spec["git"].get("in", str(self.world.project)))
        detail = []
        if "worktrees" in spec["git"]:
            listing = [
                line
                for line in self.git(where, "worktree", "list").splitlines()
                if line.strip()
            ]
            detail.append(f"{len(listing)} worktrees")
            if len(listing) != spec["git"]["worktrees"]:
                return False, (
                    f"expected {spec['git']['worktrees']} worktrees, found "
                    f"{len(listing)}:\n        " + "\n        ".join(listing)
                )
        if pattern := spec["git"].get("branches"):
            branches = [
                line.strip()
                for line in self.git(
                    where, "branch", "--list", pattern, "--format=%(refname:short)"
                ).splitlines()
                if line.strip()
            ]
            detail.append(f"branches {branches}")
            if "count" in spec["git"] and len(branches) != spec["git"]["count"]:
                return (
                    False,
                    f"expected {spec['git']['count']} branches matching {pattern}, found {branches}",
                )
        if "dirty" in spec["git"]:
            dirty = bool(self.git(where, "status", "--porcelain").strip())
            detail.append("dirty" if dirty else "clean")
            if dirty != spec["git"]["dirty"]:
                return (
                    False,
                    f"{where}: expected {'dirty' if spec['git']['dirty'] else 'clean'}, it is not",
                )
        return True, ", ".join(detail) or "ok"

    def _tasks(self, spec: dict) -> tuple[bool, str]:
        wanted = spec["tasks"]
        tasks = self.tasks()
        shape = [
            f"{task['id']}:{task['state']['state']}@{task.get('checkout')}"
            for task in tasks
        ]
        if "count" in wanted and len(tasks) != wanted["count"]:
            return (
                False,
                f"expected {wanted['count']} tasks, found {len(tasks)}: {shape}",
            )
        if "states" in wanted:
            states = sorted(task["state"]["state"] for task in tasks)
            if states != sorted(wanted["states"]):
                return (
                    False,
                    f"expected states {sorted(wanted['states'])}, found {states}: {shape}",
                )
        if "checkouts" in wanted:
            checkouts = {task.get("checkout") for task in tasks if task.get("checkout")}
            if len(checkouts) != wanted["checkouts"]:
                return (
                    False,
                    f"expected {wanted['checkouts']} distinct checkouts, found {sorted(checkouts)}",
                )
        if "newest_state" in wanted and (
            not tasks or tasks[-1]["state"]["state"] != wanted["newest_state"]
        ):
            return False, f"newest task is not {wanted['newest_state']!r}: {shape}"
        if "any_state" in wanted and not any(
            task["state"]["state"] == wanted["any_state"] for task in tasks
        ):
            return False, f"no task is {wanted['any_state']!r}: {shape}"
        if remembered := wanted.get("newest_checkout_in"):
            before = self.runner.captures.get(remembered)
            if before is None:
                return False, f"no capture named {remembered!r}"
            if not tasks or tasks[-1].get("checkout") not in set(before):
                return False, (
                    f"the newest task took a checkout outside {sorted(before)}: "
                    f"{tasks[-1].get('checkout') if tasks else 'no tasks'}"
                )
        return True, f"{shape}"

    def _link(self, spec: dict) -> tuple[bool, str]:
        pattern = spec["link"]
        found = sorted(
            path for path in globlib.glob(pattern) if Path(path).is_symlink()
        )
        if spec.get("exists") is False:
            return (not found), (
                f"{pattern}: still a link" if found else f"{pattern}: absent"
            )
        if not found:
            return False, f"{pattern}: no symlink there"
        if "count" in spec and len(found) != spec["count"]:
            return (
                False,
                f"{pattern}: expected {spec['count']} links, found {len(found)}",
            )
        targets = {path: os.readlink(path) for path in found}
        if wanted := spec.get("resolves_to"):
            wrong = {
                path: target for path, target in targets.items() if wanted not in target
            }
            if wrong:
                return (
                    False,
                    f"{pattern}: {wanted!r} is not what these point at: {wrong}",
                )
        return True, f"{[f'{Path(k).name} -> {v}' for k, v in targets.items()]}"

    def _tree(self, spec: dict) -> tuple[bool, str]:
        roots = spec["tree"] if isinstance(spec["tree"], list) else [spec["tree"]]
        now = snapshot_tree(roots)
        if remembered := spec.get("same_as"):
            before = self.runner.captures.get(remembered)
            if before is None:
                return False, f"no capture named {remembered!r}"
            gained = sorted(set(now) - set(before))
            lost = sorted(set(before) - set(now))
            changed = sorted(
                path for path in set(now) & set(before) if now[path] != before[path]
            )
            if gained or lost or changed:
                detail = []
                if gained:
                    detail.append(f"left behind: {gained}")
                if lost:
                    detail.append(f"removed that was there before: {lost}")
                if changed:
                    detail.append(f"changed: {changed}")
                return False, "; ".join(detail)
            return True, f"{len(now)} paths, identical to {remembered}"
        return True, f"{len(now)} paths"

    def _json(self, spec: dict) -> tuple[bool, str]:
        pattern = spec["json"]
        found = sorted(globlib.glob(pattern))
        if not found:
            return False, f"{pattern}: nothing there"
        values = []
        for path in found:
            try:
                document = json.loads(Path(path).read_text())
            except json.JSONDecodeError as error:
                return False, f"{path}: not JSON ({error})"
            values += resolve_json(document, spec.get("at", ""))
        readable = [
            value if isinstance(value, str) else json.dumps(value) for value in values
        ]
        where = f"{pattern} at {spec.get('at', '.')!r}"
        if "count" in spec and len(values) != spec["count"]:
            return (
                False,
                f"{where}: expected {spec['count']} values, found {len(values)}: {readable}",
            )
        if spec.get("exists") is True and not values:
            return False, f"{where}: nothing resolved"
        if spec.get("exists") is False and values:
            return False, f"{where}: resolved to {readable}"
        # Every resolved value, not the list as a whole: `at` usually walks a
        # `*`, and "each of these is true" is the question being asked. Pair
        # it with `count` when how many also matters.
        if "equals" in spec and any(value != spec["equals"] for value in readable):
            return False, f"{where}: not every value is {spec['equals']!r}: {readable}"
        for wanted in spec.get("includes") or []:
            if wanted not in readable:
                return False, f"{where}: {wanted!r} is missing from {readable}"
        return True, f"{where}: {readable}"

    def _process(self, spec: dict) -> tuple[bool, str]:
        pattern = spec["process"]["matching"]
        alive = False
        for pid in subprocess.run(
            ["pgrep", "-f", pattern], capture_output=True, text=True
        ).stdout.split():
            try:
                environ = Path(f"/proc/{pid}/environ").read_bytes()
            except OSError:
                continue
            if f"HOME={self.world.home}".encode() in environ:
                alive = True
        if alive != spec["process"].get("alive", True):
            return (
                False,
                f"{pattern}: {'alive' if alive else 'not running'} in this world",
            )
        return True, f"{pattern}: {'alive' if alive else 'not running'}"

    def _cmd(self, spec: dict) -> tuple[bool, str]:
        result = subprocess.run(
            spec["cmd"]["run"],
            shell=True,
            cwd=self.world.project,
            env=self.world.shell_env(),
            capture_output=True,
            text=True,
        )
        if result.returncode != spec["cmd"].get("exit", 0):
            return False, (
                f"exit {result.returncode}, expected {spec['cmd'].get('exit', 0)}\n"
                f"        {result.stdout.strip()}{result.stderr.strip()}"
            )
        if text := spec["cmd"].get("stdout_contains"):
            if text not in result.stdout:
                return (
                    False,
                    f"stdout has no {text!r}:\n        {result.stdout.strip()}",
                )
        return True, spec["cmd"]["run"]

    def _capture(self, spec: dict) -> tuple[bool, str]:
        name = spec["capture"]["name"]
        if roots := spec["capture"].get("tree"):
            value = snapshot_tree(roots if isinstance(roots, list) else [roots])
        elif pattern := spec["capture"].get("dirs"):
            value = sorted(
                Path(path).name for path in globlib.glob(pattern) if Path(path).is_dir()
            )
        elif spec["capture"].get("task_checkouts"):
            value = sorted(
                {task["checkout"] for task in self.tasks() if task.get("checkout")}
            )
        else:
            return False, f"capture {name!r} names nothing to remember"
        self.runner.captures[name] = value
        summary = value if isinstance(value, list) else sorted(value)
        if len(summary) > 6:
            summary = [*summary[:6], f"… {len(summary) - 6} more"]
        return True, f"{name} = {summary}"


# ── the spec ─────────────────────────────────────────────────────────────


def load(path: Path) -> dict:
    try:
        return yaml.safe_load(path.read_text())
    except Exception as error:  # noqa: BLE001 - reported as-is
        die(f"{path}: {error}")


def validate(spec: dict, path: Path | None = None) -> list[str]:
    problems = []
    if not spec.get("journey"):
        problems.append("the journey has no name")
    # A journey may name the user-facing page whose claim it backs. This
    # catches structural drift — a page that lost its proof, a proof that
    # points nowhere — and tells whoever changes the flow which page to
    # re-read. It cannot tell you the prose went wrong; nothing can.
    proves = spec.get("proves") or []
    for page in [proves] if isinstance(proves, str) else proves:
        if not (REPO / page).exists():
            problems.append(f"`proves` names {page!r}, which does not exist")
    for index, scene in enumerate(spec.get("scenes", []), 1):
        where = scene.get("scene", f"scene {index}")
        for step in scene.get("when", []):
            action = next((name for name in GESTURES if name in step), None)
            if action is None:
                problems.append(f"{where}: a step performs nothing: {step!r}")
                continue
            if action in AIMED and "expect" not in step:
                problems.append(
                    f"{where}: {action} {step[action]!r} states no `expect` — a gesture "
                    "must say what proves it landed"
                )
        for check in scene.get("then", []):
            if not any(name in check for name in VERBS):
                problems.append(f"{where}: a check names no verb: {check!r}")
    if not spec.get("scenes"):
        problems.append("the journey has no scenes")
    return problems


# ── commands ─────────────────────────────────────────────────────────────


def binary_path() -> Path:
    """The binary under test. `JOURNEY_UZE` names it directly, which is how
    a container runs against a binary the host's cargo cache already built."""
    if named := os.environ.get("JOURNEY_UZE"):
        return Path(named)
    for candidate in (
        REPO / "target" / "debug" / "uze",
        REPO / "target" / "release" / "uze",
    ):
        if candidate.exists():
            return candidate
    die("no uze binary: run `cargo build --bin uze` first")


def command_list(args) -> int:
    """The index, read from the journeys themselves. A hand-maintained table
    of what a suite proves is the same trap as a hand-copied test matrix: it
    is right on the day it is written."""
    chapter = None
    for path in specs_of(args):
        spec = load(path)
        where = path.parent.name
        if where != chapter:
            chapter = where
            print(f"\n{BOLD}{chapter}{OFF}")
        tags = " ".join(f"{DIM}#{tag}{OFF}" for tag in spec.get("tags") or [])
        scenes = len(spec.get("scenes") or [])
        print(f"  {GREEN}{path.name}{OFF}  {tags}")
        print(f"      {spec.get('journey', '(unnamed)')}")
        checks = sum(len(scene.get("then", [])) for scene in spec.get("scenes") or [])
        print(f"      {DIM}{scenes} scenes, {checks} checks{OFF}")
        proves = spec.get("proves") or []
        for page in [proves] if isinstance(proves, str) else proves:
            print(f"      {DIM}proves {page}{OFF}")
    print()
    return 0


def command_validate(args) -> int:
    failed = 0
    for path in specs_of(args):
        spec = load(path)
        problems = validate(spec, path)
        for problem in problems:
            print(f"{RED}✕{OFF} {path.name}: {problem}")
        if problems:
            failed += 1
        else:
            say(f"{spec['journey']}: {len(spec['scenes'])} scenes, valid")
    return 1 if failed else 0


def command_seed(args) -> World:
    spec = load(Path(args.spec))
    world = build_world(spec, Path(args.spec).stem, binary_path(), keep=args.keep)
    say(f"world at {world.root}")
    return world


def command_probe(args) -> int:
    spec = load(Path(args.spec))
    world = build_world(spec, Path(args.spec).stem, binary_path(), keep=args.keep)
    runner = Runner(world=world, binary=binary_path())
    runner._open({"open": "{repo}/target/debug/uze", "cols": 150, "rows": 40})
    say(f"world at {world.root}")
    say(f"attach with:  tmux attach -t {runner.screen.session}")
    say(f"read it with: tmux capture-pane -t {runner.screen.session} -p")
    return 0


def specs_of(args) -> list[Path]:
    """One spec, or every spec in a directory that carries `--tag`. The tag
    is how a gate runs the fast journeys on a pull request and everything on
    a nightly, without a second list to keep in step with this one."""
    target = Path(args.spec)
    if target.is_file():
        return [target]
    # Recursive and sorted: chapters are directories, and the numeric prefix
    # is what puts a reader at the start of the story rather than in the
    # middle of it. Nothing at run time depends on the order — every journey
    # builds its own world — so the numbers are for people.
    found = sorted(target.rglob("*.yml")) + sorted(target.rglob("*.yaml"))
    if tag := getattr(args, "tag", None):
        found = [spec for spec in found if tag in (load(spec).get("tags") or [])]
    if not found:
        die(
            f"{target}: no journey to run"
            + (f" tagged {args.tag!r}" if getattr(args, "tag", None) else "")
        )
    return found


def command_run_all(args) -> int:
    specs = specs_of(args)
    if len(specs) == 1:
        return run_one(args, specs[0])
    failed = []
    for spec in specs:
        if run_one(args, spec) != 0:
            failed.append(spec.name)
    print()
    if failed:
        print(
            f"{RED}{len(failed)} of {len(specs)} journeys failed{OFF}: {', '.join(failed)}"
        )
        return 1
    say(f"{len(specs)} journeys held")
    return 0


def run_one(args, path: Path) -> int:
    spec = load(path)
    if problems := validate(spec, path):
        for problem in problems:
            print(f"{RED}✕{OFF} {problem}")
        return 1

    world = build_world(spec, path.stem, binary_path(), keep=args.keep)
    stamp = time.strftime("%Y%m%d-%H%M%S")
    evidence = EVIDENCE / f"{path.stem}-{stamp}"
    evidence.mkdir(parents=True, exist_ok=True)
    cast = None
    if args.record:
        if shutil.which("asciinema"):
            cast = evidence / "take.cast"
        else:
            print(f"{YELLOW}!{OFF} asciinema is not installed; running without a cast")

    runner = Runner(world=world, binary=binary_path(), cast=cast, title=spec["journey"])
    checker = Checker(runner)
    transcript: list[str] = []

    def log(line: str = "") -> None:
        print(line)
        transcript.append(re.sub(r"\033\[[0-9;]*m", "", line))

    record = {
        "journey": spec["journey"],
        "spec": str(path),
        "world": str(world.root),
        "binary": str(binary_path()),
        "uze_version": subprocess.run(
            [str(binary_path()), "--version"], capture_output=True, text=True
        ).stdout.strip(),
        "started_at": time.strftime("%Y-%m-%dT%H:%M:%S"),
        "scenes": [],
    }
    began = time.time()

    log(f"\n{BOLD}{spec['journey']}{OFF}")
    log(f"{DIM}world {world.root}{OFF}\n")
    failures = 0
    try:
        for index, scene in enumerate(spec["scenes"], 1):
            name = scene.get("scene", f"scene {index}")
            entry = {"scene": name, "gestures": [], "checks": []}
            record["scenes"].append(entry)
            log(f"{BOLD}▪ {name}{OFF}")
            try:
                for step in scene.get("when", []):
                    at = time.time()
                    runner.perform(step)
                    entry["gestures"].append(
                        {
                            "did": Runner.label(step),
                            "seconds": round(time.time() - at, 2),
                            "gate": step.get("expect") or step.get("until"),
                        }
                    )
                    log(f"  {DIM}·{OFF} {Runner.label(step)}")
            except Failed as error:
                failures += 1
                entry["gestures"].append(
                    {"did": Runner.label(step), "failed": str(error)}
                )
                log(f"  {RED}✕ {error}{OFF}")
                # The frame is captured before leaving: a gesture that did
                # not land is exactly when the screen is worth keeping.
                entry["screen"] = capture_frame(runner, evidence, index)
                break
            for check in scene.get("then", []):
                ok, detail = checker.check(check)
                verb = next((name for name in VERBS if name in check), "?")
                entry["checks"].append(
                    {
                        "about": check.get("about") or detail,
                        "verb": verb,
                        "asked": {
                            key: value for key, value in check.items() if key != "about"
                        },
                        "read": detail,
                        "held": ok,
                    }
                )
                mark = f"{GREEN}✓{OFF}" if ok else f"{RED}✕{OFF}"
                log(f"  {mark} {check.get('about') or detail}")
                log(
                    f"        {DIM}{detail}{OFF}"
                    if ok
                    else f"        {RED}{detail}{OFF}"
                )
                failures += 0 if ok else 1
            entry["screen"] = capture_frame(runner, evidence, index)
            if failures:
                break
            log()
    finally:
        record["ended_at"] = time.strftime("%Y-%m-%dT%H:%M:%S")
        record["seconds"] = round(time.time() - began, 1)
        record["verdict"] = "failed" if failures else "held"
        record["counts"] = {
            "scenes": len(record["scenes"]),
            "gestures": sum(len(scene["gestures"]) for scene in record["scenes"]),
            "checks": sum(len(scene["checks"]) for scene in record["scenes"]),
            "failed": failures,
        }
        write_evidence(runner, evidence, record, transcript)
        if runner.screen and not args.keep_session:
            runner.screen.kill()
        if not args.keep_session:
            stop_world_servers(world)

    log()
    if failures:
        log(f"{RED}{failures} failed{OFF} — evidence in {evidence}")
        return 1
    log(f"{GREEN}▸{OFF} {spec['journey']}: every scene held")
    log(
        f"{DIM}  {record['counts']['checks']} checks read the machine · "
        f"{record['seconds']}s · evidence in {evidence}{OFF}"
    )
    return 0


def capture_frame(runner: Runner, evidence: Path, index: int) -> str | None:
    """The settled frame each scene ended on — written whether it held or
    not, because a passing run is the baseline the next failure is read
    against."""
    if not runner.screen:
        return None
    name = f"screen-{index}.txt"
    (evidence / name).write_text(runner.screen.pane())
    return name


def write_evidence(
    runner: Runner, evidence: Path, record: dict, transcript: list[str]
) -> None:
    """What the run can be audited from once it is over: the verdict with
    what every check actually read, the transcript, the world's tree, and
    the processes it was holding."""
    (evidence / "verdict.json").write_text(json.dumps(record, indent=2) + "\n")
    (evidence / "run.log").write_text("\n".join(transcript) + "\n")
    world = runner.world
    (evidence / "world.txt").write_text(
        subprocess.run(
            ["find", str(world.project), str(world.uze_home), "-maxdepth", "4"],
            capture_output=True,
            text=True,
        ).stdout
    )
    processes = []
    for line in subprocess.run(
        ["pgrep", "-a", "."], capture_output=True, text=True
    ).stdout.splitlines():
        number = line.split(" ", 1)[0]
        try:
            environ = Path(f"/proc/{number}/environ").read_bytes()
        except OSError:
            continue
        if f"HOME={world.home}".encode() in environ:
            where = Path(f"/proc/{number}/cwd")
            processes.append(
                f"{line}\n    cwd {where.resolve() if where.exists() else '?'}"
            )
    (evidence / "processes.txt").write_text("\n".join(processes) + "\n")


def stop_world_servers(world: World) -> None:
    """Stops every process this world started. The terminal server is a
    daemon by design — it outlives the client so a pane survives a client
    leaving — so nothing else would ever stop it."""
    stopped = []
    for pid in subprocess.run(
        ["pgrep", "-f", "uze"], capture_output=True, text=True
    ).stdout.split():
        try:
            environ = Path(f"/proc/{pid}/environ").read_bytes()
        except OSError:
            continue
        if f"HOME={world.home}".encode() not in environ:
            continue
        try:
            os.kill(int(pid), 15)
            stopped.append(int(pid))
        except OSError:
            pass
    # Waited on, not fired and forgotten: the endpoint is named after the
    # world's UZE_HOME, so a server still shutting down when the next run
    # starts is a live socket the next client connects to and then watches
    # die — which shows up as a tab whose pane never paints.
    deadline = time.time() + 10
    while stopped and time.time() < deadline:
        stopped = [pid for pid in stopped if Path(f"/proc/{pid}").exists()]
        if stopped:
            time.sleep(0.2)
    if stopped:
        for pid in stopped:
            try:
                os.kill(pid, 9)
            except OSError:
                pass


def main() -> int:
    parser = argparse.ArgumentParser(prog="journey")
    sub = parser.add_subparsers(dest="command", required=True)
    for name in ("list", "validate", "seed", "probe", "run"):
        child = sub.add_parser(name)
        child.add_argument("spec")
        child.add_argument(
            "--keep", action="store_true", help="reuse the existing world"
        )
        child.add_argument("--keep-session", action="store_true", help="leave tmux up")
        child.add_argument(
            "--record",
            action="store_true",
            help="also record an asciinema cast of the run",
        )
        child.add_argument(
            "--tag", help="when SPEC is a directory, only journeys with this tag"
        )
    args = parser.parse_args()
    if args.command == "list":
        return command_list(args)
    if args.command == "validate":
        return command_validate(args)
    if args.command == "seed":
        command_seed(args)
        return 0
    if args.command == "probe":
        return command_probe(args)
    return command_run_all(args)


if __name__ == "__main__":
    for tool in ("tmux", "git"):
        if not shutil.which(tool):
            die(f"{tool} is required")
    raise SystemExit(main())
