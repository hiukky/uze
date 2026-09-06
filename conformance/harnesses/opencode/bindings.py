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

    def session_in(self, cfg, prov_ip, cwd, prelude):
        final = f"{prelude}\ncd {cwd} && {self.launch}"
        return Tui(cfg, opencode_container(cfg, prov_ip, final), "opencode-isolation")

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

    def mcp_inventory(self, tui):
        """`/mcps` — plural here — opens the MCP toggle surface.

        The warmup applies to every surface, not just the first: each
        contract opens its own session, so each pays the same wait before
        the surfaces behind the prompt have loaded.
        """
        time.sleep(self.warmup)
        tui.type("/mcps")
        time.sleep(1)
        tui.submit()
        return tui.collect(reads=6)

    def unsupported(self, prop):
        """`/skills` lists every delivered Skill, whatever `slash` says.

        Re-asked at beta-19192 (2026-09-06). The old reason — "no
        documented control hides a Skill from explicit invocation" — is
        false: the skill parser reads `metadata."opencode/slash"` falling
        back to a top-level `slash`, and two catalog builders filter with
        `skills.filter((s) => s.slash !== false)`. UZE writes that control,
        and its own routing calls this Native.

        What is still true is narrower and was measured, not assumed:
        the surface this vertical reads renders `flow:analyze` alongside
        the others, so the property cannot be observed *here*. Removing
        the declaration made the check fail on exactly that.

        What would retire this: reading the surface those two filters
        build — the `/` invocation palette — rather than the `/skills`
        browser, and proving on the same capture that a default Skill is
        listed there while the model-only one is not.
        """
        if prop == "model-only-is-not-user-invocable":
            return (
                "OpenCode honours `slash: false` in its `/` palette builders but "
                "its `/skills` browser lists every delivered Skill regardless; "
                "the property is not observable on the surface read here"
            )
        return None

    def names_server(self, inventory, server):
        """OpenCode's `/mcps` surface is a toggle list showing connection
        state; it was not observed to print the server id.

        So presence is read from the connected row rather than the name. A
        weaker signal than an id, and recorded as such in
        `conformance/DECISIONS.md` — the surface is the vendor's, and
        asserting an id it does not render would be asserting fiction.
        """
        squeezed = inventory.replace(" ", "")
        return "Connected" in inventory or "disconnectspace" in squeezed
