#!/usr/bin/env python3
import json
import subprocess

"""Codex scenario (latest channel) — Real Harness + Synthetic World.

Phase A (TUI): auth.json seed skips login; trust prompt dismissed; prompt;
/skills lists the default Skill delivered through the generated plugin;
/plugins lists the plugin; /mcp lists the UZE-delivered server; deterministic
turn; the model request carries `flow:commit` and never `flow:review`.

Phase B (CLI/state): `codex plugin list` reports the UZE plugins installed +
enabled (secondary; the /plugins TUI surface is the primary assertion).

Phase C (invocation policy, ADR-030): `codex debug prompt-input` renders the
model-visible catalog with zero model calls; a user-only Skill must be absent
from it and a default one present, with a sidecar-removal control proving the
exclusion is caused by Codex reading UZE's policy sidecar.

Every absence assertion here is guarded by a presence precondition in the
same capture: an empty catalog hides nothing, it proves nothing.
"""
import os
import re
import sys
import time

import pexpect

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", ".."))
import shared.common as common
from shared.common import (
    check,
    describe,
    docker_base,
    generate_certs,
    make_screen,
    make_waiter,
    materialize_marketplace,
    provider_struct,
)


def codex_setup(cfg, prov_ip, final_cmd, plugins="flow mcp-plugin"):
    return f"""
set -e
export PATH=/usr/local/bin:/usr/bin:/bin:/usr/local/.local/bin
export HOME=/work/home CODEX_HOME=/work/home/.codex UZE_HOME=/work/home/.uze
export OPENAI_API_KEY=uze-conformance-invalid-by-design
export CODEX_CA_CERTIFICATES=/app/ca.crt
export SSL_CERT_FILE=/app/ca.crt
mkdir -p /work/home/.codex /work/home/.agents
cp /app/fixtures/auth.json /work/home/.codex/auth.json
cat > /work/home/.codex/config.toml <<'TOML'
[features]
hooks = true
TOML
{materialize_marketplace(cfg)}
uze market add /work/market >/dev/null 2>&1
for p in {plugins}; do uze plugin install $p@uze-lab >/dev/null 2>&1; done
{final_cmd}
"""


def codex_container(cfg, prov_ip, final_cmd, plugins="flow mcp-plugin"):
    cmd = docker_base(
        cfg, prov_ip, codex_setup(cfg, prov_ip, final_cmd, plugins=plugins)
    )
    ca_crt, _, _ = generate_certs(cfg)
    i = cmd.index(common.HARNESS_IMAGE)
    cmd = (
        cmd[:i]
        + ["-v", f"{ca_crt}:/app/ca.crt:ro", "-e", "CODEX_HOME=/work/home/.codex"]
        + cmd[i:]
    )
    return cmd


def drive_onboarding(child):
    """auth.json seed skips the login screen; the directory-trust prompt is
    dismissed with Enter (default '1. Yes, continue') until the main prompt
    stays. Returns (screen, plain)."""
    screen = make_screen(child)
    wait_for = make_waiter(screen)
    t, p, m = wait_for(
        ["Ask Codex to do anything", "Doyoutrust", "Do you trust"], tries=18
    )
    for _ in range(8):
        if "Doyoutrust" in p.replace(" ", "") or "Do you trust" in p:
            child.send("\r")
            time.sleep(3)
            t, p, m = wait_for(
                ["Ask Codex to do anything", "Doyoutrust", "Do you trust"], tries=8
            )
        else:
            break
    return t, p


def phase_tui(cfg, prov_ip):
    cmd = codex_container(cfg, prov_ip, "exec codex")
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

    t, p = drive_onboarding(child)
    snap("01_prompt", t)
    check(
        "tui-reached-prompt",
        "Ask Codex to do anything" in p,
        "codex TUI reached its prompt"
        if "Ask Codex" in p
        else p[-120:].replace("\n", " "),
    )
    check("synthetic-credential", "Ask Codex" in p, "dummy key mode (no login screen)")

    # /skills
    for ch in "/skills":
        child.send(ch)
        time.sleep(0.1)
    time.sleep(1)
    child.send("\r")
    t, p, m = wait_for(["Choose an action", "skills"], tries=8, stop_on_death=True)
    snap("02_skills", t)
    check(
        "skills-surface-in-tui",
        "Choose an action" in p,
        "/skills opens the skill management surface",
    )
    child.send("2")
    time.sleep(2.5)
    t, p, m = wait_for(["Enable/Disable"], tries=8, stop_on_death=True)
    snap("02b_skills_list", t)
    check(
        "skills-list-opens",
        "Enable/Disable" in p,
        "the Enable/Disable skill list opens",
    )
    child.send("\x1b")
    time.sleep(1.0)

    # /plugins
    for ch in "/plugins":
        child.send(ch)
        time.sleep(0.08)
    time.sleep(1)
    child.send("\r")
    t, p, m = wait_for(["Installed", "Plugins"], tries=8, stop_on_death=True)
    snap("02c_plugins", t)
    check(
        "plugins-in-tui",
        "Installed" in p and "flow" in p,
        "/plugins shows the UZE-delivered `flow` plugin installed"
        if "flow" in p
        else p[-240:].replace("\n", " "),
    )
    child.send("\x1b")
    time.sleep(1.0)

    # /mcp — the inventory must name the UZE-delivered server. The heading
    # "MCP Tools" renders even over "No MCP servers configured", so the
    # heading alone is not evidence.
    for ch in "/mcp":
        child.send(ch)
        time.sleep(0.08)
    time.sleep(1)
    child.send("\r")
    t, p, m = wait_for(
        ["uze-conformance", "No MCP servers configured"], tries=8, stop_on_death=True
    )
    snap("02d_mcp", t)
    check(
        "mcp-server-in-tui-inventory",
        "uze-conformance" in p,
        "/mcp lists the UZE-delivered `uze-conformance` server"
        if "uze-conformance" in p
        else p[-240:].replace("\n", " "),
    )
    child.send("\x1b")
    time.sleep(1.0)

    # deterministic turn
    for ch in "hi":
        child.send(ch)
        time.sleep(0.08)
    time.sleep(1)
    try:
        child.read_nonblocking(size=100000, timeout=3)
    except Exception:
        pass
    child.send("\r")
    t3, p3, _ = wait_for(
        ["UZE_CONFORMANCE_OK", "error", "Error"], tries=20, gap=2.5, stop_on_death=True
    )
    snap("03_after_prompt", t3)
    check(
        "deterministic-response-rendered",
        "UZE_CONFORMANCE_OK" in p3,
        "UZE_CONFORMANCE_OK rendered in TUI"
        if "UZE_CONFORMANCE_OK" in p3
        else p3[-160:].replace("\n", " "),
    )

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
        check(
            "provider-request-captured", bool(struct), "requests structurally recorded"
        )
        check(
            "skills-instructions-in-request",
            bool(has_catalog),
            "the model request carries the skills catalog section",
        )
        default_listed = bool(markers.get("flow:commit"))
        check(
            "model-visible-skill-present",
            default_listed,
            "flow:commit in the request codex sent to its provider"
            if default_listed
            else ", ".join(f"{m}={markers.get(m)}" for m in sorted(markers)),
        )
        check(
            "model-only-skill-present",
            bool(markers.get("flow:analyze")),
            "flow:analyze (model-only, delivered individually) in the request",
        )
        # Only meaningful once the catalog is proven to carry this plugin's
        # skills: an empty catalog would hide `flow:review` for free.
        check(
            "user-only-skill-hidden-from-model",
            default_listed and not markers.get("flow:review"),
            "flow:review absent from the request while flow:commit is present"
            if default_listed
            else "not proven: the catalog carries no flow skill at all",
        )
    else:
        check("provider-request-captured", False, "no provider request captured")

    child.send("\x03")
    time.sleep(0.5)
    child.send("\x03")
    time.sleep(0.5)
    child.close(force=True)


def phase_plugin_cli(cfg, prov_ip):
    """Secondary state check: `codex plugin list` reports the UZE plugins
    installed + enabled (the /plugins TUI surface is the primary assertion)."""
    final = """
echo '===== codex plugin list ====='
codex plugin list 2>&1
"""
    setup = codex_setup(cfg, prov_ip, final)
    cmd = docker_base(cfg, prov_ip, setup, tty=False)
    ca_crt, _, _ = generate_certs(cfg)
    i = cmd.index(common.HARNESS_IMAGE)
    cmd = (
        cmd[:i]
        + ["-v", f"{ca_crt}:/app/ca.crt:ro", "-e", "CODEX_HOME=/work/home/.codex"]
        + cmd[i:]
    )
    proc = subprocess.run(cmd, capture_output=True, text=True)
    out = proc.stdout + proc.stderr
    with open(f"{cfg.outdir}/05_plugin_list.txt", "w") as f:
        f.write(out)
    check(
        "plugin-delivery",
        "flow@uze-store" in out and "installed, enabled" in out,
        "codex plugin list reports the UZE plugins installed + enabled",
    )


def phase_hooks(cfg, prov_ip, kind):
    """Portable-hook evidence inside the REAL Codex TUI (ADR-033).

    The provider scripts a `Bash` function call whose arguments the hook
    `guard` examines (delivered through ~/.codex/hooks.json); `kind` selects
    the scenario with the same semantics as the claude/antigravity/opencode
    verticals:

      deny  : arguments contain `secrets` -> the hook denies (reason
              "blocked by protect-env") and the second handler never runs;
              Bash itself never executes.
      allow : plain echo arguments -> the hook allows, Bash runs.
      order : a two-handler group whose first handler always denies -> the
              second handler's marker must never appear (first-deny-wins).

    NOTE: Codex's approval gate may intercept tool use before or in addition
    to the hook (the MCP vertical documents the same gate); the phase asserts
    the hook evidence that is observable either way and records the vendor
    limitation honestly when the gate wins.
    """
    scenarios = {
        "deny": {
            "plugin": "hook-plugin",
            # codex 0.150.1's shell tool is `exec_command` with a `cmd`
            # argument (the Bash/command pair died on this channel); the
            # other harnesses still receive the Bash tool.
            "args": '{"cmd":"echo API secrets"}',
            "deny_present": "blocked by protect-env",
            "deny_absent": ["second-handler-reached"],
        },
        "allow": {
            "plugin": "hook-plugin",
            "args": '{"cmd":"echo plain output"}',
            "deny_present": None,
            "deny_absent": ["blocked by protect-env"],
            # The allow path asserts only that no denial reached the
            # conversation. Whether exec_command then actually ran is NOT
            # asserted: locally it does (`plain output` returns, exit 0),
            # but under GitHub-hosted Docker Codex's bubblewrap sandbox
            # fails before the command (`bwrap: Failed to make / slave:
            # Permission denied`, exit 1) — an environment gap, not hook
            # evidence, and an asserted check would flip between the two.
        },
        "order": {
            "plugin": "hook-order-plugin",
            "args": '{"cmd":"echo any"}',
            "deny_present": "first-handler-denied",
            "deny_absent": ["second-handler-ran"],
        },
    }
    spec = scenarios[kind]
    common.start_provider(
        cfg,
        "toolcall",
        {"TOOL_NAME": "exec_command", "TOOL_ARGS": spec["args"]},
    )
    time.sleep(1)
    cmd = codex_container(
        cfg,
        prov_ip,
        "exec codex --dangerously-bypass-hook-trust",
        plugins=f"flow {spec['plugin']}",
    )
    child = pexpect.spawn(
        cmd[0], cmd[1:], encoding="utf-8", codec_errors="replace", timeout=300
    )
    child.setwinsize(50, 160)
    try:
        child.logfile_read = common.CastRecorder(cfg.outdir, f"tui-hooks-{kind}")
    except Exception:
        pass
    screen = make_screen(child)
    wait_for = make_waiter(screen)

    t, p = drive_onboarding(child)

    # codex 0.150.1 shows a startup hooks-review screen ("3 hooks need
    # review... Press t to trust all") whenever ~/.codex/hooks.json carries
    # entries, capturing the whole keyboard. The official automation path
    # for self-vetted sources is the CLI flag (no persisted trust needed);
    # drive the hooks phases with it — the lab vets exactly its own
    # fixtures, and the flag's DANGEROUS warning is our documented
    # acceptance of that automation contract.
    def type_with_echo(text, tries=10, gap=1.5):
        # codex renders input chars at absolute cursor columns and echoes
        # spaces only as cursor moves — the plain text carries
        # "runtheAPIcheck" while the user read "run the API check"; compare
        # space-stripped so a real echo with per-char redraws still matches.
        needle = text.replace(" ", "")
        for attempt in range(tries):
            if attempt > 0:
                child.send("\x15")  # Ctrl-U: clear any partial line
                time.sleep(0.5)
            for ch in text:
                child.send(ch)
                time.sleep(0.08)
            time.sleep(2.0)
            _t, p = screen(gap)
            if needle in p.replace(" ", ""):
                return True
            print(f"    … input not echoed yet (try {attempt + 1}/{tries})", flush=True)
        return False

    typed = type_with_echo("run the API check")
    check(
        "hooks-input-echoed",
        typed,
        "the prompt text was accepted by the TUI"
        if typed
        else "typed input was lost — the TUI never echoed it",
    )
    child.send("\r")
    t3, p3, m3 = wait_for(
        [
            "UZE_CONFORMANCE_PASS",
            "UZE_CONFORMANCE_OK",
            "blocked by protect-env",
            "Denied by UZE hook",
            "denied",
            "Sandbox mode",
        ],
        tries=24,
        gap=2.5,
    )
    with open(f"{cfg.outdir}/hooks_{kind}.raw", "w") as f:
        f.write(t3)
    # Absence checks may only evaluate once the turn settled and the TUI
    # went quiet (ADR-035).
    settled = m3 is not None and common.settle_and_quiet(screen)

    struct = provider_struct(cfg)
    with open(f"{cfg.outdir}/hooks_{kind}_struct.json", "w") as f:
        json.dump(struct, f, indent=1)
    check(
        f"hooks-{kind}-turn-requested",
        bool(struct) or m3 is not None,
        "the turn reached the provider or a stable marker"
        if struct or m3 is not None
        else p3[-160:].replace("\n", " "),
    )
    markers = {}
    has_call = False
    has_output = False
    for r in struct:
        s = r.get("summary", {})
        markers.update(s.get("hook_markers", {}))
        has_output = has_output or bool(s.get("hook_markers", {}).get("plain output"))
        has_call = has_call or bool(s.get("has_function_call"))
    if spec["deny_present"]:
        # The denial reason in the function_call_output is the evidence that
        # the hook ran and Codex relayed its decision instead of the tool's
        # output. Without it the absence checks below hold for a turn where
        # no hook ran at all.
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

    child.send("\x03")
    time.sleep(0.5)
    child.send("\x03")
    time.sleep(0.5)
    child.close(force=True)


def listed_skills(prompt_input):
    """The plugin skills `codex debug prompt-input` offers to the model, as
    the `- <name>: <description>` catalog lines name them. Plugin skills are
    listed as `<plugin>:<skill>`; individually attached ones carry the same
    label from their wrapper's frontmatter."""
    return set(re.findall(r"- (flow:[a-z]+): ", prompt_input))


def phase_skill_invocation_policy(cfg, prov_ip):
    """Invocation policy (ADR-030) as the REAL Codex binary renders it.

    This is the real-harness half of what `tests/integrations/harness/codex.rs`
    proves deterministically. It used to live there too, spawning `codex` from
    the developer's own PATH — which is UZE's runtime shim on any dogfooding
    machine, so it measured the host rather than Codex. Here the binary, the
    HOME, and the network are the container's.

    `codex debug prompt-input` renders exactly what the model would receive,
    with zero model calls. The `flow` fixture carries every shape: `commit`
    (default: model-visible), `review` (`invoke: {model: false, user:
    true}`: user-only) and `analyze` (model-only). Expected: `flow:commit`
    and `flow:analyze` are offered, `flow:review` is not.

    Delivery shape, so the evidence is read where it lives: `commit` and
    `review` arrive through the GENERATED native plugin (`uze plugin inspect`
    reports them "provided by package"), which Codex stages into its own
    cache under `$CODEX_HOME/plugins/cache/uze-store/flow/<version>/` — the
    sidecar Codex actually reads is that cache copy. `analyze` is Degraded
    on Codex and attached individually under `~/.agents/skills`.

    The control matters as much as the assertion. Deleting the
    `agents/openai.yaml` policy sidecar from the cache copy must bring
    `flow:review` back — proving the exclusion is caused by Codex genuinely
    reading the sidecar UZE wrote, not by the Skill being absent, misnamed,
    or undelivered for some unrelated reason.
    """
    final = """
echo '===== sidecar in the generated envelope ====='
find /work/home/.uze/state/attachments/codex/generated -path '*/skills/review/agents/openai.yaml' 2>/dev/null
echo '===== sidecar in the codex plugin cache ====='
find /work/home/.codex/plugins/cache -path '*/skills/review/agents/openai.yaml' 2>/dev/null
echo '===== prompt-input (policy present) ====='
codex debug prompt-input 2>&1
echo '===== prompt-input (policy removed: control) ====='
find /work/home/.codex/plugins/cache -path '*/skills/review/agents/openai.yaml' -delete 2>/dev/null
codex debug prompt-input 2>&1
"""
    setup = codex_setup(cfg, prov_ip, final, plugins="flow")
    cmd = docker_base(cfg, prov_ip, setup, tty=False)
    ca_crt, _, _ = generate_certs(cfg)
    i = cmd.index(common.HARNESS_IMAGE)
    cmd = (
        cmd[:i]
        + ["-v", f"{ca_crt}:/app/ca.crt:ro", "-e", "CODEX_HOME=/work/home/.codex"]
        + cmd[i:]
    )
    proc = subprocess.run(cmd, capture_output=True, text=True)
    out = proc.stdout + proc.stderr
    with open(f"{cfg.outdir}/06_skill_invocation_policy.txt", "w") as f:
        f.write(out)

    envelope_section, _, rest = out.partition(
        "===== sidecar in the codex plugin cache ====="
    )
    cache_section, _, rest = rest.partition("===== prompt-input (policy present) =====")
    with_policy, _, without_policy = rest.partition(
        "===== prompt-input (policy removed: control) ====="
    )
    offered = listed_skills(with_policy)
    offered_without_policy = listed_skills(without_policy)

    check(
        "policy-sidecar-delivered",
        "/generated/flow@uze-lab/skills/review/agents/openai.yaml" in envelope_section,
        "UZE writes the invocation-policy sidecar into the generated envelope",
    )
    check(
        "policy-sidecar-ingested",
        "/plugins/cache/uze-store/flow/" in cache_section
        and "/skills/review/agents/openai.yaml" in cache_section,
        "Codex staged the envelope, sidecar included, into its plugin cache",
    )
    check(
        "default-skill-offered",
        "flow:commit" in offered,
        "flow:commit (generated plugin) is offered to the model"
        if "flow:commit" in offered
        else f"offered: {sorted(offered)}",
    )
    check(
        "model-only-skill-offered",
        "flow:analyze" in offered,
        "flow:analyze (individually attached) is offered to the model"
        if "flow:analyze" in offered
        else f"offered: {sorted(offered)}",
    )
    hidden = "flow:commit" in offered and "flow:review" not in offered
    check(
        "user-only-skill-hidden",
        hidden,
        "flow:review is absent while flow:commit from the same plugin is present"
        if hidden
        else f"not proven — offered: {sorted(offered)}",
    )
    control = hidden and "flow:review" in offered_without_policy
    check(
        "control-sidecar-drives-the-exclusion",
        control,
        "removing the cached sidecar restores flow:review, proving Codex reads it"
        if control
        else f"offered without the sidecar: {sorted(offered_without_policy)}",
    )


def run(cfg, prov_ip):
    with describe("tui"):
        phase_tui(cfg, prov_ip)
    with describe("cli.state"):
        phase_plugin_cli(cfg, prov_ip)
    with describe("skill-invocation-policy"):
        phase_skill_invocation_policy(cfg, prov_ip)
    with describe("hooks"):
        for kind in ("deny", "allow", "order"):
            with describe(kind):
                phase_hooks(cfg, prov_ip, kind)
