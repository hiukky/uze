# Testing

Tests are organized by **what each one can fail on**. That is the only
organizing principle here, and it is worth stating plainly: a test that can
fail for two unrelated reasons cannot tell you which one happened.

| Tier | Lives in | Can fail because | Needs | Runs in CI |
|---|---|---|---|---|
| **L0 — Unit** | `#[cfg(test)]` beside the code | the logic is wrong | nothing | always |
| **L1 — Contract** | `tests/` | UZE's observable behavior is wrong | nothing | always |
| **L2 — Conformance** | `e2e/` | UZE, a harness, or a model | Docker (+ a credential for some tiers) | opt-in |
| **L3 — Vendor** | manual / release | a real vendor route changed | vendor account | manual |

`cargo test` runs L0 and L1 completely: **139 tests, nothing behind
`#[ignore]`, nothing behind an opt-in environment variable.** If it passes, the
product contract holds. The properties those tests defend are listed in
[architecture invariants](architecture/invariants.md).

---

## L0 — Unit

Ordinary Rust unit tests in a `#[cfg(test)]` module next to the code they
cover. 70 of them across `src/`.

```bash
cargo test --lib
```

## L1 — Contract

`tests/` holds seven integration-test binaries. Each covers one seam of the
product, through its public API or its CLI — never through a harness.

| File | Covers | Tests |
|---|---|---|
| `store_engine_contract.rs` | Store installation and Engine composition: one package in, one `EffectiveEnvironment` out. Path derivation, symlink and permission preservation, multi-server MCP decomposition. | 7 |
| `integration_contract.rs` | The `IntegrationPort` seam: how each peer chooses an exposure route, how setup state changes that choice, and that attach/detach are idempotent. | 8 |
| `managed_exposure_lifecycle.rs` | What a peer actually writes and removes: prepare → managed artifact → cleanup, and that the caller's project is untouched throughout. Also that two peers sharing a projection path refuse to clobber each other. | 5 |
| `plugin_first_vertical_slice.rs` | End to end through the library: one install planned once for a native harness and a decomposed one, without a harness-specific copy. | 1 |
| `cli.rs` | The `uze` binary's own surface — commands, output formats, exit codes. Spawns `uze`, never a harness. | 8 |
| `package_containment.rs` | An installed package is self-contained: no persisted symlink resolves outside its root, and discovery terminates on any tree containment allows. | 10 |
| `git_acquisition.rs` | Git acquisition against **local bare repositories only** — ref resolution, pinning, reinstall/update semantics, subdirectory containment, credential rejection — plus the remote consent boundary. | 20 |

### The rule for `tests/`

**Never spawn a harness CLI. Never read a credential. Never use `#[ignore]`.**

This is load-bearing. Earlier revisions had real-harness probes here behind
`UZE_E2E_UZE_HARNESSES`, which meant they ran against the developer's own
machine and home directory when someone remembered the variable, and never ran
in CI at all. That is how "it works on my machine but not in Docker" happened:
the passing evidence came from an environment nobody else had.

It also forced the product to export `src/conformance.rs` — a process runner —
from its public API purely to serve those tests. Both are gone.

## L2 — Conformance

`e2e/` is a separate crate and a disposable Docker image. It runs the real
Claude Code, Codex, OpenCode and Gemini CLIs against a real UZE installation. Full
detail in [`e2e/README.md`](../e2e/README.md).

It is itself split by determinism:

| Sub-tier | Proves | Needs | Model can fail it |
|---|---|---|---|
| **Attachment** | every receipt UZE wrote reconciles | nothing | no |
| **Discovery** | each harness reports the attachment as usable | the harness binary | no |
| **Behavior** | a model turn surfaces a proof the prompt never contains | gateway + credential | **yes** |
| **Baseline** | *control*: does the harness reach a project-local skill with UZE absent? | gateway + credential | **yes** |

Attachment and discovery run offline, read-only, with no credential, in about
eight seconds. That pair is a viable CI gate. Behavior costs tokens and is the
only tier a model can fail.

```bash
docker compose --env-file e2e/.env -f e2e/compose.yaml build harness

# offline, no credential
docker run --rm --network none \
  --tmpfs /tmp:rw,noexec,nosuid,size=128m,uid=1000,gid=1000,mode=700 \
  --tmpfs /work:rw,noexec,nosuid,size=256m,uid=1000,gid=1000,mode=700 \
  --read-only --security-opt no-new-privileges:true --cap-drop ALL \
  -e HOME=/work/home -e UZE_HOME=/work/uze-home \
  e2e-harness:latest uze-conformance deterministic
```

The runner's own unit tests are L0 for that crate:

```bash
cargo test --manifest-path e2e/Cargo.toml
```

## L3 — Vendor

Real harness against a real vendor account and model. Manual, recorded as
release evidence. Never automated here, and never substituted by an L2 pass.

---

## Where does a new test go?

```text
Does it need a real harness CLI?
├── yes ──> e2e/, as a tier (see e2e/README.md, "Adding a harness")
└── no
    └── Does it go through the uze library or the uze binary?
        ├── yes ──> tests/, in the file matching the seam
        └── no  ──> a #[cfg(test)] module beside the code
```

If a test would need a credential, a network call, or an environment variable
to be meaningful, it does not belong in `tests/`.

## Reading a conformance result

Two things are easy to misread.

**Equal states do not mean equal depth.** Codex and Claude can both report
`DiscoveryVerified` while proving different things — Codex offers no model-free
MCP health check, so its probe establishes registration, not connectivity. The
JSON evidence carries a `claim` string per probe for exactly this reason. Known
gaps are listed in `e2e/README.md`.

**A model failure is not an integration failure.** When a model declines to use
a capability that attachment and discovery already proved present, the report
says `ModelFailure` and the integration verdict is untouched. Likewise a 429 or
a rejected key reports as `BlockedByEnvironment`, never as a harness defect.
