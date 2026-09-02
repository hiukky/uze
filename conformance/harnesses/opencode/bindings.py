"""How OpenCode is driven. No assertions live here."""

import time

from contract.bindings import Bindings
from contract.tui import Tui

from .scenarios import opencode_container


class OpenCodeBindings(Bindings):
    harness = "opencode"
    launch = "UZE_HOME=/usr/local/.uze PATH=/usr/local/.uze/shims:$PATH exec opencode --standalone"
    ready_markers = ("Ask anything",)
    #: The prompt renders long before the skill and MCP surfaces finish
    #: loading, and input typed into that window is dropped. Measured, not
    #: guessed: 25s is what a working manual probe needed.
    warmup = 25.0

    def session(self, cfg, prov_ip):
        return Tui(
            cfg, opencode_container(cfg, prov_ip, self.launch), "opencode-contract"
        )

    def skill_catalog(self, tui):
        time.sleep(self.warmup)
        tui.type("/skills")
        time.sleep(1)
        tui.submit()
        # This surface renders by region, so a name arrives split across
        # repaint frames; accumulating is the only way to see it whole.
        return tui.collect(reads=8)

    def lists(self, catalog, skill):
        """OpenCode names a Skill by its qualified invocation label."""
        return f"flow:{skill}" in catalog.replace(" ", "")
