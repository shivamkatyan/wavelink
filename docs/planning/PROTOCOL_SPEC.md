# Protocol Specification

## Versioning & compatibility
- `PROTO_MAJOR.MINOR` exchanged during negotiation; min/max compatible range; incompatible → clear failure.
- Unknown optional fields forward-compatible (TLV/extension-bit rules). Deprecation policy: minor-field deprecation with N-version grace; major bumps require a mirrored, separately versioned compatibility fixture set.
- One source of truth for schemas (postcard + serde derives), from which bindings and golden vectors are generated.

## Discovery
- mDNS `_wdr._tcp` TXT: protocol/version, role, entropy-tagged instance; **no identity key, no device-identifying data** (privacy-minimized metadata).
- Bounds: TXT ≤512 B; record-count cap; cache TTL bounded; saturated browse must not starve the paired peer.
- Manual IP + QR fallback (QR encodes address + pairing nonce). QR also used for offline revocation propagation.

## Pairing & authentication
- Noise **XX** (snow) over the control stream; ed25519 long-term identity (pairing pubkey), X25519 ephemeral → authenticated key agreement with forward secrecy.
- SAS (6-digit) or QR binds to the **actual handshake pubkey**; auto-connect never trusts an mDNS identity without a store-pinned fingerprint match; never connect to an IP lacking a verified fingerprint.
- Pairing FSM: `Discovered → Connecting → SAS-Shown → Confirmed | Rejected`; `confirmed` flag stored atomically with the key; re-confirm on key rotation.
- Rate limits: per-source-IP and per-identity attempt cap + lockout; concurrent in-progress handshake cap (8).
- Revocation: signed revoke records with map-style versioning (not last-writer-wins); offline propagation via QR; "forget all peers".
- **0-RTT disabled for media/control carrying audio** (replayable → audio injection).
- Session keys: XChaCha20-Poly1305 or counter-sourced 12-byte nonces with monotonic persisted counter + DTLS-style replay window; separate control/media keys; renewal triggers defined.

## State machine
`Idle → Discover → Connect → Pairing → Negotiating (incl. agree_common_policy) → Streaming → Paused → Renegotiating (mode/policy/route) → Recovering → Terminated | Error`.

Numeric bounds (locked):
- reconnect window 1–10 s; backoff ×1.5 max 30 s
- session idle expiry 60 s (no payload)
- pairing window 60 s
- request rate ≤20/s
- control response timeout 5 s
- reorder window ≤ (jitter-buffer depth − safety)
- media retransmission deadline ≤50 ms
- control message cap ≤16 KB; audio frame payload cap ≤4 KB (enforced **before** allocation)
- max pending handshakes 8
- datagram credit high-water metric asserted in soak

## Capability negotiation (FR-006/FR-046)
`SESSION_DESCRIPTOR` + `CAPABILITY` (codec, sample rate, channels, bit depth, frame duration, transport features, buffer profile, feature flags) — required even for N=1 (fan-out ready). Intersect policies (`agree_common_policy` incl. Free∩Pro and mid-session change); highest-common; explicit confirm; never silent downgrade (FR-026/FR-047).

## Audio frame header
version, stream ID, sequence **u64**, media timestamp **u64** emitter sample counter + wall-clock base, codec, sample rate, sample representation, channel layout, frame sample count, flags, integrity (per-frame CRC on lossless; AEAD tag on all).

## Codec profiles
- Opus: 44.1/48 kHz, 20 ms frames (10 ms low-latency profile), ~128–256 kbps stereo VBR; TPDF dither + optional noise shaping when down-converting 24→16-bit; **>48 kHz sources do not route through lossy** (documented).
- Lossless: FLAC small fixed blocks (~240 samples @48k) + raw PCM profile; per-frame CRC.

## Feedback
Receiver→emitter: RTT, jitter, loss, reorder, late-discard, buffer fill, underruns, output clock estimate, requested adaptation (in-app SR/RR substitute).

## Timing & sequence rules
Carry u64 counters (wrap-proof at 2^64); prop-tested. Keepalive + disconnect detection; resume vs fresh-session rules per path change. Measurement discipline: timestamp points, clock method (render/DAC presentation time for render; source sample time for capture), warm-up excluded, run length ≥5 min/profile, p50/p95/p99/max + spread/uncertainty; simulated vs physical separated.

## Error taxonomy
Retryable (network, temporary route) vs terminal (policy, capability, permission) with user actions. Bounded parsers, fuzz targets (control, mDNS TXT, frame header); no parser allocates on attacker-controlled sizes.
