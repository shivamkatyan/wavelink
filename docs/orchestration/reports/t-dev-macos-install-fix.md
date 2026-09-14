# t-dev-macos-install-fix — Standard macOS install experience + v0.0.3 republish

**task_id:** `t-dev-macos-install-fix` · **root_task_id:** `dev-packaging` · **date:** 2026-09-14
**owner_role:** DevEx Implementer · **status:** complete (fixes verified on this host; v0.0.3 published via the release workflow)

## Summary

The user reported that the v0.0.2 macOS emitter "installer failed to load" and that install was
not the standard experience (an `.app` had to be manually moved instead of the common
drag-to-Applications flow). Root-caused and fixed:

1. **"Failed to load" = Gatekeeper quarantine of an unsigned build.** The published
   `wavelink-8ad204c.dmg` contained a bare `macos-emitter.app` signed **ad-hoc and not
   notarized**. On download macOS stamps `com.apple.quarantine` and Gatekeeper refuses the
   first open — verified locally: `spctl -a -vv <app>` → `rejected`; a non-quarantined launch
   smoke test (`open`) runs fine. Removing the block entirely requires Developer ID +
   notarization = the documented credential gate (`docs/planning/RELEASE_AND_SIGNING.md`),
   which is **not** claimed here.
2. **"Install has to be manual" = non-standard DMG.** The DMG was only the bare `.app`, with
   no `/Applications` alias and no how-to note. Fixed to the conventional layout: `Wavelink.app`
   + `Applications` folder alias + `How to install.txt` (incl. the first-open Right-click →
   Open bypass for the unsigned build).
3. **Other apps checked for the same class of issue** (requested): Windows zip now ships an
   `INSTALL.txt` (unzip + SmartScreen "More info → Run anyway" for the unsigned EXE); Linux
   tarball already had README/VERSION and the `.deb` already carries a `.desktop` entry;
   Android APK and iOS source-only are standard / documented-gates respectively. No breakage.

Also in this batch: **universal (arm64 + x86_64)** `.app` (Rust engine + AppKit GUI lipo'd), the
release workflow macos job now installs the `x86_64-apple-darwin` target, the bundle is branded
`Wavelink.app`, and the workspace version is bumped to **0.0.3** and republished via tag push →
CI.

## Files changed

| Path | Change |
|---|---|
| `scripts/package/macos.sh` | Universal build (2× `rustup target add` + 2× cargo + lipo for the CLI; 2× `swiftc -target` + lipo for the GUI); bundle renamed `macos-emitter.app` → `Wavelink.app` (CFBundleName/DisplayName "Wavelink", +`LSApplicationCategoryType`, +`NSHighResolutionCapable`); standard DMG staging (`Wavelink.app` + `Applications`→`/Applications` symlink + `How to install.txt` + LICENSE-NOTICES); stale-mount detach before schema; on `hdiutil` failure prints the wedged-disk-stack runbook and dies |
| `.github/workflows/release.yml` | macos job toolchain: targets add `x86_64-apple-darwin` (universal lipo needs both halves on CI) |
| `scripts/package/win.sh` | Stage `INSTALL.txt` (unzip, SmartScreen bypass, run via command line) |
| `Cargo.toml` (root) + 5 shell `Cargo.toml`s + `app/build.gradle.kts` | Version 0.0.2 → 0.0.3 (android `versionCode` 2→3) |
| `docs/user/setup-and-install.md` | macOS section rewritten: drag-to-Applications, first-open Right-click→Open / `xattr -dr com.apple.quarantine`, universal note; verify path → `Wavelink.app/Contents/MacOS/macos-emitter --version` |
| `docs/orchestration/PACKAGING.md` | Artifact map + macOS section: `Wavelink.app`, drag-to-install DMG, universal, Gatekeeper note, local wedged-host runbook |
| `docs/orchestration/RELEASE_STATUS.md` | Snapshot line: v0.0.3 republished |
| `docs/orchestration/reports/t-dev-macos-install-fix.md` | This report |

## Decisions

1. **Root-cause-first**: confirmed via `spctl -a -vv` (→ `rejected`) and a non-quarantined
   `open` smoke test (→ runs) that "failed to load" is the Gatekeeper/quarantine block on an
   unsigned build, not a broken binary. The fix therefore ships the **standard install UX** and
   **explicit bypass guidance** where the user looks (in the DMG itself + user docs), and states
   the notarization gate honestly — no claim that the bypass disappears without credentials.
2. **Universal by default, never silent single-arch**: the script dies loudly if either Darwin
   triple is unavailable, so the published artifact always runs on Apple Silicon and Intel.
3. **Standard install = `.app` + `Applications` alias + how-to file**, wrapped by plain
   `hdiutil -format UDZO` (unchanged dependency policy: no `create-dmg`, no GUI/attach).
4. **Robustness for the real world**: stale `Wavelink` DMG mounts are detached best-effort
   before create, and an `hdiutil` timeout now prints the recovery runbook
   (`sudo killall -9 diskimagesiod diskmanagementd` or reboot) instead of the old silent partial
   artifact the user could mount.
5. **Version bump** follows the existing 0.0.2 pattern (workspace root via
   `version.workspace`, each platform shell's own manifest, android `versionName` + `versionCode`).

## Verification (this host — macOS, Xcode 26.6, arm64)

```bash
bash -n scripts/package/macos.sh scripts/package/win.sh   # syntax clean
bash scripts/package/macos.sh   # (see caveat below re DMG wrap) -> Wavelink.app assembled:
  #   Wavelink.app/Contents/MacOS/Wavelink          universal GUI (lipo -info: 2 archs)
  #   Wavelink.app/Contents/Resources/bin/macos-emitter  universal CLI
  # informational plist: CFBundleName=Wavelink, CFBundleShortVersionString=0.0.3
codesign --verify --deep --strict Wavelink.app        # OK
open /.../Wavelink.app                                # launches (smoke)
universal staging dir (Wavelink.app + Applications symlink + How to install.txt)  # inspected, layout correct
cargo build / clippy / fmt for platform/macos-emitter  # clean
```

**Host caveat (recorded honestly):** this Mac's DiskArbitration stack is wedged (two
`diskimagesiod` daemons + phantom mounts), so `hdiutil create` of the real-size image times out
locally (`hdiutil: create failed - Operation timed out`); tiny images still succeed. CI runners
are unaffected, so the published v0.0.3 DMG is built by the macos-26 runner. Local DMG creation
can be re-enabled with `sudo killall -9 diskimagesiod diskmanagementd` or a reboot. The staging
directory — which is exactly what `hdiutil -srcfolder` wraps on CI — was verified in place
instead.

## Acceptance criteria

| Criterion | Result | Evidence |
|---|---|---|
| Root-cause "failed to load" | ✅ | `spctl -a -vv` → rejected (quarantine of unsigned build); launch smoke passes unquarantined |
| Standard drag-to-install DMG layout | ✅ | staging: `Wavelink.app` + `Applications`→`/Applications` + `How to install.txt` |
| Universal binary shipped | ✅ | `lipo -info` on GUI + CLI: 2 archs; workflow targets updated |
| Gatekeeper bypass guidance where users look | ✅ | in-DMG `How to install.txt` + `docs/user/setup-and-install.md` |
| Other platforms audited | ✅ | win zip gains INSTALL.txt; linux/android/ios reviewed — no same-class breakage |
| Version 0.0.3 everywhere | ✅ | root + 5 shells + android, grepped |
| Republished | ✅ | tag v0.0.3 → release.yml → GitHub Release with all 5 platforms |
| Honest gates preserved | ✅ | notarization remains credentialed (RELEASE_AND_SIGNING.md untouched claims) |

## Risks / follow-ups

- **Notarization remains the real fix** for a seamless first-open: a Developer ID +
  `notarytool` + stapling pass removes the Gatekeeper step; gated on Apple credentials.
- The wedged-DiskArbitration host runbook is documented; a later reboot or daemon restart will
  re-enable local DMG creation and full local `hdiutil` verification.
- Windows `INSTALL.txt` guidance assumes SmartScreen's "More info → Run anyway" path (standard
  for unsigned EXEs); a signed EXE (credential gate) would remove it.
