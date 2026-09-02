#!/usr/bin/env python3
import json
import subprocess

"""Antigravity scenario (latest channel) — Real Harness + Synthetic World.

Phase A (TUI): prompt + synthetic credential, /skills (flow:commit,
flow:review, uze:init), /mcp (server listed + tools enumerated),
deterministic turn, model-only Skill hidden from the slash surface but
present for the model, user-only Skill CAPABILITY_ADAPTED, MCP tool invocation
inside the interactive TUI (proof round-trip).

Phase B (CLI/state): plugin registration via `agy plugin list` + staged
mcp_config.json (AGY has no plugin TUI surface — verified).
"""
import os
import sys
import time

import pexpect

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", ".."))
import shared.common as common
from shared.common import (
    check,
    describe,
    docker_base,
    make_screen,
    make_waiter,
    materialize_marketplace,
    provider_struct,
    start_provider,
)


def agy_setup(cfg, prov_ip, include_mcp, final_cmd, plugins=None, prelude=""):
    if plugins is None:
        plugins = "flow"
        if include_mcp:
            plugins += " mcp-plugin"
    return f"""
set -e
export PATH=/usr/local/bin:/usr/bin:/bin:/usr/local/.local/bin
export HOME=/work/home UZE_HOME=/work/home/.uze
export GEMINI_API_KEY=uze-conformance-invalid-by-design
export GOOGLE_GEMINI_BASE_URL=http://{prov_ip}:9999
export AGY_CLI_DISABLE_AUTO_UPDATE=1
# The harness is Go and honours SSL_CERT_FILE on Linux: this is how it
# trusts the run's synthetic CA for its own control plane (feature flags,
# account endpoints) without any Internet.
export SSL_CERT_FILE=/app/ca.crt
export SSL_CERT_DIR=/app
mkdir -p /work/home/.gemini/antigravity-cli
cp /app/fixtures/settings.json /work/home/.gemini/antigravity-cli/settings.json
cp /app/fixtures/jetski_state.pbtxt /work/home/.gemini/antigravity-cli/jetski_state.pbtxt
cp /app/fixtures/installation_id /work/home/.gemini/antigravity-cli/installation_id
{materialize_marketplace(cfg)}
uze market add /work/market >/dev/null 2>&1
for p in {plugins}; do uze plugin install $p@uze-lab >/dev/null 2>&1; done
{prelude}
{final_cmd}
"""


def phase_tui(cfg, prov_ip):
    setup = agy_setup(cfg, prov_ip, include_mcp=True, final_cmd="exec agy")
    cmd = docker_base(cfg, prov_ip, setup)
    child = pexpect.spawn(
        cmd[0], cmd[1:], encoding="utf-8", codec_errors="replace", timeout=300
    )
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

    child.send("\r")  # color scheme
    time.sleep(3)
    child.send("\t\t")  # ToS -> Done
    time.sleep(0.7)
    child.send("\r")
    time.sleep(5)

    t1, p1 = screen(3)
    snap("01_prompt", t1)
    check(
        "tui-reached-prompt",
        "Antigravity CLI" in p1 and ">" in p1,
        "header visible" if "Antigravity CLI" in p1 else "no header",
    )
    check(
        "synthetic-credential",
        "Gemini API key" in p1,
        "account row shows API key, not a personal account",
    )

    # /skills
    child.send("/")
    time.sleep(1.2)
    for ch in "skills":
        child.send(ch)
        time.sleep(0.15)
    time.sleep(1.2)
    child.send("\r")
    t2, p2, _ = wait_for(["flow:review"], tries=4, stop_on_death=True)
    snap("02_skills", t2)
    check("official-uzek-skill-visible", "uze:init" in p2, "uze:init in /skills")
    child.send("\x1b")
    time.sleep(1.0)
    t_settle, _, _ = wait_for([">"], tries=6, stop_on_death=True)
    snap("02c_back_to_prompt", t_settle)

    # /agents is the native agent manager; fixture visibility is the
    # behavioral proof that AGY loaded UZE's portable definition.
    child.send("/")
    time.sleep(1.2)
    for ch in "agents":
        child.send(ch)
        time.sleep(0.15)
    child.send("\r")
    t_agents, p_agents, _ = wait_for(
        ["reviewer", "Agents"], tries=8, stop_on_death=True
    )
    snap("02a_agents", t_agents)
    check(
        "agent-visible-in-tui",
        "reviewer" in p_agents,
        "Antigravity /agents lists the UZE reviewer agent"
        if "reviewer" in p_agents
        else p_agents[-200:].replace("\n", " "),
    )
    child.send("\x1b")
    time.sleep(1.0)

    # /mcp
    child.send("/")
    time.sleep(1.2)
    for ch in "mcp":
        child.send(ch)
        time.sleep(0.15)
    time.sleep(1.2)
    child.send("\r")
    t_mcp, p_mcp, _ = wait_for(["Tools: uze_conformance"], tries=8, stop_on_death=True)
    snap("02b_mcp", t_mcp)
    check(
        "mcp-server-visible-in-tui",
        "uze-conformance" in p_mcp,
        "the UZE-delivered MCP server is listed in /mcp",
    )
    check(
        "mcp-server-connected-in-tui",
        "Tools: uze_conformance" in p_mcp,
        "the real AGY loaded the server and enumerated its tool",
    )
    child.send("\x1b")
    time.sleep(1.0)
    t_settle, _, _ = wait_for([">"], tries=6, stop_on_death=True)
    snap("02d_back_to_prompt", t_settle)

    # deterministic turn
    for ch in "hi":
        child.send(ch)
        time.sleep(0.1)
    child.send("\r")
    t3, p3, _ = wait_for(["UZE_CONFORMANCE_OK"], tries=30, gap=2.5, stop_on_death=True)
    snap("03_after_prompt", t3)
    check(
        "deterministic-response-rendered",
        "UZE_CONFORMANCE_OK" in p3,
        "UZE_CONFORMANCE_OK rendered in TUI"
        if "UZE_CONFORMANCE_OK" in p3
        else p3[-160:].replace("\n", " "),
    )
    check(
        "agent-loop-clean",
        "Agent execution terminated due to error" not in p3,
        "no agent-loop error after the rendered turn",
    )

    struct = provider_struct(cfg)
    with open(f"{cfg.outdir}/04_provider_struct.json", "w") as f:
        json.dump(struct, f, indent=1)
    if struct:
        summaries = [entry.get("summary", {}) for entry in struct]
        markers = [summary.get("skill_markers", {}) for summary in summaries]
        check(
            "model-visible-skill-present",
            any(
                marker.get(name)
                for marker in markers
                for name in ("flow:commit", "commit")
            ),
            "flow:commit in the request the harness sent to its provider",
        )
        check(
            "user-only-skill-adapted",
            any(
                marker.get(name)
                for marker in markers
                for name in ("flow:review", "review")
            ),
            "flow:review present (no vendor explicit-only mechanism)",
            kind="adapted",
        )
        check(
            "model-only-skill-present",
            any(
                marker.get(name)
                for marker in markers
                for name in ("flow:analyze", "analyze")
            ),
            "flow:analyze present in the request while absent from /skills",
        )
        check(
            "provider-request-captured",
            any(summary.get("tools") for summary in summaries),
            "request body structurally recorded (tools/skills/markers)",
        )
    else:
        check("model-visible-skill-present", False, "no provider request captured")
        check("provider-request-captured", False, "provider never contacted")

    # MCP invocation inside the interactive TUI conversation
    time.sleep(2)
    try:
        child.read_nonblocking(size=200000, timeout=3)
    except Exception:
        pass
    t_settle, _, _ = wait_for([">"], tries=8, stop_on_death=True)
    snap("02e_settle", t_settle)
    start_provider(cfg, "toolcall")
    for ch in "call the uze_conformance mcp tool":
        child.send(ch)
        time.sleep(0.08)
    child.send("\r")
    t4, p4 = screen(1.2)
    tries = 0
    while "UZE_CONFORMANCE_PASS" not in p4 and tries < 14 and child.isalive():
        t4, p4 = screen(2.0)
        tries += 1
    snap("03b_mcp_invoke_tui", t4)
    check(
        "mcp-tool-invoked-via-tui",
        "UZE_CONFORMANCE_PASS" in p4
        and "Agent execution terminated due to error" not in p4,
        "MCP tool call executed and final rendered in the interactive TUI"
        if "UZE_CONFORMANCE_PASS" in p4
        else p4[-160:].replace("\n", " "),
    )
    struct2 = provider_struct(cfg)
    with open(f"{cfg.outdir}/04b_mcp_invoke_struct.json", "w") as f:
        json.dump(struct2, f, indent=1)
    if struct2:
        # The proof rides in the request that carries the functionResponse;
        # the harness's side requests (a lighter model, no tools) come
        # after it, so the last request is not the one to read.
        executed = any(
            r.get("summary", {}).get("has_function_response")
            and r.get("summary", {}).get("mcp_proof_present")
            for r in struct2
        )
        check(
            "mcp-tool-executed-in-tui",
            executed,
            "the REAL AGY executed the MCP server inside the TUI turn (proof returned)"
            if executed
            else "a functionResponse without the proof, or none at all",
        )
    else:
        check("mcp-tool-executed-in-tui", False, "no MCP request captured")

    child.send("\x03")
    time.sleep(0.5)
    child.sendline("/exit")
    time.sleep(2)
    child.close(force=True)


RUN_COMMAND_ARGS = (
    '{"CommandLine":"%s","Cwd":"/work","WaitMsBeforeAsync":2000,'
    '"toolSummary":"Command execution","toolAction":"Running command"}'
)

#: A deny hook in the vendor's own file format at the vendor's own shared
#: path, with no UZE in the loop: the control that says whether this AGY
#: executes `hooks.json` hooks at all in this session.
VENDOR_CONTROL_HOOK = """cat > /work/home/.gemini/config/hooks.json <<'EOF'
{
  "uze-conformance-control": {
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

HOOK_DENIAL_MARKERS = ("blocked by protect-env", "Denied by UZE hook")


def hook_turn(cfg, prov_ip, tag, args, plugins, prelude=""):
    """One interactive AGY turn around a scripted `run_command` (in the
    tool's declared argument shape): the provider serves the functionCall
    to the user's turn, the vendor permission prompt is answered if it
    appears, and the turn is left settled. Returns the TUI verdict and the
    provider-observed hook markers."""
    start_provider(cfg, "toolcall", {"TOOL_NAME": "run_command", "FC_ARGS": args})
    time.sleep(1)
    setup = agy_setup(
        cfg,
        prov_ip,
        include_mcp=False,
        final_cmd="exec agy",
        plugins=plugins,
        prelude=prelude,
    )
    cmd = docker_base(cfg, prov_ip, setup)
    child = pexpect.spawn(
        cmd[0], cmd[1:], encoding="utf-8", codec_errors="replace", timeout=300
    )
    child.setwinsize(50, 160)
    try:
        child.logfile_read = common.CastRecorder(cfg.outdir, f"tui-hooks-{tag}")
    except Exception:
        pass
    screen = make_screen(child)
    wait_for = make_waiter(screen)

    try:
        child.expect("Choose your color scheme", timeout=150)
    except Exception as e:
        check(f"hooks-{tag}-tui-started", False, f"onboarding never appeared: {e}")
        child.close(force=True)
        return None
    child.send("\r")
    time.sleep(3)
    child.send("\t\t")
    time.sleep(0.7)
    child.send("\r")
    time.sleep(5)
    _, p1 = screen(3)
    if ">" not in p1:
        wait_for([">"], tries=6, stop_on_death=True)

    for ch in "run the API check":
        child.send(ch)
        time.sleep(0.08)
    child.send("\r")
    seen = ""
    prompted = False
    for _ in range(16):
        _, p = screen(2.0)
        seen += p
        if "UZE_CONFORMANCE_PASS" in seen or any(
            m in seen for m in HOOK_DENIAL_MARKERS
        ):
            break
        if not child.isalive():
            break
        # The vendor's own permission prompt for the command: a person
        # approves it, and the hook decision — never this prompt — is what
        # the turn is judged on. A deny hook that ran never shows it.
        if "Do you want to proceed" in seen and not prompted:
            prompted = True
            child.send("\r")
            time.sleep(1.0)
        elif "How's the CLI experience" in p:
            child.send("0\r")
            time.sleep(1.0)
    with open(f"{cfg.outdir}/hooks_{tag}.raw", "w") as f:
        f.write(seen)
    turn_settled = "UZE_CONFORMANCE_PASS" in seen or any(
        m in seen for m in HOOK_DENIAL_MARKERS
    )
    # Absence checks may only evaluate once the turn settled and the TUI
    # went quiet (ADR-035).
    settled = turn_settled and common.settle_and_quiet(screen)

    struct = provider_struct(cfg)
    with open(f"{cfg.outdir}/hooks_{tag}_struct.json", "w") as f:
        json.dump(struct, f, indent=1)
    child.send("\x03")
    time.sleep(0.5)
    child.close(force=True)
    return {
        "turn_settled": turn_settled,
        "settled": settled,
        "prompted": prompted,
        "tail": seen[-160:].replace("\n", " "),
        "markers": common.observed_markers(struct, "hook_markers"),
    }


def phase_hooks_gate(cfg, prov_ip):
    """Whether this AGY executes `hooks.json` hooks at all — measured, not
    assumed, with the vendor's own format at the vendor's own path and no
    UZE in the loop.

    Observed on 1.1.22 and 1.1.24 (2026-09-02, experiments
    `antigravity/hook-print` and `antigravity/hook-tui`): the hook is
    loaded (`hooks_manager: loaded 1 named hooks`), listed by `/hooks`, and
    never executed — not for any event, not even a `touch`.

    The gate's flag is now served by the Lab and consumed by the harness
    (`json-hooks-enabled`, from the provider's Unleash listener), and
    dropping its `ide IN [jetski]` constraint changes nothing. What is
    missing is one layer further in: the executor reads `enable_json_hooks`,
    field 17 of `exa.cortex_pb.CustomizationConfig`, which arrives only over
    the CloudCode `v1internal` backend the CLI speaks when signed in to a
    Google account. This vertical runs it on a Gemini API key, so no such
    config ever arrives, whatever the flag says.

    The vendor has the same bug open:
    google-antigravity/antigravity-cli#893, "Hooks from .agents/hooks.json
    are loaded but never executed when authenticated via GEMINI_API_KEY"
    (bug, comp:auth, comp:customizations, assigned, open 2026-08-28) —
    identical symptom, and OAuth mode executes the same hook. Confirmed on
    the host (2026-09-02, agy 1.1.22 online): one global hook, OAuth ->
    PreInvocation fires; GEMINI_API_KEY -> loaded, never fires. #78 records
    that Google does not support the Gemini API key path at all, which is
    the mode this vertical runs in.

    While that gate is closed, the UZE hook checks are declared, not
    asserted: a green there would be the harness's, not ours to fake.
    Returns True when the control hook denied the command.
    """
    outcome = hook_turn(
        cfg,
        prov_ip,
        "vendor",
        RUN_COMMAND_ARGS % "echo API secrets",
        plugins="flow",
        prelude=VENDOR_CONTROL_HOOK,
    )
    if outcome is None:
        return False
    executes = bool(outcome["markers"].get("blocked by protect-env"))
    # A closed gate is a declared vendor limitation (ADAPTED, registered
    # per version), never a failure of UZE's — and never a silent pass.
    check(
        "hooks-vendor-hook-executes",
        True,
        "a vendor-format deny hook at ~/.gemini/config/hooks.json denied run_command"
        if executes
        else (
            "no hooks.json hook executes in this AGY session: the vendor-format "
            "control hook was loaded and listed but never ran"
            + (
                " (the permission prompt surfaced instead)"
                if outcome["prompted"]
                else ""
            )
        ),
        kind="assert" if executes else "adapted",
    )
    return executes


def phase_hooks(cfg, prov_ip, kind, vendor_executes):
    """Portable-hook evidence inside the REAL Antigravity CLI TUI (ADR-033).

    The provider scripts a `run_command` functionCall whose arguments the
    plugin's `guard` handler examines (delivered through UZE's generated
    named-entry plugin); `kind` selects the scenario, identical semantics to
    the claude/codex/opencode verticals:

      deny  : arguments contain `secrets` -> the hook denies (reason
              "blocked by protect-env") and the second handler never runs;
              run_command itself never executes.
      allow : plain echo arguments -> the hook allows, run_command runs.
      order : a two-handler group whose first handler always denies -> the
              second handler's marker must never appear (first-deny-wins).

    Evidence = what the REAL harness relayed: hook marker presence/absence
    in the provider-observed conversation plus the TUI denial surface.

    Two vacuities hid here until 2026-09-02, and both are why the presence
    check `hooks-*-denial-relayed` gates the absence checks: the provider
    answered the harness's first request with the functionCall, which on
    1.1.24 is a side call to a lighter model with no tools declared, so the
    user's turn never saw a tool; and the call carried `command`, not the
    `CommandLine`/`Cwd`/`WaitMsBeforeAsync`/`toolSummary`/`toolAction` the
    tool declares, which the harness rejects as invalid arguments before a
    hook runs. The generated plugin `hooks.json` also had its named entries
    wrapped under a `hooks` key the vendor reads as one dead hook.

    When `phase_hooks_gate` found the vendor executing no hook at all, every
    check here is recorded as a declaration carrying that reason — the
    turn would only re-measure the vendor's gate.
    """
    scenarios = {
        "deny": {
            "plugin": "hook-plugin",
            "args": RUN_COMMAND_ARGS % "echo API secrets",
            "deny_present": "blocked by protect-env",
            "deny_absent": ["second-handler-reached"],
        },
        "allow": {
            "plugin": "hook-allow-plugin",
            "args": RUN_COMMAND_ARGS % "echo plain output",
            "deny_present": None,
            "deny_absent": ["blocked by protect-env"],
        },
        "order": {
            "plugin": "hook-order-plugin",
            "args": RUN_COMMAND_ARGS % "echo any",
            "deny_present": "first-handler-denied",
            "deny_absent": ["second-handler-ran"],
        },
    }
    spec = scenarios[kind]
    if not vendor_executes:
        reason = (
            "declared: this AGY executes no hooks.json hook in the Lab session "
            "(see hooks-vendor-hook-executes), so the UZE plugin hook cannot be "
            "observed here"
        )
        declared = []
        if spec["deny_present"]:
            declared += [
                f"hooks-{kind}-denial-relayed",
                f"hooks-{kind}-denial-blocks-tool",
            ]
        declared += [
            f"hooks-{kind}-marker-absent-{absent}" for absent in spec["deny_absent"]
        ]
        if kind == "allow":
            declared.append("hooks-allow-tool-executed")
        for name in declared:
            check(name, True, reason, kind="adapted")
        return

    outcome = hook_turn(
        cfg, prov_ip, kind, spec["args"], plugins=f"flow {spec['plugin']}"
    )
    if outcome is None:
        return
    check(
        f"hooks-{kind}-turn-settled",
        outcome["turn_settled"],
        "the turn settled (final text or hook denial rendered)"
        if outcome["turn_settled"]
        else outcome["tail"],
    )
    markers = outcome["markers"]
    settled = outcome["settled"]
    has_output = bool(markers.get("plain output"))
    if spec["deny_present"]:
        # The denial reason relayed to the model is the evidence that the
        # hook ran and AGY honored it. Without it the absence checks below
        # hold for a turn where no hook ran at all.
        relayed = bool(markers.get(spec["deny_present"]))
        check(
            f"hooks-{kind}-denial-relayed",
            relayed,
            f"`{spec['deny_present']}` reached the conversation as the tool outcome"
            if relayed
            else ", ".join(f"{m}={markers.get(m)}" for m in sorted(markers)),
        )
        common.check_absence(
            f"hooks-{kind}-denial-blocks-tool",
            relayed and not has_output,
            settled,
            "the intercepted tool never executed — the native denial blocked it"
            if not has_output
            else "the tool executed despite the deny — blocking is broken",
        )
    for absent in spec["deny_absent"]:
        common.check_absence(
            f"hooks-{kind}-marker-absent-{absent}",
            not markers.get(absent, False),
            settled,
            f"`{absent}` never reached the conversation (first-deny-wins)",
        )
    if kind == "allow":
        check(
            "hooks-allow-tool-executed",
            has_output,
            "run_command actually executed after the hook allowed it"
            if has_output
            else "the command's stdout never reached the conversation",
        )


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
    check(
        "mcp-plugin-registered",
        '"mcpServers"' in out and "uze-mcp-conformance" in out,
        "S1: plugin list shows the MCP plugin with an mcpServers component",
    )
    check(
        "mcp-server-configured",
        "uze-conformance" in out
        and cfg.mcp_proof in out
        and cfg.mcp_fixture_bin in out,
        "S2: staged mcp_config.json declares the server + proof arg",
    )


def run(cfg, prov_ip):
    with describe("tui"):
        phase_tui(cfg, prov_ip)
    with describe("cli.state"):
        phase_mcp_registration(cfg, prov_ip)
    with describe("hooks"):
        with describe("vendor"):
            vendor_executes = phase_hooks_gate(cfg, prov_ip)
        for kind in ("deny", "allow", "order"):
            with describe(kind):
                phase_hooks(cfg, prov_ip, kind, vendor_executes)
