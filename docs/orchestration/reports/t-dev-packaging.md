# t-dev-packaging — Packaging & Release pipeline

**task_id:** `t-dev-packaging` · **root_task_id:** `dev-packaging` · **hypothesis_id:** `H-DEV-PKG-1`
**owner_role:** DevEx Implementer · **status:** complete (android + macos + ios + linux-container pack runs pass on this macOS host; windows/real-Release/real-.deb are CI-gated)
**date:** 2026-09-10

## Summary

Delivered the WDR **packaging + release** pipeline: per-emit-crate CLI bins,
`scripts/package/*.sh` (bash-3.2-safe, idempotent, no sudo), the `just package`
driver, a `.github/workflows/release.yml` that builds and publishes all five
platform artifacts to a GitHub Release, `dist/` gitignoring, and the honest gate
docs.

Real, reproducible runs **on this macOS host**: `just package` (android + macos)
completes green; `just package linux` produces a genuine Linux tarball + `.deb`
through the `wdr-dev` container; `just package ios` produces the unsigned
simulator-SDK core libraries; the new `win_emitter` bin (and the crate it links)
passes `cargo check --target x86_64-pc-windows-msvc`; `win.sh` correctly exits 2
on macOS with a clear "requires the Windows runner" message.

**One required out-of-scope repair (disclosed):** `platform/win-emitter/src/wasapi.rs`
did not compile under the crate's locked `windows` 0.62.2 dependency (21 errors —
pre-existing; the shell was validated at rev-9 against an older windows-rs shape
and the lockfile was since regenerated). The supervisor's stated acceptance gate
(`cargo check --target x86_64-pc-windows-msvc` clean) is impossible while the
`#[cfg(windows)]` backend breaks the whole package, so `wasapi.rs` was minimally
repaired to the 0.62.2 API surface (behavior unchanged; documented in the file
header). This is the only write outside the allowed paths, and it is recorded
here + as a follow-up for the owning shell task.

## Files changed

| Path | Change |
|---|---|
| `platform/macos-emitter/src/bin/macos_emitter.rs` | NEW CLI bin (`--list-format` / `--permission-state` / `--version`), ~70 lines incl. docs, crate public API only |
| `platform/win-emitter/src/bin/win_emitter.rs` | NEW CLI bin (portable surface only; `cargo check --target x86_64-pc-windows-msvc` clean) |
| `platform/linux-emitter/src/bin/linux_emitter.rs` | NEW CLI bin (portable surface only; builds on any host, real binary on Linux) |
| `platform/win-emitter/src/wasapi.rs` | REPAIRED to compile under locked `windows` 0.62.2 (API-shape only; behavior unchanged) — out-of-scope exception, disclosed above |
| `scripts/package/common.sh` | Shared env/bootstrap: `ROOT`, `WDR_DIST_DIR` (default `<repo>/dist`), `WDR_NO_INTERACTIVE=1`, `log/warn/die`, `rev()` (+ `WDR_RUN_REV` for containers), license-notice emitter |
| `scripts/package/all.sh` | `just package` driver: default = android + macos; explicit `linux`/`windows`/`ios`; maps target `windows` → `win.sh` |
| `scripts/package/android.sh` | Per `platform/android-*` project: dev keystore created once; `./gradlew :app:assembleDebug :app:assembleRelease` (JDK17+ANDROID_HOME); `zipalign` + `apksigner sign --ks`; copies `-debug.apk` / `-release-signed.apk`; verifies signature+alignment |
| `scripts/package/macos.sh` | `cargo build --release`; `.app` assembly (Info.plist, MacOS/, Resources/, LICENSE-NOTICES/README); `install_name_tool -add_rpath /usr/lib/swift`; ad-hoc `codesign`; read-only `hdiutil -format UDZO` DMG |
| `scripts/package/linux.sh` | native Linux/container build → `linux-emitter-<rev>.tar.gz` (+ `.deb`); macOS→`wdr-dev` Docker path; else clear exit 2 |
| `scripts/package/win.sh` | Windows runner only: `cargo build --release --target x86_64-pc-windows-msvc`, `powershell Compress-Archive` (fallback `tar -a`), zip of EXE + NOTICES + README; on macOS prints "requires the Windows runner" + exits 2 |
| `scripts/package/ios.sh` | Simulator-SDK `swiftc -emit-library` for both iOS cores → `dist/ios/` + README + RELEASE_AND_SIGNING.md + `wdr-ios-unsigned-<rev>.tar.gz` |
| `justfile` | `package *target` recipe replaces the placeholder; routes to `scripts/package/all.sh`; `WDR_DIST_DIR` passthrough |
| `.gitignore` | `+ dist/`, `+ *.dmg` (keystore/jks entries already present) |
| `.github/workflows/release.yml` | NEW: push `v*` + `workflow_dispatch`; jobs android/macos/linux/windows/ios (thin wrappers, official actions only, upload-artifact) + `publish` (`gh release create`, skip-if-exists guard, `contents: write`) |
| `docs/orchestration/PACKAGING.md` | Usage, per-script description, artifact map, workflow walkthrough, honest gate matrix |
| `docs/orchestration/reports/t-dev-packaging.md` | This report |

Not modified (verified untouched): `crates/**`, other `docs/**`, `docker/**`,
`compose.yml`, `Dockerfile.dev`, `.github/workflows/{ci,license-audit,reference-sim}.yml`,
android Gradle sources, `dev/**`, `platform/{ios-receiver,ios-emitter,linux-receiver,android-*}/**`.

## Decisions

1. **Bins are `src/bin/<crate>.rs` in each emit crate** (own `[workspace]`), so
   `cargo build --release --bin …` from the crate dir — no manifest changes, no
   new deps, no root-workspace coupling.
2. **macos bin gets its own `format_meta_of_source()`** so `--list-format` works
   via the deterministic `FakeCaptureSource`; `--permission-state` uses the real
   `PermissionGate<ScShareableContentProbe>` (headless-safe: returns a probe
   outcome, never triggers a prompt by itself).
3. **Dev keystore is project-keyed and never clobbered** (`<project>.keystore` +
   `<project>.keystore.pass` under `dist/`); password generated on first run.
4. **DMG is `-format UDZO`** (read-only) — no GUI session needed; plain
   `hdiutil`, no `create-dmg`.
5. **`install_name_tool -add_rpath /usr/lib/swift`** on the macOS binary before
   codesign: the standard Xcode-emitted Swift-runtime rpath; without it the
   screencapturekit-linked binary cannot resolve `@rpath/libswift_Concurrency.dylib`.
   On this dev image the system copy of that dylib is missing (documented host
   quirk — running the CLI here needs `DYLD_LIBRARY_PATH=<Xcode>/usr/lib/swift-5.5/macosx`);
   packaged `.app`s run normally on real Macs.
6. **win.sh exits 2 on non-Windows; `all.sh` maps `windows`→`win.sh`** — `just
   package` never implicitly runs windows/linux to completion on macOS.
7. **linux.sh takes the container path on macOS** (`docker run --rm -v $PWD:/workspace
   wdr-dev …`) and passes the host-resolved rev via `WDR_RUN_REV`, because git may
   be absent inside the image — this genuinely produces the Linux artifacts here
   while the ubuntu CI job remains its primary home.
8. **release.yml publish is skip-if-exists**: if a Release for the tag already
   exists it logs a warning and exits 0 (never clobbers); a branch-based
   `workflow_dispatch` skips publication entirely.
9. **`on:` is quoted (`"on"`)** in the workflow so strict YAML-1.1 linters don't
   parse the trigger key as a boolean.
10. **Every artifact ships LICENSE-NOTICES.txt** from
    `docs/planning/LICENSE-NOTICES.md`; signing stays ad-hoc/dev — production
    signing/notarization is a documented + never-faked credential gate.

## Commands run (this host — macOS, Xcode 26.6, Rust 1.98.1, arm64)

```bash
# Bins
cd platform/macos-emitter && cargo build --release --bin macos_emitter        # OK (19.7s cold, 0.16s incr)
cd platform/win-emitter    && cargo check --target x86_64-pc-windows-msvc --bin win_emitter   # OK after wasapi repair
cd platform/win-emitter    && cargo clippy --all-targets -- -D warnings        # OK (host)
cd platform/linux-emitter  && cargo check --bin linux_emitter                  # OK
cd platform/linux-emitter  && cargo clippy --all-targets -- -D warnings        # OK
cd platform/macos-emitter  && cargo clippy --all-targets --all-features -- -D warnings  # OK
cargo fmt --all -- --check   # all three crates OK (after cargo fmt --all)

# macOS CLI smoke (needs the dev-image Swift dylib: DYLD_LIBRARY_PATH=…/swift-5.5/macosx)
macos_emitter --version        -> macos-emitter 0.1.0
macos_emitter --list-format    -> rate=48000 bits=16 channels=2
macos_emitter --permission-state -> screen-recording-tcc=Authorized capture_allowed=true
macos_emitter (no args)        -> exit 2

# Packaging scripts
for f in scripts/package/*.sh; do bash -n "$f"; done                  # all OK
WDR_DIST_DIR=$PWD/dist bash scripts/package/macos.sh                   # .app + codesign OK + DMG created
WDR_DIST_DIR=$PWD/dist bash scripts/package/android.sh                 # BOTH projects: BUILD SUCCESSFUL, signed+verified APKs
WDR_DIST_DIR=$PWD/dist bash scripts/package/ios.sh                     # both cores + tarball OK
WDR_DIST_DIR=$PWD/dist bash scripts/package/linux.sh                   # docker path: tarball + .deb OK
WDR_DIST_DIR=$PWD/dist bash scripts/package/win.sh                     # exit 2 "requires the Windows runner"
WDR_DIST_DIR=$PWD/dist just package                                    # default android+macos, exit 0
WDR_DIST_DIR=$PWD/dist just package macos|linux|ios|windows            # 0 / 0 / 0 / 2 (expected)

# Workflow YAML lint
ruby -e 'require "yaml"; y=YAML.load_file(".github/workflows/release.yml"); …'  # parses; jobs = android, macos, linux, windows, ios, publish
```

## Validation results (this host) vs CI-gated

| Check | This host | CI-gated (not claimed here) |
|---|---|---|
| `cargo build --release` macos bin | ✅ | — |
| `cargo check --target x86_64-pc-windows-msvc` (win bin + crate) | ✅ exit 0 | real `win_emitter.exe` **link** (needs link.exe) |
| linux bin check/clippy/fmt | ✅ | real Linux binary produced via wdr-dev/ubuntu (✅ container here) |
| android assembleDebug+Release, zipalign, apksigner sign+verify | ✅ (emitter + receiver, signed cert verified, aligned) | store/dev signing creds |
| macos .app + ad-hoc codesign + DMG | ✅ (codesign verify + hdiutil imageinfo OK) | hardened runtime + notarization (creds) |
| ios simulator-SDK core libs | ✅ (both `.a` + tarball) | signed `.ipa` (Xcode project + account) |
| linux tarball + `.deb` | ✅ via wdr-dev container; `linux-emitter --version/--list-format` ran in-container | published artifact from ubuntu job |
| GitHub Release create | ❌ not runnable locally (needs tag + repo perms) | `publish` job |

## Acceptance criteria

| Criterion | Result | Evidence |
|---|---|---|
| 3 CLI bins, each ≤ ~70 lines, crate public API, no new deps | ✅ | the three `src/bin/*.rs`; clippy/fmt clean |
| macos bin `cargo build --release` on this host | ✅ | build + CLI smoke |
| win bin `cargo check --target x86_64-pc-windows-msvc` clean | ✅ | exit 0 (post wasapi repair) |
| `scripts/package/*.sh` bash-3.2 safe, `set -euo pipefail`, idempotent, no sudo | ✅ | bash -n all OK; re-runs green; no sudo anywhere |
| android.sh generates keystore once + builds both + signs with build-tools 34.0.0, MUST RUN here | ✅ | full run green; `apksigner verify` + `zipalign -c` pass |
| macos.sh .app + ad-hoc codesign + DMG, runs here without GUI | ✅ | codesign verify + DMG created (UDZO) |
| linux.sh native/container/exit-2 logic, Docker path REAL | ✅ | docker path produced + ran the real linux bin |
| win.sh macOS local run → clear message + exit 2 | ✅ | exit 2 observed |
| ios.sh runs here, produces unsigned cores + docs | ✅ | both libs + tarball + RELEASE_AND_SIGNING.md |
| `just package` driver (default android+macos; explicit targets; WDR_DIST_DIR) | ✅ | all four target styles exercised |
| release.yml: triggers, 5 build jobs + publish, skip-if-exists, official actions only | ✅ | YAML parses; jobs enumerated; `gh` only release tool |
| `.gitignore` gains `dist/`, `*.dmg` | ✅ | diff |
| `docs/orchestration/PACKAGING.md` + report | ✅ | written; gate matrix honest |

## Risks / limitations

- **wasapi.rs repair was out-of-scope but required** (disclosed): without it the
  win bin's declared compile gate fails. The repair is API-shape only; a
  follow-up should re-validate behavior on a real Windows endpoint and consider
  a `From<windows::core::Error>` in `lib.rs` (still absent, so wasapi maps errors
  explicitly via `wasapi_err`).
- **macOS CLI run needs the dev-image Swift dylib** (`DYLD_LIBRARY_PATH` to
  Xcode's `swift-5.5/macosx`) because this dev image lacks
  `/usr/lib/swift/libswift_Concurrency.dylib`; the shipped `.app` carries the
  standard `/usr/lib/swift` rpath so real Macs resolve it. Neither the packaging
  script nor the workflow requires launching the binary.
- **linux-emitter `--all-features` clippy fails on this host** (pipewire feature
  needs libspa/pipewire dev libs) — pre-existing, CI's macos-shell job already
  uses `|| true` for that crate; default (portable) clippy is clean.
- **`workflow_dispatch` publish is a no-op on branches** (by design) and the android
  CI job depends on the pinned Google `commandlinetools-linux-11076708` build
  (any other official build-tools zip requires updating that URL).
- Artifacts here are **dev/unsigned intermediates** by design; store signing,
  notarization, hardened runtime, and App Store/Play submission remain
  credential-gated and unfaked.

## Follow-up tasks

- Re-validate `win-emitter` WASAPI behavior on a real Windows audio runner; fold
  the wasapi repair back into the owning shell task and its build-check.
- Add `--features pipewire` native linux build/deb on a PipeWire runner (the .deb
  currently ships the portable surface only).
- Optionally emit the iOS cores for the `iphoneos` SDK and a generated Xcode
  project wrapper for the signed `.ipa` (credential gate).
- Re-check the pinned `commandlinetools-linux-*` SDK zip and dtolnay pins on
  every release cycle bump.
- Record the first tag-push Release run in RELEASE_STATUS.md once CI runners are
  enabled.

## Blockers

None for the deliverables runnable on this host. The explicitly CI-gated items
are the opposite of blockers — they are honest pending gates: real Windows EXE
link, the published-by-ubuntu `.deb`, and actual GitHub Release creation need
the workflow's runners + a `v*` tag + `contents: write` (all default-GitHub).
