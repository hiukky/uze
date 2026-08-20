# UZE Harness Conformance Lab

Test-only infrastructure for running **real** harness CLIs in a disposable
environment. It is not part of the UZE product crate and it does not mock a
harness, implement a provider, or call an integration directly.

## Current spike

The image is pin-able through build arguments:

```text
CLAUDE_VERSION=2.1.237
CODEX_VERSION=0.148.0
OPENCODE_VERSION=1.18.19
```

It contains the UZE release binary, Claude Code, Codex, OpenCode, Git,
`ripgrep`, and minimal runtime dependencies. The image build may access package registries;
runtime harness containers must not have host credentials, a host HOME, Docker
socket, privileged mode, or direct internet access. The test-only gateway is
the sole exception: it has limited provider egress and the provider credential.
OpenCode's official runtime plugin dependency is also baked into the image so
its first-run bootstrap does not need npm access from an isolated harness.

Build it locally:

```bash
docker build \
  --file e2e/Dockerfile \
  --build-arg CLAUDE_VERSION=2.1.237 \
  --build-arg CODEX_VERSION=0.148.0 \
  --build-arg OPENCODE_VERSION=1.18.19 \
  --tag uze-harness-lab:local .
```

The non-privileged image user is `node` (UID/GID 1000). A disposable tmpfs
must therefore be mounted with that ownership:

```bash
docker run --rm --network none \
  --tmpfs /work:rw,noexec,nosuid,size=64m,uid=1000,gid=1000,mode=700 \
  --env HOME=/work/home \
  --env UZE_HOME=/work/uze-home \
  --entrypoint sh uze-harness-lab:local -lc '
    mkdir -p "$HOME" "$UZE_HOME" /work/project
    claude -p --help >/dev/null
    codex exec --help >/dev/null
    opencode run --help >/dev/null
  '
```

This tests installation, isolation and headless command surfaces only. It does
not claim plugin discovery or model behavior.

## Runner

`uze-conformance` is a standalone Rust process runner. Its narrow contract is:

```text
HarnessRunSpec
  executable, args, env, HOME, UZE_HOME, cwd, timeout, stdin

HarnessRunResult
  exit status, timeout, stdout, stderr, elapsed
```

It clears inherited environment variables before spawning a process and does
not know harness output schemas, Docker internals, UZE integrations or model
protocols.

```bash
cargo test --manifest-path e2e/Cargo.toml
```

## Compose

The default topology contains a harness service and a pinned LiteLLM gateway.
The harness reaches the gateway on an internal network; only the gateway has
egress and receives the provider key. The initial OpenAI route is described in
[the provider contract](provider-contract.md). It does not download or bundle
a model. Validate the configuration with:

```bash
OPENAI_API_KEY=provided-outside-the-repository \
docker compose --env-file e2e/.env.example \
  -f e2e/compose.yaml config
```

## Evidence tiers

| Tier | Scope |
|---|---|
| L0 | Pure Rust unit tests. |
| L1 | Product contracts: Store, planning, receipts and filesystem/config. |
| L2 | Opt-in Docker: real harness plus isolated local or routed inference. |
| L3 | Opt-in real vendor/provider conformance. |

The selected first routed L2 behavioral route is OpenCode. Codex is a separate
Responses-protocol spike; Claude's Anthropic gateway route is experimental and
never replaces L3 vendor evidence. See the
[ecosystem research](../openspec/changes/establish-local-real-harness-conformance/research-notes.md).

OpenCode's first E2E route uses its built-in `openai` provider pointed at the
internal gateway. It intentionally does not resolve an npm provider adapter at
runtime and sets `OPENCODE_DISABLE_MODELS_FETCH=1`: the explicit test model
uses the pinned minimal catalog at `opencode-models.json`, while the harness
retains no direct egress by design.
