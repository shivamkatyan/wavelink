#!/usr/bin/env bash
set -euo pipefail
# containerized soak entrypoint: needs host docker socket + ref binaries mounted.
export ROOT=/workspace
exec bash /workspace/docker/soak.sh
