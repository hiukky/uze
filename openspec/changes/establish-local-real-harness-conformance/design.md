## Design

The conformance lab is test infrastructure, not a fourth UZE integration.
Its Rust runner will have a narrow process contract (`HarnessRunSpec` and
`HarnessRunResult`) for executable, arguments, environment, HOME, UZE_HOME,
working directory, stdin, timeout, output, and exit status. It invokes the
real selected harness executables; it does not reuse `IntegrationPort` for
process orchestration or infer product compatibility from model output.

Each L2 run starts in a disposable container with a fresh HOME, UZE_HOME, and
project. UZE installs the portable multi-capability fixture once, plans and
attaches it through its real peer integrations, then the selected harness runs
headlessly against either a local service or a test-only routed provider. The runner emits independent
attachment, discovery, and behavioral evidence. A missing executable,
unsupported local-provider protocol, timeout, or model inability is reported
at its own layer, never as a false product incompatibility.

The route is selected per harness after a protocol spike. Current research
identifies the following candidates:

```text
OpenCode            -> LiteLLM /v1/chat/completions -> Groq
Codex               -> LiteLLM /v1/responses (spike required) -> Groq
Claude Code         -> LiteLLM Anthropic endpoint (spike required) -> Groq
```

LiteLLM is deliberately test-only and is the sole service with egress and a
provider credential. The harness network reaches only the gateway. The gateway
is configured with a stable `uze-conformance` model alias, so adapters do not
learn provider-specific model names. `research-notes.md` is the selection
authority.

The image is built with explicit harness version build arguments and no host
HOME mount. Provider credentials are supplied only as runtime shell/CI secrets
to the gateway. This avoids an implicit model downloader and keeps credentials
out of the repository and harness containers.
