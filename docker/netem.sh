#!/usr/bin/env bash
#
# docker/netem.sh — WDR impaired-network controller for the compose harness.
#
# Runs INSIDE the `netem` container (the only service with cap_add: [NET_ADMIN]).
# Applies a `tc netem` impairment profile to the bridge egress and keeps the
# container alive as the traffic gateway for the sims.
#
# TOPOLOGY (network-namespace shared "sidecar" model):
#   netem        - carries NET_ADMIN only. Attached to the `wdr-net` bridge.
#   emitter-sim  - `network_mode: service:netem` => SHARES the netem netns.
#   receiver-sim - plain peer on the `wdr-net` bridge.
#   Because the emitter SHARES the netem network namespace, `tc netem` on the
#   netem bridge interface shapes the EMITTER's egress — the direction that
#   matters (source -> receiver). No other service needs (or gets) NET_ADMIN.
#
# PROFILES (WDR_NETEM_PROFILE on the netem service; default clean):
#   clean          no impairment (default)
#   loss0.5        loss 0.5%   (TEST_PLAN.md low anchor)
#   loss1          loss 1%     (TEST_PLAN.md mid anchor)
#   loss5          loss 5%     (TEST_PLAN.md high anchor)
#   jitter30       delay 30ms ±10ms, normal distribution
#   reorder        reorder 25% gap 3 (delay 10ms) — drives reorder_event > 0
#   duplication    duplicate 10%
#   bandwidth      rate 512kbit
#   disconnect     100% packet loss (simulated link drop)
#
# SAFETY: fails fast (exit != 0) with a clear message if `tc` is missing.
# On success writes /tmp/netem-ready so startup/assert scripts can poll for
# readiness (bounded polling — never sleep-and-assume).

set -euo pipefail

PROFILE="${WDR_NETEM_PROFILE:-clean}"
BRIDGE_IF="${WDR_NETEM_IFACE:-eth0}"

info() { printf '[netem] %s\n' "$*"; }
die()  { printf '[netem][error] %s\n' "$*" >&2; exit 1; }

if ! command -v tc >/dev/null 2>&1; then
    die "tc (iproute2) not found in the netem container. Rebuild the dev image: 'docker compose build netem' (Dockerfile.dev installs iproute2)."
fi

# `replace` is idempotent: re-running a profile (restart, reassign) never errors.
apply_netem() {
    tc qdisc replace dev "$BRIDGE_IF" root netem "$@"
}

case "$PROFILE" in
    clean)
        tc qdisc del dev "$BRIDGE_IF" root 2>/dev/null || true
        info "profile=clean: no impairment."
        ;;
    loss0.5) apply_netem loss 0.5% ; info "profile=loss0.5: loss 0.5%." ;;
    loss1)   apply_netem loss 1%   ; info "profile=loss1: loss 1%." ;;
    loss5)   apply_netem loss 5%   ; info "profile=loss5: loss 5%." ;;
    jitter30) apply_netem delay 30ms 10ms distribution normal ; info "profile=jitter30: delay 30ms ±10ms (normal)." ;;
    reorder)  apply_netem delay 10ms reorder 25% gap 3          ; info "profile=reorder: reorder 25% gap 3 (delay 10ms)." ;;
    duplication) apply_netem duplicate 10%                     ; info "profile=duplication: duplicate 10%." ;;
    bandwidth)
        # netem `rate` needs kernel NETEM_RATE; `tbf` is universally available.
        tc qdisc replace dev "$BRIDGE_IF" root tbf rate 512kbit burst 32kbit latency 400ms
        info "profile=bandwidth: tbf rate 512kbit."
        ;;
    disconnect) apply_netem loss 100%                          ; info "profile=disconnect: 100% loss (link dropped)." ;;
    *)
        die "unknown WDR_NETEM_PROFILE='$PROFILE'. Valid: clean loss0.5 loss1 loss5 jitter30 reorder duplication bandwidth disconnect."
        ;;
esac

# Surface the applied profile + readiness marker for assertions on the shared
# volume (mounted into the metrics collector at /tmp/metrics).
RUN_ID="$(date +%s)"
mkdir -p /tmp/metrics 2>/dev/null || true
printf '{"profile":"%s","run_id":"%s","iface":"%s"}\n' "$PROFILE" "$RUN_ID" "$BRIDGE_IF" > /tmp/metrics/netem-ready.json

# Readiness marker: assert.sh / sim-start.sh poll this volume file with a
# bounded retry loop — never sleep-and-assume.
printf '%s' "$RUN_ID" > /tmp/netem-ready
info "ready: profile=$PROFILE applied on $BRIDGE_IF (run_id=$RUN_ID)."

# Keep the container alive as the traffic gateway for the sims.
exec sleep infinity