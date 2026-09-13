#!/usr/bin/env bash
#
# docker/soak.sh — 60-minute clean soak for the reference system.
#
# Runs ref_receiver (fresh) + ref_emitter (--lossless flac, --seconds 3600)
# through the netem bridge (clean profile, no impairment). Samples the
# receiver's RSS every 60s (memory-growth signal) and asserts on completion:
#   - receiver status complete, underruns=0, fatal_count=0
#   - emitter status ok
#   - receiver hash == emitter hash (lossless preserved over the long run)
# Writes a summary to $WDR_SOAK_OUT (default /tmp/opencode/soak-result.json).
#
# Usage: WDR_SOAK_SECONDS=3600 bash docker/soak.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SOAK_SECS="${WDR_SOAK_SECONDS:-3600}"
OUT="${WDR_SOAK_OUT:-/tmp/opencode/soak-result.json}"
RSS_LOG="${WDR_SOAK_RSS:-$(dirname "$OUT")/soak-rss.log}"
NET="${WDR_NET:-wdr_wdr-net}"
# Unique container names (env-overridable) so a fresh run can never collide
# with a stale container from a prior run with the same hardcoded name.
RECV="${WDR_SOAK_RECV:-wdr-soak-recv}"
EMIT="${WDR_SOAK_EMIT:-wdr-soak-emit}"

mkdir -p "$(dirname "$OUT")"
: > "$RSS_LOG"

cleanup() {
  docker rm -f "$EMIT" "$RECV" >/dev/null 2>&1 || true
}
trap cleanup EXIT

echo "[soak] starting clean-soak for ${SOAK_SECS}s (lossless flac, pseudo-random)"

# ensure topology containers exist
docker start wdr-netem wdr-metrics >/dev/null 2>&1 || \
  (cd "$ROOT" && docker compose up -d netem metrics >/dev/null 2>&1) || true
# ensure clean profile (no netem qdisc)
docker exec wdr-netem sh -c 'tc qdisc del dev eth0 root 2>/dev/null || true; tc -s qdisc show dev eth0' >/dev/null 2>&1 || true

docker rm -f "$RECV" "$EMIT" >/dev/null 2>&1 || true

# Clear STALE sim metrics from the shared volume before this run — otherwise a
# killed/restarted run reads the PREVIOUS run's emitter/receiver JSON (observed
# on the macOS host 2026-09-10: a killed 3600s run was reported against the
# prior 30s run's hashes). Mirror ccspike.sh.
docker run --rm -v wdr_wdr-metrics:/tmp/metrics --entrypoint sh wdr-dev \
  -c 'rm -f /tmp/metrics/receiver-sim.json /tmp/metrics/emitter-sim.json' >/dev/null 2>&1 || true

# receiver
docker run -d --name "$RECV" --network "$NET" \
  -e WDR_SIM_ROLE=receiver -e WDR_METRICS_DIR=/tmp/metrics \
  -v wdr_wdr-metrics:/tmp/metrics \
  --entrypoint /workspace/target/release/ref_receiver wdr-dev \
  --role receiver --buffer balanced 0.0.0.0:9000 >/dev/null 2>&1
sleep 1
RIP="$(docker inspect "$RECV" --format '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}')"
echo "[soak] receiver at $RIP"

# memory sampler (background): record receiver RSS every 60s, only while the
# receiver container is actually running (docker stats on a stopped container
# returns 0B, which previously garbled the memory-growth signal).
sleep 5
(
  for _ in $(seq 1 $(( (SOAK_SECS / 60) + 2 ))); do
    if docker ps --filter "name=$RECV" --filter "status=running" --format x | grep -q x; then
      docker stats --no-stream --format "{{.MemUsage}} {{.CPUPerc}}" "$RECV" >> "$RSS_LOG" 2>/dev/null || true
    fi
    sleep 60
  done
) &
SAMPLER=$!

# emitter (detached; writes its own metrics). Paced by each frame's audio
# duration (WDR_EMIT_REAL_TIME=1) so `--seconds 3600` genuinely spans ~one hour
# of wall-clock (real-time soak) regardless of codec frame size — a fixed
# per-frame sleep would overrun for small frames (512 @48k = 10.7ms/frame).
docker run -d --name "$EMIT" --network "container:wdr-netem" \
  -e WDR_SIM_ROLE=emitter -e WDR_METRICS_DIR=/tmp/metrics -e WDR_ENT_TIER=pro \
  -e WDR_EMIT_REAL_TIME=1 \
  -v wdr_wdr-metrics:/tmp/metrics \
  --entrypoint /workspace/target/release/ref_emitter wdr-dev \
  --role emitter --lossless --codec flac --source pseudo-random \
  --seconds "$SOAK_SECS" "$RIP:9000" >/dev/null 2>&1

# bounded wait for emitter to finish (poll container state, not sleep-and-assume).
# The paced run's wall time is SOAK_SECS of audio + connect/teardown + per-frame
# scheduling overhead on the shared host; the fixed 600s pad was calibrated on
# WSL2 and UNDER-CALIBRATES macOS/Docker-Desktop virtualization (a 3600s run did
# not finish within +600s on 2026-09-10 — INC-004). Default pad is now
# ratio-based: 50% of SOAK_SECS + 120s, overridable via WDR_SOAK_DEADLINE_PAD.
PAD="${WDR_SOAK_DEADLINE_PAD:-}"
if [ -z "$PAD" ]; then
  PAD=$(( (SOAK_SECS * 50) / 100 + 120 ))
fi
echo "[soak] emitter started; monitoring up to $((SOAK_SECS + PAD))s"
deadline=$(( $(date +%s) + SOAK_SECS + PAD ))
while docker ps -a --filter "name=$EMIT" --filter "status=running" --format x | grep -q x; do
  if [ "$(date +%s)" -ge "$deadline" ]; then
    echo "[soak] emitter did not finish in time; killing" >&2
    docker kill "$EMIT" >/dev/null 2>&1 || true
    break
  fi
  sleep 10
done

wait "$SAMPLER" 2>/dev/null || true

EMIT_METRICS="$(docker run --rm -v wdr_wdr-metrics:/tmp/metrics:ro --entrypoint sh wdr-dev -c 'cat /tmp/metrics/emitter-sim.json 2>/dev/null' 2>/dev/null || echo '{}')"
RECV_METRICS="$(docker run --rm -v wdr_wdr-metrics:/tmp/metrics:ro --entrypoint sh wdr-dev -c 'cat /tmp/metrics/receiver-sim.json 2>/dev/null' 2>/dev/null || echo '{}')"
EMIT_EXIT="$(docker inspect "$EMIT" --format '{{.State.ExitCode}}' 2>/dev/null || echo 1)"

E_HASH="$(printf '%s' "$EMIT_METRICS" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("hash",""))' 2>/dev/null || true)"
R_HASH="$(printf '%s' "$RECV_METRICS" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("hash",""))' 2>/dev/null || true)"
R_STATUS="$(printf '%s' "$RECV_METRICS" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("status",""))' 2>/dev/null || true)"
R_UNDERRUNS="$(printf '%s' "$RECV_METRICS" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("underruns",""))' 2>/dev/null || true)"
R_FATAL="$(printf '%s' "$RECV_METRICS" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("fatal_count",""))' 2>/dev/null || true)"
E_STATUS="$(printf '%s' "$EMIT_METRICS" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("status",""))' 2>/dev/null || true)"
E_LAT="$(printf '%s' "$EMIT_METRICS" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("latency_us",""))' 2>/dev/null || true)"

if [ "$EMIT_EXIT" = "0" ] && [ "$E_STATUS" = "ok" ] && [ "$R_STATUS" = "complete" ] \
   && [ "$E_HASH" = "$R_HASH" ] && [ "$R_UNDERRUNS" = "0" ] && [ "$R_FATAL" = "0" ] && [ -n "$E_HASH" ]; then
  RESULT="PASS"
else
  RESULT="FAIL"
fi

python3 - "$OUT" "$RESULT" "$SOAK_SECS" "$E_HASH" "$EMIT_EXIT" "$E_STATUS" "$R_STATUS" "$R_UNDERRUNS" "$R_FATAL" "$E_LAT" "$RSS_LOG" <<'PYEOF'
import json, sys, os
out,res,secs,eh,ee,es,rs,ru,rf,el,rss_log = sys.argv[1:]
# min/max RSS from the sampler log (memory-growth signal). Honor WDR_SOAK_RSS;
# docker stats MemUsage is "<x>MiB / <y>GiB <cpu>%", so the value is token 0.
rss=[]
if os.path.exists(rss_log):
    for line in open(rss_log):
        try:
            mem=line.split()[0].rstrip("MiB")
            rss.append(float(mem))
        except Exception:
            pass
summary=dict(result=res, soak_seconds=int(secs),
  emitter=dict(exit=int(ee), status=es, hash=eh, latency_us=el),
  receiver=dict(status=rs, underruns=int(ru or 0), fatal=int(rf or 0), hash=eh),
  memory_mib=dict(min=min(rss) if rss else None, max=max(rss) if rss else None,
                  samples=len(rss)),
  rss_log=os.path.abspath(rss_log))
json.dump(summary, open(out,"w"), indent=2)
print("[soak] RESULT=" + res)
PYEOF
echo "[soak] summary -> $OUT"
