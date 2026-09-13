# Open Questions (non-blocking; reversible defaults in force)

1. **Bit-perfect hardware-loopback rig ownership** — default: define the rig in HARDWARE_VALIDATION.md (DAC w/ digital loopback or USB analyzer), receiver-side only, bounded-slip semantics. Owner: Reliability. Gate: B6.
2. **Degraded-audio acceptance oracle** — default: bounded objective metrics (SNR/PESQ-floor) over synthetic corpus; exact metric oracle locked in B1. Owner: Audio.
3. **Bluetooth per-platform custom-throughput numbers** — default: measure on reference hardware in B5; until then classify custom BT as low-bitrate lossy fallback-grade. Owner: BT impl.
4. **Hosted-runner audio endpoint** — pending first CI sprint probe; virtual WASAPI endpoint + self-hosted fallback. Owner: DevEx.
5. **PipeWire exact pin in box image** — pin a known-good 1.6.x in the Docker base at B0/B1. Owner: DevEx.
6. **iOS >48 kHz USB DAC sample rates** — Apple docs say session rates typically 8–48 kHz; treat >48 kHz as P (unsupported until measured). Owner: iOS impl. Gate: B4.
7. **App Review outcome for ReplayKit/SCK capture features** — compliance via system pickers; final verdict is per-submission; record in RELEASE_AND_SIGNING.md. Owner: iOS.
8. **Revocation offline-propagation UX** — default: QR share of signed revoke record + "forget all". Owner: UX. Gate: B5/B6.
9. **Branding/store naming** — placeholder "Wavelink"; replaceable, no decision needed now.
10. **Commerce-project migration boundary** — documented adapter boundary (EntitlementProvider + RELEASE_AND_SIGNING.md); no implementation. Owner: Architecture.
