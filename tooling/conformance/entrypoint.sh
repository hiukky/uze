#!/bin/sh
set -eu

# The runner passes a distinct HOME/UZE_HOME for each run. These defaults make
# an ad-hoc container invocation isolated as well, without ever mounting host
# state into the image.
: "${HOME:=/home/node}"
: "${UZE_HOME:=${HOME}/.uze}"

mkdir -p "$HOME" "$UZE_HOME" /work
exec "$@"
