#!/usr/bin/env bash
#
# Build the Wavelink static site into site_build/ (default) or
# $WDR_SITE_OUT. Requires python3 + the `markdown` package (pure-Python, MIT);
# if missing, installs it for the current user (no sudo).
#
# Usable identically on a developer macOS host and on the GitHub Actions
# ubuntu runner (scripts/site/build.py is CWD-independent).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

python3 - <<'PY' || python3 -m pip install --user --quiet markdown
import markdown  # noqa: F401
PY

cd "$ROOT"
python3 scripts/site/build.py

echo "[site] tip: WDR_SITE_OUT=/tmp/wdr-site just site   (override output dir)"
