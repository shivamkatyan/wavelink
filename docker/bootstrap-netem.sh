#!/usr/bin/env bash
#
# docker/bootstrap-netem.sh — WDR compose harness bootstrap (idempotent).
#
# For a FRESH clone (or any machine): validates Docker + NET_ADMIN, builds the
# dev image, brings up the harness, runs every netem profile with the
# assertion suite, then tears down. Safe to re-run: it is idempotent and does
# not disturb other Docker objects (only the wdr harness namespace).
#
# Usage:
#   bash docker/bootstrap-netem.sh            # full suite, default profiles
#   WDR_PROFILE_SUITE="clean loss1" bash docker/bootstrap-netem.sh
#
# Requires:
#   - Docker engine reachable (docker version)
#   - A bridge driver that honors cap_add: [NET_ADMIN] (Linux/WSL2 backend).
#   - iproute2 (host, for the preflight tc check) — the container image brings it.
#
# The suite is structure-validated even when the wdr_refsim binaries have not
# landed yet: services start, wait, and report "sim not ready" gracefully while
# still validating topology + NET_ADMIN. When the binaries exist, the full
# acceptance floor is enforced (see docker/assert.sh).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE="${WDR_DEV_IMAGE:-wdr-dev}"
PROFILE_SUITE="${WDR_PROFILE_SUITE:-clean loss0.5 loss1 loss5 jitter30 reorder duplication}"

info() { printf '[bootstrap-netem] %s\n' "$*"; }
die()  { printf '[bootstrap-netem][error] %s\n' "$*" >&2; exit 1; }

# ---------------------------------------------------------------------------
# 1. Validate docker + NET_ADMIN capability
# ---------------------------------------------------------------------------
require_docker() {
    if ! command -v docker >/dev/null 2>&1; then
        die "docker CLI not found on PATH."
    fi
    if ! docker version >/dev/null 2>&1; then
        die "docker daemon not reachable (is Docker Desktop/WSL2 backend running?)."
    fi
    info "docker: $(docker version --format '{{.Server.Version}}' 2>/dev/null || echo present)"
    # The netem service needs cap_add: [NET_ADMIN]. Preflight by creating a
    # throwaway container with that cap and applying a trivial tc qdisc in a
    # scratch network namespace is overkill; the compose suite itself is the
    # gate (netem.sh fails fast if tc is unavailable). Here we only sanity-check
    # the host has a bridge-capable engine.
    if ! docker network create --driver bridge --internal --subnet=10.254.255.0/29 wdr-preflight >/dev/null 2>&1; then
        die "could not create a throwaway internal bridge — NET_ADMIN/bridge support may be missing."
    fi
    docker network rm wdr-preflight >/dev/null 2>&1 || true
    info "bridge capability: OK (NET_ADMIN validated by the netem service itself)."
}

# ---------------------------------------------------------------------------
# 2. Build the dev image
# ---------------------------------------------------------------------------
build_dev_image() {
    info "building $IMAGE from Dockerfile.dev..."
    docker build -f "$REPO_ROOT/Dockerfile.dev" -t "$IMAGE" "$REPO_ROOT"
    info "dev image built: $IMAGE"
}

# ---------------------------------------------------------------------------
# 3. Run the harness + assertions
# ---------------------------------------------------------------------------
run_port() {
    info "--- bringing up harness (profile='$1') ---"
    # netem profile is applied via the per-service env override.
    cd "$REPO_ROOT"
    WDR_NETEM_PROFILE="$1" WDR_DEV_IMAGE="$IMAGE" \
        docker compose up --build -d --force-recreate
    info "--- running assertions for profile='$1' ---"
    # The sims' metrics live on the shared named volume, reachable only from a
    # container that mounts it — so run the assertion layer inside a throwaway
    # container from the dev image that mounts the volume.
    VOL="$(docker volume ls --format '{{.Name}}' | grep 'wdr-metrics' | head -1)"
    [ -n "$VOL" ] || die "could not find the wdr-metrics volume (compose up may have failed)."
    docker run --rm \
        -v "$VOL:/tmp/metrics:ro" \
        -v "$REPO_ROOT/docker:/workspace/docker:ro" \
        -e WDR_NETEM_PROFILE="$1" \
        -e WDR_METRICS_DIR=/tmp/metrics \
        -e WDR_ASSERT_SINGLE_PROFILE=1 \
        "$IMAGE" bash /workspace/docker/assert.sh
    info "--- tearing down (profile='$1') ---"
    docker compose down -v --remove-orphans 2>/dev/null || true
}

run_suite() {
    local n=0 total failed=0
    total=$(wc -w <<< "$PROFILE_SUITE")
    for profile in $PROFILE_SUITE; do
        n=$((n + 1))
        info "===[$n/$total] profile '$profile'==="
        if ! run_port "$profile"; then
            failed=$((failed + 1))
            info "ERROR: profile '$profile' failed."
        fi
    done
    if [ "$failed" -ne 0 ]; then
        die "harness suite finished with $failed/$total profile failure(s)."
    fi
    info "ALL PROFILES PASSED ($total)."
}

# ---------------------------------------------------------------------------
main() {
    require_docker
    build_dev_image
    run_suite
    info "bootstrap-netem complete: image, harness, suite, teardown all green."
}
main "$@"