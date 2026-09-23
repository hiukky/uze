NAME = "konsole"
DISPLAY = "x11"
VERSION = ["konsole", "--version"]


def launch(case):
    data = case.scratch / "share"
    (data / "konsole").mkdir(parents=True, exist_ok=True)
    (data / "konsole" / "Lab.colorscheme").write_text(
        f"""[Background]
Color={int(case.background[:2], 16)},{int(case.background[2:4], 16)},{int(case.background[4:], 16)}

[Foreground]
Color={int(case.foreground[:2], 16)},{int(case.foreground[2:4], 16)},{int(case.foreground[4:], 16)}

[General]
Description=Lab
"""
    )
    (data / "konsole" / "Lab.profile").write_text(
        f"""[Appearance]
ColorScheme=Lab
Font={case.family},{case.size},-1,5,400,0,0,0,0,0,0,0,0,0,0,1

[General]
Name=Lab
TerminalColumns={case.columns}
TerminalRows={case.rows}
TerminalMargin=0
TerminalCenter=false

[Scrolling]
ScrollBarPosition=2
"""
    )
    return [
        "konsole",
        "--profile",
        "Lab",
        "--separate",
        "--hide-menubar",
        "--hide-tabbar",
        "--nofork",
        "-e",
        *case.command,
    ], {"XDG_DATA_HOME": str(data), "QT_QPA_PLATFORM": "xcb"}
