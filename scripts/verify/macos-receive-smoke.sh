#!/usr/bin/env bash
# scripts/verify/macos-receive-smoke.sh — the software gate for the macOS
# desktop **receiver** render path (WS3): `macos-emitter --receive --sink null`
# (the `QuicRenderReceiver` seam → null render device) receives a Pro/FLAC
# fixture emitted by `macos-emitter --stream` and must hash equal to the
# canonical golden — the same b7a3c25c… the `--stream` smoke proves. This
# exercises BOTH shell roles (fixture emitter + receiver) host-to-host over
# loopback. NO hardware/TCC required: `--sink null` needs no output device, so
# this is the host-verified proof that the shell's receive half rides the
# exact proven decode pipeline.
#
# Usage: bash scripts/verify/macos-receive-smoke.sh
# Exit 0 = gate PASS, non-zero = gate FAIL.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
# shellcheck disable=SC1091
. "$ROOT/scripts/package/common.sh"

need_cmd cargo python3
source "$ROOT/dev/env.sh" >/dev/null 2>&1 || true

RX_ADDR="127.0.0.1:9102"
GOLDEN="b7a3c25c8ccaa05643f14dfb5bc0e223f4ace3154c11ecd25bed5975d839c223"

log "build macos_emitter (shell CLI with --stream + --receive)"
( cd "$ROOT/platform/macos-emitter" && cargo build --release --bin macos_emitter )

# The SCK shim links the Swift runtime (libswift_Concurrency.dylib), NOT in
# the OS dyld shared cache on this host — same resolver the stream smoke uses.
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
if [ -n "${SWIFT_LIB:-}" ]; then
    DYLD_LIBRARY_PATH="$SWIFT_LIB"; export DYLD_LIBRARY_PATH
else
    warn "Swift runtime not found; --stream/--receive may fail to load (SCK shim)"
fi

OUT="$(mktemp -d)/receive.ndjson"
"$ROOT/platform/macos-emitter/target/release/macos_emitter" --receive \
    --addr "$RX_ADDR" --sink null --buffer balanced --timeout 120 >"$OUT" &
local_rx=$!
sleep 0.5   # let the receiver bind before the emitter dials

# Fixture emitter (no TCC needed) reproduces the canonical 4096-sample golden.
"$ROOT/platform/macos-emitter/target/release/macos_emitter" --stream \
    --addr "$RX_ADDR" --tier pro --codec flac --fixture pseudo-random
wait "$local_rx"

# The shell's receive driver emits NDJSON: start → complete → (fatal on error).
python3 - "$OUT" "$GOLDEN" <<'PY'
import json, sys
path, golden = sys.argv[1], sys.argv[2]
events = [json.loads(line) for line in open(path) if line.strip()]
assert events, "no NDJSON events from macos-emitter --receive"
assert events[0].get("ev") == "start", events
last = events[-1]
assert last.get("ev") == "complete", last
assert last.get("status") == "complete", last
assert last.get("hash") == golden, (last.get("hash"), golden)
assert last.get("packets_recv", 0) > 0, last
for k in ("loss", "duplicate", "reorder", "late_discard"):
    assert last.get(k, 0) == 0, (k, last)
assert last.get("underruns", 0) == 0, last
print(f"PASS receive/null: packets={last['packets_recv']} hash={last['hash']}")
PY

log "macOS receive (null sink) gate PASS"
