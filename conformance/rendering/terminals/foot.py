NAME = "foot"
DISPLAY = "wayland"
VERSION = ["foot", "--version"]


def launch(case):
    config = case.scratch / "foot.ini"
    config.write_text(
        f"""[main]
font={case.family}:size={case.size}
pad=0x0

[colors]
background={case.background}
foreground={case.foreground}
"""
    )
    return ["foot", "--config", str(config), *case.command], {}
