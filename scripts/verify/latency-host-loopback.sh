#!/usr/bin/env bash
#
# latency-host-loopback.sh — WS-H host-loopback latency evidence.
#
# Runs `wdr_refsim --example latprobe` for every buffer profile (5 runs × 256
# FLAC frames each) and writes a dated, labeled section into
# docs/orchestration/reports/t-P1-elat.md.
#
# Honesty (LATENCY_MEASUREMENT.md): these are SIMULATED / UPPER-BOUND reference
# loopback numbers (no physical DAC; no device SLOs). The in-code T0..T3 hooks
# (§11) and the physical-device probes (Balanced ≤150 ms / Low ≤80 ms; start
# ≤3 s) remain device-gated. The first-frame measurement (≈52 ms p50) is the
# host upper-bound for "publish → first render"; recovery ≤5 s is proven by the
# wdr_session reconnect/backoff FSM tests (FR-025), not by this loopback run.
#
# Usage:
#   bash scripts/verify/latency-host-loopback.sh          # appends to the report

set -euo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

# shellcheck disable=SC1091
source dev/env.sh

REPORT=docs/orchestration/reports/t-P1-elat.md
PROFILES=(balanced low resilient)
RUNS=5
FRAMES=256
DATE="$(date +%Y-%m-%d)"

run_probe() {
    local profile="$1"
    echo "### \`${profile}\`"
    echo '```'
    cargo run -q -p wdr_refsim --release --example latprobe \
        -- --profile "${profile}" --runs "${RUNS}" --frames "${FRAMES}" \
        | grep -E "first_frame_to_render_ms|per_frame_latency_ms"
    echo '```'
}

{
    echo
    echo "---"
    echo
    echo "# Host reference-loopback latency — p50/p95/p99 re-measure (${DATE}, simulated)"
    echo
    echo "Re-run driven by \`scripts/verify/latency-host-loopback.sh\`:"
    echo "\`wdr_refsim --example latprobe\` times \`QuicAudioSink → QuicRenderReceiver\`"
    echo "per frame over quinn loopback (FLAC i16/48k stereo, 512 spc), ${RUNS} runs × ${FRAMES}"
    echo "frames per profile. **Simulated / upper-bound** — no physical DAC; device-SLO"
    echo "probes (Balanced ≤150 ms, Low ≤80 ms, start ≤3 s) stay device-gated. The"
    echo "first-frame row is the host 'publish → first render' upper bound; recovery ≤5 s"
    echo "is proven by the wdr_session reconnect/backoff FSM (FR-025), not this loopback."
    echo
    for p in "${PROFILES[@]}"; do
        run_probe "$p"
        echo
    done
} >> "${REPORT}"

echo "appended measured section to ${REPORT}"
