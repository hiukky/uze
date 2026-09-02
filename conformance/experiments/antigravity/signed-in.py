"""Exploration: does the real AGY accept the Lab's synthetic signed-in
identity, and does it then execute `hooks.json` hooks?

The vertical ran on a Gemini API key, where this harness loads hooks and
never runs them (google-antigravity/antigravity-cli#893). This drives the
same container with the synthetic `consumer` token file in place and the
CloudCode plane served by the provider, so a run answers two questions at
once: which endpoints the CLI asks for (read the provider's own log, and
`--discovery` for the bodies), and whether a vendor-format control hook —
no UZE in the loop — actually fires.

Switches (environment):
  SIGNED_IN_MODE=print   headless `--print` turn (fast: auth + endpoints,
                         but headless is not a surface for hooks — see the
                         `hook-print` experiment). Default is the TUI.
  SIGNED_IN_HOOK=touch   the control hook only touches a file per event
                         (execution regardless of how the answer is read);
                         `none` installs no control hook, leaving only UZE's
                         delivered plugin hook; default is the vendor-format
                         deny hook.
  SIGNED_IN_AUTH=apikey  run the same probe on the API key instead — the
                         control for "the mode is what changed".

Run: python3 conformance/lab.py --harness antigravity \
       --experiment antigravity/signed-in --discovery
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
#: The named-hook key. UZE's delivered entries are namespaced
#: `<package>:<group-id>`, so the probe can ask whether the vendor accepts
#: that character set at all (`SIGNED_IN_HOOK_NAME`).
HOOK_NAME = os.environ.get("SIGNED_IN_HOOK_NAME", "probe")
DENY_HOOK = """cat > %s <<'EOF'
{
  "%s": {
    "PreToolUse": [
      {
        "matcher": "run_command",
        "hooks": [
          {
            "type": "command",
            "command": "touch /work/hook-pre-tool; printf '{\\"decision\\":\\"deny\\",\\"reason\\":\\"blocked by protect-env\\"}'"
          }
        ]
      }
    ]
  }
}
EOF
""" % (GLOBAL_HOOKS, HOOK_NAME)
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
#: `|| true` throughout: the setup script runs under `set -e`, and a grep
#: that matches nothing is an answer, not a failure to abort on.
INSPECT = (
    "ls -la /work/hook-* 2>&1 || true;"
    " echo '----- account -----';"
    " grep -n -iE 'login|logged|auth|token|tier|consumer' /work/agy.log | head -30 || true;"
    " echo '----- errors -----';"
    " grep -n -iE 'error|fail|denied|unauth' /work/agy.log | head -40 || true;"
    " echo '----- model path -----';"
    " grep -n -iE 'generatecontent|cascade|executor|model' /work/agy.log | tail -40 || true;"
    " echo '----- hooks -----';"
    " grep -n -i 'hook' /work/agy.log | grep -v Migration | head -40 || true;"
    " echo '----- log tail -----'; tail -c 4000 /work/agy.log || true"
)


def prelude():
    """The control hook installed before the session. `none` installs no
    control at all, which is how UZE's own plugin hook is observed alone."""
    choice = os.environ.get("SIGNED_IN_HOOK", "deny")
    if choice == "none":
        return ""
    return TOUCH_HOOK if choice == "touch" else DENY_HOOK


def auth():
    return "apikey" if os.environ.get("SIGNED_IN_AUTH") == "apikey" else "consumer"


def run_command(cfg, prov_ip):
    """`SIGNED_IN_CMD=<shell>` — one command inside the provisioned,
    signed-in container (e.g. `agy models`), with the same inspection tail.
    The cheapest loop there is for "what does the CLI think it has"."""
    final = f"""{prelude()}
{os.environ["SIGNED_IN_CMD"]}
echo '===== inspect ====='
{INSPECT}
"""
    setup = agy_setup(
        cfg,
        prov_ip,
        include_mcp=False,
        final_cmd=final,
        plugins="flow hook-plugin",
        auth=auth(),
    )
    r = subprocess.run(
        common.docker_base(cfg, prov_ip, setup, tty=False),
        capture_output=True,
        text=True,
        errors="replace",
        timeout=400,
    )
    print(r.stdout[-9000:], flush=True)
    if r.stderr:
        print(r.stderr[-1500:], flush=True)
    return ""


def run_print(cfg, prov_ip):
    final = f"""{prelude()}
agy --print "run the API check" --output-format stream-json \\
  --dangerously-skip-permissions --print-timeout 90s --log-file /work/agy.log 2>&1 | tail -c 2500
echo '===== inspect ====='
{INSPECT}
"""
    setup = agy_setup(
        cfg,
        prov_ip,
        include_mcp=False,
        final_cmd=final,
        plugins="flow hook-plugin",
        auth=auth(),
    )
    r = subprocess.run(
        common.docker_base(cfg, prov_ip, setup, tty=False),
        capture_output=True,
        text=True,
        errors="replace",
        timeout=400,
    )
    print(r.stdout[-9000:], flush=True)
    if r.stderr:
        print(r.stderr[-1500:], flush=True)
    return ""


def run_tui(cfg, prov_ip):
    setup = agy_setup(
        cfg,
        prov_ip,
        include_mcp=False,
        final_cmd=f"{prelude()}\nagy --log-file /work/agy.log",
        plugins="flow hook-plugin",
        auth=auth(),
    )
    cmd = common.docker_base(cfg, prov_ip, setup)
    # Named so the vendor's log can be read from outside while the session
    # is alive — the container dies with the TUI.
    name = f"signed-in-{os.getpid()}"
    cmd[2:2] = ["--name", name]
    child = pexpect.spawn(
        cmd[0], cmd[1:], encoding="utf-8", codec_errors="replace", timeout=300
    )
    child.setwinsize(50, 160)
    child.logfile_read = common.CastRecorder(cfg.outdir, "tui-signed-in")
    screen = common.make_screen(child)

    child.expect("Choose your color scheme", timeout=150)
    child.send("\r")
    time.sleep(3)
    child.send("\t\t")
    time.sleep(0.7)
    child.send("\r")
    time.sleep(5)
    _, first = screen(3)
    with open(f"{cfg.outdir}/signed_in_prompt.raw", "w") as f:
        f.write(first)
    print("----- prompt -----\n" + first[-2500:], flush=True)

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
        if not child.isalive():
            break
        if "Do you want to proceed" in seen and not prompted:
            prompted = True
            child.send("\r")
            time.sleep(1.0)
        elif "How's the CLI experience" in p:
            child.send("0\r")
            time.sleep(1.0)
    with open(f"{cfg.outdir}/signed_in_turn.raw", "w") as f:
        f.write(seen)
    common.settle_and_quiet(screen)
    log = subprocess.run(
        ["docker", "exec", name, "sh", "-c", INSPECT],
        capture_output=True,
        text=True,
        errors="replace",
        timeout=60,
    )
    print(log.stdout[-9000:], log.stderr[-500:], flush=True)
    child.close(force=True)
    subprocess.run(["docker", "rm", "-f", name], capture_output=True)
    common.check(
        "signed-in-permission-prompted",
        prompted,
        "the vendor permission prompt appeared before any hook decision"
        if prompted
        else "no permission prompt",
        kind="observe",
    )
    return seen


def run(cfg, prov_ip):
    common.start_provider(
        cfg, "toolcall", {"TOOL_NAME": "run_command", "FC_ARGS": ARGS}
    )
    time.sleep(1)
    if os.environ.get("SIGNED_IN_CMD"):
        seen = run_command(cfg, prov_ip)
    elif os.environ.get("SIGNED_IN_MODE") == "print":
        seen = run_print(cfg, prov_ip)
    else:
        seen = run_tui(cfg, prov_ip)
    struct = common.provider_struct(cfg)
    with open(f"{cfg.outdir}/signed_in_struct.json", "w") as f:
        json.dump(struct, f, indent=1)
    print(
        "----- provider paths -----\n"
        + "\n".join(f"{r['seq']}: {r['method']} {r['path']}" for r in struct),
        flush=True,
    )
    markers = common.observed_markers(struct, "hook_markers")
    common.check(
        "signed-in-turn-reached-model",
        any(r.get("summary", {}).get("tools") for r in struct),
        "the harness sent a request declaring its tools (the model path works)",
    )
    common.check(
        "signed-in-control-hook-executed",
        bool(markers.get("blocked by protect-env")) or "blocked by protect-env" in seen,
        ", ".join(f"{m}={v}" for m, v in sorted(markers.items())),
    )
