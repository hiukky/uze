"""How Codex is driven. No assertions live here."""

import time

from contract.bindings import Bindings
from contract.tui import Tui

from .scenarios import codex_container, drive_onboarding


class CodexBindings(Bindings):
    harness = "codex"
    launch = "exec codex"
    #: The real prompt, not the splash. A marker loose enough to match
    #: onboarding passes every check against a screen that accepts no
    #: input — which it did, once, here.
    ready_markers = ("Ask Codex to do anything",)
    warmup = 6.0

    def session(self, cfg, prov_ip):
        return Tui(cfg, codex_container(cfg, prov_ip, self.launch), "codex-contract")

    def session_in(self, cfg, prov_ip, cwd, prelude):
        final = f"{prelude}\ncd {cwd} && {self.launch}"
        return Tui(cfg, codex_container(cfg, prov_ip, final), "codex-isolation")

    def prepare(self, tui):
        """Codex opens on an onboarding flow; the prompt only accepts input
        once it is driven through."""
        _, plain = drive_onboarding(tui.child)
        tui.snapshot("ready", plain)
        return plain, "Ask Codex" in plain

    def skill_catalog(self, tui):
        """`/skills` opens a menu; option 2 is the Enable/Disable list, which
        is the surface that names every Skill a person can invoke."""
        time.sleep(self.warmup)
        tui.type("/skills")
        time.sleep(1)
        tui.submit()
        tui.until(["Choose an action", "skills"])
        tui.child.send("2")
        catalog, _ = tui.until(["Enable/Disable"])
        return catalog

    def lists(self, catalog, skill):
        """Codex names a plugin Skill `<skill> (<plugin>)` in this list, and
        an individually attached one by its namespaced label."""
        squeezed = catalog.replace(" ", "")
        return f"{skill}(flow)" in squeezed or f"flow:{skill}" in squeezed

    def mcp_inventory(self, tui):
        """`/mcp` lists every configured server."""
        tui.type("/mcp")
        time.sleep(1)
        tui.submit()
        inventory, _ = tui.until(["uze-conformance", "MCP"], tries=8)
        return inventory

    def unsupported(self, prop):
        """Codex documents no way to disable explicit `$skill` invocation, so
        a canonical `user: false` cannot be enforced here.

        The product already says this rather than inventing it: the exposure
        plan routes model-only as `Degraded`. Declaring it here keeps the
        contract asking every harness the same question and getting an
        honest answer, instead of the check quietly not existing in this
        vertical — which is how the previous suite hid divergence.
        """
        if prop == "model-only-is-not-user-invocable":
            return (
                "Codex has no documented way to disable explicit `$skill` "
                "invocation; the product routes this as Degraded"
            )
        return None
