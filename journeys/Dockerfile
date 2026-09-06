# The journey runtime, pinned. Not a build image: the binary under test comes
# in through `JOURNEY_UZE`, because CI's cargo cache has already built it and
# building it a second time inside an image would double the slowest step of
# the run for nothing.
#
# What this image is actually for: pinning the terminal the frames are read
# from. A journey aims a click at a glyph on a rendered screen, so the tmux
# version, the locale and the unicode width tables are part of the contract —
# and they are the parts a runner image is free to change under you.
# ubuntu:24.04, not debian-slim: the binary comes from the host/CI build, and
# it must find the glibc it was linked against — the runner image is where
# that is decided.
FROM ubuntu:24.04

RUN apt-get update \
    && apt-get install --no-install-recommends -y \
        ca-certificates git tmux python3 python3-yaml procps \
    && rm -rf /var/lib/apt/lists/*

# Not root by default: the guard that refuses a sandbox overlapping a real
# home is worth more when the home it protects is not the image's own /root.
#
# Callers pass `--user "$(id -u):$(id -g)"` anyway, because the evidence
# directory is a mount owned by whoever invoked docker — uid 1000 on a
# developer's machine, 1001 on a GitHub runner — and an image that guesses at
# that number is right in exactly one of those places. `HOME` comes in with
# it, since a uid the image has no passwd entry for has no home to resolve.
RUN userdel --remove ubuntu 2>/dev/null || true \
    && useradd --create-home --uid 1000 --shell /bin/bash journey
USER journey
WORKDIR /journeys

ENV LANG=C.UTF-8 \
    TERM=xterm-256color \
    JOURNEY_WORLDS=/tmp/uze-journeys \
    JOURNEY_EVIDENCE=/evidence \
    JOURNEY_UZE=/usr/local/bin/uze \
    JOURNEY_FAKE_HARNESS=/usr/local/bin/uze-fake-harness

ENTRYPOINT ["python3", "/journeys/journey.py"]
CMD ["--help"]
