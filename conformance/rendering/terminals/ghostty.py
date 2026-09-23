NAME = "ghostty"
DISPLAY = "x11"
VERSION = ["ghostty", "+version"]


def launch(case):
    return [
        "ghostty",
        "--config-default-files=false",
        f"--font-family={case.family}",
        f"--font-size={case.size}",
        f"--background={case.background}",
        f"--foreground={case.foreground}",
        "--window-padding-x=0",
        "--window-padding-y=0",
        "--window-padding-balance=false",
        f"--window-width={case.columns}",
        f"--window-height={case.rows}",
        "--window-decoration=false",
        "--gtk-single-instance=false",
        "--cursor-style-blink=false",
        "-e",
        *case.command,
    ], {"GDK_BACKEND": "x11"}
