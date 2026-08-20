# syntax=docker/dockerfile:1

FROM rust:1.97-bookworm AS uze-build

WORKDIR /src
COPY . .
RUN cargo build --locked --release --bin uze

FROM node:22-bookworm-slim AS harness-lab

ARG CLAUDE_VERSION=2.1.237
ARG CODEX_VERSION=0.148.0
ARG OPENCODE_VERSION=1.18.19

RUN apt-get update \
    && apt-get install --no-install-recommends -y ca-certificates git tini \
    && rm -rf /var/lib/apt/lists/*

# The image build may use the network to obtain pinned CLI releases. Runtime
# conformance containers need no internet and receive neither host HOME nor
# provider credentials.
RUN npm install --global --no-audit --no-fund \
    "@anthropic-ai/claude-code@${CLAUDE_VERSION}" \
    "@openai/codex@${CODEX_VERSION}" \
    "opencode-ai@${OPENCODE_VERSION}"

COPY --from=uze-build /src/target/release/uze /usr/local/bin/uze
COPY tooling/conformance/entrypoint.sh /usr/local/bin/uze-harness-entrypoint

RUN chmod 0755 /usr/local/bin/uze-harness-entrypoint \
    && mkdir -p /work \
    && chown node:node /work

# `node` is the non-privileged user supplied by node:22-bookworm-slim (UID
# 1000). Keeping that stable makes a disposable tmpfs mount writable with
# `uid=1000,gid=1000` without granting the runtime container root access.
USER node
WORKDIR /work
ENV HOME=/home/node
ENV UZE_HOME=/home/node/.uze
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/uze-harness-entrypoint"]
CMD ["sh"]
