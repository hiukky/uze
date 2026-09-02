"""How Claude Code is driven. No assertions live here."""

import time

from contract.bindings import Bindings
from contract.tui import Tui

from .scenarios import claude_container, drive_onboarding


class ClaudeBindings(Bindings):
    harness = "claude"
    launch = "exec claude"
    ready_markers = ("Opus", "API Usage Billing", "❯")
    warmup = 6.0

    def session(self, cfg, prov_ip):
        return Tui(cfg, claude_container(cfg, prov_ip, self.launch), "claude-contract")

    def prepare(self, tui):
        """Claude opens on a chain of first-run dialogs — welcome, security
        guide, API key, theme, folder trust — before the prompt exists."""
        _, plain, _ = drive_onboarding(tui.child)
        if not plain or not any(m in plain for m in self.ready_markers):
            plain, _ = tui.until(self.ready_markers)
        tui.snapshot("ready", plain)
        return plain, any(marker in plain for marker in self.ready_markers)

    def skill_catalog(self, tui):
        time.sleep(self.warmup)
        tui.type("/skills")
        time.sleep(1)
        tui.submit()
        return tui.collect(reads=6)

    def lists(self, catalog, skill):
        """Claude names a Skill by its namespaced invocation label."""
        return f"flow:{skill}" in catalog.replace(" ", "")

    def mcp_inventory(self, tui):
        """`/mcp` lists every configured server and its connection state."""
        tui.type("/mcp")
        time.sleep(1)
        tui.submit()
        inventory, _ = tui.until(["uze-conformance", "MCP"], tries=8)
        return inventory

    def unsupported(self, prop):
        """Claude Code exposes `disable-model-invocation` — which UZE uses to
        honour `invoke.model: false` — but no verified inverse that hides a
        Skill from explicit user invocation.

        Declared rather than asserted because the control may exist and
        simply not be used: see `conformance/DECISIONS.md`. Declaring keeps
        the question visible; omitting the check would bury it again.
        """
        if prop == "model-only-is-not-user-invocable":
            return (
                "no verified Claude control hides a Skill from explicit "
                "invocation (the inverse of disable-model-invocation is unconfirmed)"
            )
        return None
