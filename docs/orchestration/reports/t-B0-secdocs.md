# t-B0-secdocs — Security Documentation

**task_id:** t-B0-secdocs
**root_task_id:** R-B0-SEC
**owner_role:** Security
**status:** complete
**date:** 2026-09-06

## Deliverable

- **`/home/shivam/ps/docs/planning/SECURITY_SPEC.md`** — created. Implementation-ready security specification expanding THREAT_MODEL into a concrete, numeric contract. No `TBD` values; uncertain hard-defaults are explicitly marked **(measure in B0 spike)**.

## Coverage against the 6 task bullets

1. **Threat model table** — §1 expands THREAT_MODEL's 8 controls into a 14-row asset/attacker/threat/control table (T1–T14), each row citing the spec section that implements the control.
2. **Pairing protocol spec** — §2: Noise XX locked (`Noise_XX_25519_ChaChaPoly_BLAKE2s`, snow); SAS/QR channel binding to the actual handshake pubkey; rejection/downgrade rules persisting `protocol_version` + `handshake_pattern`; key rotation requiring re-confirmation; "confirmation" FSM with atomic key+confirmed write; versioned signed map-style revocation records (chained `prev_sha`, not last-writer-wins) + offline QR propagation + "forget all".
3. **Crypto spec** — §3: XChaCha20-Poly1305 default (24B nonce) with counter-sourced 12B-nonce AES-GCM alternative backed by persisted monotonic counter; separate control/media keys (4 per session) fresh per session; DTLS-style 256-entry replay window; nonce management; key lifetime/renewal triggers (24 h / 2^56); **0-RTT explicitly disabled for both control and media**.
4. **Numeric security bounds** — §4 + §4.1: control ≤16 KB, frame ≤4 KB (enforced before allocation), rate ≤20/s, max handshakes 8, pairing window 60 s, per-IP + per-identity caps with lockout, TXT ≤512 B, record caps, cache TTL, QR ≤512 B.
5. **Log/diagnostic redaction spec** — §5: allow/deny lists; correlation IDs resolved as *session-local, retained in local logs, stripped from exports*; retention policy (5×5 MiB, 7-day TTL, rotation); machine-checkable export manifest.
6. **Secure storage adapter table** — §6: iOS Keychain, Android Keystore, Windows DPAPI/TPM(CNG), macOS Keychain, Linux TPM→Secret Service→file-encrypted with explicit honesty caveat + tier reporting.
7. **"Market-not-secure-until"** — §7: named SEC-01…SEC-13 acceptance tests gating marketing claims at B6, each referencing TEST_PLAN §Security.
8. **Secret-handling policy** — §8: no secrets in chat/logs/docs/code; env-var/CI secret names only; CI secret scanner.

## Appendix A (supervisor decision-log fodder)

Decisions made here that are NOT in ADR-006 (listed in SECURITY_SPEC §Appendix A for folding into `docs/orchestration/DECISION_LOG.md`): exact Noise suite, exact AEAD default + nonce format, 0-RTT disabled entirely, correlation-ID redaction policy, replay window width 256, key lifetime/renewal numbers, log retention numbers, attempt-cap/lockout defaults, storage tier reporting, SEC-01…13 gate set.

## Validation

- Section headings + cross-references reconciled after writing (numbering consistency check).
- `grep` confirms no `TBD` placeholder values.
- Files written are strictly within WRITE SCOPE: `docs/planning/SECURITY_SPEC.md` (deliverable) + `docs/orchestration/reports/t-B0-secdocs.md` (this report). No product code created.

## Commands run

- `ls`/`find` on `docs/orchestration/reports/` (confirmed empty before write)
- `grep -nE '^#{1,3} ' SECURITY_SPEC.md` (heading audit)
- `grep -n 'TBD' SECURITY_SPEC.md` (placeholder audit)
