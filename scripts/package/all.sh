#!/usr/bin/env bash
# scripts/package/all.sh — `just package` driver.
#
# Runs the packaging scripts for the requested targets (bash 3.2 compatible —
# no array/associative-array features). The scripts are always invoked via
# `bash scripts/package/<t>.sh` so zsh-less /usr/bin/bash correctness holds.
#   no targets   -> android + macos (everything native to this macOS host)
#   just package linux|windows|ios -> additionally those explicit targets
#
# `just package` must NOT run the linux/windows scripts to completion on macOS
# implicitly: only android + macos are auto-run; linux/windows/ios are opt-in
# and their scripts decide whether the host can produce real artifacts (or
# exit 2 with a clear "requires <platform> runner" message).
#
# WDR_DIST_DIR is honored (passed through from the calling environment).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
# shellcheck disable=SC1091
. "$ROOT/scripts/package/common.sh"

log "dist dir: $WDR_DIST_DIR  (override with WDR_DIST_DIR=...)"

if [ "$#" -eq 0 ]; then
    log "==> android packaging (scripts/package/android.sh)"
    bash "$ROOT/scripts/package/android.sh"
    log "==> macos packaging (scripts/package/macos.sh)"
    bash "$ROOT/scripts/package/macos.sh"
else
    for t in "$@"; do
        case "$t" in
            android | macos | linux | ios)
                log "==> $t packaging (scripts/package/$t.sh)"
                bash "$ROOT/scripts/package/$t.sh"
                ;;
            windows)
                # Target name is `windows`; the backing script is win.sh.
                log "==> windows packaging (scripts/package/win.sh)"
                bash "$ROOT/scripts/package/win.sh"
                ;;
            *)
                die "unknown package target '$t' (want: android | macos | linux | windows | ios)"
                ;;
        esac
    done
fi

log "done. artifacts under $WDR_DIST_DIR"
