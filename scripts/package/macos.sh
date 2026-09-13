#!/usr/bin/env bash
# scripts/package/macos.sh — build the macOS emitter and wrap it in a .app + DMG.
#
# Produces, under $WDR_DIST_DIR/macos/:
#   macos-emitter.app/          ad-hoc signed .app bundle (MacOS/, Info.plist, Resources/)
#   macos-emitter-<shortrev>.dmg  read-only UDZO disk image (hdiutil, no GUI)
#
# Dependency-free (no create-dmg): plain hdiutil + codesign + xcrun swiftc.
# Ad-hoc signature only (no identity on this host / CI): a production, hardened
# runtime + notarized build is a documented credential gate
# (docs/planning/RELEASE_AND_SIGNING.md).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
# shellcheck disable=SC1091
. "$ROOT/scripts/package/common.sh"

[ "$(uname -s)" = "Darwin" ] || die "macOS packaging must run on a macOS host (got '$(uname -s)')"

need_cmd cargo codesign hdiutil xcrun swiftc cmake python3

CRATE_DIR="$ROOT/platform/macos-emitter"
OUT_DIR="$WDR_DIST_DIR/macos"
STAGE="$OUT_DIR/macos-emitter.app"
CONTENTS="$STAGE/Contents"

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$CRATE_DIR/Cargo.toml" | head -n 1)"
[ -n "${VERSION:-}" ] || VERSION="0.1.0"
REV="$(rev)"

# --- 1) release build -----------------------------------------------------------
log "cargo build --release (macos-emitter, bin)"
(
    cd "$CRATE_DIR"
    cargo build --release --bin macos_emitter
)
BIN="$CRATE_DIR/target/release/macos_emitter"
[ -x "$BIN" ] || die "release binary not produced: $BIN"

# --- 2) assemble the .app ---------------------------------------------------------
rm -rf "$STAGE"
mkdir -p "$CONTENTS/MacOS" "$CONTENTS/Resources/bin"

# 2a) Compile the real AppKit GUI (platform/macos-emitter/app/main.swift).
#     @main + top-level helper code => MUST pass -parse-as-library.
#     Target the host arch, min macOS 13 (matches LSMinimumSystemVersion).
#     The GUI is the .app's main executable; it shells out to the Rust CLI in
#     Resources/bin/ so capture logic lives only in the Rust crate.
log "swiftc app/main.swift -> Contents/MacOS/macos-emitter"
ARCH="$(uname -m)"   # arm64 | x86_64
xcrun swiftc -parse-as-library -target "$ARCH-apple-macos13.0" -O \
    -o "$CONTENTS/MacOS/macos-emitter" \
    "$CRATE_DIR/app/main.swift"
[ -x "$CONTENTS/MacOS/macos-emitter" ] \
    || die "swiftc did not produce a GUI executable"

# 2b) Bundle the Rust CLI next to the GUI (main.swift looks it up there).
cp -f "$BIN" "$CONTENTS/Resources/bin/macos-emitter"
cp -f "$CRATE_DIR/Resources/AppIcon.icns" "$CONTENTS/Resources/AppIcon.icns"
chmod +x "$CONTENTS/Resources/bin/macos-emitter"

# No Swift-runtime embedding on macOS (unlike iOS): since macOS 10.14.4 the
# Swift runtime is part of the OS (libswiftCore.dylib etc. resolve from
# /usr/lib/swift via the dyld shared cache). A modern Xcode toolchain doesn't
# even ship the macOS runtime dylibs to copy. Load commands reference
# /usr/lib/swift/... directly (otool -L), which is what stock Swift apps do.

cat > "$CONTENTS/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key>
  <string>macos-emitter</string>
  <key>CFBundleIdentifier</key>
  <string>dev.wavelink.macos</string>
  <key>CFBundleName</key>
  <string>macos-emitter</string>
  <key>CFBundleDisplayName</key>
  <string>Wavelink</string>
  <key>CFBundleIconFile</key>
  <string>AppIcon</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleShortVersionString</key>
  <string>$VERSION</string>
  <key>CFBundleVersion</key>
  <string>$VERSION</string>
  <key>NSPrincipalClass</key>
  <string>NSApplication</string>
  <key>NSScreenCaptureUsageDescription</key>
  <string>Wavelink captures the system audio mix to stream it to your receiver device. Screen Recording access is required for system-audio capture.</string>
  <key>LSMinimumSystemVersion</key>
  <string>13.0</string>
  <key>NSHumanReadableCopyright</key>
  <string>Copyright (C) 2026 Wavelink — proprietary, source-available. See LICENSE.</string>
</dict>
</plist>
EOF

emit_license_notice "$CONTENTS/Resources"
printf 'macOS emitter %s (%s)\n\nWavelink — see PACKAGING.md, RELEASE_AND_SIGNING.md\n' \
    "$VERSION" "$REV" > "$CONTENTS/Resources/README.txt"

# --- 3) ad-hoc codesign ----------------------------------------------------------
# Sign every Mach-O explicitly (GUI main exec + the bundled CLI under
# Resources/), then seal the bundle. --verify --deep --strict then passes.
log "codesign --force --sign - (ad-hoc)"
codesign --force --sign - --timestamp=none "$CONTENTS/Resources/bin/macos-emitter"
codesign --force --sign - --timestamp=none \
    "$CONTENTS/MacOS/macos-emitter"
codesign --force --sign - --timestamp=none "$STAGE"
codesign --verify --deep --strict "$STAGE"
log "codesign verify OK"

# --- 4) read-only DMG (hdiutil; no GUI session needed for UDZO/UDRO) -------------
DMG="$OUT_DIR/wavelink-$REV.dmg"
rm -f "$DMG"
log "hdiutil create -format UDZO (reads-only image)"
hdiutil create \
    -volname "Wavelink $VERSION" \
    -srcfolder "$STAGE" \
    -ov \
    -format UDZO \
    "$DMG"
[ -f "$DMG" ] || die "dmg not produced"

log "macOS packaging complete:"
ls -lh "$DMG"
log "app: $STAGE"
