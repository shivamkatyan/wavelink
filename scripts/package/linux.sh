#!/usr/bin/env bash
# scripts/package/linux.sh — build the Linux emitter + A2DP-sink receiver for
# distribution.
#
# Behaviour (bash 3.2 compatible):
#   * Linux host (or inside the wdr-dev dev container at /workspace):
#       - cargo build --release --bin linux-emitter AND --bin linux-receiver
#         (portable surfaces; the dedicated PipeWire `--features pipewire` and
#         BlueZ `bt` builds are follow-ups on PipeWire/bt-lab runners)
#       - tar czf $WDR_DIST_DIR/linux/linux-<emitter|receiver>-<rev>.tar.gz
#         (bin + README + LICENSE-NOTICES)
#       - if dpkg-deb is present: also emit a minimal .deb per component
#   * macOS host with Docker + the wdr-dev image: re-runs THIS script inside the
#     Linux container (mounting the repo at /workspace), producing real Linux
#     artifacts into the mounted dist/ tree.
#   * Anything else: prints a clear message and exits 2 (CI gate).
#
# Intended primary environment: the ubuntu CI runner. `just package linux` on a
# macOS host that lacks the wdr-dev image also exits 2 with a clear message.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
# shellcheck disable=SC1091
. "$ROOT/scripts/package/common.sh"

EMITTER_CRATE="$ROOT/platform/linux-emitter"
RECEIVER_CRATE="$ROOT/platform/linux-receiver"
OUT_DIR="$WDR_DIST_DIR/linux"
REV="$(rev)"
EMITTER_VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$EMITTER_CRATE/Cargo.toml" | head -n 1)"
[ -n "${EMITTER_VERSION:-}" ] || EMITTER_VERSION="0.1.0"
RECEIVER_VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$RECEIVER_CRATE/Cargo.toml" | head -n 1)"
[ -n "${RECEIVER_VERSION:-}" ] || RECEIVER_VERSION="0.1.0"

# Inside the wdr-build dev-container the repo is mounted at /workspace; treat
# it as a Linux native host. The BUILD container keeps the toolchain; the
# runtime wdr-dev image is lean (no cargo) by design.
is_container() { [ "$ROOT" = "/workspace" ]; }
BUILD_IMAGE="${WDR_BUILD_IMAGE:-wdr-build}"

if [ "$(uname -s)" != "Linux" ] && ! is_container; then
    if [ "$(uname -s)" = "Darwin" ] && command -v docker >/dev/null 2>&1 \
        && docker image inspect "$BUILD_IMAGE" >/dev/null 2>&1; then
        log "running on a macOS host — building via the $BUILD_IMAGE Linux container"
        log "docker run --rm -v $ROOT:/workspace $BUILD_IMAGE bash /workspace/scripts/package/linux.sh"
        docker run --rm -v "$ROOT":/workspace \
            -e WDR_RUN_REV="$(rev)" \
            "$BUILD_IMAGE" bash /workspace/scripts/package/linux.sh
        exit 0
    fi
    die "Linux packaging needs the $BUILD_IMAGE container (docker build -f Dockerfile.dev --target builder -t wdr-build .) or a Linux runner; got '$(uname -s)'. CI-gated."
fi

log "Linux host detected ($(uname -s), ROOT=$ROOT)"

# --- 1) build -----------------------------------------------------------------
need_cmd cargo tar
log "cargo build --release --bin linux-emitter"
(
    cd "$EMITTER_CRATE"
    cargo build --release --bin linux_emitter
)
EMITTER_BIN="$EMITTER_CRATE/target/release/linux_emitter"
[ -x "$EMITTER_BIN" ] || die "emitter release binary not produced: $EMITTER_BIN"

log "cargo build --release --bin linux-receiver"
(
    cd "$RECEIVER_CRATE"
    cargo build --release --bin linux_receiver
)
RECEIVER_BIN="$RECEIVER_CRATE/target/release/linux_receiver"
[ -x "$RECEIVER_BIN" ] || die "receiver release binary not produced: $RECEIVER_BIN"

# --- 2) tarballs --------------------------------------------------------------
mkdir -p "$OUT_DIR"
stage_tarball() {
    # $1 = crate name, $2 = bin path, $3 = bin display name, $4 = version,
    # $5 = crate README.md path
    local name="$1" bin="$2" binname="$3" version="$4" readme="$5"
    local stage="$OUT_DIR/stage-${name}-$$"
    rm -rf "$stage"
    mkdir -p "$stage"
    cp -f "$bin" "$stage/$binname"
    emit_license_notice "$stage"
    [ -f "$EMITTER_CRATE/assets/wavelink.png" ] && cp -f "$EMITTER_CRATE/assets/wavelink.png" "$stage/wavelink.png"
    if [ -f "$readme" ]; then
        cp -f "$readme" "$stage/README.md"
    else
        warn "crate README.md missing for $name; skipping"
    fi
    printf 'Wavelink — Linux %s %s (%s)\nBuilt via scripts/package/linux.sh\n' \
        "$name" "$version" "$REV" > "$stage/VERSION"
    local tarball="$OUT_DIR/${name}-${REV}.tar.gz"
    tar -C "$stage" -czf "$tarball" .
    rm -rf "$stage"
    log "tarball -> $tarball"
}
stage_tarball "wavelink"  "$EMITTER_BIN"  "wavelink"  "$EMITTER_VERSION"  "$EMITTER_CRATE/README.md"
stage_tarball "wavelink-receiver" "$RECEIVER_BIN" "wavelink-receiver" "$RECEIVER_VERSION" "$RECEIVER_CRATE/README.md"

# --- 3) optional minimal .debs --------------------------------------------------
if command -v dpkg-deb >/dev/null 2>&1; then
    build_deb() {
        # $1 = package name, $2 = bin path, $3 = bin display name, $4 = version,
        # $5 = crate README.md path, $6 = desktop Name, $7 = desktop Exec,
        # $8 = desktop Comment
        local pkg="$1" bin="$2" binname="$3" version="$4" readme="$5"
        local dname="$6" dexe="$7" dcomment="$8"
        local deb_dir="$OUT_DIR/deb-${pkg}-$$"
        local root="$deb_dir/root"
        mkdir -p "$root/usr/bin"
        cp -f "$bin" "$root/usr/bin/$binname"
        mkdir -p "$root/usr/share/doc/$pkg"
        emit_license_notice "$root/usr/share/doc/$pkg"
        mkdir -p "$root/usr/share/icons/hicolor/512x512/apps"
        [ -f "$EMITTER_CRATE/assets/wavelink.png" ] && \
            cp -f "$EMITTER_CRATE/assets/wavelink.png" \
            "$root/usr/share/icons/hicolor/512x512/apps/wavelink.png" || true
        [ -f "$readme" ] && cp -f "$readme" "$root/usr/share/doc/$pkg/README.md" || true

        # A .desktop entry so installing the .deb actually gives "Open" in the
        # app menu / dash. Terminal=true keeps the console visible and readable
        # (the CLI + menu mode run from terminal; without this the app seems to
        # "do nothing" after install — the same failure as the old macOS .app).
        mkdir -p "$root/usr/share/applications"
        cat > "$root/usr/share/applications/wdr-${pkg}.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Version=1.0
Name=$dname
GenericName=Wi-Fi audio $pkg
Comment=$dcomment
Exec=$dexe
Icon=wavelink
Terminal=true
Categories=AudioVideo;Audio;
Keywords=audio;wifi;dac;wireless;
DESKTOP

        local deb_name="${pkg}_${version}-${REV}_amd64.deb"
        # dpkg-deb treats <root>/DEBIAN/control as metadata and everything else
        # under <root> as the file payload — so the control dir must live INSIDE
        # $root (building from "$deb_dir" instead would pack payloads under a
        # literal "./root/" prefix and install everything into /root/...).
        mkdir -p "$root/DEBIAN"
        {
            echo "Package: $pkg"
            echo "Version: ${version}-${REV}"
            echo "Section: sound"
            echo "Priority: optional"
            echo "Architecture: amd64"
            echo "Maintainer: Wavelink <dev@wdr.invalid>"
            echo "Description: Wavelink Linux $pkg (portable surface)"
        } > "$root/DEBIAN/control"
        dpkg-deb --build "$root" "$OUT_DIR/$deb_name" >/dev/null
        rm -rf "$deb_dir"
        log "deb -> $OUT_DIR/$deb_name"
    }
    build_deb "wavelink"  "$EMITTER_BIN"  "wavelink"  "$EMITTER_VERSION"  "$EMITTER_CRATE/README.md" \
        "Wavelink Emitter" "linux-emitter" \
        "Emits Linux system/application audio to a Wavelink receiver"
    build_deb "wavelink-receiver" "$RECEIVER_BIN" "wavelink-receiver" "$RECEIVER_VERSION" "$RECEIVER_CRATE/README.md" \
        "Wavelink Receiver" "linux-receiver" \
        "Linux A2DP-sink receiver (headless; registers as a BlueZ speaker, renders to its output)"
else
    log "dpkg-deb not present — .debs skipped (native Linux runner will include them)"
fi

log "linux packaging complete:"
ls -lh "$OUT_DIR"
