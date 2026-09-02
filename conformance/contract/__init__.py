"""What every harness must prove, regardless of how it does it.

The Lab used to hold four independent scripts. Measured, they shared three
checks out of forty-two — all three liveness — and two thirds of the checks
existed in exactly one vertical. That is not four verticals testing one
product; it is four products' worth of assertions about one binary each.

The cause was that each vertical asserted the *mechanism* a vendor happens
to use. Mechanism has to diverge: `Native > Generated Native > Safe
Adaptation` is the product working. Outcome must not — a Skill that answers
differently on two harnesses is the thesis failing, and catching that is
the only reason this Lab is worth its runtime.

So a contract states an outcome and names no vendor; a binding says how one
harness is driven and carries no assertion. Neither can drift into the
other's job without the split becoming obvious.

It also gives a check a second opinion. The `hooks-*` vacuity that shipped
for months survived because the assertion lived in one vertical and nothing
contradicted it.
"""

from . import mcp, skill

#: Every capability contract, in the order a run exercises them.
CONTRACTS = (skill, mcp)


def run(cfg, prov_ip, bindings):
    """Runs every capability contract against one harness."""
    for contract in CONTRACTS:
        contract.assert_contract(cfg, prov_ip, bindings)
