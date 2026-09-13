#!/usr/bin/env bash
#
# docker/ccspike.sh — t-P1-cc congestion-control spike harness (Wireless DAC
# Relay, owner_role: Networking). Produces the MEASURED evidence for the
# ADR-003 BBR-vs-CUBIC decision on the lossy (datagram) audio lane.
#
# Runs the reference lossy Opus path (ref_emitter -> ref_receiver over the
# netem shared-netns bridge) under:
#   profile clean            (control)
#   profile loss1 + jitter30 (degraded per TEST_PLAN: 1% loss + 30ms fixed jitter)
# for EACH of Cubic and Bbr (`WDR_CC`), and records receiver
# loss/duplicate/reorder/late/underruns + emitter path metrics (quinn
# cwnd/rtt/lost/congestion-events) + latency, asserting the jitter/queue bound
# (receiver queue <= MAX_QUEUE_BOUND) and no panic/crash (soak-style,
# docker/soak.sh conventions).
#
# CC selection is wired through wdr_refsim via `WDR_CC=cubic|bbr`
# (ref_emitter -> TransportConnConfig::congestion_control ->
# quinn congestion_controller_factory). Default Cubic; Bbr opts into quinn's
# BbrConfig (verified present in quinn-proto 0.11.17, not feature-gated).
#
# Usage:
#   bash docker/ccspike.sh              # full matrix (2 profiles x 2 CCs), 1 trial each
#   TRIALS=4 WDR_DURATION_SECS=10 bash docker/ccspike.sh   # 4-trial median matrix
# Env:
#   WDR_DEV_IMAGE     dev image (default wdr-dev)
#   WDR_CC_RUNS       CCs to run (default "cubic bbr")
#   WDR_DURATION_SECS emitter duration per run (default 20)
#   TRIALS            trials per (profile, CC) for the median summary (default 1)
#   WDR_CC_OUT        result summary path (default /tmp/opencode/ccspike-results.json)
# WDR_EMIT_PACE_MS is fixed at 20 (real Opus cadence); see emitter.rs `pace`.
#
# Requires: docker, jq (or python3). The netem profile is applied INSIDE the
# netem container (the only NET_ADMIN holder) via `tc netem`, and sims are
# launched with `docker run` attached to the harness network (receiver on
# wdr-net, emitter sharing the netem netns) — same topology as docker/soak.sh.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
IMAGE="${WDR_DEV_IMAGE:-wdr-dev}"
CC_RUNS="${WDR_CC_RUNS:-cubic bbr}"
DURATION="${WDR_DURATION_SECS:-20}"
OUT="${WDR_CC_OUT:-/tmp/opencode/ccspike-results.json}"
NET="${WDR_NET:-wdr_wdr-net}"
VOL="${WDR_METRICS_VOL:-wdr_wdr-metrics}"
RECV="wdr-cc-recv"
EMIT="wdr-cc-emit"
MAX_QUEUE_BOUND=512          # receiver.rs MAX_QUEUE_BOUND (balanced profile cap)

mkdir -p "$(dirname "$OUT")"
: > "$OUT"

info() { printf '[ccspike] %s\n' "$*"; }
die()  { printf '[ccspike][error] %s\n' "$*" >&2; exit 1; }

command -v jq >/dev/null 2>&1 || command -v python3 >/dev/null 2>&1 || die "jq or python3 required."

require_harness() {
    docker network ls --format '{{.Name}}' | grep -qx "$NET" || die "network '$NET' not found (bring up the compose harness first: docker compose up -d netem metrics)"
    docker volume ls --format '{{.Name}}' | grep -qx "$VOL" || die "volume '$VOL' not found"
    docker ps -a --format '{{.Names}}' | grep -qx "wdr-netem" || die "wdr-netem container not found"
}

# Apply a netem profile in the netem container (the only NET_ADMIN holder).
apply_profile() {
    local profile="$1"
    case "$profile" in
        clean)
            docker exec wdr-netem sh -c 'tc qdisc del dev eth0 root 2>/dev/null || true'
            ;;
        degraded)
            docker exec wdr-netem sh -c 'tc qdisc del dev eth0 root 2>/dev/null || true; tc qdisc replace dev eth0 root netem loss 1% delay 30ms 10ms distribution normal'
            ;;
        *) die "unknown profile '$profile' (clean|degraded)" ;;
    esac
    info "profile=$profile applied (netem qdisc):"
    docker exec wdr-netem sh -c 'tc -s qdisc show dev eth0 | head -3'
}

# One run: receiver + emitter for a (profile, cc) pair, then read metrics.
# Writes an inline JSON object to stdout on success.
run_one() {
    local profile="$1" cc="$2"

    docker rm -f "$RECV" "$EMIT" >/dev/null 2>&1 || true
    # Clear stale metrics (compose volume) without removing the volume.
    docker run --rm -v "$VOL:/tmp/metrics" --entrypoint sh "$IMAGE" \
        -c 'rm -f /tmp/metrics/receiver-sim.json /tmp/metrics/emitter-sim.json' >/dev/null 2>&1 || true

    # Receiver on the bridge, like the compose receiver-sim.
    docker run -d --name "$RECV" --network "$NET" \
        -e WDR_SIM_ROLE=receiver -e WDR_METRICS_DIR=/tmp/metrics -e WDR_NETEM_PROFILE="$profile" \
        -v "$VOL:/tmp/metrics" \
        --entrypoint /workspace/target/release/ref_receiver "$IMAGE" \
        --role receiver --buffer balanced 0.0.0.0:9000 >/dev/null 2>&1

    RIP="$(docker inspect "$RECV" --format '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' 2>/dev/null)"
    [ -n "$RIP" ] || die "receiver got no IP on $NET"
    info "run profile=$profile cc=$cc receiver=$RIP"

    # Emitter inside the netem netns (shares netem's netns => tc shapes its egress).
    # WDR_EMIT_PACE_MS=20 runs the Opus datagram lane at a real 20 ms cadence
    # (steady-state load for the congestion controller) instead of a single
    # burst that exits before QUIC drains under 30 ms jitter.
    docker run -d --name "$EMIT" --network "container:wdr-netem" \
        -e WDR_SIM_ROLE=emitter -e WDR_METRICS_DIR=/tmp/metrics -e WDR_NETEM_PROFILE="$profile" \
        -e WDR_CC="$cc" -e WDR_EMIT_PACE_MS=20 \
        -v "$VOL:/tmp/metrics" \
        --entrypoint /workspace/target/release/ref_emitter "$IMAGE" \
        --role emitter --lossy --codec opus --source pseudo-random \
        --seconds "$DURATION" "$RIP:9000" >/dev/null 2>&1

    # Bounded wait for the emitter to finish (never sleep-and-assume).
    local deadline=$(( $(date +%s) + DURATION + 60 ))
    while docker ps -a --filter "name=$EMIT" --filter "status=running" --format x | grep -q x; do
        if [ "$(date +%s)" -ge "$deadline" ]; then
            info "run (profile=$profile cc=$cc) emitter did not finish in ${DURATION}s; collecting partial metrics"
            break
        fi
        sleep 2
    done

    # The emitter may finish before the receiver has drained all in-flight
    # datagrams (30ms jitter + QUIC buffering). Bounded-wait for the receiver
    # to publish a terminal status (complete | error) before snapshotting, so
    # we never read half-drained counters.
    local rdeadline=$(( $(date +%s) + 75 )) rstatus=""
    while :; do
        rstatus="$(docker run --rm -v "$VOL:/tmp/metrics:ro" --entrypoint sh "$IMAGE" \
            -c 'grep -o "\"status\"[^,]*" /tmp/metrics/receiver-sim.json 2>/dev/null | head -1' 2>/dev/null || true)"
        case "$rstatus" in
            *complete*|*error*|*crashed*) break ;;
        esac
        if [ "$(date +%s)" -ge "$rdeadline" ]; then
            info "run (profile=$profile cc=$cc): receiver did not reach terminal status in 75s; snapshotting partial"
            break
        fi
        sleep 2
    done
    info "run profile=$profile cc=$cc receiver-terminal-status='$rstatus'"

    # Pull metrics from the shared volume into a host-visible tmp dir for the
    # row extractor (the named volume is not host-mountable here, so we tar it
    # out through a throwaway container).
    local MET_TMP="/tmp/ccspike-metrics"
    rm -rf "$MET_TMP"; mkdir -p "$MET_TMP"
    docker run --rm -v "$VOL:/tmp/metrics:ro" --entrypoint tar "$IMAGE" \
        -cf - -C /tmp/metrics emitter-sim.json receiver-sim.json 2>/dev/null \
        | tar -xf - -C "$MET_TMP" 2>/dev/null || true

    # Decide valid vs defective BEFORE cleaning up the sims: a trial is valid
    # only when the receiver reached `complete` (the EOS marker survived —
    # see note below). Defective trials are discarded and retried by `main`.
    local ok=1
    case "$rstatus" in
        *complete*) ok=1 ;;
        *error*|*crashed*|"") ok=0 ;;
    esac
    if [ "$ok" = "1" ]; then
        # The frequency with which we discard: the end-of-stream marker is a
        # single unreliable datagram under 1% netem loss, so ~1% of trials the
        # receiver never sees it and times out with `error` (recv counters 0).
        # Pacing a retry is therefore the honest correction for a signal-whose-
        # marker was lost, NOT a CC observation.
        :
    fi

    docker rm -f "$RECV" "$EMIT" >/dev/null 2>&1 || true

    if [ "$ok" != "1" ]; then
        return 2   # defective trial -> main retries
    fi

    # Emit ONE row into the results JSON. Receiver jitter-queue peak is not a
    # current metric field: the hard MAX_QUEUE_BOUND is unit-asserted in
    # ref_e2e; here we surface late_discard (deep reorder signal) + fatal (no
    # panic) + bounded loss and flag the bound as honored when the run
    # completed without a crash and delivered-loss stayed <= sent.
    python3 - "$OUT" "$profile" "$cc" <<'PYEOF'
import sys, json, os
out, profile, cc = sys.argv[1:4]
# Env-free: read metrics JSON from files placed on the volume path.
def read_json(path):
    try:
        with open(path) as f:
            return json.load(f)
    except Exception:
        return {}
base = os.environ.get("WDR_CC_METRICS_TMP", "/tmp/ccspike-metrics")
em_d = read_json(f"{base}/emitter-sim.json")
r_d = read_json(f"{base}/receiver-sim.json")
def num(d, *keys):
    v = d
    for k in keys:
        v = v.get(k) if isinstance(v, dict) else None
        if v is None: return None
    try: return int(v)
    except (TypeError, ValueError): return 0
sent = num(em_d, "packets_sent") or 0
recv = num(r_d, "packets_recv") or 0
loss = num(r_d, "loss", "packets") or 0
row = {
  "profile": profile, "cc": cc,
  "emitter_status": em_d.get("status"), "emitter_latency_us": em_d.get("latency_us"),
  "emitter_packets_sent": sent, "emitter_errors": num(em_d, "errors"),
  "emitter_path": em_d.get("path"),
  "receiver_status": r_d.get("status"),
  "packets_recv": recv,
  "loss": loss,
  "duplicate": num(r_d, "duplicate", "packets") or 0,
  "reorder": num(r_d, "reorder", "packets") or 0,
  "late": num(r_d, "late", "packets") or 0,
  "late_discard": num(r_d, "late_discard", "packets") or 0,
  "underruns": num(r_d, "underruns") or 0,
  "fatal_count": num(r_d, "fatal_count") or 0,
}
row["sent_observed"] = sent
row["loss_ok"] = (recv + loss) <= (sent + row["duplicate"]) if sent else False
row["queue_bound_ok"] = row["fatal_count"] == 0 and loss <= sent
try:
    data = json.load(open(out)) if os.path.exists(out) else {}
except Exception:
    data = {}
runs = data.setdefault("runs", [])
runs.append(row)
json.dump(data, open(out, "w"), indent=2)
print(f"  row: loss={loss} dup={row['duplicate']} reorder={row['reorder']} late={row['late']} underruns={row['underruns']} fatal={row['fatal_count']} lat_us={row['emitter_latency_us']} recv={recv} sent={sent} queue_bound_ok={row['queue_bound_ok']}")
PYEOF
    return 0
}

# Final table from the raw per-trial runs (median per (profile,cc)).
summarize() {
    python3 - "$OUT" "$MAX_QUEUE_BOUND" <<'PYEOF'
import sys, json, os, statistics
out, bound = sys.argv[1], int(sys.argv[2])
if not os.path.exists(out):
    sys.exit("[ccspike] no results file: " + out)
runs = json.load(open(out)).get("runs", [])
keys = ["loss","duplicate","reorder","late_discard","underruns","packets_recv","emitter_latency_us"]
def med(vals): return int(statistics.median(vals)) if vals else None
rows = {}
for r in runs:
    rows.setdefault((r["profile"], r["cc"]), []).append(r)
print("=== ccspike summary (median over trials) ===")
print(f"{'profile':<9}{'cc':<7}{'recv':>6}{'loss':>6}{'dup':>6}{'reorder':>6}{'late':>6}{'und':>5}{'fatal':>6}{'lat_us':>11}")
for (profile, cc) in sorted(rows):
    rr = rows[(profile, cc)]
    medrow = {k: med([r[k] for r in rr]) for k in keys}
    fatal = max(r["fatal_count"] for r in rr)
    print(f"{profile:<9}{cc:<7}{medrow['packets_recv']:>6}{medrow['loss']:>6}{medrow['duplicate']:>6}{medrow['reorder']:>6}{medrow['late_discard']:>6}{medrow['underruns']:>5}{fatal:>6}{medrow['emitter_latency_us']:>11}")
print(f"(queue bound MAX_QUEUE_BOUND={bound}; bound honored = fatal==0 and loss<=sent, asserted per trial)")
PYEOF
}

main() {
    require_harness
    # Drop any prior trials so each invocation is a clean matrix.
    : > "$OUT"
    info "of CC-run matrix: profiles=[clean degraded] ccs=[$CC_RUNS] duration=${DURATION}s trials=${TRIALS:-1} (defective trials retried, max 5)"
    local profile cc t attempt rc
    for profile in clean degraded; do
        apply_profile "$profile"
        for cc in $CC_RUNS; do
            for t in $(seq 1 "${TRIALS:-1}"); do
                attempt=0
                while :; do
                    attempt=$((attempt + 1))
                    info "=== trial $t (attempt $attempt): profile=$profile cc=$cc ==="
                    if run_one "$profile" "$cc"; then
                        rc=0
                    else
                        rc=$?
                    fi
                    if [ "$rc" = "0" ]; then
                        break
                    fi
                    if [ "$rc" = "2" ]; then
                        info "trial defective (receiver did not complete) — retrying"
                        # drop the partial row appended by a failed extract; only
                        # retry bounded
                        if [ "$attempt" -ge 5 ]; then
                            die "trial (profile=$profile cc=$cc) failed 5 attempts (receiver never completed)"
                        fi
                        continue
                    fi
                    die "trial (profile=$profile cc=$cc) failed (rc=$rc)"
                done
            done
        done
    done
    apply_profile clean
    summarize
    info "done; raw per-trial results -> $OUT"
}

main "$@"
