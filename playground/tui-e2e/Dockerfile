FROM ubuntu:24.04

RUN apt-get update \
 && apt-get install -y --no-install-recommends git python3 python3-pip ca-certificates less \
 && rm -rf /var/lib/apt/lists/*
RUN pip3 install --break-system-packages pyte

# A harness the picker can offer and a pane can actually run: it does
# nothing but sit there, which is all the placement flow needs from it.
# The pane's *foreground process name* is how a tab is recognized as an
# agent, so the stand-in has to be a real binary called `claude` — a shell
# script would report its interpreter and the tree would list no agent.
RUN cp /usr/bin/cat /usr/local/bin/claude

COPY uze /usr/local/bin/uze
COPY drive.py /drive.py

ENV HOME=/root
ENV TERM=xterm-256color
WORKDIR /work
