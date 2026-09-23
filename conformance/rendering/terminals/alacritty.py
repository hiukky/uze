NAME = "alacritty"
DISPLAY = "x11"
VERSION = ["alacritty", "--version"]


def launch(case):
    config = case.scratch / "alacritty.toml"
    config.write_text(
        f"""[font]
normal = {{ family = "{case.family}" }}
size = {case.size}

[colors.primary]
background = "#{case.background}"
foreground = "#{case.foreground}"

[window]
dimensions = {{ columns = {case.columns}, lines = {case.rows} }}
padding = {{ x = 0, y = 0 }}
decorations = "None"
"""
    )
    return ["alacritty", "--config-file", str(config), "-e", *case.command], {
        "WINIT_UNIX_BACKEND": "x11"
    }
