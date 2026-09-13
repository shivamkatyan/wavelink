#!/usr/bin/env bash
# scripts/package/ios.sh — verify + package the merged iOS app source (unsigned).
#
# The shipped iOS surface is the single merged Wavelink app under
# `platform/ios` (Emitter + Receiver roles chosen in-app; the split-era core
# libraries were retired, so the cores are inlined and there is no
# `-emit-library` output to ship).
#
# Produces, under $WDR_DIST_DIR/ios/:
#   wavelink-ios-source-<rev>.tar.gz  — simulator-SDK type-check gate report +
#                                       docs (the signed .ipa path still
#                                       credential/Xcode-project gated)
#
# Runs on a macOS runner / this macOS host with NO device and NO signing: the
# exact simulator-SDK `xcrun -sdk iphonesimulator swiftc ... -typecheck
# -warnings-as-errors` command is the one documented in platform/ios/build-check.md.
# A real signed .ipa requires an Xcode project wrapper + Apple signing/dev
# account — a documented credential + Xcode-project gate, not fabricated here.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
# shellcheck disable=SC1091
. "$ROOT/scripts/package/common.sh"

[ "$(uname -s)" = "Darwin" ] || die "iOS packaging must run on a macOS host"
need_cmd xcrun tar

OUT_DIR="$WDR_DIST_DIR/ios"
REV="$(rev)"
mkdir -p "$OUT_DIR"
rm -rf "$OUT_DIR"/stage-* "$OUT_DIR"/wavelink-ios-source-*.tar.gz "$OUT_DIR"/wdr-ios-unsigned-*.tar.gz

APP="$ROOT/platform/ios"
[ -d "$APP/Sources/WavelinkApp" ] || die "merged iOS app source not found at $APP"

# --- 1) compile gate: the merged app must type-check against the simulator SDK.
TARGET="arm64-apple-ios14.0-simulator"
SDK="iphonesimulator"
GATE_LOG="$OUT_DIR/typecheck.log"
log "swiftc -typecheck (simulator SDK) merged Wavelink app -> $GATE_LOG"
if ! (
    cd "$APP"
    xcrun -sdk "$SDK" swiftc \
        -target "$TARGET" -parse-as-library \
        -typecheck -warnings-as-errors \
        Sources/WavelinkApp/*.swift Sources/WavelinkApp/Shared/*.swift \
        Sources/WavelinkApp/Emitter/*.swift Sources/WavelinkApp/Receiver/*.swift \
        > "$GATE_LOG" 2>&1
); then
    cat "$GATE_LOG"
    die "merged iOS app type-check FAILED (0-error gate) — see $GATE_LOG"
fi
printf 'type-check PASS: 0 errors (warnings-as-errors); command documented in platform/ios/build-check.md\n' > "$GATE_LOG"

# --- 2) docs (honest about the signed .ipa path) --------------------------------------
emit_license_notice "$OUT_DIR"

cat > "$OUT_DIR/README.md" <<'EOF'
# Wavelink iOS — merged app source (compile gate)

The shipped iOS surface is the single merged Wavelink app under `platform/ios`
(Emitter + Receiver roles chosen in-app). The old split core libraries were
retired — the cores are inlined, so this artifact is the **simulator-SDK
type-check gate** (0 errors, warnings-as-errors) plus the docs for producing a
real build. It is NOT an installable or signed app.

To distribute a real signed `.ipa` you must:

1. Wrap `platform/ios` in an **Xcode project** (the repo intentionally holds no
   `.xcodeproj` — generated e.g. with `xcodegen`) with the SwiftUI app target
   plus the emitter's ReplayKit **broadcast-extension** embedded target.
2. Obtain **Apple signing + a Developer/Account team** and build against the
   **device** SDK (`iphoneos`) with a provisioning profile.
3. Notarize (App Store or Developer ID) — see `RELEASE_AND_SIGNING.md` and
   `docs/planning/RELEASE_AND_SIGNING.md`.

Device runtime (ReplayKit broadcast, `.usbAudio` DAC, local-network TCC) is a
hardware/device gate — see `platform/ios/build-check.md`.
EOF

cp -f "$ROOT/docs/planning/RELEASE_AND_SIGNING.md" "$OUT_DIR/RELEASE_AND_SIGNING.md"
[ -f "$APP/build-check.md" ] && cp -f "$APP/build-check.md" "$OUT_DIR/build-check.md"
printf 'Wavelink iOS — merged app source + type-check gate (rev %s)\n' "$REV" > "$OUT_DIR/VERSION"

# --- 3) tarball ------------------------------------------------------------------------
TARBALL="$OUT_DIR/wavelink-ios-source-$REV.tar.gz"
STAGE="$OUT_DIR/stage-$$"
mkdir -p "$STAGE"
cp "$OUT_DIR/README.md" "$OUT_DIR/RELEASE_AND_SIGNING.md" "$OUT_DIR/build-check.md" \
   "$OUT_DIR/typecheck.log" "$OUT_DIR/LICENSE-NOTICES.txt" "$STAGE/"
tar -C "$STAGE" -czf "$TARBALL" .
rm -rf "$STAGE"
[ -f "$TARBALL" ] || die "tarball not produced"

log "iOS packaging complete:"
ls -lh "$TARBALL"
