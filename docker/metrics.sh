#!/usr/bin/env bash
#
# docker/metrics.sh — WDR harness metrics collector (lightweight, std-only).
#
# Runs INSIDE the `metrics` compose service. The dev image deliberately has no
# python (Dockerfile.dev), so this is pure bash + awk + grep + sed. It mounts
# the shared /tmp/metrics volume that the sims + netem publish into, and:
#
#   1. watches for emitter-sim.json / receiver-sim.json / netem-ready.json,
#   2. when all three exist, writes a combined collector.json snapshot plus a
#      runs.log suite-history line ("assert across runs"),
#   3. keeps running so `docker compose up` stays healthy; never blocks the
#      harness. The authoritative pass/fail is docker/assert.sh (host side).
#
# Metrics are extracted leniently (grep -o key), so this survives the refsim
# worker changing the exact JSON shape.

set -euo pipefail

METRICS_DIR="${WDR_METRICS_DIR:-/tmp/metrics}"
SNAPSHOT_INTERVAL="${WDR_SNAPSHOT_INTERVAL:-3}"
DEADLINE_LOOPS="${WDR_DEADLINE_LOOPS:-600}"    # ~30 min max; bounded, never infinite

info() { printf '[metrics] %s\n' "$*"; }

mkdir -p "$METRICS_DIR"
info "collector started (dir=$METRICS_DIR, interval=${SNAPSHOT_INTERVAL}s)."

# Extract a top-level scalar value for `key` from a JSON doc, leniently.
# Prints "0" when absent/unparseable so arithmetic never breaks.
jget() { # jget <file> <key>
    local f="$1" key="$2" val
    val="$(grep -oE "\"$key\"[[:space:]]*:[[:space:]]*[^,}]+" "$f" 2>/dev/null | head -1 | sed -E 's/^"[^"]*"[[:space:]]*:[[:space:]]*//; s/^"(.*)"$/\1/' )"
    [ -n "$val" ] && printf '%s' "$val" || printf '0'
}

iter=0
while [ "$iter" -lt "$DEADLINE_LOOPS" ]; do
    iter=$((iter + 1))
    em="$METRICS_DIR/emitter-sim.json"
    rx="$METRICS_DIR/receiver-sim.json"
    nm="$METRICS_DIR/netem-ready.json"

    if [ -f "$em" ] && [ -f "$rx" ] && [ -f "$nm" ]; then
        profile="$(jget "$nm" profile)"
        e_hash="$(jget "$em" hash)"
        r_hash="$(jget "$rx" hash)"
        e_pkts="$(jget "$em" packets_sent)"
        r_loss="$(jget "$rx" loss)"
        # Equality is only meaningful when both hashes look real (64-hex blake3).
        eq=false
        if [ "$e_hash" != "0" ] && [ "$e_hash" != "null" ] && [ -n "$e_hash" ] \
           && [ "$r_hash" != "0" ] && [ "$r_hash" != "null" ] && [ -n "$r_hash" ]; then
            [ "$e_hash" = "$r_hash" ] && eq=true
        fi
        ts="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

        cat > "$METRICS_DIR/collector.json" <<EOF2
{"generated_at":"$ts","profile":"$profile",
 "emitter":{"hash":"$e_hash","packets_sent":$e_pkts},
 "receiver":{"hash":"$r_hash","loss":$r_loss},
 "hash_equal":$eq}
EOF2
        printf '%s profile=%s hash_equal=%s emitter_packets=%s receiver_loss=%s\n' \
            "$ts" "$profile" "$eq" "$e_pkts" "$r_loss" >> "$METRICS_DIR/runs.log"
    fi

    if [ -f "$METRICS_DIR/.suite-done" ]; then
        info "suite-done marker observed; collector exiting cleanly."
        exit 0
    fi
    sleep "$SNAPSHOT_INTERVAL"
done

info "collector deadline reached (${DEADLINE_LOOPS} loops); exiting (non-fatal)."
exit 0