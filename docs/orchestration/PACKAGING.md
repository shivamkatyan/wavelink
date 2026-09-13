# Packaging & Releases

How the WDR platform shells get turned into distributable artifacts, and how
those artifacts get published to a GitHub Release. The **end-user** story
(install, usage, Free vs Pro) lives in [`docs/user/`](../user/README.md) and on
the static site (built by `just site`, deployed by `.github/workflows/pages.yml`).

- Task: `t-dev-packaging` · Docs that own the honest limits:
  [`RELEASE_STATUS.md`](./RELEASE_STATUS.md) and
  [`docs/planning/RELEASE_AND_SIGNING.md`](../planning/RELEASE_AND_SIGNING.md).

## One-liner

```bash
just package                 # android + macos (native to this macOS host)
just package linux           # tarball (+ .deb) via the wdr-dev container
just package windows         # zip of win-emitter.exe  (WINDOWS CI runner ONLY)
just package ios             # unsigned simulator-SDK core libs + docs
WDR_DIST_DIR=/tmp/dist just package macos   # override the artifact root
```

`just package` is a thin driver over `scripts/package/all.sh`, which calls the
per-platform scripts. Everything honors `WDR_DIST_DIR` (default `<repo>/dist`)
and `WDR_NO_INTERACTIVE=1`; every script is `set -euo pipefail`, idempotent,
bash-3.2-safe, needs no sudo, and prints the reproducible commands it ran.

On this macOS host, `just package` (no args) automatically runs only what is
native here: **android + macos**. `linux` uses the real `wdr-dev` Docker image;
`windows` correctly refuses on macOS with `exit 2` (real EXE link needs the
Windows runner); the same guards make the workflow's CI jobs the primary home
for the non-native platforms.

## Artifact map

| Target | Script | Artifacts under `$WDR_DIST_DIR/…` |
|---|---|---|
| android | `scripts/package/android.sh` | `android/android-wavelink-debug.apk`, `android/android-wavelink-release-signed.apk` — **one combined Wavelink app** (roles picked in-app; the old split emitter/receiver APKs are not shipped), dev keystore at `android-keystore/` |
| macos | `scripts/package/macos.sh` | `macos/macos-emitter.app` (ad-hoc signed), `macos/wavelink-<rev>.dmg` |
| linux | `scripts/package/linux.sh` | `linux/linux-emitter-<rev>.tar.gz`, `linux/linux-emitter_<ver>-<rev>_amd64.deb` (when `dpkg-deb` exists) |
| windows | `scripts/package/win.sh` | `windows/win-emitter-<rev>.zip` |
| ios | `scripts/package/ios.sh` | `ios/wavelink-ios-source-<rev>.tar.gz` (type-check gate report + build-check.md + README + RELEASE_AND_SIGNING.md) |

Every artifact ships a `LICENSE-NOTICES.txt` (from
`docs/planning/LICENSE-NOTICES.md`). `<rev>` is the short git HEAD (or
`unknown` when git is unavailable — a container without git honors
`WDR_RUN_REV`, which the macOS→container path sets automatically).

### Android
- Builds **one combined Wavelink app** (`platform/android-wavelink`): a single
  APK whose role picker exposes Emitter and Receiver in-app. The split-era
  `android-emitter` / `android-receiver` shells were retired (removed from the
  tree) and are NOT packaged.
- JDK 17 + `ANDROID_HOME` (defaults for this host, overridable via `JAVA_HOME` /
  `ANDROID_HOME` / `ANDROID_BUILD_TOOLS`).
- `./gradlew --no-daemon :app:assembleDebug :app:assembleRelease` (release is
  `minify=false`). Then `zipalign -f -p 4` + `apksigner sign --ks …` from
  build-tools `34.0.0`, verified with `apksigner verify` + `zipalign -c`.
- **Keystore**: a self-signed RSA keystore is generated **once** at
  `$WDR_DIST_DIR/android-keystore/android-wavelink.keystore` (+`*.keystore.pass`).
  DEV-ONLY — a real store ships different signing secrets
  (`RELEASE_AND_SIGNING.md`); `dist/` is gitignored so dev keys never land in
  VCS.

### macOS
- `cargo build --release --bin macos_emitter` in `platform/macos-emitter`,
  assembled into `macos-emitter.app` (minimal `Info.plist`, `MacOS/`,
  `Resources/`), `install_name_tool -add_rpath /usr/lib/swift` (the standard
  Swift-runtime rpath Xcode emits), then `codesign --force --sign -` (ad-hoc —
  no identity on this host/CI; production notarized signing is the documented
  credential gate), then a read-only `hdiutil create -format UDZO` DMG. No
  `create-dmg`; no GUI/attach workflows.

### Linux
- Native on a Linux runner (CI) or the `wdr-dev` dev container (`/workspace`):
  `cargo build --release --bin linux_emitter` (portable surface; the dedicated
  `--features pipewire` build is a follow-up on a PipeWire runner), `tar czf`
  tarball; `.deb` when `dpkg-deb` is present.
- On macOS with Docker: re-runs itself inside `wdr-dev`
  (`docker run --rm -v $PWD:/workspace …`) so the tarball is genuinely produced
  by a Linux toolchain into the mounted `dist/`.
- Otherwise prints a clear message and exits 2.

### Windows
- Only runs on the Windows runner (git-bash style shells); elsewhere prints
  "Windows packaging requires the Windows runner" and exits 2.
- `cargo build --release --target x86_64-pc-windows-msvc`, zip via
  `powershell Compress-Archive` (fallback `tar -a -cf`), includes the EXE +
  LICENSE-NOTICES + README.
- The macOS local **compile gate** for the same code is
  `cargo check --target x86_64-pc-windows-msvc` in `platform/win-emitter`
  (rustup target; no linker needed).

### iOS
- **The shipped iOS surface is the merged single-app source under
  `platform/ios`** (one Wavelink app, roles picked in-app). The split-era core
  libraries were retired, so `ios.sh` runs the merged app's simulator-SDK
  `swiftc … -typecheck -warnings-as-errors` gate (0 errors, no device, no
  signing) and packages the gate report + docs.
- A real signed `.ipa` needs an Xcode project wrapper + Apple signing/dev
  account — honest, not fabricated (see `RELEASE_AND_SIGNING.md` and the iOS
  gate in `RELEASE_STATUS.md`).

## Release workflow (`.github/workflows/release.yml`)

- **Triggers**: push of a `v*` tag and `workflow_dispatch`.
- **Jobs** (thin wrappers over the same scripts):
  `android` (ubuntu, temurin 17, `sdkmanager` android-34+build-tools 34.0.0),
  `macos` (macos-26), `linux` (ubuntu, native), `windows` (windows-2025,
  `x86_64-pc-windows-msvc`), `ios` (macos-26). Each uploads its `dist/<platform>`
  via `actions/upload-artifact@v4`.
- **`publish`** (`needs: [android, macos, linux, windows, ios]`,
  `permissions: contents: write`): downloads all artifacts, then
  `gh release create "$TAG" …` with `GH_TOKEN` — **skip-if-exists**: if a
  Release for the tag already exists it logs a warning and exits 0 (never
  clobbers; replace deliberately by deleting the Release and re-running).
  A `workflow_dispatch` run on a branch simply skips publication.
- Dependency policy: official actions only (`actions/*`, `dtolnay/rust-toolchain`,
  `actions/setup-java`) + the runner-hosted `gh` CLI.

## Gate matrix (honest)

| Item | Status | Where |
|---|---|---|
| Android assembleDebug + assembleRelease + zipalign + apksigner sign (dev keystore) | **run on this macOS host** | `scripts/package/android.sh` locally and CI |
| macOS .app + ad-hoc codesign + DMG | **run on this macOS host** | `scripts/package/macos.sh` locally and CI |
| Linux tarball + .deb (real Linux toolchain) | run via `wdr-dev` Docker here; native on the ubuntu CI job | CI-gated for the published artifact |
| Windows win-emitter.exe linked + zipped | `cargo check --target` passes locally; **real EXE link requires the Windows runner** | CI `windows` job |
| iOS merged-app type-check gate (unsigned source package) | **run on this macOS host** | `scripts/package/ios.sh` locally and CI |
| GitHub Release creation | requires a `v*` tag + repo permissions | CI `publish` job (not reproducible locally) |
| Real store/dev signing + notarization + App Store | **credential-gated** — never fabricated here | `RELEASE_AND_SIGNING.md` |

Signed production artifacts, hardened runtime / notarization and store uploads
are out of scope for these scripts on purpose: the pipeline always emits
unsigned/store-gated intermediates plus the exact command logs to extend.

Also see `docs/orchestration/reports/t-dev-packaging.md` (task report with the
exact local test matrix).
