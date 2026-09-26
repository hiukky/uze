#!/bin/sh
# The handler the stub group in hooks.json runs. The contract (ADR-033):
#
#   HOOK_* environment in, exit code out.
#     0 — allow
#     3 — deny; write the reason on stderr first
#     anything else, or the timeout above — fails the way the group's
#         `effect` says
#
# The wrapper the install generates needs `jq` on PATH; `uze doctor`
# reports it when it is missing. Match the event with the same `matcher`
# words hooks.json uses: a vendor-native name, or a tool alias like
# `shell` that every harness has an equivalent for.
#
# Replace this with the real check, and keep it fast — the timeout is
# seconds, and a handler that sits on it fails like an error does.
exit 0
