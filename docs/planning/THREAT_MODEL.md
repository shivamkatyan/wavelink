# Threat Model

Assets: live audio (confidentiality, integrity), pairing trust store (integrity, availability), diagnostic logs (privacy). Attackers: hostile peer on the LAN, passive eavesdropper, discovery spoofer, supply-chain adversary.

## Top threats → controls
1. Hostile peer on same LAN: authenticated encryption (XChaCha20-Poly1305 / counter-sourced nonces), authenticated Noise handshake, separate control/media keys, fresh per-session keys, replay window, rate-limited/bounded parsers.
2. Discovery spoofing: mDNS identity is never trusted; connect only to a store-pinned ed25519 fingerprint; SAS/QR bind to the actual handshake pubkey; never connect to an IP lacking a verified fingerprint.
3. Unauthorized injection / eavesdropping: AEAD + authenticated peer + pairing gate; 0-RTT disabled (replayable → injection).
4. Pairing downgrade / replay: persist `(peer_id, static_key, protocol_version, handshake_pattern)`; reject any session below stored version/pattern; explicit re-confirm when elevation needed; SAS comparison discipline (never auto-set, atomic with key).
5. Malformed packets / resource exhaustion: numeric bounds (≤16 KB control, ≤4 KB frame payload enforced before allocation; rate ≤20/s; max handshakes 8; bounded TXT/records; per-source connection limits); fuzz targets; saturation must not starve the paired peer's reconnect.
6. Lost or sold trusted devices: signed revocation records with map-style versioning; offline propagation via QR; "forget all peers"; secure per-OS storage.
7. Diagnostic bundle disclosure: redaction allow/denylist incl. correlation IDs, device identifiers, MACs, peer IDs, weak hashes; bounded retention; no audio payload; no key material; secret scans in CI.
8. Supply chain: pinned toolchains/lockfiles, `cargo audit`, `cargo-deny`, SBOM, maintenance rubric, no unverified binary consumption.

## Secure storage per OS
iOS Keychain · Android Keystore · Windows DPAPI/TPM · macOS Keychain · Linux TPM/Secret Service with documented fallback (encrypted-at-rest) and its trust-boundary caveat.

## Acceptance tests (gate P6)
Named security test cases in `TEST_PLAN.md`; product is not marketed secure until these pass (paired MITM, key establishment, untrusted-sender rejection, replay/duplicate session, malformed/high-rate, key rotation, downgrade rejection, revoked identity, mDNS saturation, redaction + secret scans).
