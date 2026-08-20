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

The initial service uses the official pinned `server-b7728` image tag. Before
calling a test reproducible across machines, record the resolved image digest
as well. It enables `--jinja` because the selected model's chat template is
part of tool-calling behavior.

A model is acceptable for the first L2 only when it can reliably follow an
explicit instruction, emit a structured tool call and return a deterministic
token. The lab deliberately does not choose or download a GGUF yet: behavior
must be recorded against a concrete model hash, not an unpinned “latest”
download.

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
