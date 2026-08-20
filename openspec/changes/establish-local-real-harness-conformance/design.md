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
headlessly against a local inference service. The runner emits independent
attachment, discovery, and behavioral evidence. A missing executable,
unsupported local-provider protocol, timeout, or model inability is reported
at its own layer, never as a false product incompatibility.

The route is selected per harness after a protocol spike. Current research
identifies the following candidates:

```text
OpenCode            -> llama.cpp /v1/chat/completions
GitHub Copilot CLI  -> llama.cpp /v1/chat/completions
Codex               -> llama.cpp /v1/responses (spike required)
Claude Code         -> llama.cpp /v1/messages (lab-only experiment)
Gemini CLI          -> Gemini-protocol gateway -> llama.cpp
```

The pinned llama.cpp server must retain every endpoint used by the selected
route. A gateway remains optional test-only infrastructure when a later pinned
version proves translation necessary; it is not introduced into Compose by
assumption. `research-notes.md` is the selection authority.

The image is built with explicit harness version build arguments and no host
HOME mount. The model is supplied as an explicit read-only bind mount at run
time, with its SHA256 recorded by the runner. This avoids adding an implicit
model downloader or credentials to the repository.
