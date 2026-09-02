"""Exploration: does the real AGY load and honour a PreToolUse hook in a
headless turn, and does UZE's generated plugin hook reach it?

A scripted `run_command` (in the tool's declared argument shape) is served
to a `--print` session, so the only thing standing between the call and
its execution is the PreToolUse hook. Evidence: AGY's own log (hook
loading), the stream-json steps, and what reached the provider.

Switches (environment):
  HOOK_PRINT_GLOBAL=1  copy the generated plugin hooks to the vendor's
                       shared `~/.gemini/config/hooks.json` first — separates
                       "plugin hooks are not discovered" from "hooks do not
                       run headless".
  HOOK_PRINT_VENDOR=1  install a pure vendor-format deny hook (no UZE
                       wrapper) at that global path — a control for "does any
                       PreToolUse hook run in this session mode".
  HOOK_PRINT_ASK=1     drop `--dangerously-skip-permissions`.

Finding (2026-09-02, 1.1.24): with the call in its declared shape the
tool executes and the turn completes; `hooks_manager` reports `loaded 0
named hooks from 0 hooks.json file(s)` for the plugin, and with the hooks
at the global path — loaded — the vendor-format deny hook still gates
nothing headless (skip-permissions executes; ask mode auto-denies on
permission, before any hook). Headless is not a surface for hooks.

Run: python3 conformance/lab.py --harness antigravity --experiment antigravity/hook-print
"""

import json
import os
import subprocess

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


def final_script():
    prelude = ""
    if os.environ.get("HOOK_PRINT_GLOBAL"):
        prelude += f"cp /work/home/.gemini/config/plugins/hook-plugin/hooks.json {GLOBAL_HOOKS}\n"
    if os.environ.get("HOOK_PRINT_VENDOR"):
        prelude += VENDOR_HOOK
    skip = "" if os.environ.get("HOOK_PRINT_ASK") else "--dangerously-skip-permissions"
    return f"""{prelude}
agy --print "run the API check" --output-format stream-json \\
  {skip} --print-timeout 90s --log-file /work/agy.log 2>&1 | tail -c 3000
echo '===== agy.log (hooks) ====='
grep -n -i "hook" /work/agy.log | grep -v Migration | head -40
echo '===== agy.log (feature flags) ====='
grep -n -iE "unleash|experiment|feature|flag|json-hooks" /work/agy.log | head -40
"""


def run(cfg, prov_ip):
    common.start_provider(
        cfg, "toolcall", {"TOOL_NAME": "run_command", "FC_ARGS": ARGS}
    )
    setup = agy_setup(
        cfg,
        prov_ip,
        include_mcp=False,
        final_cmd=final_script(),
        plugins="flow hook-plugin",
    )
    r = subprocess.run(
        common.docker_base(cfg, prov_ip, setup, tty=False),
        capture_output=True,
        text=True,
        errors="replace",
        timeout=300,
    )
    print(r.stdout[-6000:], flush=True)
    if r.stderr:
        print(r.stderr[-1500:], flush=True)
    struct = common.provider_struct(cfg)
    with open(f"{cfg.outdir}/hook_print_struct.json", "w") as f:
        json.dump(struct, f, indent=1)
    markers = common.observed_markers(struct, "hook_markers")
    common.check(
        "hook-print-denial-relayed",
        bool(markers.get("blocked by protect-env")),
        ", ".join(f"{m}={v}" for m, v in sorted(markers.items())),
    )
