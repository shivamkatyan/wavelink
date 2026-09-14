# Setup & install

Wavelink ships several types of artifact. This page explains how to
get each one onto your device or machine. **Everything below is a developer /
beta build** — none of it is a finished store release yet.

## Get the artifacts

The quickest source is the **GitHub Release** page: each `v*` release attaches
the packages built by the release workflow. Or build them yourself with
`just package` (see [`docs/orchestration/PACKAGING.md`](../orchestration/PACKAGING.md)).

Store a copy of the honest gate matrix in your head: **macOS** artifacts are
unsigned (ad-hoc) developer builds; **Android** APKs are signed with a
self-generated development keystore (fine for sideloading, not the Play Store);
**iOS** artifacts are unsigned simulator-SDK builds until an Apple Developer
account + Xcode project exist; **Windows/Linux** packages are produced on CI
runners.

## Android app

There is **one** Wavelink app for Android (`dev.wavelink.app`): it opens on a
**role picker** and asks what this device will do — **Emitter** (capture &
stream this device's audio) or **Receiver** (play Wavelink streams through its
output / USB DAC). You do not need to install two apps.

1. Take `android-wavelink-release-signed.apk` from the release artifacts (or
   from `dist/android/` after `just package`).
2. Transfer the APK to your phone (USB, cloud drive, or `adb install`).
3. On Android, allow **"Install unknown apps"** for the source you used.
4. Open the app and pick a role. You'll be asked for permissions *with an
   explanation first* (FR-052). As **Emitter**, Android requires a
   **MediaProjection consent picked fresh every capture session** — that's how
   Android grants app/screen audio capture. As **Receiver**, the app runs a
   foreground service and scans for a relaying emitter on the LAN.
5. The receiver's demo values for stream health are placeholders until the
   network transport is wired in (see the project status in `docs/user/README.md`).

> Android 14+: MediaProjection tokens are single-use — a new consent screen
> appears each capture session. That's the OS contract, not a bug.

## macOS

1. Mount `wavelink-<rev>.dmg` (double-click; it's read-only). Drag **Wavelink.app**
   onto the **Applications** folder alias inside the window — the standard macOS
   install (no manual copying). The app is a **universal** binary: one build that
   runs on Apple Silicon and Intel Macs.
2. Open `/Applications/Wavelink.app`. The app opens on a role selector —
   **Emitter** is the shipped, streamable role; **Receiver** is staged in the UI
   (it needs the macOS render path that is still a gate).
3. Because this is a **developer, unsigned** build, macOS Gatekeeper quarantines
   the downloaded app and refuses the first open ("can't be opened because Apple
   cannot check it for malicious software"). That is **expected** for unsigned
   developer builds — do one of:
   - Right-click (Control-click) **Wavelink.app** in Finder → **Open** → **Open**
     again in the dialog, or
   - `xattr -dr com.apple.quarantine /Applications/Wavelink.app`
4. First use: grant **Screen Recording** permission when macOS asks
   (System Settings → Privacy & Security → Screen Recording). The app explains
   why before the prompt. Without this grant the app still lists audio
   endpoints/hardware but cannot capture system audio.
5. The CLI inside the app supports `--list-format` (audio endpoints/USB DAC),
   `--permission-state`, and `--version` (see Verify the install).

> A real Developer ID + notarized release removes the Gatekeeper step entirely —
> that is a credentials gate, tracked in `docs/planning/RELEASE_AND_SIGNING.md`.

## iOS

The iOS **receiver** and **emitter** are source shells today. There is no
ready-to-install `.ipa` yet because shipping one needs an Xcode project +
signing (`docs/planning/RELEASE_AND_SIGNING.md`). To try them:

1. Have Xcode + a Mac (this repo builds against the iPhoneSimulator SDK).
2. Build the core libraries (`just package ios` produces them).
3. The App layer type-checks against the iOS SDK — running it on a device
   requires opening it in an Xcode project (device gates: `.usbAudio` DAC
   output, local-network permission, ReplayKit/SCK capture are device-gated).

## Windows

`win-emitter.zip` (built on the Windows CI runner) contains a WASAPI loopback
emitter CLI (`win_emitter.exe`). Unzip and run from a terminal:

```bat
win_emitter.exe --list-format   :: enumerate render endpoints
win_emitter.exe --version
```

Capture is **system-wide only** (WASAPI loopback — there is no per-process PCM
API on Windows). Protected content is muted by the OS.

## Linux

`linux-emitter-<rev>.tar.gz` (and `.deb`) is built for amd64. It is a
PipeWire-aware emitter CLI (system-wide capture; per-app capture is available on
PipeWire via node targeting). Install the `.deb`:

```bash
sudo dpkg -i linux-emitter_*.deb   # or extract the tarball and run ./linux-emitter
```

The **Linux A2DP-sink receiver** (`linux-receiver`) turns a Linux box into a
Bluetooth audio receiver that renders to its output/USB DAC — the one BT path
the product supports. It requires Linux + BlueZ/PipeWire and is validated in a
BT lab (`bt-lab` gate). See [Bluetooth](bluetooth.md).

## Verify the install

- macOS: `Wavelink.app/Contents/MacOS/macos-emitter --version`
- Windows: `win_emitter.exe --version`
- Linux: `linux-emitter --version`
- Android: the app opens the role picker, and each role's activity shows the
  Free/Pro toggle and a status panel; the Emitter role requests MediaProjection
  consent; the Receiver role starts its foreground service.
