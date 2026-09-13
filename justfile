set shell := ["bash", "-uc"]

project_name := "wdr"

default:
    @just --list

# Format check (no writes).
format:
    cargo fmt --all -- --check

# Lint with clippy, warnings as errors.
lint:
    cargo clippy --all-targets --all-features -- -D warnings

# Unit tests for the DevEx placeholder crate.
unit:
    cargo test -p wdr_dev

# Build the whole workspace.
build:
    cargo build --workspace

# Fast DevEx self-test: unit tests + clippy.
selftest: unit lint

# Benchmark placeholder (B0: stub only; real benches arrive with P1 spikes).
bench:
    @echo "[bench] stub — real benchmark harness arrives with P1 spike reports."

# Build distributable platform packages via scripts/package/all.sh (native: android + macos on this host; linux/windows/ios are explicit, CI-gated targets). WDR_DIST_DIR honored.
package *target:
    @bash scripts/package/all.sh {{target}}

# Clean-machine validation placeholder (B0: stub; real flow via dev/validate-clean in clean container/VM).
clean-verify:
    @echo "[clean-verify] stub — will build+test+package from a fresh clone in a clean container/VM."

# Static docs site — builds site_build/ (deploy via .github/workflows/pages.yml).
site:
    @bash scripts/site/build.sh

# SBOM + license inventory (implementable now; full cargo-cyclonedx/syft runs on CI runner).
sbom:
    @bash scripts/sbom.sh

# Vulnerability gate: RustSec advisory-db scan of Cargo.lock (0 findings on
# 2026-09-10, 273 crates).
audit:
    cargo audit

# License gate: deny.toml allow-list vs every crate license (permissive-only policy).
license:
    cargo deny check licenses
