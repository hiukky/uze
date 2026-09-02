# syntax=docker/dockerfile:1

FROM rust:1.97-bookworm AS uze-build

WORKDIR /src
COPY . .
# Cache mounts make every rebuild incremental: the crate registry and the
# build target persist across builds even when sources change. The binaries
# are copied to /out because cache mounts are not part of the image.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --locked --release --bin uze --bin uze-mcp-conformance-fixture \
    && mkdir -p /out \
    && cp /src/target/release/uze /src/target/release/uze-mcp-conformance-fixture /out/

FROM node:22-bookworm-slim AS harness-lab

# Latest-channel policy: the harnesses update on their own; the Lab always
# tests what the channel delivers. Actual versions are NOT asserted here —
# they are recorded per run in the L2 evidence (each harness's own
# version probe, scenario R6).
# ARG PNPM_VERSION=10.0.0  # (reserved; see note below)

# `jq` is the generated hook wrapper's own dependency (ADR-033): the file a
# harness executes reads the payload with it. The Lab installs it the way a
# machine running delivered hooks must have it, and the wrapper's
# missing-dependency behaviour is proven in the deterministic suite.
RUN apt-get update \
    && apt-get install --no-install-recommends -y ca-certificates curl git jq ripgrep tini bubblewrap \
    && rm -rf /var/lib/apt/lists/*

COPY --from=uze-build /out/uze /usr/local/bin/uze

# UZE is its own provisioner: `uze setup` installs the primary V0 harnesses
# (Claude Code, Codex, OpenCode, Antigravity CLI) through each vendor's
# official installer — the same mechanism the product uses for its users.
# The image build runs it with network, so it installs the channel-latest
# versions; the L2 runtime then runs `--network none` and keeps them.
# `HOME=/usr/local` pins the vendor default install location (~/.local/bin)
# into the image, reachable by the runtime user; the PATH below carries it.
ENV PATH="/usr/local/.local/bin:/usr/local/.opencode/bin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
RUN HOME=/usr/local /usr/local/bin/uze setup

COPY --from=uze-build /out/uze-mcp-conformance-fixture /usr/local/bin/uze-mcp-conformance-fixture
# The Lab owns a complete, isolated marketplace fixture. It represents the
# final resources exercised by real harnesses; unit/integration fixtures stay
# under tests/_fixtures and remain independent.
COPY conformance/_fixtures /opt/uze-conformance-fixtures
COPY conformance/entrypoint.sh /usr/local/bin/uze-harness-entrypoint

RUN chmod 0755 /usr/local/bin/uze-harness-entrypoint \
    && mkdir -p /work \
    && chown -R node:node /opt/uze-conformance-fixtures /usr/local/.local /usr/local/.opencode \
    && chown node:node /work /usr/local/bin/uze \
    && chown -R node:node /usr/local/bin/uze-mcp-conformance-fixture

# Rootless runtime: `node` (UID 1000) owns every writable tree — HOME,
# UZE_HOME, fixture cache, package-manager home, harness installs (vendors
# self-update in place; networkless L2 makes those writes no-ops, and a
# permission error must never be the reason a probe fails). No step of the
# runtime user path can escalate: read-only root, cap_drop ALL,
# no-new-privileges, tmpfs-only writes, no Docker socket, no host HOME.
USER node
WORKDIR /work
ENV HOME=/home/node
ENV UZE_HOME=/home/node/.uze
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/uze-harness-entrypoint"]
CMD ["sh"]
