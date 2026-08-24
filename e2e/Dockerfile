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
    && cargo build --locked --release --manifest-path e2e/Cargo.toml \
    && mkdir -p /out \
    && cp /src/target/release/uze /src/target/release/uze-mcp-conformance-fixture /src/target/release/uze-conformance /out/

FROM node:22-bookworm-slim AS harness-lab

# Latest-channel policy: the harnesses update on their own; the Lab always
# tests what the channel delivers. Actual versions are NOT asserted here —
# they are recorded per run in the L2 evidence (each harness's own
# version probe, scenario R6).
# ARG PNPM_VERSION=10.0.0  # (reserved; see note below)

RUN apt-get update \
    && apt-get install --no-install-recommends -y ca-certificates curl git ripgrep tini \
    && rm -rf /var/lib/apt/lists/*

# Deterministic JS package management: corepack + pinned pnpm (a package
# manager, not a harness — the harness versions it installs are always
# latest). Global bins land in PNPM_HOME (on PATH below).
RUN corepack enable \
    && corepack prepare pnpm@9.15.9 --activate
ENV PNPM_HOME=/usr/local/pnpm
ENV PATH="${PNPM_HOME}:${PATH}"

COPY --from=uze-build /out/uze /usr/local/bin/uze

# UZE is its own provisioner: `uze setup` installs the primary V0 harnesses
# (Claude Code, Codex, OpenCode, Antigravity CLI) through each vendor's
# official installer — the same mechanism the product uses for its users.
# The image build runs it with network, so it installs the channel-latest
# versions; the L2 runtime then runs `--network none` and keeps them.
# `HOME=/usr/local` pins the vendor default install location (~/.local/bin)
# into the image, reachable by the runtime user; the PATH below carries it.
ENV PATH="/usr/local/.local/bin:/usr/local/.opencode/bin:/usr/local/pnpm:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
RUN HOME=/usr/local /usr/local/bin/uze setup

# OpenCode resolves its official runtime plugin package on first use. Bake it
# into the disposable image so a harness with intentionally no egress does
# not try to reach npm at test time.
RUN mkdir -p /opt/opencode-runtime \
    && pnpm --dir /opt/opencode-runtime init --silent >/dev/null \
    && pnpm --dir /opt/opencode-runtime add --reporter=silent \
    @opencode-ai/plugin \
    @ai-sdk/openai-compatible

COPY --from=uze-build /out/uze-mcp-conformance-fixture /usr/local/bin/uze-mcp-conformance-fixture
COPY --from=uze-build /out/uze-conformance /usr/local/bin/uze-conformance
# Single canonical fixture source: the Lab consumes tests/fixtures through
# uze-testkit — it never maintains a second canonical tree.
COPY tests/fixtures /opt/uze-fixtures/tests-fixtures
COPY e2e/fixtures/control/native-skill-discovery /opt/uze-fixtures/control/native-skill-discovery
COPY e2e/opencode-models.json /opt/uze-e2e/opencode-models.json
COPY e2e/entrypoint.sh /usr/local/bin/uze-harness-entrypoint

# The Lab resolves the canonical fixture tree from this runtime path (see
# uze-testkit::fixtures::root) — compile-time manifest paths do not exist in
# the container.
ENV UZE_TESTKIT_FIXTURES_ROOT=/opt/uze-fixtures/tests-fixtures

RUN chmod 0755 /usr/local/bin/uze-harness-entrypoint \
    && mkdir -p /work \
    && chown -R node:node /opt/uze-fixtures /opt/opencode-runtime /usr/local/pnpm /usr/local/.local /usr/local/.opencode \
    && chown node:node /work /usr/local/bin/uze /usr/local/bin/uze-conformance \
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
