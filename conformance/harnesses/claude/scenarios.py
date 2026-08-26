#!/usr/bin/env python3
import json
"""Claude Code scenario (latest channel) — Real Harness + Synthetic World.

Phase A (TUI): onboarding drive -> prompt; /plugin, /mcp (server connected,
1 tool), deterministic turn, model-visible skill present and user-only skill
ABSENT from the PRIMARY model request (disable-model-invocation — genuine
policy preservation), MCP registration + connection via /mcp.

Preflight: TLS interception of the hardcoded Anthropic hosts via /etc/hosts
+ injected CA (NODE_EXTRA_CA_CERTS). `ANTHROPIC_BASE_URL` is ignored by the
interactive TUI — the TLS interception is the required hook.
"""
import time

import pexpect

import os
import sys
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", ".."))
import shared.common as common
from shared.common import (check, docker_base, generate_certs, make_screen,
                           make_waiter, materialize_marketplace, provider_struct)


def claude_setup(cfg, prov_ip, final_cmd):
    return f"""
set -e
export PATH=/usr/local/bin:/usr/bin:/bin:/usr/local/.local/bin
export HOME=/work/home CLAUDE_CONFIG_DIR=/work/home/.claude UZE_HOME=/work/home/.uze
export ANTHROPIC_API_KEY=uze-conformance-invalid-by-design
export ANTHROPIC_BASE_URL=https://api.anthropic.com
export NODE_EXTRA_CA_CERTS=/app/ca.crt
mkdir -p /work/home/.claude
cp /app/fixtures/claude.json /work/home/.claude.json
{materialize_marketplace(cfg)}
uze market add /work/market >/dev/null 2>&1
for p in flow mcp-plugin; do uze plugin install $p@uze-lab >/dev/null 2>&1; done
{final_cmd}
"""


def claude_container(cfg, prov_ip, final_cmd):
    cmd = docker_base(cfg, prov_ip, claude_setup(cfg, prov_ip, final_cmd))
    ca_crt, _, _ = generate_certs(cfg)
    i = cmd.index(common.HARNESS_IMAGE)
    cmd = cmd[:i] + ["-v", f"{ca_crt}:/app/ca.crt:ro",
                     "-e", "CLAUDE_CONFIG_DIR=/work/home/.claude"] + cmd[i:]
    return cmd


def drive_onboarding(child):
    """Dialogs: theme -> API key (Yes) -> security notes -> trust (Yes) ->
    Tips/What's-new popup -> prompt. Returns (screen, plain, marker)."""
    screen = make_screen(child)
    wait_for = make_waiter(screen)
    DIALOGS = ["Detected a custom API key", "theme", "Security notes",
               "Quick safety check", "Accessing workspace",
               "login method", "Opus5", "❯"]
    t, p, m = wait_for(DIALOGS, tries=16)
    for i in range(8):
        if m == "Detected a custom API key":
            child.send("\x1b[A"); time.sleep(0.3); child.send("\r")   # Yes
        elif m in ("theme", "Security notes"):
            child.send("\r")
        elif m in ("Quick safety check", "Accessing workspace"):
            child.send("\r")                                          # trust Yes
        elif m == "login method":
            child.send("\x1b[B"); time.sleep(0.3); child.send("\r")
        else:
            break
        t, p, m = wait_for(DIALOGS, tries=14)
        if m in ("Opus5", "❯"):
            break
    # The Tips/What's-new overlay popup does not block the prompt; the first
    # slash-command keystroke dismisses it. Never send Esc here (Esc on an
    # empty prompt exits the TUI).
    if m not in ("Opus5", "❯"):
        t, p, m = wait_for(["Opus5", "❯"], tries=8)
    return t, p, m


def phase_tui(cfg, prov_ip):
    cmd = claude_container(cfg, prov_ip, "exec claude")
    child = pexpect.spawn(cmd[0], cmd[1:], encoding="utf-8", codec_errors="replace",
                          timeout=300)
    child.setwinsize(50, 160)
    try:
        child.logfile_read = common.CastRecorder(cfg.outdir, "tui")
    except Exception:
        pass
    screen = make_screen(child)
    wait_for = make_waiter(screen)

    def snap(tag, t):
        with open(f"{cfg.outdir}/{tag}.raw", "w") as f:
            f.write(t)

    t, p, m = drive_onboarding(child)
    snap("01_prompt", t)
    joined = p.replace(" ", "")
    check("tui-reached-prompt", "Opus5" in joined and "❯" in p,
          "claude TUI reached its prompt" if "Opus5" in joined else p[-120:].replace("\n", " "))
    check("synthetic-credential", "APIUsageBilling" in joined,
          "billing row shows API usage (API key mode)")

    # /plugin
    for ch in "/plugin":
        child.send(ch)
        time.sleep(0.06)
    child.send("\r")
    t, p, m = wait_for(["Installed", "Plugins"], tries=8)
    snap("02_plugin", t)
    check("plugin-surface-in-tui", "Plugins" in p and "Installed" in p,
          "/plugin opens the plugins surface")
    child.send("\x1b")
    time.sleep(1.0)

    # /agents is Claude's native custom-subagent manager. This proves the
    # harness consumes the canonical Agent, not merely that UZE wrote a path.
    for ch in "/agents":
        child.send(ch)
        time.sleep(0.06)
    child.send("\r")
    t, p, _ = wait_for(["reviewer", "Agents"], tries=10)
    snap("02a_agents", t)
    check("agent-visible-in-tui", "reviewer" in p,
          "Claude /agents lists the UZE reviewer agent" if "reviewer" in p else p[-200:].replace("\n", " "))
    child.send("\x1b")
    time.sleep(1.0)

    # /mcp
    for ch in "/mcp":
        child.send(ch)
        time.sleep(0.06)
    child.send("\r")
    t, p, m = wait_for(["connected", "tool"], tries=10)
    snap("02b_mcp", t)
    joined = p.replace(" ", "")
    check("mcp-server-connected-in-tui", "connected" in joined and "1tool" in joined,
          "/mcp shows the UZE MCP server connected with 1 tool")
    child.send("\x1b")
    time.sleep(1.0)

    # deterministic turn
    for ch in "hi":
        child.send(ch)
        time.sleep(0.07)
    child.send("\r")
    t3, p3, _ = wait_for(["UZE_CONFORMANCE_OK"], tries=20, gap=2.5)
    snap("03_after_prompt", t3)
    check("deterministic-response-rendered", "UZE_CONFORMANCE_OK" in p3,
          "UZE_CONFORMANCE_OK rendered in TUI" if "UZE_CONFORMANCE_OK" in p3
          else p3[-160:].replace("\n", " "))

    struct = provider_struct(cfg)
    with open(f"{cfg.outdir}/04_provider_struct.json", "w") as f:
        json.dump(struct, f, indent=1)
    if struct:
        # The model-facing contract is the PRIMARY request (the one carrying
        # the Skill tool). Auxiliary no-tools calls (title/context) may
        # include the full skill listing — a documented secondary leak, never
        # the primary contract.
        primary = [r for r in struct if "Skill" in r.get("summary", {}).get("tools", [])]
        base = primary or struct
        markers = {}
        for r in base:
            markers.update(r.get("summary", {}).get("skill_markers", {}))
        check("model-visible-skill-present",
              any(markers.get(m) for m in ("flow:commit", "commit")),
              "flow:commit in the primary request claude sent to its provider")
        check("user-only-skill-hidden",
              not any(markers.get(m) for m in ("flow:review", "Review code")),
              "flow:review absent from the primary model request (disable-model-invocation preserved)")
        check("provider-request-captured",
              any(r.get("summary", {}).get("tools") for r in struct),
              "request body structurally recorded (tools/skill markers)")
    else:
        check("model-visible-skill-present", False, "no provider request captured")
        check("provider-request-captured", False, "provider never contacted")

    # MCP execution inside the conversation: PARTIAL — claude defers MCP tools
    # behind ToolSearch (deferred-tool protocol); a direct mcp__ tool_use fails
    # with "No such tool available". Registration + connection are proven via
    # the /mcp TUI surface. Documented, never a pass.
    child.send("\x03")
    time.sleep(0.6)
    child.send("\x03")
    time.sleep(0.6)
    child.close(force=True)


def run(cfg, prov_ip):
    phase_tui(cfg, prov_ip)
