"""What a Skill must do, on every harness.

The canonical Skill carries an invocation policy (ADR-030): who may invoke
it, expressed once, translated by each integration into whatever that
vendor encodes it with. The policy is portable; the encoding is not. So
this asserts the policy's *effect* and never looks at an encoding — no
sidecar path, no config key, no screen chrome.

The `flow` fixture carries one Skill of each shape:

    commit    no `invoke:` block   → default: model and user
    analyze   model, not user      → discovered, not user-invocable
    review    user, not model      → user-invocable, not discovered

Every absence assertion here is conditional on a presence assertion.
"`review` is not offered to the model" and "nothing was offered to the
model" are the same observation unless something proves the surface was
populated — and for months they were, on three harnesses, which is exactly
how a Skill that never reached the model read as a policy working.
"""

from shared.common import check, check_absence, describe

#: The fixture's three shapes, by canonical name.
DEFAULT = "commit"
MODEL_ONLY = "analyze"
USER_ONLY = "review"

#: What an invoked Skill answers with. The fixture bodies are inert, so a
#: harness that merely *lists* a Skill cannot produce this by accident.
INVOKED_MARKER = "UZE_CONFORMANCE_PASS"


def _declined(bindings, prop):
    """Records a harness's declaration that it cannot deliver `prop`.

    A declaration is a result: it appears in the evidence beside the passes,
    with the reason, and review can disagree with it. An omitted check
    cannot be disagreed with — which is why the contract asks every harness
    every question and lets it answer "no, because".
    """
    reason = bindings.unsupported(prop)
    if reason:
        check(
            f"skill-{prop}",
            True,
            f"{bindings.harness} cannot: {reason}",
            kind="adapt",
        )
    return reason


def assert_contract(cfg, prov_ip, bindings):
    with describe("skill"):
        _assert_catalog(cfg, prov_ip, bindings)


def _assert_catalog(cfg, prov_ip, bindings):
    """The catalog a person sees, and the one the model is given.

    Both are read from the same run: opening a list and asking a question
    are the same session, and splitting them would let a harness pass one
    while failing the other with nobody noticing.
    """
    with bindings.session(cfg, prov_ip) as tui:
        plain, matched = bindings.prepare(tui)
        check(
            "skill-tui-ready",
            bool(matched),
            f"{bindings.harness} reached its prompt"
            if matched
            else plain[-160:].replace("\n", " "),
        )
        if not matched:
            return

        catalog = bindings.skill_catalog(tui)
        tui.snapshot("catalog", catalog)

        # The precondition every absence below depends on. Without it,
        # an empty surface proves every policy at once.
        default_listed = bindings.lists(catalog, DEFAULT)
        check(
            "skill-default-is-user-invocable",
            default_listed,
            f"`{DEFAULT}` is offered to the user"
            if default_listed
            else f"`{DEFAULT}` absent — nothing below can be concluded: "
            f"{catalog[-160:]}".replace("\n", " "),
        )
        if not default_listed:
            return

        # user-only is the inverse of model-only, and the surface is proven
        # populated, so an absence here means the policy, not an empty list.
        check(
            "skill-user-only-is-user-invocable",
            bindings.lists(catalog, USER_ONLY),
            f"`{USER_ONLY}` declares user: true and is offered",
        )
        if not _declined(bindings, "model-only-is-not-user-invocable"):
            check_absence(
                "skill-model-only-is-not-user-invocable",
                not bindings.lists(catalog, MODEL_ONLY),
                settled=True,
                detail=f"`{MODEL_ONLY}` declares user: false",
            )
