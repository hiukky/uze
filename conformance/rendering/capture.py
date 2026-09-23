#!/usr/bin/env python3
"""Inside the Lab image: draw the specimen in every terminal and font, capture
the screen, and judge each capture.

    capture.py [--terminal kitty ...] [--font "JetBrains Mono" ...]
               [--set nerd] [--out /evidence]

Each case gets its own fontconfig holding only the font under test (and
DejaVu, which is what a Symbols-only font is a fallback *for*), so no other
font of the catalog can stand in for a missing glyph. Each capture is taken
once the specimen reports it has drawn and the screen has stopped changing.
There is no fixed sleep and no retry.
"""

import argparse
import hashlib
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from PIL import Image

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import analyze  # noqa: E402
import fonts as catalog  # noqa: E402
import terminals  # noqa: E402

# Large enough that one pixel of hinting is a few percent of an icon, not
# the tenth of it it is at a 10px cell.
SIZE = 20
COLUMNS, ROWS = 60, 100
SCREEN = (1400, 3800)
BACKGROUND, FOREGROUND = "ffffff", "000000"
DEJAVU = Path("/usr/share/fonts/truetype/dejavu")
# How long a terminal may take to start and draw, and how long its screen
# may keep changing after the specimen has finished printing, before the
# case is recorded as not drawn rather than waited on further.
DRAW_DEADLINE = 45.0
SETTLE_DEADLINE = 10.0
SETTLE_INTERVAL = 0.4


def family_of(path: Path) -> str:
    return subprocess.run(
        ["fc-scan", "--format", "%{family[0]}", str(path)],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()


def font_cases():
    """(label, build, file, the family a terminal is given) for the catalog.
    A Symbols-only font is measured the way it is used: as the fallback of
    an unpatched monospace font."""
    for font in catalog.installed():
        family = (
            "DejaVu Sans Mono" if font.family == "Symbols" else family_of(font.path)
        )
        yield font, family


def fontconfig(scratch: Path, font_path: Path) -> Path:
    fonts = scratch / "fonts"
    fonts.mkdir()
    (fonts / font_path.name).symlink_to(font_path)
    config = scratch / "fonts.conf"
    config.write_text(
        f"""<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
<fontconfig>
  <dir>{fonts}</dir>
  <dir>{DEJAVU}</dir>
  <cachedir>{scratch / "cache"}</cachedir>
</fontconfig>
"""
    )
    return config


class X11:
    def __init__(self):
        self.env = {"DISPLAY": ":99"}
        self.server = subprocess.Popen(
            [
                "Xvfb",
                ":99",
                "-screen",
                "0",
                f"{SCREEN[0]}x{SCREEN[1]}x24",
                "-dpi",
                "96",
                # A white root, the colour the terminals are given, so the
                # analyzer's "most common colour" is the terminal's own
                # background rather than the empty screen around it.
                "-wr",
                "-nolisten",
                "tcp",
            ],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        wait_for(
            lambda: (
                subprocess.run(
                    ["xdpyinfo"], env={**os.environ, **self.env}, capture_output=True
                ).returncode
                == 0
            ),
            15,
            "Xvfb",
        )

    def launch(self, argv, env):
        return subprocess.Popen(
            argv,
            env={**os.environ, **env, **self.env},
            start_new_session=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
        )

    def capture(self, target: Path):
        subprocess.run(
            ["import", "-window", "root", str(target)],
            env={**os.environ, **self.env},
            check=True,
            capture_output=True,
        )

    def close(self):
        self.server.terminate()


class Wayland:
    """A headless sway per case: foot is started by sway's own `exec`, so it
    inherits the case's environment and fills the one output there is."""

    def __init__(self):
        self.runtime = Path(tempfile.mkdtemp(prefix="xdg-"))
        self.runtime.chmod(0o700)
        self.env = {"XDG_RUNTIME_DIR": str(self.runtime)}
        self.sway = None

    def launch(self, argv, env):
        config = Path(env["LAB_SCRATCH"]) / "sway.conf"
        quoted = " ".join("'" + a.replace("'", "'\\''") + "'" for a in argv)
        config.write_text(
            f"output HEADLESS-1 resolution {SCREEN[0]}x{SCREEN[1]}\n"
            "default_border none\ndefault_floating_border none\ngaps inner 0\n"
            f"output * bg #{BACKGROUND} solid_color\n"
            f"exec {quoted}\n"
        )
        self.sway = subprocess.Popen(
            ["sway", "--unsupported-gpu", "-c", str(config)],
            env={
                **os.environ,
                **env,
                **self.env,
                "WLR_BACKENDS": "headless",
                "WLR_RENDERER": "pixman",
                "WLR_LIBINPUT_NO_DEVICES": "1",
            },
            start_new_session=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
        )
        return self.sway

    def capture(self, target: Path):
        sockets = sorted(
            p.name
            for p in self.runtime.glob("wayland-*")
            if not p.name.endswith(".lock")
        )
        subprocess.run(
            ["grim", str(target)],
            env={**os.environ, **self.env, "WAYLAND_DISPLAY": sockets[0]},
            check=True,
            capture_output=True,
        )

    def close(self):
        shutil.rmtree(self.runtime, ignore_errors=True)


def wait_for(condition, deadline: float, what: str):
    until = time.monotonic() + deadline
    while time.monotonic() < until:
        if condition():
            return
        time.sleep(0.1)
    raise TimeoutError(f"{what} did not come up within {deadline:.0f}s")


def settled_capture(display, target: Path) -> bool:
    """Captures until two consecutive screens are identical: the terminal
    has finished drawing what the specimen printed."""
    previous = None
    until = time.monotonic() + SETTLE_DEADLINE
    while time.monotonic() < until:
        display.capture(target)
        digest = hashlib.sha256(Image.open(target).tobytes()).hexdigest()
        if digest == previous:
            return True
        previous = digest
        time.sleep(SETTLE_INTERVAL)
    return False


def stop(process):
    if process and process.poll() is None:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.wait()


def version_of(binding) -> str:
    try:
        run = subprocess.run(
            binding.VERSION,
            capture_output=True,
            text=True,
            timeout=20,
            env={**os.environ, "DISPLAY": ":99"},
        )
        return (run.stdout or run.stderr).strip().splitlines()[0]
    except (OSError, subprocess.TimeoutExpired, IndexError):
        return "unknown"


def run_case(display, binding, font, family, layout, set_name, out: Path) -> dict:
    slug = f"{binding.__name__.split('.')[-1]}--{font.file.removesuffix('.ttf')}"
    with tempfile.TemporaryDirectory(prefix="case-") as scratch:
        scratch = Path(scratch)
        ready = scratch / "drawn"
        shell = f"uze theme specimen {set_name}; printf '\\033[?25l'; touch {ready}; exec sleep 600"
        case = terminals.Case(
            family,
            SIZE,
            COLUMNS,
            ROWS,
            BACKGROUND,
            FOREGROUND,
            scratch,
            ["sh", "-c", shell],
        )
        argv, env = binding.launch(case)
        env = {
            **env,
            "FONTCONFIG_FILE": str(fontconfig(scratch, font.path)),
            "UZE_HOME": str(scratch / "uze"),
            "HOME": str(scratch),
            "LAB_SCRATCH": str(scratch),
        }
        record = {
            "terminal": binding.NAME,
            "family": font.family,
            "build": font.build,
            "font": family,
            "file": font.file,
            "manual": False,
            "capture": f"{slug}.png",
        }
        process = display.launch(argv, env)
        try:
            try:
                wait_for(lambda: ready.exists(), DRAW_DEADLINE, "the specimen")
            except TimeoutError as error:
                stderr = ""
                if process.poll() is not None and process.stderr:
                    stderr = process.stderr.read().decode(errors="replace")[-800:]
                return {
                    **record,
                    "drawn": False,
                    "passes": False,
                    "error": f"{error}. {stderr}".strip(),
                }
            target = out / f"{slug}.png"
            if not settled_capture(display, target):
                return {
                    **record,
                    "drawn": False,
                    "passes": False,
                    "error": "the screen never settled",
                }
            try:
                verdict = analyze.analyze(Image.open(target), layout)
            except SystemExit as error:
                return {**record, "drawn": False, "passes": False, "error": str(error)}
            return {**record, **verdict}
        finally:
            stop(process)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--terminal", action="append", choices=terminals.NAMES)
    parser.add_argument(
        "--font", action="append", help="catalog family, e.g. 'JetBrains Mono'"
    )
    parser.add_argument("--build", action="append", choices=["plain", "mono", "propo"])
    parser.add_argument("--set", default="nerd")
    parser.add_argument("--out", type=Path, default=Path("/evidence"))
    args = parser.parse_args()

    args.out.mkdir(parents=True, exist_ok=True)
    layout = json.loads(
        subprocess.run(
            ["uze", "theme", "specimen", args.set, "--format", "json"],
            check=True,
            capture_output=True,
            text=True,
            env={**os.environ, "UZE_HOME": "/tmp/uze-layout"},
        ).stdout
    )
    uze_version = subprocess.run(
        ["uze", "--version"], capture_output=True, text=True
    ).stdout.strip()
    cases = [
        (font, family)
        for font, family in font_cases()
        if (not args.font or font.family in args.font)
        and (not args.build or font.build in args.build)
    ]

    x11 = X11()
    results = []
    try:
        for name in args.terminal or terminals.NAMES:
            binding = terminals.binding(name)
            if getattr(binding, "UNSUPPORTED", None):
                results.append(
                    {"terminal": binding.NAME, "unsupported": binding.UNSUPPORTED}
                )
                continue
            display = x11 if binding.DISPLAY == "x11" else Wayland()
            version = version_of(binding)
            for font, family in cases:
                result = run_case(
                    display, binding, font, family, layout, args.set, args.out
                )
                result.update({"terminal_version": version, "uze_version": uze_version})
                results.append(result)
                failing = {k: v for k, v in result.get("criteria", {}).items() if v}
                status = "pass" if result.get("passes") else "FAIL"
                detail = result.get("error") or "; ".join(
                    f"{k}: {', '.join(v)}" for k, v in failing.items()
                )
                print(
                    f"{status:4}  {binding.NAME:22} {font.id:28} size {result.get('icon_size')}  {detail}",
                    flush=True,
                )
            if display is not x11:
                display.close()
    finally:
        x11.close()

    (args.out / "verdict.json").write_text(
        json.dumps(
            {
                "set": args.set,
                "uze_version": uze_version,
                "layout": layout,
                "results": results,
            },
            indent=2,
        )
        + "\n"
    )
    return 0 if all(r.get("passes") or r.get("unsupported") for r in results) else 1


if __name__ == "__main__":
    sys.exit(main())
