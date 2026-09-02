#!/usr/bin/env python3
import json
import subprocess

"""Antigravity scenario (latest channel) — Real Harness + Synthetic World.

The session runs **signed in**, against a synthetic identity: a `consumer`
token file the CLI reads as a Google account, and the CloudCode plane
answered by the run's own provider (identity, tier, flags, model catalogue,
model path). That is the mode users are in, and the only one in which this
harness executes `hooks.json` hooks at all — under `GEMINI_API_KEY` they are
loaded and never run (vendor bug google-antigravity/antigravity-cli#893).
API-key mode keeps one declared check so the bug stays on the report.

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

#: Signed-in ("consumer") mode is how the vertical runs: it is the mode
#: users are in, and the only one in which this harness executes
#: `hooks.json` hooks at all (vendor bug
#: google-antigravity/antigravity-cli#893). API-key mode stays reachable —
#: `phase_hooks_api_key_mode` measures the bug there — and is what
#: `auth="apikey"` selects.
CONSUMER_TOKEN = "/work/home/.gemini/antigravity-cli/antigravity-oauth-token"


def auth_fragment(prov_ip, auth):
    if auth == "apikey":
        # `modelProvider: gemini` is what routes the turn at the API key:
        # without it the CLI expects its signed-in backend, and with it the
        # CLI refuses to start unless GEMINI_API_KEY is set — the two travel
        # together, which is why the mode carries its own settings fixture.
        return f"""export GEMINI_API_KEY=uze-conformance-invalid-by-design
export GOOGLE_GEMINI_BASE_URL=http://{prov_ip}:9999
cp /app/fixtures/settings-api-key.json /work/home/.gemini/antigravity-cli/settings.json
rm -f {CONSUMER_TOKEN}"""
    # The synthetic account: a token file the CLI reads as a signed-in
    # session, whose every value is a literal and whose expiry is far
    # enough away that no refresh is attempted. With it in place the CLI
    # ignores GOOGLE_GEMINI_BASE_URL and speaks CloudCode over TLS — which
    # the provider's signed-in listener answers.
    return f"cp /app/fixtures/antigravity-oauth-token {CONSUMER_TOKEN}"


def agy_setup(
    cfg, prov_ip, include_mcp, final_cmd, plugins=None, prelude="", auth="consumer"
):
    if plugins is None:
        plugins = "flow"
        if include_mcp:
            plugins += " mcp-plugin"
    return f"""
set -e
export PATH=/usr/local/bin:/usr/bin:/bin:/usr/local/.local/bin
export HOME=/work/home UZE_HOME=/work/home/.uze
export AGY_CLI_DISABLE_AUTO_UPDATE=1
# The harness is Go and honours SSL_CERT_FILE on Linux: this is how it
# trusts the run's synthetic CA for its own signed-in plane (identity,
# feature flags, account endpoints, the model path) without any Internet.
export SSL_CERT_FILE=/app/ca.crt
export SSL_CERT_DIR=/app
mkdir -p /work/home/.gemini/antigravity-cli
cp /app/fixtures/settings.json /work/home/.gemini/antigravity-cli/settings.json
cp /app/fixtures/jetski_state.pbtxt /work/home/.gemini/antigravity-cli/jetski_state.pbtxt
cp /app/fixtures/installation_id /work/home/.gemini/antigravity-cli/installation_id
{auth_fragment(prov_ip, auth)}
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
        "conformance@uze.invalid" in p1,
        "the signed-in account row is the Lab's synthetic identity, "
        "never a personal account"
        if "conformance@uze.invalid" in p1
        else p1[-200:].replace("\n", " "),
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
    # The answer is streamed and the TUI repaints around it, so a marker can
    # land across two screen reads (`UZE_CONFORMA` … `NCE_OK`): the wait
    # searches everything read since the turn started.
    t3, p3, _ = wait_for(
        ["UZE_CONFORMANCE_OK"],
        tries=30,
        gap=2.5,
        stop_on_death=True,
        accumulate=True,
    )
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
    # Transcript plus rendered screen, for the same reason as the
    # deterministic turn: a streamed final is continued by cursor motion.
    t4, chunk = screen(1.2)
    raw4, plain4 = t4, chunk
    p4 = f"{plain4}\n{common.render_screen(raw4)}"
    tries = 0
    while "UZE_CONFORMANCE_PASS" not in p4 and tries < 14 and child.isalive():
        t4, chunk = screen(2.0)
        raw4 += t4
        plain4 += chunk
        p4 = f"{plain4}\n{common.render_screen(raw4)}"
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


def hook_turn(cfg, prov_ip, tag, args, plugins, prelude="", auth="consumer"):
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
        auth=auth,
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
    # The transcript, plus the screen it renders to: AGY streams its answer
    # and continues the line by moving the cursor, so a marker can exist on
    # screen while no snapshot — and no concatenation of snapshots — holds it
    # contiguously (`… UZE_CONFORMA`, then `ESC[3A ESC[12C NCE_PASS`).
    seen_raw = ""
    seen = ""
    prompted = False

    def settled_now():
        rendered = f"{seen}\n{common.render_screen(seen_raw)}"
        return "UZE_CONFORMANCE_PASS" in rendered or any(
            m in rendered for m in HOOK_DENIAL_MARKERS
        )

    for _ in range(16):
        t, p = screen(2.0)
        seen_raw += t
        seen += p
        if settled_now():
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
        f.write(f"{seen}\n===== rendered =====\n{common.render_screen(seen_raw)}")
    turn_settled = settled_now()
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

    Signed in, it does: the deny hook runs, the TUI renders `Tool call
    denied by pre-tool hook: blocked by protect-env`, and the reason reaches
    the conversation as the tool outcome (1.1.24, 2026-09-02, experiment
    `antigravity/signed-in`). The UZE hook checks that follow are therefore
    asserted, not declared.

    The gate stays a live precondition rather than an assumption because the
    thing it measures is a vendor gate, not ours: the executor reads
    `enable_json_hooks`, field 17 of `exa.cortex_pb.CustomizationConfig`,
    which the CLI only ever receives over the CloudCode backend it speaks
    when signed in. Under `GEMINI_API_KEY` no such config arrives, whatever
    the `json-hooks-enabled` flag says — vendor bug
    google-antigravity/antigravity-cli#893, and `phase_hooks_api_key_mode`
    keeps that on the report. If a future build closed the gate in signed-in
    mode too, this check would say so instead of the suite quietly proving
    nothing.

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


HOOKS_LOADED_PATTERN = "loaded [0-9]* named hooks from [0-9]* hooks.json file(s)"


def phase_hooks_delivery(cfg, prov_ip):
    """Whether the harness loads the hooks UZE delivers — the second live
    precondition, and the cheap one: the harness's own log says how many
    `hooks.json` files it read, so a headless start answers it.

    UZE delivers Antigravity's hooks as `plugins/<name>/hooks.json` inside
    the generated native plugin, which is what the vendor's shipped plugin
    guide documents: "Hooks defined in `plugins/<name>/hooks.json` are
    registered and run during the agent's lifecycle". On 1.1.24 they are not:
    `agy plugin validate` counts the plugin's three hooks, the plugin is
    listed with a `hooks` component and enabled in `config.json`, and the
    session still reports `loaded 0 named hooks from 0 hooks.json file(s)`
    — it never opens the file (no `skipping hooks.json at …`, no `No
    hooks.json found at …`). The same document at the vendor's shared path
    (`~/.gemini/config/hooks.json`) loads and executes in the same session,
    which is what `hooks > vendor` proves.

    Measured every run rather than declared once: the day the vendor scans
    plugin directories, this passes and the gate escalates the declarations
    that depend on it.
    """
    final = (
        """
agy --print "hi" --print-timeout 30s --log-file /work/agy.log >/dev/null 2>&1 || true
echo '===== hooks_manager ====='
grep -o '%s' /work/agy.log | head -2 || true
echo '===== plugin validate ====='
agy plugin validate /work/home/.gemini/config/plugins/hook-plugin 2>&1 \
  | sed 's/\\x1b\\[[0-9;]*m//g' | head -10 || true
echo '===== hooks.json present ====='
ls -la /work/home/.gemini/config/plugins/hook-plugin/hooks.json || true
"""
        % HOOKS_LOADED_PATTERN
    )
    setup = agy_setup(
        cfg, prov_ip, include_mcp=False, final_cmd=final, plugins="flow hook-plugin"
    )
    proc = subprocess.run(
        docker_base(cfg, prov_ip, setup, tty=False), capture_output=True, text=True
    )
    out = proc.stdout + proc.stderr
    with open(f"{cfg.outdir}/hooks_delivery.txt", "w") as f:
        f.write(out)
    loaded = any(
        line.startswith("loaded ") and not line.startswith("loaded 0 ")
        for line in out.splitlines()
    )
    # A shut vendor gate is a declaration (ADAPTED, registered per version),
    # never a failure of UZE's — and the gate escalates it the day it opens.
    check(
        "hooks-delivered-hooks-loaded",
        True,
        "the harness loaded the hooks UZE delivered in its generated plugin"
        if loaded
        else (
            "the harness reads no hooks.json from a plugin directory: "
            "`agy plugin validate` counts the delivered hooks and the session "
            "still reports `loaded 0 named hooks from 0 hooks.json file(s)`, "
            "while the same document at ~/.gemini/config/hooks.json executes "
            "(see hooks > vendor)"
        ),
        kind="assert" if loaded else "adapted",
    )
    return loaded


def phase_hooks_api_key_mode(cfg, prov_ip):
    """The same control hook, the same turn, on a Gemini API key — the one
    variable is the auth mode.

    Signed in it denies the command; on the API key the harness loads the
    hook and runs nothing, so the command reaches the vendor's permission
    prompt instead. That is google-antigravity/antigravity-cli#893 ("Hooks
    from .agents/hooks.json are loaded but never executed when authenticated
    via GEMINI_API_KEY", open 2026-08-28); #78 records that Google does not
    support the API-key path at all. Recorded as a declaration so the bug
    stays visible on the report without gating the vertical — and so the day
    the vendor fixes it, this check fails and says so.
    """
    outcome = hook_turn(
        cfg,
        prov_ip,
        "api-key",
        RUN_COMMAND_ARGS % "echo API secrets",
        plugins="flow",
        prelude=VENDOR_CONTROL_HOOK,
        auth="apikey",
    )
    if outcome is None:
        return
    executes = bool(outcome["markers"].get("blocked by protect-env"))
    check(
        "hooks-api-key-mode-runs-no-hook",
        not executes,
        "the vendor-format deny hook that fires signed in never runs under "
        "GEMINI_API_KEY (google-antigravity/antigravity-cli#893)"
        + (" — the permission prompt surfaced instead" if outcome["prompted"] else "")
        if not executes
        else "the hook executed under GEMINI_API_KEY: #893 is fixed, retire this "
        "declaration and assert the API-key path too",
        kind="adapted",
    )


def phase_hooks(cfg, prov_ip, kind, blocked):
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

    `blocked` carries the reason a live precondition gave for not judging
    UZE's delivery this run (the vendor runs no hook at all, or it never
    loads the one UZE delivered). Every check here is then recorded as a
    declaration carrying that reason — the turn would only re-measure a
    precondition that already answered.
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
    if blocked:
        reason = blocked
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
        # Two live preconditions, in the order a failure should be read:
        # does this harness run `hooks.json` hooks at all, and does it load
        # the ones UZE delivered? Only with both answered yes is UZE's
        # delivery what the scenarios below measure.
        with describe("vendor"):
            vendor_executes = phase_hooks_gate(cfg, prov_ip)
        with describe("delivery"):
            delivered_loaded = phase_hooks_delivery(cfg, prov_ip)
        blocked = None
        if not vendor_executes:
            blocked = (
                "declared: this AGY executes no hooks.json hook in the Lab "
                "session (see hooks-vendor-hook-executes), so the UZE plugin "
                "hook cannot be observed here"
            )
        elif not delivered_loaded:
            blocked = (
                "declared: this AGY executes hooks.json hooks (see "
                "hooks-vendor-hook-executes) but reads none from a plugin "
                "directory, which is how UZE delivers them (see "
                "hooks-delivered-hooks-loaded), so nothing of UZE's reaches "
                "the session to be observed"
            )
        for kind in ("deny", "allow", "order"):
            with describe(kind):
                phase_hooks(cfg, prov_ip, kind, blocked)
        with describe("api-key"):
            phase_hooks_api_key_mode(cfg, prov_ip)
