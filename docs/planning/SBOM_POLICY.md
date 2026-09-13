# SBOM Policy

Owner: OSS/Licensing (task `t-B0-ossdep`, root `R-B0-OSSDEP`). Policy effective for all releases from B0 onward; CI automation lands with the B0 lockfile (see `.github/workflows/license-audit.yml`).

## What an SBOM must include

Every release artifact's SBOM **must** enumerate:

1. **All crates** resolved by `Cargo.lock` (direct + transitive), each with: name, version, `purl`, license (SPDX), and the `source`/link field (required for MPL-2.0 source availability of `uniffi`).
2. **Vendored C libraries** statically linked into the binary: `libopus` (BSD-3-Clause), `libFLAC` (BSD-3-Clause Xiph variant). Enumerate their source objects and license origins (per LICENSE-NOTICES.md placeholders).
3. **Platform SDK bits** used by the builds: NDK + Android Sysroot components, Xcode SDK, Windows SDK / MSVC runtime, base image / glibc / PipeWire runtime for Linux packaging (recorded as environment/component entries, not "code"), `ring` asm objects (nasm assemblies) if ring is in the lockfile — all enumerated as SBOM components.
4. Build-tool components (cargo, zigbuild, cross, cbindgen, uniffi generator) recorded in a separate **tooling** SBOM section — not shipped in binaries but part of the build provenance.

Nothing on the distribution path may be un-enumerated. `cargo-deny`'s license scan on the pruned runtime graph covers (1); the Rust-FFI cross-build audits cover (2); per-platform packaging steps assert (3).

## Tooling

| Tool | Use | When |
|---|---|---|
| `cargo-cyclonedx` (`cargo cyclonedx`) | SPDX/CycloneDX SBOM from `Cargo.lock` (crates + purls) | CI release path + on demand |
| `syft` | Container/package-level SBOM generation (Linux base image, deb/rpm/AppImage/Flatpak contents) — complements cargo-cyclonedx at the image/packaging level | release path + on demand |
| `cargo-deny` | License + advisory policy gate (`deny.toml`) | every CI run incl. PRs once enabled |
| `cargo audit` | RustSec advisory scan | every CI run incl. PRs once enabled |

## When an SBOM is produced

- **CI on the release path**: every release/tag build produces `sbom.<fmt>` for each distributable artifact (attach to the release as an artifact; embed where the format allows — CPack/installer metadata, appx/MSIX manifest appendix, Debian copyright file + CycloneDX component, etc.).
- **On demand**: any engineer may generate the current-workspace SBOM via `just sbom` (runs cargo-cyclonedx + syft on the dev container / base image) for review or audits.
- **On every dependency bump**: the lockfile changes → SBOM regenerated and re-run through the license gate before merge (CI enforces once enabled).

## Where SBOMs are stored

- Release artifacts: alongside the installer/signing artifacts in the release (GitHub Releases / store submission bundles), named `wdr-<platform>-<version>-sbom.cyclonedx.json[.spdx.json]` plus `.txt` for the human NOTICE.
- In-repo: generated outputs are committed under `docs/orchestration/sbom/<rev>/` when a release revision is frozen (NOT in `target/`).

## License scan gate (`cargo-deny` / `deny.toml` policy)

`deny.toml` enforces the gate (see SBOM + CI; full file lands with B0 lockfile by devex, policy outlines here):

- **Forbidden** on runtime dependencies: GPL-2.0, GPL-3.0, AGPL-3.0, SSPL, "license missing/unknown" for runtime deps, and any source-available/noncommercial license — no exceptions (DEPENDENCY_EVALUATION.md §Policy).
- **Allowed with recorded decision**: **MPL-2.0** — only for `uniffi` / `uniffi_core` (`allow` entry with `note = "uniffi MPL-2.0 recorded decision, see DEPENDENCY_EVALUATION.md + LICENSE-NOTICES.md"`). Build-time-only MPL tools (`cbindgen`) are allowed in the tooling SBOM, `skip`'d from runtime graph.
- **Allowed outright**: BSD-2-Clause, BSD-3-Clause, MIT, Apache-2.0, ISC, Zlib, and dual combinations thereof (all our permissive deps fall here).
- LGPL: allowed **only** as dynamically-linked OS library on Linux (libpipewire LGPL-2.1, gtk4 runtime) — never vendored/static; explicitly noted in the Linux shell packaging docs. libFLAC must use the BSD-3 variant only (never LGPL option).
- Missing license on a **dev-only** dep: allowed (dev graph) but must be documented, not silent.
- Secrets guardrails (RI_S) are out of scope of this policy but enforced separately in CI secret scans (THREAT_MODEL #7).

Gate enforcement in CI: `.github/workflows/license-audit.yml` runs `cargo deny check licenses` + `cargo audit` + SBOM generation; it is **currently DISABLED (`if: false`)** pending the B0 lockfile, with a visible TODO enable marker.

## Related docs

- DEPENDENCY_EVALUATION.md — rubric + pinned versions + update policy.
- LICENSE-NOTICES.md — attribution notice template + origin file placeholders.
- THREAT_MODEL.md #8 (supply chain) — pin toolchains/lockfiles, `cargo audit`, `cargo-deny`, SBOM, maintenance rubric.
- RELEASE_AND_SIGNING.md — where SBOM/notices attach to release artifacts.

| Date | Action |
|---|---|
| 2026-09-06 | Policy authored (t-B0-ossdep) |
