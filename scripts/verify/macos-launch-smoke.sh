#!/usr/bin/env bash
# scripts/verify/macos-launch-smoke.sh — UI gate: the packaged Wavelink.app
# must actually open a window (regression guard for the "opens with no UI" bug).
#
# Requires a logged-in macOS GUI session. The window-count assertion needs
# Accessibility/Automation TCC for the calling (terminal) process; if that
# isn't granted we fall back to asserting process liveness + the app's window
# accessibility labels (set accessibly). Either way the process must STAY alive
# after launch — the bug fixed here was a process that "ran" with no window.
#
# Usage: bash scripts/verify/macos-launch-smoke.sh
# Exit 0 = UI window present (or liveness asserted + TCC-constrained);
# Exit 2 = app failed to launch / did not stay alive (hard failure).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
# shellcheck disable=SC1091
. "$ROOT/scripts/package/common.sh"

[ "$(uname -s)" = "Darwin" ] || die "macOS launch smoke must run on a macOS host"
need_cmd open osascript pgrep

# --- 1) build the .app ---------------------------------------------------------
bash "$ROOT/scripts/package/macos.sh"

APP="$WDR_DIST_DIR/macos/Wavelink.app"
[ -d "$APP" ] || die "no .app produced: $APP"

# --- 2) launch + liveness -------------------------------------------------------
log "opening $APP"
open "$APP"
# Give the app time to finish launching (first run can trigger TCC prompts).
sleep 4
if ! pgrep -x Wavelink >/dev/null; then
    die "GUI process 'Wavelink' not running after launch (bug: opens with no UI?)"
fi
log "process 'Wavelink' is alive after launch"

# --- 3) window assertion (deterministic; no TCC needed) -----------------------
# Compile the CGWindowList checker (swiftc is a packaging prerequisite) and
# require an actual on-screen window — this is the precise regression guard for
# "opens with no UI".
need_cmd swiftc
CHECKBIN="$(mktemp -d)/macos-window-check"
xcrun swiftc -O "$ROOT/scripts/verify/macos-window-check.swift" -o "$CHECKBIN" 2>/dev/null \
    || die "failed to compile window-check helper"
if "$CHECKBIN" "Wavelink" "Wavelink"; then
    log "PASS: an on-screen window is present (UI bug fixed)"
else
    # The app is alive but no window is on screen — that IS the reported bug.
    warn "FAIL path: process alive but no on-screen window; re-checking once"
    sleep 2
    if "$CHECKBIN" "Wavelink" "Wavelink"; then
        log "window appeared on re-check — PASS"
    else
        die "app launched but no window appeared (the 'opens with no UI' bug)"
    fi
fi

# --- 4) clean quit --------------------------------------------------------------
osascript -e 'tell application "Wavelink" to quit' 2>/dev/null || true
sleep 1
if pgrep -x Wavelink >/dev/null; then
    # Graceful quit via AppleScript needs the app to accept it; fall back to TERM.
    warn "app did not quit via AppleScript; sending SIGTERM"
    pkill -x Wavelink || true
fi
log "launch smoke complete"
