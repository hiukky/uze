NAME = "kitty"
DISPLAY = "x11"
VERSION = ["kitty", "--version"]


def launch(case):
    options = {
        "font_family": case.family,
        "font_size": case.size,
        "window_padding_width": 0,
        "background": f"#{case.background}",
        "foreground": f"#{case.foreground}",
        "initial_window_width": f"{case.columns}c",
        "initial_window_height": f"{case.rows}c",
        "remember_window_size": "no",
        "placement_strategy": "top-left",
        "linux_display_server": "x11",
        "cursor_blink_interval": 0,
        "hide_window_decorations": "yes",
        "update_check_interval": 0,
    }
    argv = ["kitty", "--config", "NONE"]
    for key, value in options.items():
        argv += ["-o", f"{key}={value}"]
    return argv + case.command, {}
