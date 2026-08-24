# Testing

Tests are organized by **what each one can fail on**. That is the only
organizing principle here: a test that can fail for two unrelated reasons
cannot tell you which one happened. The canonical reference for the levels,
the domain layout and the coverage matrices is
[`tests/README.md`](../tests/README.md); this file is the short version.

| Level | Lives in | Can fail because | Needs | Runs in CI |
|---|---|---|---|---|
| **L0 — Unit** | `#[cfg(test)]` beside the code | the logic is wrong | nothing | always |
| **L1 — Component/Contract** | `tests/` | UZE's observable behavior is wrong | nothing | always |
| **L2 — Real-harness conformance** | `conformance/` | UZE, a real harness, or vendor semantics | the disposable Docker image | opt-in (release gate) |
| **L3 — Product acceptance** | `tests/acceptance/` | the public UZE flow is broken | nothing | always |
| **L4 — Model behavioral** | `conformance/` | the model/provider path | gateway + credential | optional / manual |

`cargo test --workspace` runs L0, L1 and L3 completely. If it passes, the
deterministic product contract holds (620 tests at the time of writing, with
the 12-scenario acceptance suite as the release signal). If the Lab's L2 set
is additionally green, the real primary harnesses agree with what UZE
delivered.

## L0 — Unit

Ordinary Rust unit tests next to the code they cover. The Lab's own runner
tests (`cargo test --manifest-path conformance/Cargo.toml`) are L0 for that crate:
env clearing, timeouts, evidence classification, fixture composition and
never a real harness.

## L1 — Component / Contract

`tests/` is domain-organized (see `tests/README.md` for the tree): cli,
memory, packages, workspace, lifecycle, projection, integrations (with
per-harness invocation-policy semantics), acceptance.

### The rule for `tests/`

**Never require a real harness CLI. Never read a credential. Never use
`#[ignore]`.**

This is load-bearing. Earlier revisions had real-harness probes in `tests/`
behind `UZE_E2E_UZE_HARNESSES`, which meant they ran against the developer's
own machine and home directory when someone remembered the variable, and
never ran in CI at all. It also forced the product to export a process runner
from its public API purely to serve those tests. Both are gone.

## L2 — Real-harness conformance

`conformance/` is a separate crate and a disposable Docker image. It runs the real
Claude Code, Codex, OpenCode and Antigravity CLIs against a real UZE
installation, then records what each harness itself reports. It consumes the
single canonical fixture source (`tests/_fixtures` via `uze-testkit`) and
never duplicates it. Full detail in [`conformance/README.md`](../conformance/README.md).

The Lab's own evidence levels are flat: **L2** (no model, no provider, no
credential), **L4** (opt-in model behavior) and **CONTROL** scenarios that
measure the harness/provider path with UZE absent — a control is never a UZE
verdict. Scenario set: R1-R7 plus G1 (golden chain) at L2, B1-B3 at L4, C1
control.

```bash
docker compose --env-file conformance/.env -f conformance/compose.yaml build harness

# offline, no credential, real harnesses
docker run --rm --network none \
  --tmpfs /tmp:rw,noexec,nosuid,size=128m,uid=1000,gid=1000,mode=700 \
  --tmpfs /work:rw,noexec,nosuid,size=256m,uid=1000,gid=1000,mode=700 \
  --read-only --security-opt no-new-privileges:true --cap-drop ALL \
  -e HOME=/work/home -e UZE_HOME=/work/uze-home \
  uze-e2e-lab:latest uze-conformance l2
```

The runner's own unit tests are L0 for that crate:

```bash
cargo test --manifest-path conformance/Cargo.toml
```

## L3 — Product acceptance

`tests/acceptance/` drives the real `uze` binary through public,
user-level scenarios (A1-A12, including `golden_environment_is_healthy`) in a
fully isolated `TestEnvironment`, with fake harness CLIs where a process
boundary is needed. Deterministic, no credentials, always in CI.

## L4 — Model behavioral

Real harness plus a real (or gateway-routed) model. Manual/periodic, recorded
as release evidence. Never automated in ordinary CI, and never substituted
by an L2 pass.

---

## Where does a new test go?

```text
Does it need a real harness CLI?
├── yes ──> conformance/, as an L2 or L4 scenario (see conformance/README.md)
└── no
    └── Does it exercise a public UZE flow end to end?
        ├── yes ──> tests/acceptance/
        └── no
            └── Does it use the uze library or the uze binary, deterministically?
                ├── yes ──> tests/<domain>/
                └── no  ──> a #[cfg(test)] module beside the code
```

If a test would need a credential, a network call, or an environment variable
to be meaningful, it does not belong in `tests/`.

## Reading a conformance result

Two things are easy to misread.

**Equal states do not mean equal depth.** Codex and Claude can both pass an L2
probe while proving different things — Codex offers no model-free MCP health
check for marketplace plugins, so its record is registration-level, while
Claude and OpenCode report connectivity. The JSON evidence carries a `claim`
string per probe for exactly this reason. Known gaps are listed in
`conformance/README.md` and are reported as `Unverified`, never silently passed.

**A model failure is not an integration failure.** When a model declines to
use a capability that L2 already proved present, the record says
`MODEL_FAILURE` and the L2 verdict is untouched. Likewise a 429, a rejected
key or an expired quota reports as `PROVIDER_FAILURE`, never as a harness
defect.
