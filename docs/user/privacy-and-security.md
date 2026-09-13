# Privacy & security

Wavelink is built **local-first and private by default**. It is a
peer-to-peer local-network relay: no cloud, no accounts, no telemetry leaves
your devices.

## Data flow

- Emitter ⇄ Receiver directly over your local Wi-Fi (QUIC, authenticated and
  encrypted). No internet hop, no relay server, no analytics.
- Discovery advertises **privacy-minimized metadata** (no raw identities).
- Pairing uses an authenticated key exchange (Noise XX) plus ed25519 peer
  identity pinning; long-lived keys live in platform secure storage; session
  keys are memory-only with replay protection.
- **No captured audio is persisted** by default, anywhere, on any platform.

## What stays on your device

- **Diagnostics are redacted** (FR-055): the diagnostic export contains
  versions, capabilities, state transitions, metrics, and recent errors — it
  **never includes** captured audio, pairing secrets, private keys, or raw
  stable device identifiers (peer names are stripped; routes are coarsely
  classified).
- Logs are bounded and local. Telemetry stays local by design; remote
  collection would need a separate, approved requirement.

## Platform permission model (explained before asked)

Every capture / local-network / Bluetooth / background permission is **explained
in the app immediately before the OS prompt** (FR-052):

- **macOS** — Screen Recording (TCC) for system capture; per-app capture uses
  Core Audio process taps on 14.2+.
- **Android** — RECORD_AUDIO + a fresh **MediaProjection** consent **each
  session** (single-use token on 14+), running in a `mediaProjection` foreground
  service. Emitters can only capture audio the OS marks capturable; protected
  content is silenced by the OS.
- **iOS** — local network (for LAN), and any capture always goes through the
  **system picker** (ReplayKit broadcast picker / SCK picker) with the system's
  red recording indicator constantly visible (App Review 2.5.14). No silent
  background capture.
- **Linux** — none needed for audio; desktop portals apply to screen capture.

## Threat posture

The project documents a formal threat model
(`docs/planning/THREAT_MODEL.md`) and security spec
(`docs/planning/SECURITY_SPEC.md`). Core mechanisms carry tests: fresh session
keys, replay rejection, key separation (control vs media), AEAD tamper
rejection, and redaction poison-tests. Do not market the product as "secure"
beyond what these tests prove; see the security spec for the exact claims.

## Hardware gate honesty

Bit-perfect output, physical-device capture all paths, BT lab, and store
signing are **pending external gates with runbooks** — the products never
claim more than has been measured.
