# syntax=docker/dockerfile:1
#
# The Rendering Lab: real terminal emulators on a virtual display, the
# pinned font catalog, and the `uze` a user gets. Built with network, run
# without it (`--network none`): every terminal, font and binary is here
# before the first capture, so a run measures exactly what this file pins.
#
# Build from the repository root:
#   docker build -f conformance/rendering/Dockerfile -t uze-rendering-lab .

FROM rust:1.97-bookworm AS uze-build
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --locked --release --bin uze \
    && mkdir -p /out && cp /src/target/release/uze /out/

# Ghostty ships no Linux binary of its own, so it is built from the release
# tag with the Zig its build.zig.zon names — both pinned by digest.
FROM debian:trixie-slim AS ghostty-build
ARG ZIG_VERSION=0.15.2
ARG ZIG_SHA256=02aa270f183da276e5b5920b1dac44a63f1a49e55050ebde3aecc9eb82f93239
ARG GHOSTTY_VERSION=1.3.1
ARG GHOSTTY_SHA256=3349d25600ffbda281197a18314f7d18791969cffe9474f0ff16a45a9ebfccdb
RUN apt-get update \
    && apt-get install --no-install-recommends -y ca-certificates curl xz-utils \
       libgtk-4-dev libadwaita-1-dev libgtk4-layer-shell-dev gettext libxml2-utils pkg-config \
    && rm -rf /var/lib/apt/lists/*
RUN curl -fsSL -o /tmp/zig.tar.xz https://ziglang.org/download/${ZIG_VERSION}/zig-x86_64-linux-${ZIG_VERSION}.tar.xz \
    && echo "${ZIG_SHA256}  /tmp/zig.tar.xz" | sha256sum -c - \
    && mkdir /zig && tar -xJf /tmp/zig.tar.xz -C /zig --strip-components=1 \
    && curl -fsSL -o /tmp/ghostty.tar.gz https://release.files.ghostty.org/${GHOSTTY_VERSION}/ghostty-${GHOSTTY_VERSION}.tar.gz \
    && echo "${GHOSTTY_SHA256}  /tmp/ghostty.tar.gz" | sha256sum -c - \
    && mkdir /ghostty && tar -xzf /tmp/ghostty.tar.gz -C /ghostty --strip-components=1
WORKDIR /ghostty
RUN /zig/zig build -Doptimize=ReleaseFast -Dcpu=baseline --prefix /opt/ghostty

FROM debian:trixie-slim AS lab
ARG WEZTERM_VERSION=20240203-110809-5046fc22
ARG WEZTERM_SHA256=34010a07076d2272c4d4f94b5e0dae608a679599e8d729446323f88f956c60f0

# The terminals Debian carries, the displays they draw on (Xvfb for X11,
# headless sway for Wayland), Mesa's software renderer for the GPU ones,
# and the two screenshot tools. DejaVu is the unpatched font the
# Symbols-only case falls back from.
RUN apt-get update \
    && apt-get install --no-install-recommends -y \
       ca-certificates curl xz-utils python3 python3-pil python3-fonttools \
       xvfb x11-utils xauth imagemagick fontconfig fonts-dejavu-core \
       libgl1-mesa-dri libegl1 libgles2 mesa-vulkan-drivers dbus dbus-x11 \
       kitty alacritty xterm konsole xfce4-terminal foot sway grim \
       libgtk-4-1 libadwaita-1-0 libgtk4-layer-shell0 \
    && rm -rf /var/lib/apt/lists/*

# WezTerm's own AppImage, extracted: the Debian 12 package links an OpenSSL
# trixie no longer ships, and extracting needs no FUSE.
RUN curl -fsSL -o /tmp/wezterm.AppImage \
       https://github.com/wezterm/wezterm/releases/download/${WEZTERM_VERSION}/WezTerm-${WEZTERM_VERSION}-Ubuntu20.04.AppImage \
    && echo "${WEZTERM_SHA256}  /tmp/wezterm.AppImage" | sha256sum -c - \
    && chmod +x /tmp/wezterm.AppImage \
    && cd /opt && /tmp/wezterm.AppImage --appimage-extract >/dev/null \
    && mv /opt/squashfs-root /opt/wezterm && rm /tmp/wezterm.AppImage \
    && ln -s /opt/wezterm/usr/bin/wezterm /usr/local/bin/wezterm

COPY --from=ghostty-build /opt/ghostty /opt/ghostty
RUN ln -s /opt/ghostty/bin/ghostty /usr/local/bin/ghostty

COPY --from=uze-build /out/uze /usr/local/bin/uze

# The catalog, fetched and verified by digest at build time only.
COPY conformance/rendering/fonts.json conformance/rendering/fonts.py /lab/
RUN python3 /lab/fonts.py fetch --into /fonts && rm -f /fonts/*.tar.xz

COPY conformance/rendering /lab
ENV UZE_LAB_FONTS=/fonts \
    LIBGL_ALWAYS_SOFTWARE=1 \
    GALLIUM_DRIVER=llvmpipe \
    NO_AT_BRIDGE=1
WORKDIR /lab
ENTRYPOINT ["python3", "/lab/capture.py"]
