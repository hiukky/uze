"""How Antigravity CLI is driven. No assertions live here."""

import time

from contract.bindings import Bindings
from contract.tui import Tui
from shared.common import docker_base

from .scenarios import agy_setup


class AntigravityBindings(Bindings):
    harness = "antigravity"
    launch = "exec agy"
    ready_markers = ("Antigravity CLI",)
    warmup = 3.0

    def session(self, cfg, prov_ip):
        setup = agy_setup(cfg, prov_ip, include_mcp=True, final_cmd=self.launch)
        return Tui(cfg, docker_base(cfg, prov_ip, setup), "antigravity-contract")

    def session_in(self, cfg, prov_ip, cwd, prelude):
        setup = agy_setup(
            cfg,
            prov_ip,
            include_mcp=False,
            final_cmd=self.launch,
            prelude=f"{prelude}\ncd {cwd}",
        )
        return Tui(cfg, docker_base(cfg, prov_ip, setup), "antigravity-isolation")

    def prepare(self, tui):
        """agy opens on a colour-scheme picker and a terms screen; the prompt
        exists only after both are answered."""
        try:
            tui.child.expect("Choose your color scheme", timeout=150)
        except Exception as error:
            return f"onboarding never appeared: {error}", None
        tui.child.send("\r")
        time.sleep(3)
        tui.child.send("\t\t")
        time.sleep(0.7)
        tui.child.send("\r")
        time.sleep(5)
        _, plain = tui.screen(3)
        # A directory the harness has not seen before — a linked worktree,
        # for one — adds a folder-trust dialog after the terms, with "Yes,
        # I trust this folder" preselected. Text typed while it is up goes
        # to the dialog, never to the prompt.
        if "trust the contents" in plain:
            tui.child.send("\r")
            time.sleep(5)
            _, plain = tui.screen(3)
        tui.snapshot("ready", plain)
        return plain, "Antigravity CLI" in plain and ">" in plain

    def skill_catalog(self, tui):
        """`/skills` lists every Skill a person can invoke. The leading `/`
        is sent alone: typing it with the rest loses the palette trigger."""
        time.sleep(self.warmup)
        tui.child.send("/")
        time.sleep(1.2)
        tui.type("skills", per_char=0.15)
        time.sleep(1.2)
        tui.submit()
        catalog, _ = tui.until(["flow:review"], tries=4)
        return catalog

    def lists(self, catalog, skill):
        """Antigravity names a Skill by its namespaced invocation label."""
        return f"flow:{skill}" in catalog.replace(" ", "")

    def mcp_inventory(self, tui):
        """`/mcp` lists every configured server and enumerates its tools."""
        tui.child.send("/")
        time.sleep(1.2)
        tui.type("mcp", per_char=0.15)
        time.sleep(1.2)
        tui.submit()
        inventory, _ = tui.until(["Tools: uze_conformance", "uze-conformance"], tries=8)
        return inventory
