#!/usr/bin/env bash
#
# docker/sim-start.sh — run a reference simulator, or degrade to a readiness
# placeholder when crates/wdr_refsim has not landed yet.
#
# Entrypoint for emitter-sim and receiver-sim (WDR_ROLE in [emitter, receiver]).
# Sequence (bounded polling — NEVER sleep-and-assume):
#   1. Wait for the netem readiness file on the shared metrics volume
#      (bounded retry loop + deadline).
#   2. If the ref binary exists, `cargo run -q -p wdr_refsim --bin ref_<role>`
#      with the metrics dir configured via env (interface owned by the refsim
#      worker — no unknown CLI flags are passed, keeping us compatible whether
#      or not the binary supports positional args).
#   3. Else: write a "sim not ready" metrics JSON and stay alive as a
#      placeholder, still honouring the readiness protocol so the harness
#      validates topology/NET_ADMIN before refsim lands.

set -euo pipefail

ROLE="${WDR_ROLE:-emitter}"
case "$ROLE" in emitter|receiver) ;; *) echo "[sim][error] WDR_ROLE must be emitter|receiver (got '$ROLE')." >&2; exit 1 ;; esac

METRICS_DIR="${WDR_METRICS_DIR:-/tmp/metrics}"
NETEM_READY="$METRICS_DIR/netem-ready.json"   # shared volume file written by netem.sh
PROFILE="${WDR_NETEM_PROFILE:-clean}"
POLL_INTERVAL="${WDR_SIM_POLL_INTERVAL:-2}"
POLL_DEADLINE="${WDR_SIM_POLL_DEADLINE:-300}"

info() { printf '[sim:%s] %s\n' "$ROLE" "$*"; }
die()  { printf '[sim:%s][error] %s\n' "$ROLE" "$*" >&2; exit 1; }

mkdir -p "$METRICS_DIR"

# 1. Wait for netem to have applied its profile (ready file on shared volume).
info "waiting for netem readiness (max ${POLL_DEADLINE}s)..."
deadline=$(( $(date +%s) + POLL_DEADLINE ))
while [ ! -f "$NETEM_READY" ]; do
    if [ "$(date +%s)" -ge "$deadline" ]; then
        info "netem readiness not observed in time; continuing in placeholder-safe mode."
        break
    fi
    sleep "$POLL_INTERVAL"
done
info "netem ready: $(cat "$NETEM_READY" 2>/dev/null || echo '(not-yet)' )"

# 2. Reference sim availability check. Both ref_emitter and ref_receiver live in
#    crates/wdr_refsim (workspace). Dockerfile.dev preinstalls the toolchain,
#    so `cargo run` compiles on first boot inside the dev image.
SIM_BIN="ref_${ROLE}"
# Prebuilt release binary baked into the image (Dockerfile.dev). Preferred on
# hosts where the runtime compose network is internal (macOS Docker Desktop):
# there is no egress for a first-time cargo/rustup fetch, and the host's own
# target/release is a different OS/arch (Mach-O vs Linux ELF). Hermetic.
RELSIM="/workspace/target/release/$SIM_BIN"
if [ -x "$RELSIM" ]; then
    info "starting reference simulator (prebuilt release): $RELSIM"
    export WDR_METRICS_DIR WDR_SIM_ROLE="$ROLE" WDR_NETEM_PROFILE="$PROFILE"
    exec "$RELSIM"
fi
if [ -d "/workspace/crates/wdr_refsim" ] || [ -x "/workspace/target/debug/$SIM_BIN" ] || command -v "$SIM_BIN" >/dev/null 2>&1; then
    info "starting reference simulator: cargo run -q -p wdr_refsim --bin $SIM_BIN"
    # Config is passed via env only — the exact CLI surface is owned by the
    # refsim worker; we stay compatible regardless of how it parses args.
    export WDR_METRICS_DIR WDR_SIM_ROLE="$ROLE" WDR_NETEM_PROFILE="$PROFILE"
    exec cargo run --quiet -p wdr_refsim --bin "$SIM_BIN"
fi

# 3. Ref sim not landed yet -> graceful placeholder.
info "reference sim '$SIM_BIN' not found in workspace; placeholder mode."
cat > "$METRICS_DIR/${ROLE}-sim.json" <<EOF
{"role":"$ROLE","status":"not_ready","profile":"$PROFILE",
 "note":"ref sim binary unavailable; harness structure-only",
 "hash":null,"packets_sent":0,"packets_recv":0,
 "loss":{"packets":0},"duplicate":{"packets":0},"reorder":{"packets":0},
 "late":{"packets":0},"jitter_ms":null,"fatal_count":0,"bytes_sent":0}
EOF
info "placeholder: wrote $METRICS_DIR/${ROLE}-sim.json; staying alive."
exec sleep infinity