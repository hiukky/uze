"""VTE, the widget GNOME Terminal and every GTK terminal draw with, driven
through xfce4-terminal: the lightest host for it that needs no session."""

NAME = "vte (xfce4-terminal)"
DISPLAY = "x11"
VERSION = ["xfce4-terminal", "--version"]


def launch(case):
    return [
        "xfce4-terminal",
        "--disable-server",
        "--hide-menubar",
        "--hide-toolbar",
        "--hide-scrollbar",
        "--hide-borders",
        f"--geometry={case.columns}x{case.rows}+0+0",
        f"--font={case.family} {case.size}",
        f"--color-bg=#{case.background}",
        f"--color-text=#{case.foreground}",
        "-x",
        *case.command,
    ], {}
