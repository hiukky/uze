# UZE Harness Conformance Lab

## Purpose

`tests/` proves UZE behavior. This Lab proves that **real external harnesses
correctly interpret what UZE produced** — the vendor-facing half of the
release story. It is not part of the UZE product crate, never mocks a
harness, and never re-tests what the main suite already owns (Store
ingestion, lock parsing, receipt serialization, exact-coverage internals,
CLI grammar, generic lifecycle planning).

## Evidence Model

The Lab uses two evidence levels plus controls — one vocabulary everywhere,
no separate tier system:

- **L2 — real-harness conformance**: real vendor CLI, isolated clean
  environment, **no model, no provider, no credential**.
- **L4 — model behavioral**: opt-in, needs the gateway; the only level a
  model can fail. Never a compatibility verdict.
- **CONTROL**: measures the harness/provider path with UZE absent. Never a
  UZE verdict.

Statuses (per record): `PASS`, `UNVERIFIED` (no probe exists — a known gap,
never a silent pass), `SKIPPED` (scenario not this harness's / binary
absent), `INFRA_FAILURE` (Lab machinery), `HARNESS_FAILURE`, `PROVIDER_FAILURE`,
`MODEL_FAILURE`, `CAPABILITY_FAILURE`. A `MODEL_FAILURE` at L4 never
downgrades an L2 record.

Every record carries its claim: `harness`, `scenario`, `level`,
`capability`, `status`, `claim`, `evidence`, `elapsed` (`--json`).

## Architecture

```text
tests/_fixtures (single canonical source)
        │  (via uze-testkit: fixture lookup, safe paths, real-home guards)
        ▼
composed canonical package ──> real `uze` setup/install ──> real harness ──> vendor's own report
```

The Lab consumes the **same** `canonical/`, `foreign/` and `golden/` fixtures
`golden_environment_is_healthy` uses; it never maintains a second canonical
tree. Vendor-native envelope bytes come from `fixtures/foreign/codex/…`; the
Lab composes them with canonical skill/MCP bytes (and injects per-run proof
values) because no canonical package ships every delivery shape at once —
composed by the runner, not duplicated on disk.

## L2 Scenarios

| Id | Scenario | External claim |
|---|---|---|
| R1 | canonical Skill discovery | the real harness recognizes the UZE-delivered Skill |
| R2 | user-only invocation policy | the harness's model-visible prompt is honest about who may invoke (Codex `debug prompt-input`) |
| R3 | package-native install | the harness's own registry reports the UZE-installed package |
| R4 | MCP registration/discovery | the harness registers — and where possible connects — the UZE-delivered server |
| R5 | lifecycle remove/reinstall | the harness agrees at every phase |
| R6 | runtime-shim boundary | the real harness still resolves through UZE's shim |
| R7 | repeated-setup idempotency | Antigravity (truthful staged copy) stays consistent across a repeat `uze setup` — external evidence for the `attach_package_to` Matched-receipt guard |
| G1 | golden environment chain | the same golden fixture UZE declares healthy is reported healthy by the real harness |

## L4 Scenarios

- **B1** normal Skill model behavior · **B2** user-only Skill explicit
  invocation · **B3** MCP proof-tool invocation.

Opt-in. Distinguish `MODEL_FAILURE` (capability not exercised) from
`HARNESS_FAILURE`/`PROVIDER_FAILURE` and never conflate with L2.

## Harness Matrix (built from the current code)

| Harness | Version (recorded by R6 evidence) | Package-native | Skill | Invocation policy | MCP | Lifecycle | L2 status (this refactor) |
|---|---|---|---|---|---|---|---|
| Claude | latest-at-build | generated package (`claude-plugin-generated`) | generated envelope skills | no model-free surface → UNVERIFIED | `claude mcp list`: **connected** | remove/reinstall ✓ | all primary scenarios PASS (host 2.1.241 also green) |
| Codex | latest-at-build | generated + explicit envelope (`uze-store` / `uze-local`) | plugin list installed+enabled | `codex debug prompt-input` — default listed; user-only **listed in multi-harness shared root** (see Known Gaps) | `codex mcp list --json` returns no plugin-server entries → UNVERIFIED, not a pass | ✓ | R1/R3/R5/R6/G1 PASS; R2/R4 see Known Gaps |
| OpenCode | latest-at-build (V1 channel) | N/A (no package-level native concept) | `debug skill` resolved symlink | no model-free surface → UNVERIFIED | `mcp list` connected | ✓ | container verdict only (host binary is the V2 preview) |
| Antigravity | latest-at-build | `agy plugin install` staged copy + `plugin list` imports | import manifest components | no model-free surface → UNVERIFIED | per-plugin `mcpServers` component (global `agy mcp list` is empty) | ✓ incl. R7 idempotency | all PASS (real 1.1.19) |

## Isolation

- Empty `HOME`/`UZE_HOME` per run under a fresh disposable root; no host
  HOME; no Docker socket; every spawned process runs with a **cleared ambient
  environment** (`HarnessRunSpec`), so nothing is inherited from the
  operator's shell (PATH included).
- Containers: read-only root, tmpfs-only writes, `cap_drop ALL`,
  `no-new-privileges`, unprivileged user, trusted-shell entrypoint asserting
  HOME/UZE_HOME.
- Safety guard from `uze-testkit`: any run root that could overlap the
  developer's real home **panics immediately** (`safe_root`).

Credential isolation is real and verified by the runner's own tests: the
provider key is only ever injected into the gateway service; deterministic L2
never receives it. Network is **not** the security boundary — credential
isolation and container isolation are. L2 runs use `--network none` where
practical; the `conformance` network is internal to keep the run reproducible,
not because network isolation protects a credential (the gateway already
does). Claude Code's interactive TUI preflight ignores `ANTHROPIC_BASE_URL`,
so it cannot start on the internal network — headless `claude -p` is what
every scenario uses.

## Running

```bash
docker compose --env-file e2e/.env -f e2e/compose.yaml build harness

# L2 — offline, no credential, every declared harness (release gate).
docker run --rm --network none \
  --tmpfs /tmp:rw,noexec,nosuid,size=128m,uid=1000,gid=1000,mode=700 \
  --tmpfs /work:rw,noexec,nosuid,size=256m,uid=1000,gid=1000,mode=700 \
  --read-only --security-opt no-new-privileges:true --cap-drop ALL \
  -e HOME=/work/home -e UZE_HOME=/work/uze-home \
  uze-e2e-lab:latest uze-conformance l2

# L4 — needs the gateway up and a provider key supplied to it only.
docker compose --env-file e2e/.env -f e2e/compose.yaml up -d gateway
docker compose --env-file e2e/.env -f e2e/compose.yaml run --rm harness \
  uze-conformance l4

# Every level, including the control.
docker compose --env-file e2e/.env -f e2e/compose.yaml run --rm harness \
  uze-conformance all
```

`uze-conformance --help` lists every flag. `--json` emits the evidence
record; `--harness claude,codex` narrows the run. Only L2 failures gate the
exit code.

On a developer machine with real harness CLIs, the same runner works against
the host binaries for quick exploration:

```bash
cargo run --manifest-path e2e/Cargo.toml -- l2 \
  --root /tmp/uze-lab --uze target/debug/uze \
  --mcp-binary target/debug/uze-mcp-conformance-fixture
```

Host results are only authoritative when the host binary version matches the
version matches the channel at build time; the image is the verdict.

## Adding Harness

Add one entry to `HARNESSES` in `src/harness.rs` plus per-capability probes
(and an optional L4 route). Nothing else changes: scenarios are generic over
the registry. A harness that offers no model-free way to report a delivery
kind declares no probe for it — the scenario records `Unverified`, absence of
a probe is a known gap, never a silent pass.

## Version Policy

**Latest channel by policy.** The harness CLIs update continuously — often
automatically — so the Lab always tests what the channel delivers, and the
*actual* versions are recorded per run in the L2 evidence (each harness's
own version probe, scenario R6). Nothing is pinned in `.env.example`, and
Gemini is not a V0 target (not in the image).

`uze setup` provisions the image at build time through each vendor's
official installer — the same mechanism the product uses for its users, so
the image is a fresh `latest` snapshot by construction. The deterministic
`pnpm` pin (via corepack) covers only the package manager, never a harness.

## Known Gaps

- **Codex MCP enumeration**: `codex mcp list --json` returns no entries for
  marketplace-plugin MCP servers on the channel versions observed (0.148.0
  and 0.149.1). Codex exposes no model-free enumeration of plugin MCP
  servers → R4 records UNVERIFIED with this reason. Connectivity is unproven
  for Codex by design.
- **User-only Skill policy across shared-root harnesses (REAL FINDING)**: in
  multi-harness delivery, OpenCode owns the shared `.agents/skills` entry and
  Codex's `agents/openai.yaml` exclusion sidecar lives in Codex's generated
  copy — which Codex's prompt-input does not read, so a user-only Skill is
  **listed** in the model-visible prompt. Single-harness Codex delivery (the
  main suite's dogfood) correctly hides it. This is a cross-harness policy
  preservation gap, not a localized defect; fixing it needs coordination
  between shared-root integrations. Lab evidence: R2 `CAPABILITY_FAILURE`.
- **Claude policy surface**: no model-free CLI introspection for invocation
  policy → R2 UNVERIFIED for Claude.
- **OpenCode (latest channel = V2 preview)**: the channel now ships
  `opencode2` (verified `0.0.0-beta-18050`); `opencode2 mcp list` hangs
  (>120s, no headless output) and `debug` exposes no `skill` subcommand.
  L2 records the resulting HarnessFailure/InfraFailure honestly — no pass
  is invented for a channel that is not headless-friendly. The product's
  V1-era `debug skill` evidence is superseded by this channel state, not
  by a fake pass.
- **MCP depth asymmetry**: Claude/OpenCode prove connectivity, Codex and
  Antigravity prove registration only.
- **Lab vs main-suite residuals**: update-lifecycle L3, legacy fake script in
  `tests/cli/machine.rs`, `shell_path` SHELL guard, deterministic L3 MCP —
  all are main-suite follow-ups, intentionally **not** duplicated in the Lab.

## Provider / Gateway

LiteLLM exists only for L4: credential isolation (key injected to the gateway
service only), provider normalization, routing. L2 has no provider and no
credentials. See `provider-contract.md` for the routed-provider contract.

## Historical context

The old Tier 1/2/3/Baseline taxonomy is gone: attachment is product evidence
owned by the main L3 suite; discovery is L2; behavior is L4; the baseline is
the C1 control. Migration history lives in ADRs and `docs/testing.md`'s
short version.
