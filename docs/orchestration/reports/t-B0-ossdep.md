# Report: t-B0-ossdep (root R-B0-OSSDEP) — Dependency pins, SBOM/lint skeleton

Owner role: OSS/Licensing. Mode: docs-only (no product code). Date: 2026-09-06.

## Deliverables
- `docs/planning/DEPENDENCY_EVALUATION.md` — finalized at B0 with full 17-field rows for the locked list + maintenance rubric + update policy + explicit rejected section.
- `docs/planning/LICENSE-NOTICES.md` — created: attribution template (BSD/MIT/Apache/ISC/MPL) + origin-file placeholders marked generated-at-build-time; uniffi MPL-2.0 recorded decision.
- `docs/planning/SBOM_POLICY.md` — created: what/which tooling/when/where + `deny.toml` license-gate policy (forbid GPL/AGPL/SSPL/missing; allow MPL-2.0 for uniffi; allow BSD/MIT/Apache/ISC/Zlib; LGPL only dynamic-Linux).
- `.github/workflows/license-audit.yml` — created: `cargo install cargo-deny` + `cargo deny check licenses` + `cargo-audit` + SBOM, ubuntu-latest; **disabled** (`if: false`) with explicit enable markers + TODO.
- `Cargo.lock` (root) — placeholder created because a parallel worker (devex) landed a minimal root `Cargo.toml` (`crates/wdr_dev`, no deps, no lockfile). Recorded: real pins come with the B0 lockfile; cargo regenerates this file.

## Decisions made
1. Postcard over bincode (bincode archived) — carried from P0, now in locked table.
2. uniffi MPL-2.0 = single recorded weak-copyleft exception (control-plane only); deny.toml will encode it.
3. libFLAC uses BSD-3 variant only; libpipewire/gtk4 LGPL allowed only as dynamic Linux runtime libs.
4. License-audit CI ships `if: false` until lockfile + deny.toml land (no repo failure now).
5. Cargo.lock is committed per existing .gitignore policy; hand-authored placeholder is minimal and valid.
6. Cargo tool installs happen per-run in CI (matches lockfile/sys crates) rather than a prebuilt action.
7. ring/ISC + zlib notice blocks added as placeholders (land only if ring enters tree via rustls-ring).

## Validation
- Cross-checked every dep row against DECISION_LOG/PHASE_GATES, ADR-001..010, PLATFORM_MATRIX, RISK_REGISTER, THREAT_MODEL (audited reads).
- Grep-verified: 17 required fields per row; rejected list contains all requested items.
- Cargo.lock placeholder parse-checked by tooling invariants (version-4 format, one [[package]]).
- Wrote files only under allowed_paths.

## Follow-ups
- B0 lockfile (devex/core) to backfill real pins, then generate real `Cargo.lock` (overwrites placeholder).
- `deny.toml` authored by whichever worker owns the lockfile; enable license-audit per its TODO(enable-after-b0-lockfile).
- PipeWire 1.6.x pin (devex), snow↔dalek coexistence decision, SBOM `docs/orchestration/sbom/` dir plumbing.
- Rubric re-run + DECISION_LOG entries on every bump per §Maintenance rubric.
