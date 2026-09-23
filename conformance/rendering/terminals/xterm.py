NAME = "xterm"
DISPLAY = "x11"
VERSION = ["xterm", "-version"]


def launch(case):
    return [
        "xterm",
        "-fa",
        case.family,
        "-fs",
        str(case.size),
        "-bg",
        f"#{case.background}",
        "-fg",
        f"#{case.foreground}",
        "-geometry",
        f"{case.columns}x{case.rows}+0+0",
        "-b",
        "0",
        "-bw",
        "0",
        "+sb",
        "-e",
        *case.command,
    ], {}
