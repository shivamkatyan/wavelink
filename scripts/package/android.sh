#!/usr/bin/env bash
# scripts/package/android.sh — build + sign the combined Wavelink Android app.
#
# For the single combined app `platform/android-wavelink` (role picker; the
# split-era android-emitter/android-receiver shells are retired):
#   1. Generate a self-signed release keystore ONCE per project under
#      $WDR_DIST_DIR/android-keystore/ (DEV-ONLY; replace with real store/signing
#      secrets for production — see docs/planning/RELEASE_AND_SIGNING.md).
#   2. ./gradlew :app:assembleDebug :app:assembleRelease (JDK 17 + ANDROID_HOME).
#   3. zipalign + apksigner sign --ks (build-tools 34.0.0).
#   4. Copy authoritative-named artifacts into $WDR_DIST_DIR/android/.
#
# Idempotent: re-runs rebuild and overwrite the dist copies; the keystore is
# only created when absent. bash 3.2 compatible; no sudo required.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
# shellcheck disable=SC1091
. "$ROOT/scripts/package/common.sh"

# --- toolchain (overridable) --------------------------------------------------
GO_JAVA_HOME="${JAVA_HOME:-/opt/homebrew/opt/openjdk@17/libexec/openjdk.jdk/Contents/Home}"
GO_ANDROID_HOME="${ANDROID_HOME:-/opt/homebrew/share/android-commandlinetools}"
GO_BUILD_TOOLS="${ANDROID_BUILD_TOOLS:-34.0.0}"
export JAVA_HOME="$GO_JAVA_HOME"
export ANDROID_HOME="$GO_ANDROID_HOME"

[ -d "$JAVA_HOME" ] || die "JAVA_HOME not found at '$JAVA_HOME' (set JAVA_HOME to a JDK 17)"
[ -d "$ANDROID_HOME" ] || die "ANDROID_HOME not found at '$ANDROID_HOME'"
BT_DIR="$ANDROID_HOME/build-tools/$GO_BUILD_TOOLS"
[ -x "$BT_DIR/zipalign" ] || die "missing $BT_DIR/zipalign"
[ -x "$BT_DIR/apksigner" ] || die "missing $BT_DIR/apksigner"

OUT_DIR="$WDR_DIST_DIR/android"
KEY_DIR="$WDR_DIST_DIR/android-keystore"
mkdir -p "$OUT_DIR" "$KEY_DIR"

log "JDK: $JAVA_HOME"
log "Android SDK: $ANDROID_HOME (build-tools $GO_BUILD_TOOLS)"
log "keystores (DEV-ONLY): $KEY_DIR"

# --- iterate the android projects -------------------------------------------------
# One combined app per Wavelink (roles picked in-app). Build it explicitly so
# the emitter/receiver role apps are NOT shipped separately.
for proj_dir in "$ROOT/platform/android-wavelink"; do
    [ -d "$proj_dir" ] || continue
    [ -x "$proj_dir/gradlew" ] || continue

    project="$(basename "$proj_dir")"
    log "==> $project"

    # 1) DEV-ONLY signing identity, created once (never clobbered).
    KEYSTORE="$KEY_DIR/$project.keystore"
    PASSFILE="$KEY_DIR/$project.keystore.pass"
    if [ ! -f "$KEYSTORE" ]; then
        KS_PASS="wdr-dev-$(date +%s)$$"
        printf '%s\n' "$KS_PASS" > "$PASSFILE"
        chmod 600 "$PASSFILE"
        "$JAVA_HOME/bin/keytool" -genkeypair -v \
            -keystore "$KEYSTORE" \
            -storepass "$KS_PASS" -keypass "$KS_PASS" \
            -alias wdr -keyalg RSA -keysize 2048 -validity 10000 \
            -dname "CN=WDR Dev, OU=Dev, O=Wavelink, L=Local, ST=Local, C=US" \
            >/dev/null 2>&1 || die "keytool failed for $project keystore"
        log "generated dev keystore: $KEYSTORE (self-signed; DEV-ONLY)"
    else
        log "keystore exists (reused): $KEYSTORE"
    fi
    KS_PASS="$(cat "$PASSFILE")"

    # 2) Build debug + release.
    (
        cd "$proj_dir"
        log "[$project] ./gradlew :app:assembleDebug :app:assembleRelease"
        ./gradlew --no-daemon :app:assembleDebug :app:assembleRelease
    )

    # 3) Collect and sign.
    DEBUG_TMP="$(find "$proj_dir/app/build/outputs/apk/debug" -maxdepth 1 -name '*.apk' 2>/dev/null | sort | head -n 1)"
    RELEASE_UNSIGNED="$(find "$proj_dir/app/build/outputs/apk/release" -maxdepth 1 -name '*unsigned*.apk' 2>/dev/null | sort | head -n 1)"
    [ -n "$DEBUG_TMP" ] || die "[$project] no debug APK produced"
    [ -n "$RELEASE_UNSIGNED" ] || die "[$project] no unsigned release APK produced"

    cp -f "$DEBUG_TMP" "$OUT_DIR/$project-debug.apk"
    log "[$project] debug -> $OUT_DIR/$project-debug.apk"

    ALIGNED="$OUT_DIR/$project-release-aligned.apk"
    RELEASE_SIGNED="$OUT_DIR/$project-release-signed.apk"
    "$BT_DIR/zipalign" -f -p 4 "$RELEASE_UNSIGNED" "$ALIGNED"
    "$BT_DIR/apksigner" sign \
        --ks "$KEYSTORE" \
        --ks-pass "pass:$KS_PASS" \
        --key-pass "pass:$KS_PASS" \
        --out "$RELEASE_SIGNED" "$ALIGNED"
    rm -f "$ALIGNED"
    log "[$project] release (zipaligned + apksigner) -> $RELEASE_SIGNED"

    # Verify what we shipped is actually signed + aligned, then drop apksigner's
    # incremental-update .idsig sidecars (not part of the accepted artifact set).
    "$BT_DIR/apksigner" verify --verbose "$RELEASE_SIGNED" >/dev/null
    "$BT_DIR/zipalign" -c -p 4 "$RELEASE_SIGNED"
    rm -f "$OUT_DIR"/*.idsig
    log "[$project] apksigner verify OK; alignment OK"
done

log "android packaging complete: $OUT_DIR"
ls -lh "$OUT_DIR"
