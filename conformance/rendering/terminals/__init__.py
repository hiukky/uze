"""How each terminal is launched with a font, and on which display.

A binding says how its terminal is driven and nothing else: every one is
judged by `contract.py` through the same analyzer. A terminal that cannot
run in the Lab declares `UNSUPPORTED` with the reason rather than being
left out, so the catalog can say "not verified" instead of saying nothing.

Each binding module defines:

- `NAME`, and `DISPLAY` (`"x11"` or `"wayland"`);
- `VERSION`: the argv that prints the terminal's version;
- `launch(case) -> (argv, env)`, where `case` carries the font family,
  size, grid, colours, a scratch directory, and the command to run.
"""

from dataclasses import dataclass
from importlib import import_module
from pathlib import Path

NAMES = ["kitty", "wezterm", "alacritty", "ghostty", "foot", "vte", "konsole", "xterm"]


@dataclass(frozen=True)
class Case:
    family: str
    size: int
    columns: int
    rows: int
    background: str  # "ffffff"
    foreground: str  # "000000"
    scratch: Path
    command: list[str]


def binding(name: str):
    return import_module(f"terminals.{name}")


def all_bindings():
    return [binding(name) for name in NAMES]
