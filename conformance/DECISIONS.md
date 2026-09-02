# Decisions taken while restructuring the Lab

Recorded because the operator was not available to arbitrate. Each is the
most conservative, most reversible option available at the time; each names
what was discarded so it can be revisited cheaply.

See `openspec/changes/assert-one-capability-contract/` for the change these
belong to.

---

## A vertical without bindings keeps running unchanged

**Context.** Adopting the contract needs every harness to grow a
`bindings.py`. Doing all four in one step makes an unreviewable diff and
risks four simultaneous regressions.

**Chosen.** `lab.load_bindings` returns `None` for a harness that has no
bindings module, and the run then behaves exactly as before. Adoption is
per harness.

**Discarded.** Requiring bindings for every harness at once (a single large
change, all-or-nothing); a feature flag (a second way to say the same thing
as "the module is absent").

---

## `Unsupported` is asked and answered, never omitted

**Context.** Codex cannot enforce a canonical `user: false` — it documents
no way to disable explicit `$skill` invocation, and the product already
routes that as `Degraded`. The contract asks every harness the same
questions, so it will ask one Codex cannot satisfy.

**Chosen.** The harness declares it through `bindings.unsupported(prop)`,
returning a reason. The run records an `adapt` result carrying that reason,
beside the passes.

**Discarded.** Omitting the check for that harness — which is precisely how
the old suite hid divergence: a check nobody wrote is a check nobody can
disagree with. Also discarded: failing the harness for a limitation the
product already reports honestly.

---

## Absence assertions are gated on a presence assertion

**Context.** `user-only-skill-hidden` passed for months while nothing at all
was delivered: "the policy worked" and "the surface was empty" are the same
observation.

**Chosen.** Every absence check in the contract runs only after a presence
check proved the surface was populated. `check_absence` already refuses an
unsettled turn (ADR-035); this adds the other half.

**Discarded.** Trusting the settled-turn contract alone — it proves the turn
finished, not that the surface has content.

---

## The driver returns the text a wait consumed

**Context.** `wait_for` consumes reads; a `collect()` afterwards sees an
empty screen. Every vertical had learned this separately and expressed it
differently, and the contract's first run failed on exactly this.

**Chosen.** `Tui.until()` returns the plain text that satisfied the wait.

**Discarded.** Re-reading after the wait (racy); having each binding
remember (which is what produced four different spellings of it).

---

## Ready markers must name the prompt, not the process

**Context.** The first Codex bindings used `("Ask Codex", "▌", "codex")`.
`"codex"` matched the onboarding splash, so `skill-tui-ready` passed against
a screen that accepts no input — the same class of false pass the audit had
just found elsewhere.

**Chosen.** `ready_markers` names the real prompt only, and a harness with a
pre-prompt flow overrides `prepare()` to drive it.

**Discarded.** Keeping a loose marker with a longer warmup (hides the
problem behind a sleep).

---

## Three of four harnesses do not enforce `user: false`

**Context.** Asking every harness the same question surfaced that Codex,
OpenCode *and* Claude Code all offer a model-only Skill to explicit user
invocation. Only Antigravity enforces it, through
`disable-slash-command: true`. No vertical asked before, so a canonical
policy honoured on one harness out of four was invisible.

**Chosen.** Each declares it through `unsupported`, with a reason grounded
in what the product already reports where one exists — Codex routes
model-only as `Degraded`, OpenCode routes every Skill as `Adaptable`. Claude
is the weakest of the three: it *has* `disable-model-invocation` (UZE uses
it to honour `invoke.model: false`), so the inverse may exist and simply not
be used. Its reason says so rather than claiming a limitation.

**Discarded.** Failing both (the product does not claim to enforce it, so
the suite would be asserting something never promised); staying silent (the
old behaviour, and the reason this went unnoticed).

**Worth a second look — this is the most important thing here.**
`invoke.user: false` is enforced on **one harness out of four**. ADR-030
makes invocation policy the canonical Skill's portable semantics; a
canonical semantic delivered once out of four times is either a product gap
or an over-promise in the ADR. Claude specifically deserves a check: if an
inverse of `disable-model-invocation` exists, this is a bug in the Claude
integration, not a harness limitation.

**Followed up (2026-09-02).** The inverse exists and is documented:
`user-invocable: false` hides a Skill from the `/` menu and refuses
`/name`. UZE already emitted it; the declaration survived because the
binding read the `/skills` *management* view, which lists every Skill
whatever its policy. The Claude binding now reads the `/` completions and
the check is asserted, not declined. Codex and OpenCode keep their
declarations — those are product routing (`Degraded`/`Adaptable`), not an
unread control.

---

## OpenCode's MCP presence is read from connection state, not a name

**Context.** The contract asks whether the harness shows the delivered
server in its MCP inventory. Three harnesses print `uze-conformance`.
OpenCode's `/mcps` is a toggle surface: the captured screen carries
`Connected` but was not observed to carry the server id.

**Chosen.** `names_server` is a binding, like `lists` — the harness says how
its surface spells a server. OpenCode answers from the connected row, the
same signal its own vertical already trusted.

**Discarded.** Asserting the id anyway (asserting fiction about a vendor's
surface); weakening the contract for everyone to accept a bare "Connected"
(three harnesses can prove more, and a contract should ask for the most any
of them can give).

**Weaker than the others, on purpose.** A connection row proves a server is
attached, not *which*. If OpenCode grows a surface that names it, this
binding should tighten.

---

## UZE's own vertical asserts the bridge only for a contributing project

**Context.** `uze context reconcile` in a project with a hand-written
`AGENTS.md` and no installed plugins reports the Claude bridge as `Missing`
and writes nothing. The code gates the bridge on a *package contribution*
(`plan_action_for_region(would_have_contribution, …)`), so with no packages
there is nothing to bridge.

`AGENTS.md`'s own description reads more broadly — "`CLAUDE.md` is the one
generated bridge (`@AGENTS.md`) produced by `uze context reconcile`" — with
no mention of that condition.

**Chosen.** The phase asserts that `reconcile` *names* the bridge and its
state for a harness it detected — which it does, reliably. Asserting the
bridge **file** needs a fixture that contributes instructions to
`AGENTS.md`, and none of the Lab's fixtures do: `flow` carries agents and
skills only. Adding one touches every vertical's shared fixture set, which
is more than this change should absorb.

**Discarded.** Asserting a bridge for a package-less project (would fail
against behaviour that may well be correct, on a reading of a doc sentence);
asserting the `Missing` report as correct (would freeze a behaviour nobody
confirmed was intended).

**Open question.** A project with a hand-written `AGENTS.md` and no plugins
gets no `CLAUDE.md`, so Claude Code does not receive that project's context
through UZE. If that is intended, `AGENTS.md`'s sentence should say so. If
not, it is a product gap this vertical is now positioned to catch.

---

## No Hook contract yet

**Context.** The task list calls for a `hook` contract beside `skill` and
`mcp`. The investigation that preceded this change established that Claude
Code's scripted tool call has been rejected before any hook runs since at
least 2.1.252, and that Antigravity 1.1.24 produces no `functionResponse`
at all. Both verticals fail their hook phases honestly today, and each was
recorded as needing its own change.

**Chosen.** No hook contract. Writing one now would encode the current
broken state as the shared expectation, and a contract that two of four
harnesses cannot even reach is not a contract — it is a pending bug with
extra ceremony.

**Discarded.** Writing it and letting two harnesses declare `unsupported`
(that mechanism exists for a *harness limitation*, not for an unfixed
scenario — using it here would launder a bug into a documented gap, which
is the exact failure this whole change was made to stop).

**Unblocked by.** Whatever the Claude and Antigravity hook changes discover
about how each harness actually accepts a scripted tool call. That
knowledge is what the contract must be written against.

**Followed up (2026-09-02).** Both scripted calls were the Lab's, not the
harnesses'. Claude Code accumulates a tool's input only from
`input_json_delta` events — the provider put it on `content_block_start`,
so every `Bash` call arrived empty. Antigravity 1.1.24 validates a call
against the tool's declared schema before any hook runs — the provider
sent `command`, the tool declares `CommandLine`/`Cwd`/`WaitMsBeforeAsync`
plus the `toolSummary`/`toolAction` pair — and answers the harness's first
request, which is now a side call to a lighter model with no tools. With
both fixed, Claude's hook phase is real (deny relayed, tool blocked,
first-deny-wins, allow executes); Antigravity's MCP call is real too.

Antigravity's hooks are a different case. UZE's generated plugin
`hooks.json` had its named entries wrapped under a `hooks` key, which the
vendor reads as one dead hook named `hooks` — fixed, and `agy plugin
validate` now counts every group. But no `hooks.json` hook executes in the
Lab session at all: a deny hook in the vendor's own format at the vendor's
own shared path is loaded (`hooks_manager: loaded 1 named hooks`), listed
by `/hooks`, and never runs — not for any event, not even a `touch`, on
1.1.22 and 1.1.24 alike (experiments `antigravity/hook-print` and
`antigravity/hook-tui`). The executor is gated by
`CustomizationConfig.enable_json_hooks`, which the CLI's SDK takes from a
server-delivered feature provider the offline Lab never receives; it is
not a setting, a plugin field or agent frontmatter, so nothing UZE ships
can open it and nothing the Lab may honestly stub can either.

So the Antigravity vertical measures the gate instead of assuming it:
`hooks > vendor` runs that control hook first. When it denies, the UZE
hook checks are asserted exactly as on Claude; when it does not, each is
recorded as a declaration carrying that reason, registered per version.
The registry escalates the moment the vendor opens the gate, so the
declarations cannot outlive the limitation. This is the `unsupported`
mechanism used for what it is for — a harness limitation measured live —
and the reason the earlier "no hook contract" objection no longer holds:
the shared expectation is now known on all four harnesses.

---

## No Agent contract yet

**Context.** Two verticals assert `agent-visible-in-tui`; two do not. The
canonical Agent capability is delivered on all four.

**Chosen.** Deferred. `skill` and `mcp` establish the seam and prove it
across four harnesses; a third capability adds coverage but no new
structure, and the remaining budget was better spent proving the seam works
than widening it.

**Discarded.** A partial Agent contract covering only the two harnesses
that already assert it — that is the per-vertical divergence this change
exists to remove.
