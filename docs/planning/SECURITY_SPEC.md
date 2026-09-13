# Security Specification — Wavelink

**Status:** Implementation-ready (B0 security foundation).
**Owner role:** Security.
**References:** `THREAT_MODEL.md` (source of the table below), `PROTOCOL_SPEC.md` (wire bounds), `TEST_PLAN.md` §Security (named acceptance cases), `ADR-006` (discovery & pairing), `ARCHITECTURE.md` (trust boundaries), `RELEASE_AND_SIGNING.md` (secret names).
**Scope:** LAN-local device pairing, authenticated encrypted control/media, trust store life-cycle, diagnostic privacy, and the security acceptance gates that marketing may claim.

Every numeric bound below is a concrete value; where a value is a hard default that must be re-measured in a B0 spike it is explicitly marked **(measure in B0 spike)**. No `TBD` values exist in this document.

---

## 1. Threat model table (expanded from THREAT_MODEL.md)

Assets and security goals:

| Asset | Security goals |
|---|---|
| Live audio stream (control→media) | Confidentiality, integrity (no injection, no tamper undetected) |
| Session keys / key material | Confidentiality (memory-only, never persisted) |
| Pairing trust store (peer fingerprints, confirmation state) | Integrity, availability (no silent mutation by attackers; survives restart) |
| Revocation store | Integrity (anti-forgery), availability (offline-propagation capable) |
| Diagnostic logs / exports | Privacy (no audio, no secrets, no raw identifiers beyond need) |
| Build & release artifacts | Integrity (supply chain) |
| Control channel / session state | Integrity, availability (bounded, not starve-able) |

| # | Attacker | Threat (with CIA goal) | Control (with spec ref) |
|---|---|---|---|
| T1 | Hostile peer on same LAN (A1) | Eavesdrop/reassemble audio; the only means to key establishment is the authenticated handshake | Authenticated Noise XX handshake (§3); AEAD per-message (XChaCha20-Poly1305 default, §4); separate control/media keys, fresh per session (§4.2); replay window (§4.4) |
| T2 | Hostile peer on same LAN (A1) | Inject forged control/audio into an unpaired session | Pairing gate gates all streams; auto-connect only to store-pinned fingerprint (§3.2); 0-RTT disabled entirely — nothing replayable into the session (§4.7); AEAD rejects tamper (§4) |
| T3 | Passive eavesdropper (A2) | Sniff ciphertext offline; break long-term secrecy given later key compromise | X25519 ephemeral → forward secrecy (§3.1); fresh per-session keys; XChaCha20-Poly1305 over every record; no long-term key ever on the wire in clear |
| T4 | Passive eavesdropper (A2) | Infer identity/behavior from discovery metadata | mDNS TXT privacy-minimized: protocol/version, role, entropy-tagged instance only — no identity key, no device-identifying data (§4.1, PROTOCOL_SPEC) |
| T5 | Discovery spoofer (A3) | mDNS identity spoof → connect to attacker device; poison discovery | mDNS identity never trusted (§3.2); fingerprint-pinned trust store; SAS/QR bind to the **actual handshake pubkey** (§3.2); never connect to an IP lacking a verified fingerprint |
| T6 | Man-in-the-middle (A1) | Active MITM during first pairing (impersonate receiver to emitter and vice versa) | SAS 6-digit shown from *final handshake hash*; compare off-channel; never auto-set; atomic with key store (§3.4, SEC-01) |
| T7 | Man-in-the-middle (A1) after pairing | Downgrade to older protocol/pattern on re-connect | Persist `(peer_id, static_key, protocol_version, handshake_pattern)`; reject < stored version/pattern (§3.3, SEC-08) |
| T8 | Replay attacker (A1) | Replay captured datagrams/control to duplicate or inject | DTLS-style replay window (§4.4); monotonic persisted counter for the AES-GCM nonce path (§4.3); QUIC reliable control has its own ordering+duplicate handling (§4.4); SEC-04 |
| T9 | Resource-exhaustion adversary (A4) | Malformed/oversized/truncated/high-rate packets → crash, OOM, CPU stall | Numeric bounds enforced **before allocation** (§5); rate limits + attempts caps + lockout (§5.4); bounded parsers, fuzz targets (§5.6); RT-code path never allocates (ADR-002) |
| T10 | Resource-exhaustion adversary (A4) | Saturate discovery (mDNS) to starve the paired peer | Discovery caps: TXT ≤512 B, bounded record instances, bounded cache TTL; saturated browse never starves the paired peer's reconnect (§5.5, SEC-10) |
| T11 | Formerly-trusted device / holder (A5) | Pairing a lost/sold/stolen device; stale trusted device connects | Revocation records: signed, versioned, map-style; offline QR propagation; "forget all peers"; trust store wipe (§3.5, SEC-09) |
| T12 | Local attacker (A7) | Read persisted keys off disk | Per-OS secure storage adapter (§6); session keys memory-only; Linux fallback documented with caveat (§6) |
| T13 | Diagnostic bundle disclosure (A7 / A1 data-at-rest) | Exported logs leak pairing secrets, audio, identifiers, fingerprints | Redaction allow/denylist (§5); bounded retention (§5.3); export manifest (§5.4); secret scans in CI (§8, SEC-11) |
| T14 | Supply-chain adversary (A6) | Malicious dependency or toolchain in released artifacts | Pinned toolchains + committed `Cargo.lock`; `cargo audit` + `cargo-deny` + SBOM; maintenance rubric; no unverified binary consumption (SEC-12) |

Attackers on the LAN (mDNS broadcast domain) are considered **honest-but-lazy on protocol but arbitrary on traffic**: they can send/receive any datagram and any QUIC frame; they cannot read the victim device's secure storage or OS-level session secrets **unless** the per-OS storage fallback caveat in §7 says otherwise.

---

## 2. Pairing protocol spec

Implements FR-004 / ADR-006. Wire details live in `PROTOCOL_SPEC.md`; this section is the implementation contract.

### 2.1 Noise pattern (locked)

- **Handshake:** Noise **XX** over the control stream, one handshake per session, using **snow**.
- **Handshake name (locked default):** `Noise_XX_25519_ChaChaPoly_BLAKE2s`
  - DH: X25519 (`25519`) — ephemeral keys give forward secrecy.
  - Cipher: `ChaChaPoly` = XChaCha20-Poly1305 (128-bit/256-bit key, 24-byte nonce).
  - Hash: BLAKE2s. (Alternative Hash HKDF option is **not** selected; pattern+cipher+hash are pinned for the asset/test fixtures.)
- **Identity:** ed25519 long-term keypair = pairing pubkey. For the Noise static-DH field, the X25519 key **derived deterministically from the ed25519 seed** is used (standard conversion); the identity that SAS/QR and the trust store reason about is the **ed25519 public key**, and the fingerprint pinned/verified is the ed25519 fingerprint.
- **Transmission security:** the initiator's and responder's static keys travel *inside* the Noise handshake (XX discloses statics to the peer, encrypted+authenticated by the handshake). The pairing secret and static keys are therefore never on the wire in clear, even before confirmation.
- Pattern change or cipher-suite change requires a PROTOCOL_MAJOR bump (see §3.3 downgrade rules).

### 2.2 Channel binding (SAS / QR → actual handshake pubkey)

- **SAS:** exactly 6 decimal digits, derived from the **final handshake hash `h`** (the Noise output that is a function of both peer statics and both ephemerals), not from a shortened transcript guess. Both devices compare off-channel (different device, human reads aloud / QR shows on one device, read by the other).
- **QR:** encodes (a) transport address, (b) pairing nonce, (c) the **actual handshake static pubkey** (X25519 DH pubkey exposed by the Noise payload) `≡` that device's ed25519 pairing fingerprint. The QR therefore binds the out-of-band channel directly to the handshake key material confirmed in §2.4.
- **Binding rules:**
  1. The fingerprint presented by the handshake **must equal** the fingerprint encoded in the QR the user scanned, else confirm is refused.
  2. mDNS identity is **never** a source of trust: connecting lists are seeded only from (i) a store-pinned previously-confirmed fingerprint, or (ii) a QR that itself carries the handshake pubkey.
  3. **Never connect to an IP lacking a verified fingerprint.** No "first connect = trust this IP" behavior exists.
  4. SAS is only ever compared by the user; the app never auto-sets from a SAS the user did not confirm. Confirming the SAS sets the confirmed-flag **atomically with** the key write (§2.4).

### 2.3 Rejection rules & downgrade protection (locked)

Persisted per peer (in the secure trust store, §6) as a single atomic record:

```
TrustRecord {
  peer_id: u64,                      // locally assigned, monotonic
  ed25519_pub: [u8;32],              // pairing fingerprint (also the identity)
  x25519_pub: [u8;32],               // handshake static pubkey the session uses
  protocol_version: PROTO_MAJOR.MINOR,   // version this peer was confirmed at
  handshake_pattern: "Noise_XX_25519_ChaChaPoly_BLAKE2s",
  confirmed_at: unix_ts,
  confirmed_sas_any: bool,           // true only after user-based SAS/QR confirm
}
```

- **Downgrade rejection:** on every connect, the peer must present `protocol_version ≥ stored.protocol_version` **and** `handshake_pattern == stored.handshake_pattern`.
  - Offering an older `protocol_version` → reject with `E_VERSION_TOO_OLD` (terminal for that session).
  - Offering an older/different `handshake_pattern` or weaker cipher suite → reject with `E_PATTERN_MISMATCH` (terminal).
  - Either rejection is **not** a fallback path: the session does not silently renegotiate downward. User action (re-pair after app update) is required.
- **Version policy:** incompatible major → clear failure with user guidance to update; minor within compatible range negotiates normally. Deprecation follows `PROTOCOL_SPEC` N-version grace.
- The record is **write-once per confirmation**; any mutation (rotate key, re-pair) writes a new record and is only possible through a **fresh confirmation** (§2.4/§2.6).

### 2.4 "Confirmation" state machine (locked)

```
Discovered ──(no pinned fp)→ Connecting ──(XX done)──▶ SAS-Shown
    │                          │                        │
    │ (pinned fp verifies)     │ (handshake fails/      │ (SAS matches, user confirms)
    │ connect directly         │  fingerprint mismatch) │  → atomic {write key + confirmed=1}
    ▼                          ▼                        ▼
 Trusted_stream           Rejected (E_)              Confirmed ──▶ Negotiating/Streaming
```

- States: `Discovered → Connecting → SAS-Shown → Confirmed | Rejected`.
- **Atomicity:** persisting the `ed25519_pub`/`x25519_pub`/`protocol_version`/`handshake_pattern` and setting `confirmed=1` happen in one storage transaction (single secure-store write or transactional update). A crash between "key visible" and "confirmed" leaves **no** confirmable trust — the record is not retrievable as trusted.
- **Timeout:** the whole `Connecting → Confirmed|Rejected` walk completes within the **pairing window = 60 s** (§5.3). SAS must be entered/shown and confirmed inside that window; on expiry → `Rejected` and a fresh pairing attempt must restart with a fresh ephemeral.
- No "confirmation pending" state is connectable: a session may not pass `Negotiating` while `confirmed=0`.

### 2.5 Key rotation (locked)

- Rotation = the peer presents a **new** `ed25519_pub`/`x25519_pub` (device re-keyed) or a re-key is user-initiated.
- Rule: a peer with a *stored* fingerprint that now presents a different handshake pubkey is **not** forward-connected. The UI must surface "this peer's key changed" and require **re-confirmation** (fresh SAS or QR binding to the new handshake pubkey) before the new key enters the trust store.
- The old key is **dropped** from the trust store at the same atomic write that confirms the new key (no dual-trust window).
- Rotation is a trust-store operation only; the active session's traffic keys are per-session (§4.2) and are unaffected mid-session.

### 2.6 Revocation record format (locked)

Signed, versioned, map-style — **not** last-writer-wins. A revoke record is produced by the **revoking party's ed25519 key** (the owner of a peer identity being removed, or the local store's own identity when it elects to revoke a peer).

```
RevokeRecord {
  format_version: u32 = 1,
  record_id: [u8;16],                 // random, for idempotent apply
  issuer_pub: [u8;32],                // ed25519 of the revoking identity
  subject_pub: [u8;32],               // ed25519 fingerprint being revoked
  reason: enum { sold, lost, leaked, rekeyed, admin },
  issued_at: unix_ts,
  map_version: u64,                   // monotonic per (issuer,subject)
  prev_sha: [u8;32] | null,           // chained hash of the previous record in this (issuer,subject) log
  signature: ed25519_sig(issuer_pub over canonical(preceding fields)), // v1: includes format_version
}
```

- **Map-style versioning:** the revocation store is `BTreeMap<(issuer_pub, subject_pub), RevokeRecord>`. Apply conflicts (two records, same key, different `map_version`) are resolved by **highest `map_version`** — not arrival order — and `prev_sha` chain-detects forks.
- **Anti-forgery:** a record is only applied if `issuer_pub` verifies the signature **and** the store contains that `issuer_pub` as a trusted (previously confirmed) identity, **or** the record is applied as part of an explicit "forget/self-revoke" local action. An unverifiable record is dropped with a diagnostic (reason `E_BAD_REVOKE_SIG`), never silently.
- **Subject semantics:** after a valid revoke on `subject_pub`, the trust store removes that fingerprint from trusted set at next reconcile; any incoming connection presenting that fingerprint is `Rejected(Revoked)` (§SEC-09). Pairing window and SAS flow do not restart for revoked subjects.
- **Offline QR propagation:** a revoke record encodes to the same QR transport as pairing; the QR carries the signed record (not a request). Scan → verify → apply to the local revocation store. TXT/data budget for the QR is ≤512 B (a v1 record with reason+key+sig fits comfortably; enforced at encode time).
- **"Forget all peers":** a user action that wipes the entire trust store **and** the revocation store and returns to the newborn (never-paired) state. It is a single atomic secure-store wipe (best-effort OS purge where API allows). After "forget all", the device must re-pair from scratch; previously shared revoke records remain in peers' stores and continue to protect them from this device.

---

## 3. Crypto spec

### 3.1 AEAD choice (locked default)

- **Default (locked):** **XChaCha20-Poly1305** (24-byte nonce, 256-bit key). Chosen for: constant-time software performance on all 5 platforms, no AES-NI dependence, 192-bit nonce space eliminating collision pressure, and snow's `ChaChaPoly` default — one implementation covers handshake + data plane.
- **Alternative (documented, not default):** AES-256-GCM with a **counter-sourced 12-byte nonce** backed by a **persisted monotonic counter** (§3.3). This path exists for an AESNI-only deployable or negotiated-interop case; it is **opt-in per session** and must be measured before defaulting anywhere: **(measure in B0 spike)**. The 12-byte nonce is formed `salt(4) ‖ counter(8)`, counter never resets across app restarts.

### 3.2 Key separation & freshness (locked)

- **Control vs media:** distinct keys per session — `K_CONTROL` (reliable control stream(s)) and `K_MEDIA` (all media records: lossy datagrams **and** the reliable lossless stream). Compromise of one plane's traffic key does not decrypt the other.
- **Per direction:** each plane has distinct send/receive keys (4 keys/session: `K_CONTROL_TX/RX`, `K_MEDIA_TX/RX`), each with its own nonce scope (§3.3).
- **Fresh per session:** keys are outputs of the Noise handshake `Split` (CipherState per direction). A new session ⇒ new handshake ⇒ new ephemeral ⇒ new keys. No key reuse across sessions, ever.
- **Memory-only:** all traffic keys exist only in process memory; never in secure storage, never on disk, never in logs (redaction §5.2 denies key material).

### 3.3 Nonce management (locked)

- **XChaCha20-Poly1305 (default):** 24-byte nonce = `random_salt16 ‖ counter8(BE)`.
  - `random_salt16` is drawn once per (plane, direction) at key establishment and stored transiently with the session keys.
  - `counter8` increments per encrypted record in that (plane, direction); big-endian.
  - No nonce may be reused within a key scope; enforced by construction (counter increments after every record) and asserted in unit tests.
- **AES-256-GCM (alternative):** 12-byte nonce = `salt4 ‖ persisted_counter8`; the counter is **persisted atomically** (fsync'd) after each increment batch so an app crash/restart can never wrap the nonce counter within a key lifetime. This is the "counter-sourced 12B nonce with persisted monotonic counter" requirement blown out; exposed via a trait so the default XChaCha path and this path share tests.
- **Rekey triggers (§3.5)** bound the counter before exhaustion: default counter ceiling for rekey = `2^56` records (with 8-byte counter that leaves a 256:1 headroom margin, and even 2^56 far exceeds the 24 h lifetime).
- Nonce management is unit/property tested (no reuse within a key scope, no reset across restart for the persisted counter, wrap refusal = rekey).

### 3.4 Replay window (locked)

- DTLS-style sliding replay window on **media records** (both datagram and stream paths that carry their own sequence numbers) and on **control records** (belt-and-braces above QUIC's reliability/ordering).
- Each (plane, direction) maintains a **64-bit packet index** (matching the wire u64 sequence). Replay check: index `≤ oldest_seen − window` reject; within window check bitmap; `> newest` accept and slide.
- **Window width (locked default): 256 entries** (bitmask of 256 slots). Implementation cost trivial; mismatch probability negligible. **(measure in B0 spike)** to confirm against the reorder window used by jitter buffering (so legitimate reorder ≤ configured window is never falsely replayed).
- Duplicate/older control records are rejected (SEC-04).

### 3.5 Key lifetime & renewal triggers (locked)

| Trigger | Action |
|---|---|
| Session ends / new session | Fresh handshake ⇒ fresh keys (normal path). |
| Key age ≥ **24 h** in a continuously running session | In-session rekey via Noise rehandshake (new ephemerals) at a safe control point (packet index boundary); never mid-audio-callback. |
| Records encrypted ≥ **2^56** in one key scope | Rekey before counter ceiling (XChaCha) / before persisted-counter risk (AES-GCM). |
| Media AEAD auth failure count ≥ **3** within 60 s | Assume key/tamper issue: trigger rekey attempt; on repeated failure → `Rejected`/`Terminated` + diagnostic (no silent retry). |
| Key rotation of persistent identity (§2.5) | Persisted trust-store change; sessions using pre-rotation key complete by normal session end. |

Rekey is a control-plane event: both sides agree a rekey record, new keys take effect at a negotiated index. It preserves active audio with bounded control-signal latency.

### 3.6 0-RTT (explicitly disabled — locked)

- **0-RTT / early data is disabled — not merely discouraged — for both control and media.**
- Rationale: 0-RTT ciphertext is replayable (RFC 9221 forbids datagrams in 0-RTT; replaying a 0-RTT media datagram = audio injection). With 0-RTT off, no packet ever enters the session unauthenticated or replay-able.
- Enforced: connection configuration sets `enable_early_data = false` (`quinn`), and a negative test asserts no data is accepted before the handshake completes (SEC-13).

---

## 4. Numeric security bounds (locked)

Enforced in the transport/parser layer **before any allocation** driven by attacker-controlled sizes (RT path additionally aborts on allocation per ADR-002).

| Bound | Value | Enforced at |
|---|---|---|
| `MAX_CONTROL_MSG` (control message, encoded) | **≤ 16 KB** | Parse entry; oversized → drop + `E_TOO_LARGE`, never allocate `>16 KB` from body length |
| `MAX_FRAME_PAYLOAD` (audio frame payload after AEAD) | **≤ 4 KB** | Before allocation for decode/decrypt buffers |
| Control request rate per peer | **≤ 20 requests/s** | Token bucket per (session); exceeded → `E_RATE_LIMITED`, records dropped (not queued) |
| Concurrent in-progress handshakes (listener, global) | **≤ 8** | If busy: newest pairing attempt waits, then times out in pairing window |
| Pairing window (Connecting→Confirmed/Rejected) | **60 s** | Timer; expiry → `Rejected`, fresh restart required |
| Per-source-IP pairing attempt cap | **10 failed attempts / 5 min** per IP | On cap: **lockout 600 s** **(measure in B0 spike)** — no new pairing FSM entry from that IP |
| Per-identity confirmation attempt cap | **5 failed SAS/confirm attempts** per identity | On cap: **lockout 300 s** **(measure in B0 spike)**; after 3 consecutive lockouts in 24 h → admin-only re-pair (requires "forget all" or explicit reset) |
| Control response timeout | **5 s** | PROTOCOL_SPEC |
| Session idle expiry (no payload) | **60 s** | PROTOCOL_SPEC |

Rate limiting note: the paired peer's reconnect path is exempt from the per-source-IP pairing caps but still subject to control request rate; discovery saturation must never starve a paired peer's reconnect (SEC-10).

### 4.1 Discovery record caps (locked)

| Cap | Value |
|---|---|
| Single mDNS TXT record (advertised instance) | **≤ 512 B** |
| TXT key-value entries per instance | **≤ 16** (fixed well-known keys, TLV-derived) |
| Total advertised TXT per instance | **≤ 1 KB** |
| Record-count instances accepted from a browse | **≤ 64 instances** per browse; entries beyond cap ignored (not queued) |
| Cache TTL (bounded) | **≤ 120 s** refresh, re-announce ≤ 120 s interval |
| QR (pairing bound + revoke record) payload | **≤ 512 B** (encode-time cap) |

mDNS TXT carries **no** identity key, no device-identifying data; instance name is entropy-tagged (privacy, PROTOCOL_SPEC).

---

## 5. Log / diagnostic redaction spec

Requirement: an exported diagnostic bundle contains **no** audio payload, pairing secret, private key, or raw stable device identifier (FR-053/055); local logs are bounded and useful for correlation.

### 5.1 Allow list (may appear in local logs and exported diagnostics)

| Field | Notes |
|---|---|
| Protocol version `PROTO_MAJOR.MINOR`; handshake/cipher pattern name | Version only |
| Role, transport (Wi-Fi/BT), route type | Coarse only |
| Codec, sample rate, sample representation, channels, frame duration | Fidelity-allowable |
| Latency / jitter / buffer-fill / loss / reorder / late-discard / underrun metrics | p50/p95/p99 + spread |
| Error code + error class (retryable vs terminal) and recovery action | From error taxonomy |
| State-history labels (discovered/paired/streaming/…) | No identifiers |
| Build/commit hash, toolchain version | Supply-chain diagnostics |
| Session-local correlation ID (u64 nonce) | Local logs only — stripped from exports (§5.2) |

### 5.2 Deny list (never in exported diagnostics; redact to literal `<redacted>` — or omit where structural)

| Field | Policy |
|---|---|
| Audio payload bytes (any codec/form) | **Never persisted at all** (local or export); logs record counts/sizes only |
| Pairing secret / SAS digits / QR payload | Never logged, never exported (local or export) |
| Private keys, session/static key material, nonce salts, persisted counters | Never logged, never exported; key IDs are fine |
| Raw device identifiers (device serial, stable instance names, OS device name) | Deny |
| MAC addresses | Deny throughout |
| SSIDs / Wi-Fi network names | Deny throughout |
| Peer fingerprints (ed25519 pubkey) **and their weak hashes** (any truncation/short hash of a fingerprint) | Deny — classified as "weak hash → fingerprint" |
| Peer IDs (`peer_id`) | Deny in exports; session-local internally |
| **Correlation IDs as raw strings** | **Decision: NO — correlation IDs are session-local and may be retained in local logs for correlation, but are stripped (zeroed) from exported diagnostics.** Each export is re-correlated to `export_id` only. Rationale: correlation IDs are not secrets per se, but raw strings create linkability across exports; the spec keeps them local, out of the wire format, and out of exports. |
| IP addresses / hostnames | Deny in exports **by default**; export dialog has a single explicit opt-in toggle "include network addresses" (default off). Categorized info (`subnet /24`, `link-local vs routed`) is exportable without the opt-in. |
| Telemetry metrics with < 10-sample aggregation | Deny (privacy floor — too-revealing small-N noise) |

Implementation: redaction is a typed `allow/deny` table in `core/telemetry`; serializers accept only a `RedactionPolicy` and fields are tagged allow/deny at definition site. Unit tests assert the deny list for every exporter (SEC-11).

### 5.3 Retention policy (locked)

- **Local logs** (non-verbose default): bounded rotating files, **5 × 5 MiB**, TTL **7 days**, rotate on size then time. No audio bytes ever (§5.2).
- **Verbose diagnostics** (one session, opt-in): scoped to the session; **expire at session end** unless explicitly exported, then they are cleared.
- **Metrics histograms:** memory ring, last **24 h** retained, not persisted.
- **Vendor/OS crash dumps:** OS-managed outside our control; our own crash handler redacts before hand-off (no key material, no audio, no identifiers on crash paths).
- Everything on the deny list is dropped before any of these retention stores (not merely masked at export).

### 5.4 Export manifest (locked)

Each exported diagnostic bundle is a tarball + **manifest.json**:

```
{ "manifest_version": 1,
  "exported_at": unix_ts,
  "app_version": "…", "protocol_version": "…", "os": "…",
  "redaction_policy_version": "…",           // auditability
  "files": [{ "path", "sha256", "bytes" }],
  "fields_present": [...],                   // allow-list fields actually included
  "fields_redacted": [...],                  // each deny-list field, one line per rule
  "correlation_ids": "stripped",            // always
  "network_addresses": "excluded|opt_in_included",
  "local_id_prefix": "<redacted>"          // never a raw device/perr id }
```

The manifest makes "what was included / what was withheld" machine-checkable by the redaction tests.

---

## 6. Secure storage adapter table

Trust-store and revocation-store persistence (peer records §2.3, revoke records §2.6) per OS. All adapters implement one trait; each yields the same atomicity (§2.4) and durability (§2.6) semantics.

| OS | Store | Details / caveats |
|---|---|---|
| iOS | Keychain (`kSecClassGenericPassword`) | `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`; keys flagged non-exportable (`kSecAttrTokenIDSecureEnclave` where the device's SE supports it for the key class). Secure Enclave-backed when the ed25519 key is generated there; software-pinned otherwise with attestable UI statement. |
| Android | Android Keystore (`StrongBoxBacked/TrustedExecutionEnvironment` preference) | Generate/sign inside Keystore (`KeyGenParameterSpec`), no key export; hardware-backed where present (attested via `KeyInfo`); **software-backed fallback documented** (keystore without TEE) — never claim hardware for devices without it. |
| Windows | DPAPI (CurrentUser) **or** CNG TPM-backed key (`NCrypt`/`KeyStorageProvider` TPM 2.0) | Preferred: CNG TPM-backed for the long-term identity; DPAPI CurrentUser as the widely-available baseline. Both are user-profile scoped; document that DPAPI binds to user account, not a TPM. |
| macOS | Keychain (`kSecClassGenericPassword`, partition `thisDeviceOnly`) | Secure Enclave-created key where available; otherwise software key in Keychain with `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`. |
| Linux | TPM 2.0 (tss2) preferred → Secret Service (libsecret) fallback → **encrypted-at-rest app store** (documented caveat) | Order tried at first run and retried on failure: (1) TPM-backed key; (2) `org.freedesktop.secrets` Secret Service (wallet) — integrity at rest via OS wallet; (3) app-managed encrypted file: key derived (argon2id) from a per-store random recovered from the user-credentialed wallet or a one-time printed recovery code shown at first run. **Caveat (must be surfaced):** fallback (3) does **not** provide hardware-backed key protection; the trust store's confidentiality then rests on the user's passphrase/OS account — marketing must not claim equivalent hardware security when running on the fallback. The adapter reports its tier (`tpm | secret-service | file-encrypted`) so UI can show it honestly. |

Contracts: `atomic_write(record)` (single-transaction, §2.4 confirmation atomicity), `read`, `delete`, `list`, `wipe_all` ("forget all"), `durability` (fsync/persist semantics for the persisted AES-GCM counter when that alternative path is in use, §3.3).

---

## 7. "Market-not-secure-until" — named acceptance tests

Reference: `TEST_PLAN.md` §Security (gated P6/B6). **No marketing claim of "secure", "encrypted", or "end-to-end encrypted" may ship** until every named case below passes at the B6 hardening gate (Tier-1 core tests always; Tier-3 hardware evidence where the named case requires it), with zero critical/high findings.

| ID | Named acceptance test | Failure observable |
|---|---|---|
| SEC-01 | Pairing MITM (active MITM between devices shows mismatched SAS / altered handshake pubkey) | Session proceeds or SAS auto-matches → FAIL |
| SEC-02 | Fresh key establishment (new session yields distinct keys; no reuse across sessions) | Sameness of keys or nonces across sessions → FAIL |
| SEC-03 | Rejected untrusted sender (unpaired peer cannot open a session or inject control/media) | Any session/stream accepted from unpaired peer → FAIL |
| SEC-04 | Replay + duplicate session rejection (replayed datagrams/control rejected; duplicate session index rejected) | Any replayed record accepted → FAIL |
| SEC-05 | Malformed / oversized / truncated / high-rate control handled per §4 bounds (no OOM, no crash, no unbounded allocation) | Oversize allocated, crash, stall → FAIL |
| SEC-06 | Fuzz of parsers/codec/transport boundaries (control, mDNS TXT, frame header, codec payloads) — crash-free | Crash/panic/abort → FAIL |
| SEC-07 | Key-rotation boundary (new key requires fresh SAS/QR confirm; old key rejected after rotation) | Old key still trusted, or new key trusted without confirm → FAIL |
| SEC-08 | Downgrade-attempt rejection (stored `protocol_version`+`handshake_pattern` enforced; lower/older offer refused) | Session continues at lower version/pattern → FAIL |
| SEC-09 | Revoked-identity rejection (signed revoke record removes trust; revoked fingerprint cannot re-pair; offline QR-propagation applies) | Revoked identity re-pairs or connects → FAIL |
| SEC-10 | Unauthenticated mDNS saturation (bogus/spoofed browse/advertise never starves the paired peer's reconnect; discovery caps hold) | Paired peer delayed/starved by noise → FAIL |
| SEC-11 | Redaction + secret scans (export bundle passes the §5 deny list incl. correlation-ID stripping, no audio/secrets/identifiers; CI secret scanner green; no secrets in repo) | Any deny-list field in export or repo → FAIL |
| SEC-12 | Dependency vulnerability + license + SBOM (`cargo audit` clean for shipped deps, `cargo-deny` allowlist, SBOM generated & diffed on release) | Known-vuln dep shipped / unapproved license → FAIL |
| SEC-13 | 0-RTT-off assertion (no early data accepted; no replayable media datagram before/after handshake) | Any data accepted pre-handshake → FAIL |

Failure of any denominator → release gate only with an explicit documented "not secure" label and no secure marketing claims (gate row tracked in `RELEASE_STATUS.md` / `QUALITY_DASHBOARD.md`).

---

## 8. Secret-handling policy (user auth / keys / CI)

- **No secrets in chat/logs/docs/code/commits.** Secrets are referenced by **name**; values live only in the environment or a CI secret store.
- When a value is required, request the secret **name & location** (env var name, CI secret name), never the value. Instruct the user to inject it into the environment/CI themselves.
- **Known secret names** (documented, referenced as names only):
  - Windows: `WINDOWS_CERT`, `WINDOWS_CERT_PASSWORD`, `AZURE_SIGNING_ID`
  - Apple: `APPLE_DEVELOPER_ID`, `APPLE_NOTARY_KEY_*`, `APPLE_APP_STORE_KEY_*`, `IOS_TEAM_ID`
  - Android: `ANDROID_UPLOAD_KEYSTORE_*`, `ANDROID_UPLOAD_KEY_PASSWORD`, `ANDROID_UPLOAD_KEYSTORE_PASSWORD`
  - Generic: `SIGNING_PASSPHRASE_*` (naming convention per scope)
  - Any credential the bootstrap/CI needs is passed as env/CI secret by these names; no `.env` files committed; `.env*` gitignored.
- **CI secret scan:** a scanner (e.g., gitleaks/trune) runs on every push + PR; finds ⇒ build fails (SEC-11). `cargo-deny`/`cargo audit` include credential-ish patterns as denied.
- **Key rotation of identity (§2.5)** is a runtime product path, unrelated to build secrets; build secrets follow the release key management in `RELEASE_AND_SIGNING.md`.

---

## Appendix A. Security decisions NOT already in ADR-006 (for supervisor decision log)

The following concrete implementations were chosen here and are **not** recorded in ADR-006; please fold into `docs/orchestration/DECISION_LOG.md`:

1. **Exact Noise pattern + suite locked:** `Noise_XX_25519_ChaChaPoly_BLAKE2s` (XX pattern, X25519 DH, snow `ChaChaPoly` = XChaCha20-Poly1305, BLAKE2s hash, ed25519 long-term identity with deterministic ed25519→X25519 static conversion). ADR-006 named "Noise XX" only.
2. **Exact AEAD default:** **XChaCha20-Poly1305** (24-byte nonce = `random_salt16 ‖ counter8(BE)`), adopted as the single data-plane default; AES-256-GCM counter-nonce path (10-byte → 12-byte: `salt4 ‖ persisted_counter8`) retained only as a documented, non-default alternative, and marked **(measure in B0 spike)** before any use.
3. **0-RTT disabled entirely** (not just media): RFC 9221 forbids datagrams in 0-RTT; we disable early data for both control and media, tested by SEC-13. ADR-003/006 said "0-RTT disabled for media" — tightened to none.
4. **Correlation-ID redaction policy:** correlation IDs are **session-local**, retained in local logs, **stripped entirely from exported diagnostics** (replaced by per-export `export_id`). This resolves the "raw correlation strings" ambiguity in THREAT_MODEL/TEST_PLAN.
5. **Replay window concrete width:** DTLS-style sliding window of **256** entries, per (plane, direction), at 64-bit index — **(measure in B0 spike)** against the jitter reorder window.
6. **Key lifetime/renewal numbers:** rekey after **24 h** session age or **2^56** records per key scope (whichever first); AEAD-auth-failure threshold **3 / 60 s** triggers rekey→terminate.
7. **Log retention numbers:** 5 × 5 MiB rotating file set, **7-day** TTL; metrics ring 24 h in-memory; verbose diag tied to session end.
8. **Attempt caps/lockout defaults:** per-source-IP **10 fails/5 min → 600 s lockout**; per-identity **5 fails → 300 s lockout**; after 3 consecutive lockouts in 24 h, admin-level re-pair (also **(measure in B0 spike)**).
9. **Secure-storage tier reporting:** Linux adapter exposes an explicit tier (`tpm | secret-service | file-encrypted`) to keep marketing honest; file-encrypted fallback must not be described as hardware-backed.
10. **"Market-not-secure-until" named gate set:** SEC-01…SEC-13 enumerated in §7 gate marketing claims at B6 with zero critical/high findings.
