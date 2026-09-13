# Architecture

## System context
Emitter device (audio graph → capture adapter) ⇄ [LAN: mDNS discovery; trusted QUIC control; media] ⇄ Receiver device (transport/decode → jitter/drift → render adapter → output/USB DAC). Two apps, one versioned protocol. No cloud, no WAN, no accounts.

Mermaid diagrams are authored in the build branch and embedded at `docs/architecture/*.md`; core ones:
- `sequenceDiagram` pairing/negotiation/stream
- `stateDiagram-v2` session FSM
- `flowchart` audio pipeline data path
- `graph` repository layout

## Process/component boundaries (shared Rust core + native shells)
- **core/** (Rust, platform-free): `proto` (schemas, codec bindings, golden vectors), `session` (FSM, negotiation, timers), `transport` (quinn control + datagram media + reorder), `codec` (Opus/FLAC/PCM adapters), `crypto` (Noise/keys/AEAD), `jitter+drift` (buffer, estimator, resampler), `fidelity`, `entitlement`, `telemetry` (redaction, metrics), `fakes` (synthetic source/hash sink, fake adapters), `cli-sim` (headless emitter/receiver sim).
- **Shells**: Android (Kotlin/Compose + Oboe/AudioTrack + FGS), iOS (SwiftUI/AVAudioEngine/ReplayKit or SCK + broadcast ext), Windows (WPF shell + WASAPI via windows-rs), macOS (SwiftUI/AppKit + SCK/taps + CoreAudio HAL), Linux (GTK4 shell + PipeWire).

## Audio data flow
CaptureCallback → (RT op only: memcpy into preallocated SPSC ring from a buffer pool) → capture worker:
normalize only when required → frame accumulate → encode (Opus) or lossless frame (FLAC/raw, per-frame CRC) → AEAD →
**lossy = QUIC unreliable datagram; lossless = reliable QUIC stream (bounded latency budget, retransmit deadline)** →
receiver: dedicated datagram/stream reader → SPSC ring (never blocks, credit high-water asserted) → reorder window →
decode → jitter buffer (profile bounds) → drift estimator + bounded adaptive resample / controlled correction →
render-format conversion only when required → dedicated render thread → render callback: memcpy from ring only.

## Control & media planes
One QUIC connection; reliable control stream(s) with explicit response timeout and flow-control budget; media plane as above. Separate session keys for control vs media; fresh per session; replay protection on both; 0-RTT disabled for media (replayable).

## Shared-core / adapter boundary
Platform-touching dependencies behind trait adapters: `CaptureSource`, `RenderSink`, `Discovery`, `PairingUi` (SAS/QR), `EntitlementProvider`, `Clock`, `PermissionGate`, `Storage` (secure). Adapters expose format metadata, bounded queues, overflow/underflow policy, latency metrics, lifecycle cancellation.

## Threading / real-time (P1 review findings folded in)
- Platform RT callbacks may only copy in/out of SPSC rings (preallocated pool sized ~2× worst-case in-flight). Runtime enforcement is **implemented** (RT_CONTRACT.md §4): `panic="abort"` + `wdr_rt`'s `rt-guard` `#[global_allocator]` that aborts on any allocation inside an RT-callback context; `wdr_rt` is `#![no_std]` with a zero-allocation deny-alloc probe on the RT surface and a `StallDetector` watchdog. The per-platform whitelist tables remain the by-construction contract.
- Encode/decode/resample/encrypt/transport on dedicated thread-pinned workers with worst-case budgets (not average).
- Per-platform callback allowed-operation whitelist table authored during B0 (`docs/planning/ADRS/ADR-002.md` + `core/rt/` docs).
- WSL2 simulation cannot satisfy the native real-time evidence gate; Linux RT evidence only from real PipeWire/RTKit runs (self-hosted native runner or lab).
- Instrumentation/stress tests detect allocation, blocking, priority inversion, excess duration, races.

## Trust boundaries & storage
Peer identity = ed25519 fingerprint pinned in per-OS secure storage (Keychain/Keystore/DPAPI/TPM-SecretService with documented Linux fallback). Session keys memory-only. No captured-audio persistence by default. Telemetry local-only.

## Failure & recovery
Bounded reconnect (1–10 s window, ×1.5 backoff to 30 s), session idle expiry 60 s, fresh-session rules, sleep/wake probe, route-change rebuild, never-silent lossless→lossy, terminal errors → actionable UX.

## Observability
`tracing` structured events, stable names, session-local correlation IDs, bounded rotating logs, redaction allow/denylist, metrics endpoint for the soak harness, one-session verbose diagnostics without rebuild.

## Packaging
MSIX/EXE (Win); DMG (macOS Developer ID + notarization dry-run); deb/rpm/AppImage/Flatpak (Linux); APK/AAB (Android, keystore self-generated now); TestFlight/App Store path (iOS, credential-gated). Signing jobs packaged, gated, ready on credentials.
