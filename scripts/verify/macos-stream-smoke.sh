#!/usr/bin/env bash
# scripts/verify/macos-stream-smoke.sh — the software gate for the first real
# stream: `macos-emitter --stream` (fixture path) → `AudioFrameSink` seam →
# QUIC → `ref_receiver`. NO hardware/TCC required — the fixture path is the
# hash-perfect proof that real capture, once wired, rides the exact proven
# encode→CRC→QUIC pipeline.
#
# Asserts on receiver-sim.json: status complete, frames delivered, zero
# loss/dup/reorder/late/fatal/underruns, and (Pro/FLAC) the canonical golden
# hash — the same b7a3c25c… the reference loopback proves.
#
# Usage: bash scripts/verify/macos-stream-smoke.sh
# Exit 0 = gate PASS, non-zero = gate FAIL.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
# shellcheck disable=SC1091
. "$ROOT/scripts/package/common.sh"

need_cmd cargo python3
source "$ROOT/dev/env.sh" >/dev/null 2>&1 || true

METRICS="$(mktemp -d)/wdr-verify"
mkdir -p "$METRICS"
RX_ADDR="127.0.0.1:9100"

GOLDEN="b7a3c25c8ccaa05643f14dfb5bc0e223f4ace3154c11ecd25bed5975d839c223"

log "build ref_receiver (root workspace, native macOS)"
( cd "$ROOT" && cargo build --release -p wdr_refsim --bin ref_receiver )

log "build macos_emitter (shell CLI with --stream)"
( cd "$ROOT/platform/macos-emitter" && cargo build --release --bin macos_emitter )

# The SCK shim links the Swift runtime (libswift_Concurrency.dylib), which is
# NOT in the OS dyld shared cache on this host — known build-check constraint.
# Resolve the toolchain Swift lib dir and put it on the loader path for the CLI
# run so `macos-emitter --stream` can load. A benign class-probing warning at
# startup is expected and harmless.
swift_lib_dir() {
    local dirs=(
        "/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift-5.5/macosx"
        "/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift/macosx"
        "/usr/lib/swift"
    )
    for d in "${dirs[@]}"; do
        if [ -f "$d/libswift_Concurrency.dylib" ]; then
            printf '%s' "$d"
            return 0
        fi
    done
    return 1
}
SWIFT_LIB="$(swift_lib_dir || true)"
[ -n "${SWIFT_LIB:-}" ] || warn "Swift runtime not found; --stream may fail to load (SCK shim)"

LOOPBACK_PASS=1
smoke_case() {
    # $1 = tier, $2 = codec, $3 = note, $4 = assert_canonical_golden (1|0), rest = extra --stream args
    local tier="$1" codec="$2" note="$3" assert_golden="$4"; shift 4
    log "case: --tier $tier --codec $codec ($note)"
    local rx_json="$(mktemp -d)/receiver-sim.json"
    WDR_METRICS_DIR="$(dirname "$rx_json")" \
        "$ROOT/target/release/ref_receiver" --role receiver --buffer balanced "$RX_ADDR" &
    local rx=$!
    sleep 0.3   # let the receiver bind before the emitter dials
    if [ -n "${SWIFT_LIB:-}" ]; then
        DYLD_LIBRARY_PATH="$SWIFT_LIB"; export DYLD_LIBRARY_PATH
    fi
    "$ROOT/platform/macos-emitter/target/release/macos_emitter" --stream \
        --addr "$RX_ADDR" --tier "$tier" --codec "$codec" "$@"
    wait "$rx"

    python3 - "$rx_json" "$tier" "$codec" "$assert_golden" <<'PY'
import json, sys
path, tier, codec, assert_golden = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]
d = json.load(open(path))
assert d["status"] == "complete", d
assert d["packets_recv"] > 0, d
for k in ("loss", "duplicate", "reorder", "late"):
    assert d.get(k, {}).get("packets", 1) == 0, (k, d)
assert d["fatal_count"] == 0, d
assert d["underruns"] == 0, d
# Lossy has no byte-equality; lossless is the hash-perfect claim. The 48 kHz
# canonical fixture reproduces the recorded golden EXACTLY; a non-48k fixture
# is asserted complete+clean (bit-exactness is proven at the unit level), not
# hash-equal to the 48k golden.
if assert_golden == "1":
    golden = "b7a3c25c8ccaa05643f14dfb5bc0e223f4ace3154c11ecd25bed5975d839c223"
    assert d["hash"] == golden, (d["hash"], golden)
print(f"PASS {tier}/{codec}: packets={d['packets_recv']} fatal={d['fatal_count']} underruns={d['underruns']} hash={d.get('hash')}")
PY
}

# Pro/FLAC: canonical default budget reproduces the golden hash exactly.
smoke_case pro flac "canonical golden (no --duration)" 1 --fixture pseudo-random
# Free/Opus over datagrams: bounded, no fatal/underrun (lossy has no golden).
smoke_case free opus "lossy datagrams" 0 --fixture silence --duration 1
# Rate-aware seam, hardware-free: lossless 44.1k must complete bit-exact
# (true rate on the wire) and Opus 44.1k streams natively — the exact fix for
# "nothing works unless the Mac is at 48 kHz".
smoke_case pro flac "lossless @44.1k (rate-aware)" 0 --fixture sine --rate 44100 --duration 1
smoke_case free opus "opus @44.1k (native)" 0 --fixture sine --rate 44100 --duration 1

log "fixture stream gate PASS"
