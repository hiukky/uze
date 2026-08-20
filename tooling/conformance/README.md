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

It contains the UZE release binary, Claude Code, Codex, OpenCode, Git and
minimal runtime dependencies. The image build may access package registries;
runtime conformance containers must not have host credentials, a host HOME,
Docker socket, privileged mode, or general internet access.

Build it locally:

```bash
docker build \
  --file tooling/conformance/Dockerfile \
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
cargo test --manifest-path tooling/conformance/Cargo.toml
```

## Evidence tiers

| Tier | Scope |
|---|---|
| L0 | Pure Rust unit tests. |
| L1 | Product contracts: Store, planning, receipts and filesystem/config. |
| L2 | Opt-in Docker: real harness plus local model, no vendor quota. |
| L3 | Opt-in real vendor/provider conformance. |

The selected first L2 behavioral route is OpenCode. Codex is a separate
Responses-protocol spike; Claude's non-Claude local gateway is experimental
and never replaces L3 vendor evidence. See the
[ecosystem research](../../openspec/changes/establish-local-real-harness-conformance/research-notes.md).
