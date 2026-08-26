#!/usr/bin/env python3
"""OpenCode scenario (latest channel) — Real Harness + Synthetic World.

Phase A (TUI): the global opencode.json (custom provider + model + MCP)
makes the TUI boot straight to the prompt (no onboarding, observed);
/skills (flow:commit / flow:review / uze:init listed); /mcps (the UZE
MCP server connected + enabled); deterministic turn; provider-request
observation (model-visible Skill present; user-only skill visible to the
model — opencode lists every registered skill in the system prompt, so the
policy is ADAPTED); MCP tool invocation inside the interactive TUI (proof
round-trip: the real opencode executed the real MCP server and returned the
proof value).

OpenCode is the one harness whose provider is configurable: unlike
claude/codex there is no hardcoded host to intercept — the custom
`baseURL` in the config is the hook, plain HTTP.
"""
import json
import os
import re
import sys
import time

import pexpect

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", ".."))
import shared.common as common
from shared.common import (check, docker_base, make_screen, make_waiter,
                           materialize_marketplace, provider_struct)


def opencode_setup(cfg, prov_ip, final_cmd):
    return f"""
set -e
export PATH=/usr/local/bin:/usr/bin:/bin:/usr/local/.local/bin:/usr/local/.opencode/bin
export HOME=/work/home UZE_HOME=/work/home/.uze
export OPENCODE_DISABLE_MODELS_FETCH=1
export UZE_CONFORMANCE_KEY=dummy
mkdir -p /work/home/.config/opencode /work/home/.agents
{materialize_marketplace(cfg)}
uze market add /work/market >/dev/null 2>&1
for p in flow mcp-plugin; do uze plugin install $p@uze-lab >/dev/null 2>&1; done
node -e '
const fs=require("fs");
const p="/work/home/.config/opencode/opencode.json";
const d=JSON.parse(fs.readFileSync(p,"utf8"));
d.providers={{"uze-conformance":{{"name":"UZE Conformance","env":["UZE_CONFORMANCE_KEY"],"package":"@opencode-ai/ai/providers/openai-compatible","settings":{{"baseURL":"http://{prov_ip}:9999/v1","apiKey":"{{env:UZE_CONFORMANCE_KEY}}"}},"models":{{"uze-model":{{"modelID":"uze-model","name":"UZE Conformance Model"}}}}}}}};
d.model="uze-conformance/uze-model";
d.agents={{"build":{{"model":"uze-conformance/uze-model"}}}};
fs.writeFileSync(p, JSON.stringify(d,null,1));
'
{final_cmd}
"""


def opencode_container(cfg, prov_ip, final_cmd):
    cmd = docker_base(cfg, prov_ip, opencode_setup(cfg, prov_ip, final_cmd))
    return cmd


def phase_tui(cfg, prov_ip):
    cmd = opencode_container(
        cfg,
        prov_ip,
        "UZE_HOME=/usr/local/.uze PATH=/usr/local/.uze/shims:$PATH exec opencode --standalone",
    )
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

    t, p, m = wait_for(["Ask anything"], tries=16)
    snap("01_prompt", t)
    check("tui-reached-prompt", "Ask anything" in p,
          "opencode TUI reached its prompt (no onboarding needed)" if "Ask anything" in p
          else p[-120:].replace("\n", " "))
    # The prompt renders long before the skills/MCP state finishes loading
    # (observed); typing into the palette too early loses input. The status
    # row "1 MCP" also renders early — what matters is a fixed warmup after
    # the prompt (25s matched the working manual probe), then interact.
    time.sleep(25)

    # /skills — wait for the list to load (the header renders before the
    # entries; the surface fills in async, observed)
    for ch in "/skills":
        child.send(ch)
        time.sleep(0.08)
    time.sleep(1)
    child.send("\r")
    # the surface renders by region (repaint frames split names across
    # reads); accumulate several reads and match markers that survive
    # frame splits
    accumulated = ""
    for _ in range(8):
        time.sleep(1.5)
        try:
            accumulated += child.read_nonblocking(size=400000, timeout=3)
        except Exception:
            break
    t = accumulated
    p = re.sub(r"\x1b\][^\x07]*\x07", "", t)
    p = re.sub(r"\x1b\[[0-9;?]*[A-Za-z]", "", p).replace("\x1b", "")
    snap("02_skills", t)
    joined = p.replace(" ", "")
    check("skills-surface-in-tui", "Skills" in p,
          "/skills opens the skill management surface")
    qualified_skills = ("flow:commit", "flow:review", "uze:init")
    check("qualified-uze-skills-visible",
          all(skill in joined for skill in qualified_skills),
          "the /skills list shows each UZE skill by its qualified invocation label"
          if all(skill in joined for skill in qualified_skills)
          else p[-240:].replace("\n", " "))
    child.send("\x1b")
    time.sleep(1.0)
    child.send("\x1b")
    time.sleep(1.0)

    # /mcps (trailing s — the MCP toggle surface)
    for ch in "/mcps":
        child.send(ch)
        time.sleep(0.08)
    time.sleep(1)
    child.send("\r")
    accumulated = ""
    for _ in range(8):
        time.sleep(1.5)
        try:
            accumulated += child.read_nonblocking(size=400000, timeout=3)
        except Exception:
            break
    t = accumulated
    p = re.sub(r"\x1b\][^\x07]*\x07", "", t)
    p = re.sub(r"\x1b\[[0-9;?]*[A-Za-z]", "", p).replace("\x1b", "")
    snap("02b_mcp", t)
    joined = p.replace(" ", "")
    check("mcp-surface-in-tui", "MCPs" in p or "mcps" in joined,
          "/mcps opens the MCP toggle surface")
    check("mcp-server-connected-in-tui",
          ("Connected" in p and "✓" in p) or "disconnectspace" in joined,
          "the /mcps surface shows uze-conformance connected + enabled" if (
              "Connected" in p or "disconnectspace" in joined)
          else p[-120:].replace("\n", " "))
    child.send("\x1b")
    time.sleep(1.0)
    child.send("\x1b")
    time.sleep(1.0)

    # deterministic turn
    for ch in "hi":
        child.send(ch)
        time.sleep(0.08)
    time.sleep(1)
    child.send("\r")
    t3, p3, _ = wait_for(["UZE_CONFORMANCE_OK"], tries=20, gap=2.5)
    snap("03_after_prompt", t3)
    check("deterministic-response-rendered", "UZE_CONFORMANCE_OK" in p3,
          "UZE_CONFORMANCE_OK rendered in TUI" if "UZE_CONFORMANCE_OK" in p3
          else p3[-160:].replace("\n", " "))

    # model-facing observation (structural)
    struct = provider_struct(cfg)
    with open(f"{cfg.outdir}/04_provider_struct.json", "w") as f:
        json.dump(struct, f, indent=1)
    if struct:
        markers = {}
        has_catalog = False
        for r in struct:
            s = r.get("summary", {})
            markers.update(s.get("skill_markers", {}))
            has_catalog = has_catalog or bool(s.get("has_available_skills"))
        check("provider-request-captured", bool(struct),
              "requests structurally recorded")
        check("skills-instructions-in-request", bool(has_catalog),
              "the model request carries the skills catalog section")
        check("model-visible-skill-present",
              markers.get("flow:commit", False),
              "flow:commit present in the primary request opencode sent")
        check("user-only-skill-hidden-from-model",
              not markers.get("flow:review", False),
              "flow:review is omitted from model-facing skill discovery")
        check("model-only-skill-present",
              markers.get("flow:analyze", False),
              "flow:analyze is present in model-facing skill discovery")
    else:
        check("provider-request-captured", False, "no provider request captured")

    child.send("\x03")
    time.sleep(0.5)
    child.send("\x03")
    time.sleep(0.5)
    child.close(force=True)


def phase_mcp_toolcall(cfg, prov_ip):
    """MCP tool invocation inside the interactive TUI: restart the provider
    in toolcall mode and drive a fresh TUI turn; the real opencode executes
    the real MCP server and the proof value returns through the follow-up
    provider request, rendered as UZE_CONFORMANCE_PASS."""
    common.start_provider(cfg, "toolcall")
    time.sleep(1)
    cmd = opencode_container(
        cfg,
        prov_ip,
        "UZE_HOME=/usr/local/.uze PATH=/usr/local/.uze/shims:$PATH exec opencode --standalone",
    )
    child = pexpect.spawn(cmd[0], cmd[1:], encoding="utf-8", codec_errors="replace",
                          timeout=300)
    child.setwinsize(50, 160)
    try:
        child.logfile_read = common.CastRecorder(cfg.outdir, "tui")
    except Exception:
        pass
    screen = make_screen(child)
    wait_for = make_waiter(screen)
    t, p, m = wait_for(["Ask anything"], tries=16)
    time.sleep(2)

    for ch in "use the uze_conformance mcp tool":
        child.send(ch)
        time.sleep(0.04)
    time.sleep(1)
    child.send("\r")
    t3, p3, _ = wait_for(["UZE_CONFORMANCE_PASS"], tries=24, gap=2.5)
    with open(f"{cfg.outdir}/05_mcp_toolcall.raw", "w") as f:
        f.write(t3)
    check("mcp-tool-invoked-via-tui", "UZE_CONFORMANCE_PASS" in p3,
          "UZE_CONFORMANCE_PASS rendered in the TUI after the MCP round-trip"
          if "UZE_CONFORMANCE_PASS" in p3 else p3[-160:].replace("\n", " "))

    struct = provider_struct(cfg)
    with open(f"{cfg.outdir}/06_provider_struct_toolcall.json", "w") as f:
        json.dump(struct, f, indent=1)
    has_result = any(r.get("summary", {}).get("has_tool_result") for r in struct)
    proof = any(r.get("summary", {}).get("mcp_proof_present") for r in struct)
    model_exposed = any(r.get("summary", {}).get("mcp_tool_present") for r in struct)
    check("mcp-tool-model-exposed", model_exposed,
          "the UZE MCP tool is exposed in the model request")
    check("mcp-tool-executed-in-tui", has_result,
          "the REAL opencode returned an MCP tool result inside the TUI turn")

    child.send("\x03")
    time.sleep(0.5)
    child.send("\x03")
    time.sleep(0.5)
    child.close(force=True)


def run(cfg, prov_ip):
    phase_tui(cfg, prov_ip)
    phase_mcp_toolcall(cfg, prov_ip)
