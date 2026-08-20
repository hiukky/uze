# Local model contract

The L2 lab never downloads, bundles or modifies a model. The developer supplies
a single GGUF through `UZE_CONFORMANCE_MODEL_DIR`; Compose mounts that
directory read-only into the llama.cpp service.

Record these facts next to every behavioral result:

```text
llama.cpp image tag and digest
GGUF filename
GGUF SHA256
model family and quantization
context size
llama-server flags
sampling parameters
harness version
PackageId, resource identities and exposure strategy
```

The initial service uses the official `server-b7728` image tag. The image
resolved during the laboratory spike to
`sha256:9945b5afed75c3f09a5ca73c1aadd277101922a243c09a91c4931a85312be805`.
Record that digest (or a deliberately updated one) with every result. It
enables `--jinja` because the selected model's chat template is part of
tool-calling behavior.

A model is acceptable for the first L2 only when it can reliably follow an
explicit instruction, emit a structured tool call and return a deterministic
token. The lab deliberately does not choose or download a GGUF yet: behavior
must be recorded against a concrete model hash, not an unpinned “latest”
download.

## First candidate, not an automatic download

The first candidate is the official
[`Qwen/Qwen3-8B-GGUF`](https://huggingface.co/Qwen/Qwen3-8B-GGUF) release,
initially `Q4_K_M`. It is an 8.2B model with a llama.cpp/Jinja usage path and
documented agent/function-calling capability. This is a starting hypothesis,
not a compatibility result: the exact repository revision, file SHA256 and
observed tool-call behavior must be written into L2 evidence before a test is
accepted as reproducible.

## Compose topology

```text
harness (real UZE + real harness CLI)
   |
   | internal Docker network only
   v
llama.cpp (read-only GGUF mount)
```

There is no LiteLLM service in the default topology. Add a gateway only after a
pinned route demonstrates a protocol incompatibility; it remains test
infrastructure and never becomes a UZE dependency.
