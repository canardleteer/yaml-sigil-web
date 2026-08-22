#!/bin/sh
set -eu

export TRUNK_SERVE_PORT="${PORT:-8393}"
export TRUNK_SERVE_ADDRESS="${TRUNK_SERVE_ADDRESS:-0.0.0.0}"

exec trunk serve --release true --no-autoreload true --open false
