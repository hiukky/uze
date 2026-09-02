"""Exploration: does the real AGY TUI run a PreToolUse hook, and does UZE's
generated plugin hook reach it?

Headless AGY loads plugin Skills but gates no tool through hooks (even a
vendor-format deny hook at the shared `hooks.json` lets `run_command`
execute under `--dangerously-skip-permissions`, and headless auto-denies
on permission before any hook otherwise), so hook execution is only
observable in the interactive session. This drives one: the scripted
`run_command` in its declared argument shape, AGY's own log enabled, the
permission prompt answered if it appears, and the log's hook lines read
back once the session exits.

Switches (environment):
  HOOK_TUI_VENDOR=1  install a pure vendor-format deny hook at the shared
                     `~/.gemini/config/hooks.json` instead of relying on
                     UZE's plugin hook — the control for "does the TUI run
                     hooks at all".

  HOOK_TUI_TOUCH=1   one handler per event whose only effect is a file —
                     proves execution regardless of how the answer is read.
  HOOK_TUI_ROOT=workspace  put the file at `/work/.agents/hooks.json`.
  HOOK_TUI_AGENT=<name>    launch with `--agent <name>`.

Finding (2026-09-02, 1.1.22 and 1.1.24 — pin the image with
`UZE_LAB_IMAGE`): every variant loads the hook (`hooks_manager: loaded 1
named hooks`, both processes) and executes nothing — no file for any
event, the permission prompt surfaces, the approved command runs. The
executor is gated by `CustomizationConfig.enable_json_hooks`, built by the
CLI's SDK from a server-delivered feature provider; the vertical now
measures that gate (`hooks > vendor`) before judging UZE's plugin hook.

Run: python3 conformance/lab.py --harness antigravity --experiment antigravity/hook-tui
"""

import json
import os
import subprocess
import time

import pexpect

from harnesses.antigravity.scenarios import agy_setup
from shared import common

ARGS = (
    '{"CommandLine":"echo API secrets","Cwd":"/work","WaitMsBeforeAsync":2000,'
    '"toolSummary":"Command execution","toolAction":"Running command"}'
)
GLOBAL_HOOKS = "/work/home/.gemini/config/hooks.json"
VENDOR_HOOK = (
    """cat > %s <<'EOF'
{
  "probe": {
    "PreToolUse": [
      {
        "matcher": "run_command",
        "hooks": [
          {
            "type": "command",
            "command": "printf '{\\"decision\\":\\"deny\\",\\"reason\\":\\"blocked by protect-env\\"}'"
          }
        ]
      }
    ]
  }
}
EOF
"""
    % GLOBAL_HOOKS
)
# HOOK_TUI_TOUCH=1: one handler per event whose only effect is a file —
# proves execution regardless of how the harness treats the answer.
TOUCH_HOOK = (
    """cat > %s <<'EOF'
{
  "probe": {
    "PreToolUse": [
      {"matcher": "*", "hooks": [{"type": "command", "command": "touch /work/hook-pre-tool; printf '{}'"}]}
    ],
    "PostToolUse": [
      {"matcher": "*", "hooks": [{"type": "command", "command": "touch /work/hook-post-tool; printf '{}'"}]}
    ],
    "PreInvocation": [
      {"type": "command", "command": "touch /work/hook-pre-invocation; printf '{}'"}
    ],
    "Stop": [
      {"type": "command", "command": "touch /work/hook-stop; printf '{}'"}
    ]
  }
}
EOF
"""
    % GLOBAL_HOOKS
)
MARKERS = ("UZE_CONFORMANCE_PASS", "blocked by protect-env", "Denied by UZE hook")


def final_script():
    prelude = ""
    if os.environ.get("HOOK_TUI_VENDOR"):
        prelude = VENDOR_HOOK
    if os.environ.get("HOOK_TUI_TOUCH"):
        prelude = TOUCH_HOOK
    # HOOK_TUI_ROOT=workspace writes the same file to the workspace
    # customization root instead of the shared global path.
    if os.environ.get("HOOK_TUI_ROOT") == "workspace":
        prelude = "mkdir -p /work/.agents\n" + prelude.replace(
            GLOBAL_HOOKS, "/work/.agents/hooks.json"
        )
    agent = os.environ.get("HOOK_TUI_AGENT", "")
    agent_flag = f"--agent {agent}" if agent else ""
    return f"""{prelude}
agy {agent_flag} --log-file /work/agy.log
echo '===== agy.log (hooks) ====='
grep -n -i "hook" /work/agy.log | grep -v Migration | head -60
echo '===== end ====='
"""


def run(cfg, prov_ip):
    common.start_provider(
        cfg, "toolcall", {"TOOL_NAME": "run_command", "FC_ARGS": ARGS}
    )
    time.sleep(1)
    setup = agy_setup(
        cfg,
        prov_ip,
        include_mcp=False,
        final_cmd=final_script(),
        plugins="flow hook-plugin",
    )
    cmd = common.docker_base(cfg, prov_ip, setup)
    # Named so the vendor's log can be read from outside while the session
    # is alive — the container dies with the TUI, and `/exit` is not
    # reliably accepted.
    name = f"hook-tui-{os.getpid()}"
    cmd[2:2] = ["--name", name]
    child = pexpect.spawn(
        cmd[0], cmd[1:], encoding="utf-8", codec_errors="replace", timeout=300
    )
    child.setwinsize(50, 160)
    child.logfile_read = common.CastRecorder(cfg.outdir, "tui-hook-experiment")
    screen = common.make_screen(child)

    child.expect("Choose your color scheme", timeout=150)
    child.send("\r")
    time.sleep(3)
    child.send("\t\t")
    time.sleep(0.7)
    child.send("\r")
    time.sleep(5)
    screen(3)

    for ch in "run the API check":
        child.send(ch)
        time.sleep(0.08)
    child.send("\r")
    seen = ""
    prompted = False
    for _ in range(20):
        _, p = screen(2.0)
        seen += p
        if any(m in seen for m in MARKERS):
            break
        if "Do you want to proceed" in seen and not prompted:
            prompted = True
            child.send("\r")
            time.sleep(1.0)
    with open(f"{cfg.outdir}/hook_tui.raw", "w") as f:
        f.write(seen)
    common.settle_and_quiet(screen)

    log = subprocess.run(
        [
            "docker",
            "exec",
            name,
            "sh",
            "-c",
            "ls -la /work/hook-* 2>&1; grep -n -i 'hook' /work/agy.log | grep -v Migration | head -60;"
            " echo '----- hooks.json -----'; cat /work/home/.gemini/config/hooks.json /work/.agents/hooks.json 2>&1;"
            " echo; echo '----- log tail -----'; tail -c 3000 /work/agy.log",
        ],
        capture_output=True,
        text=True,
        errors="replace",
        timeout=60,
    )
    print(log.stdout[-8000:], log.stderr[-500:], flush=True)
    child.close(force=True)
    subprocess.run(["docker", "rm", "-f", name], capture_output=True)

    struct = common.provider_struct(cfg)
    with open(f"{cfg.outdir}/hook_tui_struct.json", "w") as f:
        json.dump(struct, f, indent=1)
    markers = common.observed_markers(struct, "hook_markers")
    common.check(
        "hook-tui-permission-prompted",
        prompted,
        "the vendor permission prompt appeared before any hook decision"
        if prompted
        else "no permission prompt",
        kind="observe",
    )
    common.check(
        "hook-tui-denial-relayed",
        bool(markers.get("blocked by protect-env")),
        ", ".join(f"{m}={v}" for m, v in sorted(markers.items())),
    )
