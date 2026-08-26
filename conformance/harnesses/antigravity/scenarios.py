#!/usr/bin/env python3
import subprocess
import json
"""Antigravity scenario (latest channel) — Real Harness + Synthetic World.

Phase A (TUI): prompt + synthetic credential, /skills (flow:commit,
workflow:review, uze:init), /mcp (server listed + tools enumerated),
deterministic turn, model-visible skill present, user-only skill
CAPABILITY_ADAPTED (no vendor explicit-only mechanism), MCP tool invocation
inside the interactive TUI (proof round-trip).

Phase B (CLI/state): plugin registration via `agy plugin list` + staged
mcp_config.json (AGY has no plugin TUI surface — verified).
"""
import time

import pexpect

import os
import sys
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", ".."))
import shared.common as common
from shared.common import check, docker_base, make_screen, make_waiter, provider_struct, start_provider


def agy_setup(cfg, prov_ip, include_mcp, final_cmd):
    mcp_block = ""
    plugins = "flow workflow"
    if include_mcp:
        mcp_block = f"""
cp -r /opt/uze-fixtures/tests-fixtures/canonical/mcp-plugin /work/market/plugins/mcp-plugin
printf '%s' '{{"mcpServers": {{"uze-conformance": {{"command": "{cfg.mcp_fixture_bin}", "args": ["--proof", "{cfg.mcp_proof}"]}}}}}}' > /work/market/plugins/mcp-plugin/mcp.json
"""
        plugins += " mcp-plugin"
    return f"""
set -e
export PATH=/usr/local/bin:/usr/bin:/bin:/usr/local/.local/bin
export HOME=/work/home UZE_HOME=/work/home/.uze
export GEMINI_API_KEY=uze-conformance-invalid-by-design
export GOOGLE_GEMINI_BASE_URL=http://{prov_ip}:9999
export AGY_CLI_DISABLE_AUTO_UPDATE=1
mkdir -p /work/home/.gemini/antigravity-cli /work/market/plugins
cp /app/fixtures/settings.json /work/home/.gemini/antigravity-cli/settings.json
cp /app/fixtures/jetski_state.pbtxt /work/home/.gemini/antigravity-cli/jetski_state.pbtxt
cp /app/fixtures/installation_id /work/home/.gemini/antigravity-cli/installation_id
cp -r /opt/uze-fixtures/tests-fixtures/canonical/flow /work/market/plugins/flow
cp -r /opt/uze-fixtures/tests-fixtures/canonical/workflow /work/market/plugins/workflow
{mcp_block}
printf '%s' '{{"name":"uze-lab","description":"lab","plugins":[{{"name":"flow","source":"./plugins/flow"}},{{"name":"workflow","source":"./plugins/workflow"}}{",{\"name\":\"mcp-plugin\",\"source\":\"./plugins/mcp-plugin\"}" if include_mcp else ""}]}}' > /work/market/agents.json
uze market add /work/market >/dev/null 2>&1
for p in {plugins}; do uze plugin install $p@uze-lab >/dev/null 2>&1; done
{final_cmd}
"""


def phase_tui(cfg, prov_ip):
    setup = agy_setup(cfg, prov_ip, include_mcp=True, final_cmd="exec agy")
    cmd = docker_base(cfg, prov_ip, setup)
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

    try:
        child.expect("Choose your color scheme", timeout=150)
    except Exception as e:
        check("tui-started", False, f"onboarding never appeared: {e}")
        child.close(force=True)
        return

    child.send("\r")          # color scheme
    time.sleep(3)
    child.send("\t\t")        # ToS -> Done
    time.sleep(0.7)
    child.send("\r")
    time.sleep(5)

    t1, p1 = screen(3)
    snap("01_prompt", t1)
    check("tui-reached-prompt", "Antigravity CLI" in p1 and ">" in p1,
          "header visible" if "Antigravity CLI" in p1 else "no header")
    check("synthetic-credential", "Gemini API key" in p1,
          "account row shows API key, not a personal account")

    # /skills
    child.send("/")
    time.sleep(1.2)
    for ch in "skills":
        child.send(ch)
        time.sleep(0.15)
    time.sleep(1.2)
    child.send("\r")
    t2, p2, _ = wait_for(["workflow:review"], tries=4)
    snap("02_skills", t2)
    check("uzek-skill-visible", "flow:commit" in p2, "flow:commit in /skills")
    check("useronly-skill-human-visible", "workflow:review" in p2,
          "workflow:review in global skills surface")
    check("official-uzek-skill-visible", "uze:init" in p2, "uze:init in /skills")
    child.send("\x1b")
    time.sleep(1.0)
    t_settle, _, _ = wait_for([">"], tries=6)
    snap("02c_back_to_prompt", t_settle)

    # /mcp
    child.send("/")
    time.sleep(1.2)
    for ch in "mcp":
        child.send(ch)
        time.sleep(0.15)
    time.sleep(1.2)
    child.send("\r")
    t_mcp, p_mcp, _ = wait_for(["Tools: uze_conformance"], tries=8)
    snap("02b_mcp", t_mcp)
    check("mcp-server-visible-in-tui", "uze-conformance" in p_mcp,
          "the UZE-delivered MCP server is listed in /mcp")
    check("mcp-server-connected-in-tui", "Tools: uze_conformance" in p_mcp,
          "the real AGY loaded the server and enumerated its tool")
    child.send("\x1b")
    time.sleep(1.0)
    t_settle, _, _ = wait_for([">"], tries=6)
    snap("02d_back_to_prompt", t_settle)

    # deterministic turn
    for ch in "hi":
        child.send(ch)
        time.sleep(0.1)
    child.send("\r")
    t3, p3, _ = wait_for(["UZE_CONFORMANCE_OK"], tries=12)
    snap("03_after_prompt", t3)
    check("deterministic-response-rendered", "UZE_CONFORMANCE_OK" in p3,
          "UZE_CONFORMANCE_OK rendered in TUI" if "UZE_CONFORMANCE_OK" in p3
          else p3[-160:].replace("\n", " "))
    check("agent-loop-clean", "Agent execution terminated due to error" not in p3,
          "no agent-loop error after the rendered turn")

    struct = provider_struct(cfg)
    with open(f"{cfg.outdir}/04_provider_struct.json", "w") as f:
        json.dump(struct, f, indent=1)
    if struct:
        summaries = [entry.get("summary", {}) for entry in struct]
        markers = [summary.get("skill_markers", {}) for summary in summaries]
        check("model-visible-skill-present",
              any(marker.get(name) for marker in markers for name in ("flow:commit", "commit")),
              "flow:commit in the request the harness sent to its provider")
        check("user-only-skill-adapted",
              any(marker.get(name) for marker in markers for name in ("workflow:review", "review")),
              "workflow:review present (no vendor explicit-only mechanism)",
              kind="adapted")
        check("provider-request-captured",
              any(summary.get("tools") for summary in summaries),
              "request body structurally recorded (tools/skills/markers)")
    else:
        check("model-visible-skill-present", False, "no provider request captured")
        check("provider-request-captured", False, "provider never contacted")

    # MCP invocation inside the interactive TUI conversation
    time.sleep(2)
    try:
        child.read_nonblocking(size=200000, timeout=3)
    except Exception:
        pass
    t_settle, _, _ = wait_for([">"], tries=8)
    snap("02e_settle", t_settle)
    start_provider(cfg, "toolcall")
    for ch in "call the uze_conformance mcp tool":
        child.send(ch)
        time.sleep(0.08)
    child.send("\r")
    t4, p4 = screen(1.2)
    tries = 0
    while "UZE_CONFORMANCE_PASS" not in p4 and tries < 14:
        t4, p4 = screen(2.0)
        tries += 1
    snap("03b_mcp_invoke_tui", t4)
    check("mcp-tool-invoked-via-tui", "UZE_CONFORMANCE_PASS" in p4 and
          "Agent execution terminated due to error" not in p4,
          "MCP tool call executed and final rendered in the interactive TUI"
          if "UZE_CONFORMANCE_PASS" in p4 else p4[-160:].replace("\n", " "))
    struct2 = provider_struct(cfg)
    with open(f"{cfg.outdir}/04b_mcp_invoke_struct.json", "w") as f:
        json.dump(struct2, f, indent=1)
    if struct2:
        s = struct2[-1].get("summary", {})
        check("mcp-tool-executed-in-tui",
              bool(s.get("has_function_response") and s.get("mcp_proof_present")),
              "the REAL AGY executed the MCP server inside the TUI turn (proof returned)")
    else:
        check("mcp-tool-executed-in-tui", False, "no MCP request captured")

    child.send("\x03")
    time.sleep(0.5)
    child.sendline("/exit")
    time.sleep(2)
    child.close(force=True)


def phase_mcp_registration(cfg, prov_ip):
    final = """
echo '===== S1 plugin list ====='
agy plugin list 2>&1
echo '===== S2 staged mcp_config.json ====='
cat /work/home/.gemini/config/plugins/uze-mcp-conformance/mcp_config.json 2>&1
"""
    setup = agy_setup(cfg, prov_ip, include_mcp=True, final_cmd=final)
    cmd = docker_base(cfg, prov_ip, setup, tty=False)
    proc = subprocess.run(cmd, capture_output=True, text=True)
    out = proc.stdout + proc.stderr
    with open(f"{cfg.outdir}/05_mcp_registration.txt", "w") as f:
        f.write(out)
    check("mcp-plugin-registered", '"mcpServers"' in out and 'uze-mcp-conformance' in out,
          "S1: plugin list shows the MCP plugin with an mcpServers component")
    check("mcp-server-configured",
          'uze-conformance' in out and cfg.mcp_proof in out and cfg.mcp_fixture_bin in out,
          "S2: staged mcp_config.json declares the server + proof arg")


def run(cfg, prov_ip):
    phase_tui(cfg, prov_ip)
    phase_mcp_registration(cfg, prov_ip)
