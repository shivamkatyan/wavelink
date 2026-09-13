#!/usr/bin/env bash
# scripts/package/common.sh — shared bootstrap for the WDR packaging scripts.
#
# bash 3.2 compatible (macOS /usr/bin/bash): no arrays, no ${var,,}, etc.
# Source me at the top of a packaging script (after its shebang + doc block):
#
#     ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
#     # shellcheck disable=SC1091
#     . "$ROOT/scripts/package/common.sh"
#
# Provides (each script MUST be runnable, idempotent, and NO sudo):
#   ROOT                 — repo root
#   WDR_DIST_DIR         — artifact root (default "$ROOT/dist")
#   WDR_NO_INTERACTIVE   — always 1 (packaging never prompts)
#   rev()                — short git rev (or "unknown")
#   log() / warn()/die() — structured logging
#   need_cmd()           — fail fast when a tool is missing
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
export WDR_NO_INTERACTIVE=1
export WDR_DIST_DIR="${WDR_DIST_DIR:-$ROOT/dist}"

# --- logging -----------------------------------------------------------------
log() { printf '[package] %s\n' "$*"; }
warn() { printf '[package][warn] %s\n' "$*" >&2; }
# Hard failure: exit 2 (distinct from generic command failures; documents the
# "would need a <platform> runner" gate case that the justfile/CI relies on).
die() { printf '[package][error] %s\n' "$*" >&2; exit 2; }

# --- helpers -----------------------------------------------------------------
# Short git revision for artifact names (never blocks on a slow/broken repo).
# WDR_RUN_REV lets a parent host pass the resolved rev into a container where
# git may be missing (scripts/package/linux.sh docker path).
rev() {
    if [ -n "${WDR_RUN_REV:-}" ]; then
        printf '%s' "$WDR_RUN_REV"
        return 0
    fi
    local r
    r="$(git -C "$ROOT" rev-parse --short HEAD 2>/dev/null || true)"
    [ -n "${r:-}" ] && printf '%s' "$r" || printf 'unknown'
}

need_cmd() {
    for c in "$@"; do
        command -v "$c" >/dev/null 2>&1 || die "required command not found: $c"
    done
}

# Copy sources/LICENSE notices that ship inside every platform artifact.
# This is the same source every script bundles (docs/planning/LICENSE-NOTICES.md),
# so a shipped tarball/dmg always contains the permissive-license attribution.
emit_license_notice() {
    local dest_dir="$1"
    local src="$ROOT/docs/planning/LICENSE-NOTICES.md"
    if [ -f "$src" ]; then
        cp "$src" "$dest_dir/LICENSE-NOTICES.txt"
    else
        warn "LICENSE-NOTICES.md missing at $src; writing a pointer instead"
        printf 'WIRELESS DAC RELAY — license & attribution\n\nThird-party license notices are tracked in the repo at\n%s\n. Regenerate from `just license` / `just sbom` on a release runner.\n' \
            'docs/planning/LICENSE-NOTICES.md' > "$dest_dir/LICENSE-NOTICES.txt"
    fi
}
