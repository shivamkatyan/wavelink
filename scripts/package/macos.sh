#!/usr/bin/env bash
# scripts/package/macos.sh — build the macOS emitter and wrap it in a .app + DMG.
#
# Produces, under $WDR_DIST_DIR/macos/:
#   Wavelink.app/                  ad-hoc signed UNIVERSAL (arm64 + x86_64) .app
#   wavelink-<shortrev>.dmg        read-only UDZO install image with the standard
#                                  drag-to-install layout:
#                                    Wavelink.app   +  Applications  +  How to install.txt
#   (same standard layout a user expects: drag the app onto the Applications
#    folder alias to install, exactly like any normal macOS app DMG.)
#
# Dependency-free (no create-dmg): plain hdiutil + codesign + xcrun swiftc + lipo.
# Ad-hoc signature only (no identity on this host / CI): a production, hardened
# runtime + notarized build is a documented credential gate
# (docs/planning/RELEASE_AND_SIGNING.md). Because the build is ad-hoc/unsigned,
# a downloaded copy is quarantined by Gatekeeper on first open — the DMG ships
# explicit Right-click -> Open guidance in "How to install.txt".
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
# shellcheck disable=SC1091
. "$ROOT/scripts/package/common.sh"

[ "$(uname -s)" = "Darwin" ] || die "macOS packaging must run on a macOS host (got '$(uname -s)')"

need_cmd cargo codesign hdiutil xcrun swiftc lipo rustup cmake python3

CRATE_DIR="$ROOT/platform/macos-emitter"
OUT_DIR="$WDR_DIST_DIR/macos"
APP="$OUT_DIR/Wavelink.app"                 # branded bundle (was macos-emitter.app)
CONTENTS="$APP/Contents"
GUI_EXEC="Wavelink"                          # Mach-O name inside Contents/MacOS/
CLI_EXEC="macos-emitter"                     # Rust engine inside Resources/bin/

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$CRATE_DIR/Cargo.toml" | head -n 1)"
[ -n "${VERSION:-}" ] || VERSION="0.1.0"
REV="$(rev)"

# ---------------------------------------------------------------------------
# 1) UNIVERSAL release build of the Rust CLI engine (arm64 + x86_64).
#    Cross-building the other Darwin triple on a Mac needs no SDK gymnastics.
# ---------------------------------------------------------------------------
log "rustup target add aarch64-apple-darwin x86_64-apple-darwin"
rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null 2>&1 \
    || die "rustup could not add the Apple Darwin targets (is rustup on PATH?)"

log "cargo build --release (macos-emitter, bin) — arm64 + x86_64"
(
    cd "$CRATE_DIR"
    cargo build --release --target aarch64-apple-darwin --bin macos_emitter
    cargo build --release --target x86_64-apple-darwin --bin macos_emitter
)
CLI_ARM="$CRATE_DIR/target/aarch64-apple-darwin/release/macos_emitter"
CLI_X64="$CRATE_DIR/target/x86_64-apple-darwin/release/macos_emitter"
[ -x "$CLI_ARM" ] || die "arm64 CLI binary not produced: $CLI_ARM"
[ -x "$CLI_X64" ] || die "x86_64 CLI binary not produced: $CLI_X64"

# ---------------------------------------------------------------------------
# 2) assemble the .app
# ---------------------------------------------------------------------------
rm -rf "$APP"
mkdir -p "$CONTENTS/MacOS" "$CONTENTS/Resources/bin"

# 2a) Compile the real AppKit GUI (platform/macos-emitter/app/main.swift) for
#     BOTH arches and lipo them into one universal executable.
#     @main + top-level helper code => MUST pass -parse-as-library.
#     Min macOS 13 (matches LSMinimumSystemVersion).
#     The GUI is the .app's main executable; it shells out to the Rust CLI in
#     Resources/bin/ so capture logic lives only in the Rust crate.
log "swiftc app/main.swift (arm64 + x86_64) + lipo -> Contents/MacOS/$GUI_EXEC"
GUI_ARM="$OUT_DIR/.gui-arm64"
GUI_X64="$OUT_DIR/.gui-x86_64"
xcrun swiftc -parse-as-library -target arm64-apple-macos13.0 -O \
    -o "$GUI_ARM" "$CRATE_DIR/app/main.swift"
xcrun swiftc -parse-as-library -target x86_64-apple-macos13.0 -O \
    -o "$GUI_X64" "$CRATE_DIR/app/main.swift"
lipo -create -output "$CONTENTS/MacOS/$GUI_EXEC" "$GUI_ARM" "$GUI_X64"
rm -f "$GUI_ARM" "$GUI_X64"
[ -x "$CONTENTS/MacOS/$GUI_EXEC" ] \
    || die "swiftc/lipo did not produce a universal GUI executable"

# 2b) lipo the Rust CLI into one universal engine next to the GUI (main.swift
#     looks it up there).
CLI_UNI="$CONTENTS/Resources/bin/$CLI_EXEC"
lipo -create -output "$CLI_UNI" "$CLI_ARM" "$CLI_X64"
cp -f "$CRATE_DIR/Resources/AppIcon.icns" "$CONTENTS/Resources/AppIcon.icns"
chmod +x "$CLI_UNI"

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
  <string>$GUI_EXEC</string>
  <key>CFBundleIdentifier</key>
  <string>dev.wavelink.macos</string>
  <key>CFBundleName</key>
  <string>Wavelink</string>
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
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>LSApplicationCategoryType</key>
  <string>public.app-category.music</string>
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
printf 'Wavelink macOS emitter %s (%s)\n\nWavelink — see PACKAGING.md, RELEASE_AND_SIGNING.md\n' \
    "$VERSION" "$REV" > "$CONTENTS/Resources/README.txt"

# --- 3) ad-hoc codesign ----------------------------------------------------------
# Sign every Mach-O explicitly (GUI main exec + the bundled CLI under
# Resources/), then seal the bundle. --verify --deep --strict then passes.
log "codesign --force --sign - (ad-hoc)"
codesign --force --sign - --timestamp=none "$CLI_UNI"
codesign --force --sign - --timestamp=none "$CONTENTS/MacOS/$GUI_EXEC"
codesign --force --sign - --timestamp=none "$APP"
codesign --verify --deep --strict "$APP"
log "codesign verify OK"

# --- 4) standard drag-to-install DMG (hdiutil; no GUI session needed) ----------
# Best-effort: detach any stale Wavelink DMG mount from an earlier run of this
# script before creating the image (fast no-op when nothing is mounted).
DMG="$OUT_DIR/wavelink-$REV.dmg"
hdiutil detach "$DMG" -force >/dev/null 2>&1 || true
rm -f "$DMG"

# Stage the conventional layout a user expects: the .app, a /Applications
# alias, and a how-to-install note. hdiutil just wraps the folder verbatim.
DMG_STAGE="$OUT_DIR/Wavelink-$REV"
rm -rf "$DMG_STAGE"
mkdir -p "$DMG_STAGE"
cp -R "$APP" "$DMG_STAGE/Wavelink.app"
ln -s /Applications "$DMG_STAGE/Applications"

cat > "$DMG_STAGE/How to install.txt" <<EOF
How to install Wavelink $VERSION
===============================

1. Drag the Wavelink.app icon onto the Applications folder alias (or copy it
   to /Applications yourself).
2. Open /Applications/Wavelink.app.

First launch (important)
-----------------------
This is a developer build, signed ad-hoc but NOT notarized by Apple, so the
first time you open it macOS Gatekeeper will refuse with "can't be opened
because Apple cannot check it for malicious software". That is expected for
unsigned developer builds — not a problem with the app. To open it:

  - Right-click (Control-click) Wavelink.app in Finder, choose Open, then Open
    again in the dialog. This tells macOS once to trust this app.

  or from a terminal:

  xattr -dr com.apple.quarantine /Applications/Wavelink.app

3. First use: grant Screen Recording permission when macOS asks
   (System Settings > Privacy & Security > Screen Recording). The app explains
   why before the prompt. Without this grant it can list audio hardware but
   cannot capture system audio.

A real Developer ID + notarized release will remove this first-open step
entirely — that is a credentials gate, tracked in docs/planning/RELEASE_AND_SIGNING.md.

See docs/user/setup-and-install.md for full platform instructions.
EOF
cp -f "$CONTENTS/Resources/LICENSE-NOTICES.txt" "$DMG_STAGE/LICENSE-NOTICES.txt"

log "hdiutil create -format UDZO (read-only install image)"
if ! hdiutil create \
    -volname "Wavelink $VERSION" \
    -srcfolder "$DMG_STAGE" \
    -ov \
    -format UDZO \
    "$DMG" 2>&1; then
    warn "hdiutil create failed or timed out."
    warn "The disk-image stack on this machine may be wedged (2x diskimagesiod / phantom mounts)."
    warn "Fix: run 'sudo killall -9 diskimagesiod diskmanagementd' (daemons respawn) or reboot, then re-run."
    rm -rf "$DMG_STAGE"
    die "dmg not produced: $DMG"
fi
[ -f "$DMG" ] || die "dmg not produced: $DMG"
rm -rf "$DMG_STAGE"

log "macOS packaging complete:"
ls -lh "$DMG"
log "app: $APP"
