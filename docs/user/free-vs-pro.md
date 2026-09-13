# Free vs Pro

The Free/Pro boundary is a **development placeholder** for a future commerce
system — but it exercises the *real* feature-policy boundary today.

## What each tier allows

| Feature | Free | Pro |
|---|---|---|
| Wi-Fi **lossy** streaming (Opus) | ✅ | ✅ |
| Call-compatible Bluetooth modes | ✅ | ✅ |
| Wi-Fi **lossless** streaming (FLAC / raw PCM, hash-perfect) | ❌ | ✅ |
| Bit-perfect output indication | only ever with hardware verification | only ever with hardware verification |

Policy rules are centralized in one place (`wdr_entitlement` → `Policy`), not
scattered in each app, so every shell enforces the same boundary:

- **Unknown/unresolved tiers fail closed** → treated as Free (never silently Pro).
- **Free never receives lossless** — the gate refuses it up front.
- **Live change Pro→Free mid-lossless asks for confirmation** and never changes
  fidelity silently (FR-047).
- Policy is the **intersection of both peers'** policies — if the peer is Free,
  the session is Free, and the mismatch is visible (FR-046).

## The toggle is NOT security

Every app shell carries a **Free/Pro toggle at the top of the main screen**
(FR-040/048). This is a **development and product-demonstration switch**, not a
tamper-resistant entitlement system (FR-045). Do not rely on it to protect
paid features in production — that is a deliberately deferred commerce
project with a documented adapter boundary (`CommerceEntitlementBackend`).

## Honest fidelity labels

Lossless ≠ bit-perfect:

- `Lossless` means decoded PCM is sample-identical to the source *for every
  delivered frame* (proven by hash-equality tests).
- `Output path converted` / `unverified` / `bit-perfect` describe the *render*
  path — **bit-perfect is only claimed after a hardware loopback or USB-analyzer
  measurement** (ADR-005); a route name or negotiated sample rate is never
  treated as proof.
