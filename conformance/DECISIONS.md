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

**Followed up again (2026-09-02), with the flag now served.** The gate has
a name: `json-hooks-enabled` ("Whether to enable hooks based on json
files"), a `flexibleRollout` at 100% constrained to `ide IN [jetski]`,
delivered by Unleash at `GET https://antigravity-unleash.goog/api/client/
features`. The Lab now serves that plane — a TLS listener on 443 beside the
Gemini stub, replaying the recorded feature verbatim — and the run's
provider log proves the harness consumes it
(`[provider:flags] GET antigravity-unleash.goog/api/client/features`,
`POST /api/client/register`, `POST play.googleapis.com/log`).

Hooks still do not execute, and the reason is now measured rather than
guessed. Serving the flag with its constraint dropped
(`UNLEASH_UNCONSTRAINED=1`, the provider's own diagnostic switch) changes
nothing, so the strategy is not what fails. Reading the binary says why:
`enable_json_hooks` is field 17 of `exa.cortex_pb.CustomizationConfig`, a
*model-backend* config, and `ListExperiments` is a
`google.internal.cloud.code.v1internal` RPC. The CLI receives that config
only when it speaks the CloudCode backend protocol — which it does when
signed in to a Google account. The Lab runs it in Gemini API-key mode
(`GEMINI_API_KEY` + `GOOGLE_GEMINI_BASE_URL` at the synthetic stub), so it
never speaks CloudCode, never receives a `CustomizationConfig`, and
`enable_json_hooks` is never set. `json-hooks-enabled` is the server-side
switch that decides what CloudCode *puts in* that config; handing it to the
CLI does not synthesize the config.

This is the vendor's own open bug, not only our measurement:
google-antigravity/antigravity-cli#893, "Hooks from .agents/hooks.json are
loaded but never executed when authenticated via GEMINI_API_KEY (headless
-p, 1.1.22)" — labelled bug / comp:auth / comp:customizations, assigned,
open since 2026-08-28, with the identical symptom (`loaded 1 named hooks`,
tools run, hooks never fire) and the same contrast (OAuth works). Confirmed
on the host on 2026-09-02 with agy 1.1.22 online: the same global hook
fires PreInvocation under OAuth and never fires under GEMINI_API_KEY.
Issue #78 is the wider context — Google states the Gemini API key path is
not supported currently, and that is the mode this vertical runs in.

So the flag plane is a prerequisite that is now in place and no longer a
confound, and the remaining distance is a different, larger piece of work:
serving the CloudCode `v1internal` protocol and running the vertical in
signed-in mode, which changes which backend the whole vertical exercises.
The ten declarations stand, with that reason recorded against them, and
they expire the way every declaration should — when the vendor closes #893
the gate opens and the registry escalates.

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

---

## The wrapper is what the Lab exercises, and `jq` is in the image

**Context.** Hooks are no longer delivered as a `uze hook-exec` command
line: each harness receives a generated `hooks/exec` shell wrapper (or, on
OpenCode, a generated plugin that is the same runtime), and the handlers
read `HOOK_*` and answer with an exit code (ADR-040). The wrapper reads the
harness's payload with `jq`, which the Lab image did not have.

**Chosen.** Install `jq` in `conformance/Dockerfile`. It is the delivered
artifact's own dependency, and a machine running delivered hooks needs it
the same way; the Lab should model that machine, not a machine where the
wrapper cannot run. The missing-dependency behaviour is not thereby
untested — the deterministic suite proves it directly, both directions
(`a_missing_wrapper_dependency_follows_the_groups_effect`).

**Discarded.** Writing the wrapper without `jq` (parsing nested JSON in
POSIX `sh` is exactly the fragility the capability exists to spare authors);
leaving `jq` out and asserting the fail-closed path in the vertical (that
proves the guard, and nothing about delivery).

---

## First-deny-wins keeps its proof by gaining a second fixture

**Context.** Under the exit-code contract an *allowed* handler has no
channel to speak on: exit 0 means allow, and stderr is only read on a
denial. `hook-plugin`'s second handler used to relay
`second-handler-reached` on an allow, which was both the ordering evidence
(absent in the deny scenario) and the reason its allow scenario could not
simply drop it.

**Chosen.** `hook-plugin`'s second handler now *denies* with that marker,
so the deny scenario's absence check keeps meaning "the second handler
could have spoken and did not". The allow scenario moves to a new fixture,
`hook-allow-plugin` — the same guard with nothing behind it — so an allowed
call still proves the intercepted tool really ran. Every check name and
count is preserved, and the "an allowance lets the next handler run" case
moved to the deterministic suite, where the wrapper can be observed
directly.

**Discarded.** Dropping the absence check (it is the only first-deny-wins
evidence in `hook-plugin`, and a check nobody wrote is a check nobody can
fail); giving the wrapper an allow-reason channel from stderr (a second
decision channel, invented for a test).

---

## The vocabulary row is asserted, not assumed

**Context.** The hook suites proved that a denial was relayed and that the
tool was blocked. They did not prove that the handler received the
*portable* context — which is the whole promise: one handler, unchanged,
on every harness.

**Chosen.** The `guard` fixture echoes what it was handed
(`tool=$HOOK_TOOL native=$HOOK_TOOL_NATIVE cwd=$HOOK_CWD`) into its denial
reason, and `hooks-deny-context-relayed` asserts `tool=shell` reached the
conversation on Claude and Codex — two harnesses whose shell tools are
`Bash`/`command` and `exec_command`/`cmd`, so the alias is evidence that the
translation happened. Antigravity would only re-measure its closed vendor
gate, and OpenCode's hook path uses a `native:` matcher (no alias by
definition), so neither gains the check.

---

## The Antigravity vertical runs the harness signed in

**Context.** Antigravity executes `hooks.json` hooks only when the CLI is
signed in to a Google account: the executor reads `enable_json_hooks`, which
arrives only over the CloudCode backend that mode speaks. Under
`GEMINI_API_KEY` — the mode the vertical ran in — hooks load, list, and
never run, for any event, vendor's own format included
(google-antigravity/antigravity-cli#893; #78 records that Google does not
support the API-key path at all). So every hook check on this harness was a
declaration, and UZE's hook delivery to Antigravity had never been asserted.

**Chosen.** Run the vertical signed in against a synthetic identity. The
provider's TLS listener already answered the flag plane; it now also answers
the identity (`conformance@uze.invalid`), the account/tier RPCs, the model
catalogue, and the model path itself — `v1internal:streamGenerateContent`,
whose request is unwrapped from `{project, requestId, model, …, request:{…}}`
and whose events are re-framed as `{"response": …}`, so the one model logic
(static/toolcall/`wants_function_call`/variations) serves both auth modes.
The gate stays a *live* precondition (`hooks > vendor`, a vendor-format deny
hook with no UZE in the loop) and it now passes, and one declared check
re-runs the same control hook on the API key so #893 stays on the report and
fails the day it is fixed.

**What the run then found.** With that gate open, UZE's own hook checks
still could not be asserted — for a *different*, newly measured reason: this
harness reads no `hooks.json` from a plugin directory, which is where UZE
delivers Antigravity's hooks. So the vertical grew a second live
precondition, `hooks > delivery`, and the UZE hook checks stay declared
against it rather than against #893. Retiring them was the change's goal;
retiring them on a run that does not prove them would have been the one
thing the gate exists to prevent.

**Discarded.** Keeping API-key mode and the old declarations (their stated
reason had become false — the vertical measures a different gate now); a
second vertical for the API-key mode (one declared check measures the same
thing for one extra turn); asserting the hook suites without measuring both
gates (a green nobody measured).

---

## What the signed-in provider serves, and what it cannot claim to have observed

**Context.** The endpoint list and the request/response shapes were captured
on a real signed-in account through the sanitizing proxy (structure only).
Two things the CLI needs were *not* in that capture, and the run stops dead
without them: the body it reads its model catalogue from, and the fact that
the model request arrives `Transfer-Encoding: chunked`.

**Chosen.** Both were derived from the binary under test rather than
invented or guessed:

- `fetchAvailableModels` is answered with a catalogue whose shape is read
  from the `FetchAvailableModelsResponse` descriptor the binary embeds (the
  CLI parses it with protojson: a wrong cardinality is a hard error, and it
  found two — `tieredModelIds` tiers are repeated, not single). Its content
  is the Lab's, not a recording: two models, because the harness uses two —
  the user's turn and a lighter side call — with the ids this binary asks
  for in API-key mode and the `MODEL_PLACEHOLDER_*` enum values its own
  registry resolves. Without an enum the executor dies with "neither
  PlanModel nor RequestedModel specified"; that is the field, not a guess
  about semantics, and no tier, quota or entitlement meaning is invented.
- `capture.read_body` now decodes chunked bodies. Reading by
  `Content-Length` handed the provider an empty body, which reads as a turn
  that declared no tools — the kind of silent nothing the Lab exists to
  catch.

Two provider defects surfaced the same way and are fixed here: the RPC was
matched against the raw path, so `…:streamGenerateContent?alt=sse` fell to
the catch-all and the harness retried its own turn 798 times; and the
structural summary read only the *first* `tools` entry, while signed in the
harness sends one entry per tool.

**Discarded.** Recording a real account's catalogue (it would put a live
backend's answer, and its churn, in the repository); leaving
`fetchAvailableModels` at `{}` and declaring the vertical blocked (the
binary's own descriptor answers the shape question, and the run proves the
answer); a second provider process for the model path (the listener that
already terminates TLS for these hosts is the one the harness dials).

---

## A second live precondition: does the harness load what UZE delivered?

**Context.** `hooks > vendor` answers "does this harness run `hooks.json`
hooks at all", and signed in it does. It does not answer "does it run the
ones UZE delivered" — and on 1.1.24 it does not: the session reports
`loaded 0 named hooks from 0 hooks.json file(s)` while the generated
plugin's `hooks.json` sits in `~/.gemini/config/plugins/hook-plugin/`,
counted by `agy plugin validate`, with the plugin listed as having a `hooks`
component and `"enabled": true` in `config.json`. No `skipping hooks.json
at …` and no `No hooks.json found at …` appear either: the file is never
opened. The vendor's own shipped plugin guide says the opposite — "Hooks
defined in `plugins/<name>/hooks.json` are registered and run during the
agent's lifecycle".

**Chosen.** Measure it, every run, as `hooks > delivery`: a headless start
with the hook plugin installed, reading the harness's own count out of its
log. It is the cheapest possible probe (no TUI, no turn), it names the
cause in the verdict instead of leaving three suites failing with empty
marker lists, and — like the vendor gate — it expires by itself: the day the
harness scans plugin directories the check passes and the gate escalates
every declaration that leans on it.

**Discarded.** Declaring the UZE hook checks with the old #893 reason (it is
now false, and a stale reason is worse than none); moving UZE's Antigravity
hook delivery to the shared `~/.gemini/config/hooks.json` to make the suites
green (that is a product decision about delivery routes and receipt
ownership — ADR-033/ADR-040 — not something a Lab change may decide);
leaving the three suites to fail (a red the reviewer cannot act on, where a
declaration names exactly what the vendor must change).
