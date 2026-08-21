# UZE Harness Conformance Lab

Test-only infrastructure for running **real** harness CLIs in a disposable
environment. It is not part of the UZE product crate and it does not mock a
harness, implement a provider, or call an integration directly.

## Tiers

The lab is split by **what each tier can fail on**. That separation is the
whole design: when a deterministic wiring check and a probabilistic model turn
share one script, a fixture defect presents as a flaky environment.

| Tier | Proves | Needs | Can a model fail it? | Cost |
|---|---|---|---|---|
| **1. Attachment** | UZE wrote what it promised, and every receipt reconciles | nothing | no | ~1s |
| **2. Discovery** | each harness itself reports the attachment as usable | the harness binary | no | ~7s |
| **3. Behavior** | a real model turn surfaces a proof the prompt never contains | gateway + provider credential | **yes** | ~60s + tokens |
| **Baseline** | *control*: does the harness reach a project-local skill with UZE absent? | gateway + provider credential | **yes** | ~30s + tokens |

Tiers 1 and 2 run with `--network none`, a read-only root filesystem and no
credential of any kind. They are the pair a CI gate should run on every
change. Tier 3 and the baseline are opt-in.

The baseline never gates the exit code — it is not a verdict on UZE. It probes
a *different* surface than Tier 3: a skill in the project's own
`.agents/skills/`, where UZE instead attaches at user scope from the Store and
leaves the project untouched. Tier 3's workspace holds no skill file at all,
so a baseline pass cannot explain a behavior pass. See
[the fixture's README](fixtures/native-skill-discovery/README.md) for the full
reading table.

## Running

```bash
docker compose --env-file e2e/.env -f e2e/compose.yaml build harness

# Tiers 1 + 2 — offline, no credential, every declared harness.
docker run --rm --network none \
  --tmpfs /tmp:rw,noexec,nosuid,size=128m,uid=1000,gid=1000,mode=700 \
  --tmpfs /work:rw,noexec,nosuid,size=256m,uid=1000,gid=1000,mode=700 \
  --read-only --security-opt no-new-privileges:true --cap-drop ALL \
  -e HOME=/work/home -e UZE_HOME=/work/uze-home \
  e2e-harness:latest uze-conformance deterministic

# Tier 3 — needs the gateway up and a provider key supplied to it only.
docker compose --env-file e2e/.env -f e2e/compose.yaml up -d gateway
docker compose --env-file e2e/.env -f e2e/compose.yaml run --rm harness \
  uze-conformance behavior

# Every tier, including the native-discovery control.
docker compose --env-file e2e/.env -f e2e/compose.yaml run --rm harness \
  uze-conformance all
```

`uze-conformance --help` lists every flag. `--json` emits the evidence record
instead of the summary; `--harness claude,codex` narrows the run. The process
exits non-zero when any tier fails.

## Harnesses

| Harness | Package-native | Skill | MCP | Status |
|---|---|---|---|---|
| Claude Code | — | managed user-scope reference | `claude mcp add --scope user` | v0 |
| Codex | local marketplace catalogue (published) | managed user-scope reference | `codex mcp add` | v0 |
| OpenCode | — | managed user-scope reference (standard) | `opencode.json` `mcp.*` | v0 |
| Gemini CLI | `extensions link` at the store (no catalogue) | managed user-scope reference (standard) | `gemini mcp add --scope user` | **experimental** |

Codex and Gemini deliver a whole package natively through incompatible
mechanisms — one needs a published catalogue, the other needs none — through
the same `IntegrationPort`. That pair is the evidence that package publication
and package-native delivery are independent concepts rather than one
Codex-shaped one.

## Adding a harness

Add one entry to `HARNESSES` in `src/harness.rs`. Nothing else changes: every
tier is generic over that table. An entry declares

- the id, the `uze setup` name, and the executable;
- one deterministic probe per **artifact kind** UZE delivers to it, each with
  the claim a pass establishes;
- the Tier 3 invocation, and any provider config the route needs.

A harness that offers no model-free way to report a delivery kind simply
declares no probe for it. The tier then records `Unverified` — absence of a
probe is a known gap, never a silent pass.

No expected attachment name appears in that table. Tier 1 reads the names UZE
actually attached out of `uze inspect --format json` and hands them to Tier 2,
so a fixture rename cannot pass against a stale constant.

## Isolation contract

What the L2 spec actually requires of a harness container: an empty `HOME`,
`UZE_HOME` and project directory; no host `HOME` and no Docker socket; and,
when a routed provider is used, **provider credentials available only to the
gateway service**. The lab adds a read-only root, tmpfs-only writes,
`cap_drop ALL` and `no-new-privileges` on top.

Network isolation is *not* part of that contract, and earlier revisions of this
file wrongly stated it was. The `conformance` network is internal because it
keeps a run reproducible — no dependency on npm, models.dev or vendor
telemetry being reachable — not because it isolates a credential. The gateway
already does that.

One consequence worth knowing: Claude Code's **interactive TUI** performs a
preflight against `api.anthropic.com` that ignores `ANTHROPIC_BASE_URL`, so it
cannot start on the internal network. Headless `claude -p` routes through the
gateway there without issue, which is what every tier uses.

## Gemini CLI — experimental, and what it cost

Gemini CLI is registered as an **EXPERIMENTAL / CONFORMANCE** fourth harness.
It exists to test whether the vendor-neutral core generalises, not as a v0
support claim. Three findings from that work are recorded here rather than
left in code comments, because each is a deliberate limitation someone will
otherwise rediscover the hard way.

**1. `gemini extensions link` is used as a managed integration mechanism,
though Gemini positions it as a development workflow.** The alternative,
`gemini extensions install`, copies the package into `~/.gemini/extensions` —
a second copy of bytes the Store already owns. That breaks install-once and
makes a later detach unable to distinguish UZE's copy from content the user
put there. `link` keeps the Store the single copy, runs non-interactively with
`--consent`, and leaves the stored package untouched on uninstall (verified).
The trade-off is real: UZE depends on a verb the vendor documents for
development.

**2. Enabled/disabled is a user preference, not an ownership signal, so it
takes no part in safe detach.** It is also not observable: the listing's
`isActive` stays `true` after `gemini extensions disable` (0.56.0), with the
real state living in `extension-enablement.json` as path-scoped override
globs. Reimplementing that resolution would mean guessing at vendor internals.
And even if it were observable, treating "disabled" as drift would stop
`uze remove` from detaching UZE's own extension — worse behaviour, not safer.
Ownership is proven by identity, source and install type.

**3. A Skill delivered outside an extension has no model-free discovery in
Gemini.** There is no `gemini skills list`; skills only appear in the listing
when they belong to an installed extension. The lab therefore declares no
probe for `SYMLINK_REFERENCE` on Gemini and the tier records `Unverified`,
rather than fabricating evidence from a command that proves something else.

Two smaller notes: `gemini extensions list --output-format=json` writes its
payload to **stderr** in 0.56.0, and a user-scope MCP server is suppressed
entirely in an untrusted folder, so its connection state is not a stable
signal. Gemini has no gateway-routable behavioural tier — a model turn needs a
real Google account — so its `behavior` spec is `None` and L2b records it as
`Unverified` instead of skipping it silently.

## Known coverage gaps

Recorded rather than papered over, because two harnesses reporting
`DiscoveryVerified` do not necessarily prove the same depth:

- **Codex has no model-free MCP health check.** `codex plugin list` proves the
  package installed; `codex mcp list --json` proves the envelope decomposed
  into an enabled server registration. Neither starts the server, so a
  Codex-only run passes with an unreachable MCP binary. Claude and OpenCode
  are probed one level deeper and do report connectivity.
- **The tool-name budget check is a floor, not a guarantee.** See
  `TOOL_NAME_RESERVE` in `src/tier.rs`.
- **Gemini proves registration, not connectivity, for MCP.** `gemini mcp list`
  emits no machine-readable output, and a user-scope server is disabled
  outright in an untrusted folder, so the probe asserts the entry is listed as
  stdio and says exactly that in its claim.
- **UZE does not plumb an `mcp.json` `env` block** into its vendor-config
  writes (`environment` is recorded as an empty reference list in all three
  integrations), so the fixture proof travels in `args`, the one channel every
  delivery route persists intact.

## Runner primitives

`src/lib.rs` owns the process boundary and nothing else:

```text
HarnessRunSpec    executable, args, env, HOME, UZE_HOME, cwd, timeout, stdin
HarnessRunResult  exit status, timeout, stdout, stderr, elapsed
```

It clears the inherited environment before spawning, so anything a harness
needs is declared rather than inherited from the operator's shell — including
`PATH`.

```bash
cargo test --manifest-path e2e/Cargo.toml
```

## Image

Pinned through build arguments:

```text
CLAUDE_VERSION=2.1.237
CODEX_VERSION=0.148.0
OPENCODE_VERSION=1.18.19
```

It contains the UZE release binary, the conformance runner, the MCP fixture
server, Claude Code, Codex, OpenCode, Git, `ripgrep`, and minimal runtime
dependencies. The image build may access package registries; runtime harness
containers receive neither host credentials nor a host HOME. OpenCode's
official runtime plugin dependency is baked in so its first-run bootstrap does
not need npm.

## Evidence tiers

| Tier | Scope |
|---|---|
| L0 | Pure Rust unit tests, in the source files themselves. |
| L1 | Product contracts in `tests/`: Store, planning, receipts, filesystem/config. Fully deterministic — `cargo test` runs every one, with nothing behind `#[ignore]` or an opt-in variable. |
| L2 | Opt-in Docker: real harness plus isolated local or routed inference. |
| L3 | Opt-in real vendor/provider conformance. |

Model quality is never a compatibility claim. A model that fails to exercise a
discovered capability is reported as `ModelFailure`, which leaves attachment
and discovery evidence untouched. See the
[ecosystem research](../openspec/changes/establish-local-real-harness-conformance/research-notes.md)
and [the provider contract](provider-contract.md).

## Where a test belongs

| It … | Lives in |
|---|---|
| exercises one function or type | a `#[cfg(test)]` module beside the code |
| uses the `uze` library or the `uze` binary, deterministically | `tests/` |
| needs a real harness CLI | here, as a tier |

`tests/` must never spawn a harness binary or read a credential. Earlier
revisions did both, gated behind `UZE_E2E_UZE_HARNESSES`, which meant those
probes ran against the operator's own machine and never ran in CI — and left
`src/conformance.rs`, a process runner, exported from the product's public
API to serve them. Both are gone.
