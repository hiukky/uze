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

import time

from shared.common import (
    check,
    check_absence,
    describe,
    observed_markers,
    provider_struct,
    start_provider,
)

#: The fixture's three shapes, by canonical name.
DEFAULT = "commit"
MODEL_ONLY = "analyze"
USER_ONLY = "review"


def body_marker(skill):
    """What proves a Skill was *invoked* rather than merely offered.

    A catalog carries a Skill's name and description; its **body** reaches
    the model only once the harness expanded it, which is what invoking
    does. Each fixture body ends with its own marker, so a harness that
    lists a Skill cannot produce one by accident — and neither can the
    provider, whose canned final text is the same for every turn.
    """
    return f"UZE_SKILL_BODY_{skill.upper()}"


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
        _assert_invocation(cfg, prov_ip, bindings)


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


def _await_marker(cfg, marker, timeout=45.0, gap=2.0):
    """Waits for `marker` to appear in the provider's structural summary.

    A request lands when the harness sends it, not when the driver stops
    reading the screen, and the summary is written per request. Sampling
    once is a race the check loses silently: Claude expanded a Skill's body
    into the very request that carried `<command-name>/flow:commit`, and a
    single read taken a moment early called that a failure. Returns as soon
    as the marker is seen; a `False` here means it never arrived within the
    window, which is what an absence needs too.
    """
    deadline = time.time() + timeout
    while True:
        markers = observed_markers(provider_struct(cfg), "skill_markers")
        if markers.get(marker):
            return True
        if time.time() >= deadline:
            return False
        time.sleep(gap)


def _assert_invocation(cfg, prov_ip, bindings):
    """Invoking a Skill the way a person does, and proving it took effect.

    The catalog above proves a Skill is *offered*. Offered is not invoked,
    and for four harnesses that was the whole of the user half: a substring
    on a screen. It would have read the same if the harness had changed its
    invocation syntax underneath — which one of them did, from `/name` to
    `@name`, with nothing here noticing.

    So this types what that harness's own user types, and then reads the
    request the harness sent: the Skill's body marker is there only if the
    body was expanded into the model's context.
    """
    invoke = getattr(bindings, "invoke", None)
    if invoke is None:
        check(
            "skill-invocation-not-driven",
            True,
            f"{bindings.harness} has no invocation binding yet",
            kind="adapt",
        )
        return

    # Its own provider, twice over. Invoking a Skill puts that Skill into a
    # request by design, so these turns would otherwise be read by whatever
    # asks next whether a Skill "reached the model" — which is how this
    # assertion first turned a passing `user-only-skill-hidden-from-model`
    # red without the harness having changed at all. The struct starts
    # empty here, and starts empty again for whoever runs after.
    prov_ip = start_provider(cfg, "static")

    with bindings.session(cfg, prov_ip) as tui:
        plain, matched = bindings.prepare(tui)
        check(
            "skill-invoke-tui-ready",
            bool(matched),
            f"{bindings.harness} reached its prompt"
            if matched
            else plain[-160:].replace("\n", " "),
        )
        if not matched:
            return

        rendered = invoke(tui, DEFAULT)
        tui.snapshot("invoke-default", rendered)
        invoked = _await_marker(cfg, body_marker(DEFAULT))
        check(
            "skill-default-is-invocable",
            invoked,
            f"`{DEFAULT}`'s body reached the model after the user invoked it"
            if invoked
            else f"invoked `{DEFAULT}`, but its body never reached the model: "
            f"{rendered[-160:]}".replace("\n", " "),
        )
        if not invoked:
            # Everything below distinguishes one invocation from another,
            # which is meaningless once no invocation works at all.
            return

        rendered = invoke(tui, USER_ONLY)
        tui.snapshot("invoke-user-only", rendered)
        check(
            "skill-user-only-is-invocable",
            _await_marker(cfg, body_marker(USER_ONLY)),
            f"`{USER_ONLY}` declares user: true, and invoking it reached the model",
        )

        if not _declined(bindings, "model-only-is-not-invocable"):
            rendered = invoke(tui, MODEL_ONLY)
            tui.snapshot("invoke-model-only", rendered)
            # The same window the positives are given, so an absence means
            # it never arrived rather than that nobody waited.
            check_absence(
                "skill-model-only-is-not-invocable",
                not _await_marker(cfg, body_marker(MODEL_ONLY)),
                settled=True,
                detail=f"`{MODEL_ONLY}` declares user: false, so invoking it "
                "must not reach the model",
            )

    start_provider(cfg, "static")
