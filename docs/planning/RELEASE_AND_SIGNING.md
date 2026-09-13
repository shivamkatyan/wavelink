# Release & Signing

## Distribution per OS
- **Windows**: MSIX + EXE. Signing: Store/MSIX free re-sign; Azure Artifact Signing or CA cert for direct MSIX/EXE. SmartScreen reputation curve documented. Windows has no notarization step. Unsigned MSIX/EXE produced now.
- **macOS**: Developer ID + Hardened Runtime + notarization (`xcrun notarytool`, staple). App Sandbox for App Store. Ad-hoc/unsigned builds + notarization dry-run config now; credentials gated.
- **Linux**: deb/rpm/AppImage/Flatpak. Flatpak perms: `--socket=pulseaudio` / PipeWire socket, `--allow=bluetooth`, `--talk-name=org.bluez`, optional USB portal/`--device=usb`.
- **Android**: AAB + APK. Release keystore generated + backed up now; Play App Signing (upload key vs app-signing key) documented; store publish gated.
- **iOS/iPadOS**: TestFlight + App Store Connect; Apple Developer Program + provisioning gated; alternative-EU notarization noted (not the primary App Store gate — App Review is).

## Gated vs now (no-credential path is fully buildable)
- **Now**: unsigned installers; APK w/ self-generated keystore; ad-hoc/unsigned macOS; dry-run notarization config; SBOM + notices; docs; CI green at all tiers available.
- **Gated (credentials)**: Apple Developer Program; Windows code-signing cert / Azure Artifact Signing; Android store upload; App Store Review submission.
Secret names for env/CI: `WINDOWS_CERT`, `WINDOWS_CERT_PASSWORD`, `AZURE_SIGNING_ID`, `APPLE_DEVELOPER_ID`, `APPLE_NOTARY_KEY*`, `APPLE_APP_STORE_KEY*`, `ANDROID_UPLOAD_KEYSTORE*`, `IOS_TEAM_ID`. Never request secrets via chat; document env vars + CI secret names only.

## Release artifacts
CHANGELOG; security notes (threat model); support/user/operator docs; install/pairing guide; matrix of honest unsupported cells with one-action fallback; release-notes template carrying the honesty requirement.

## Compliance records
iOS: RPSystemBroadcastPickerView/SCContentSharingPicker; purpose strings; background mode justification (2.5.4), micro consent (2.5.14); no hidden features. Android: FGS types + permissions; MediaProjection renew. macOS: TCC keys; sandbox/hardened entitlements.
