#!/usr/bin/env bash
# scripts/package/win.sh — build + zip the Windows emitter for distribution.
#
# Intended primary environment: the windows-2025 CI runner. Produces
# $WDR_DIST_DIR/windows/win-emitter-<rev>.zip containing win-emitter.exe +
# LICENSE-NOTICES + README. Zip via `powershell Compress-Archive` with a
# `tar -a -cf` fallback (both exist on windows-2025).
#
# bash 3.2 compatible. On a non-Windows host (e.g. this macOS dev box) this
# script NEVER builds to completion: it prints a clear "Windows packaging
# requires the Windows runner" message and exits 2. (`cargo check --target
# x86_64-pc-windows-msvc` DOES work on macOS — a compile gate; producing the
# real EXE needs link.exe + the Windows runner.)
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
# shellcheck disable=SC1091
. "$ROOT/scripts/package/common.sh"

case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*) : ;; # real Windows (git-bash style shells)
    *)
        die "Windows packaging requires the Windows runner (got '$(uname -s)'). CI-gated: run the release.yml 'windows' job, or locally use cargo check --target x86_64-pc-windows-msvc as the compile gate."
        ;;
esac

need_cmd cargo

CRATE_DIR="$ROOT/platform/win-emitter"
OUT_DIR="$WDR_DIST_DIR/windows"
REV="$(rev)"

log "cargo build --release --target x86_64-pc-windows-msvc (win-emitter)"
(
    cd "$CRATE_DIR"
    rustup target add x86_64-pc-windows-msvc
    cargo build --release --target x86_64-pc-windows-msvc --bin win_emitter
)
BIN="$CRATE_DIR/target/x86_64-pc-windows-msvc/release/win_emitter.exe"
[ -f "$BIN" ] || die "release exe not produced: $BIN"

mkdir -p "$OUT_DIR"
STAGE="$OUT_DIR/stage-$$"
rm -rf "$STAGE"
mkdir -p "$STAGE"
cp -f "$BIN" "$STAGE/wavelink.exe"
emit_license_notice "$STAGE"
if [ -f "$CRATE_DIR/README.md" ]; then
    cp -f "$CRATE_DIR/README.md" "$STAGE/README.md"
else
    warn "crate README.md missing; skipping (package still valid)"
fi

ZIP="$OUT_DIR/wavelink-$REV.zip"
rm -f "$ZIP"
if command -v powershell >/dev/null 2>&1; then
    WDR_STAGE="$STAGE" WDR_ZIP="$ZIP" \
        powershell -NoProfile -Command \
        'Compress-Archive -Path (Join-Path $env:WDR_STAGE "*") -DestinationPath $env:WDR_ZIP -Force' \
        || die "powershell Compress-Archive failed"
else
    # tar -a infers zip from the extension (windows-2025 ships bsdtar).
    tar -a -C "$STAGE" -cf "$ZIP" .
fi
rm -rf "$STAGE"
[ -f "$ZIP" ] || die "zip not produced"

log "Windows packaging complete:"
ls -lh "$ZIP"
