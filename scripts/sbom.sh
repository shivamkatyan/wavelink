#!/usr/bin/env bash
# scripts/sbom.sh — SBOM + license inventory for the WDR workspace.
#
# The FULL SBOM (cargo-cyclonedx / syft, per SBOM_POLICY.md) is a CI-runner
# function: this script invokes the real generators when present and clearly
# reports SKIPPED (NOT green) when they are absent. It never fakes a scan.
#
# On the dev host (no generators), it still emits a lightweight package list
# from Cargo.lock via python3 (fast, offline, no cargo network).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${WDR_SBOM_OUT:-$ROOT/docs/orchestration/sbom}"
mkdir -p "$OUT"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"

# 1) Lightweight inventory straight from Cargo.lock (fast, offline).
LOCK="$ROOT/Cargo.lock"
if [ -f "$LOCK" ]; then
  python3 - "$LOCK" "$OUT/pkg-lock-$STAMP.txt" <<'PY'
import re, sys
lock, out = sys.argv[1], sys.argv[2]
name = None
pkgs = []
for line in open(lock):
    m = re.match(r'name = "([^"]+)"', line)
    if m:
        name = m.group(1)
        continue
    m = re.match(r'version = "([^"]+)"', line)
    if m and name:
        pkgs.append((name, m.group(1)))
        name = None
pkgs.sort()
with open(out, "w") as f:
    for n, v in pkgs:
        f.write(f"{n} v{v}\n")
print(f"[sbom] {len(pkgs)} packages from Cargo.lock")
PY
  echo "[sbom] lock inventory -> $OUT/pkg-lock-$STAMP.txt"
else
  echo "[sbom] no Cargo.lock; inventory skipped"
fi

# 2) Real generators (runner-only; honest skip when absent).
if command -v cargo-cyclonedx >/dev/null 2>&1; then
  (cd "$ROOT" && cargo cyclonedx --output-format json --output "$OUT/wdr-$STAMP.cdx.json") \
    && echo "[sbom] -> $OUT/wdr-$STAMP.cdx.json" || echo "[sbom][warn] cargo cyclonedx failed"
else
  echo "[sbom] cargo-cyclonedx NOT INSTALLED -> SKIPPED (run on CI runner)"
fi
if command -v syft >/dev/null 2>&1; then
  (cd "$ROOT" && syft dir:. -o spdx-json > "$OUT/wdr-$STAMP.syft.spdx.json") \
    && echo "[sbom] -> $OUT/wdr-$STAMP.syft.spdx.json" || echo "[sbom][warn] syft failed"
else
  echo "[sbom] syft NOT INSTALLED -> SKIPPED (run on CI runner)"
fi
ls -la "$OUT"
