NAME = "wezterm"
DISPLAY = "x11"
VERSION = ["wezterm", "--version"]


def launch(case):
    config = case.scratch / "wezterm.lua"
    config.write_text(
        f"""local wezterm = require 'wezterm'
return {{
  font = wezterm.font({case.family!r}),
  font_size = {case.size},
  colors = {{ background = '#{case.background}', foreground = '#{case.foreground}' }},
  window_padding = {{ left = 0, right = 0, top = 0, bottom = 0 }},
  initial_cols = {case.columns},
  initial_rows = {case.rows},
  enable_tab_bar = false,
  window_decorations = 'NONE',
  enable_wayland = false,
  front_end = 'Software',
  check_for_updates = false,
  warn_about_missing_glyphs = false,
}}
"""
    )
    return [
        "wezterm",
        "--config-file",
        str(config),
        "start",
        "--always-new-process",
        "--",
        *case.command,
    ], {}
