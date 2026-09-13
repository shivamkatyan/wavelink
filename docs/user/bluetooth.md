# Bluetooth

Bluetooth is handled with unusual honesty: **only the paths that public
platform APIs actually allow are implemented**; everything else is said so, out
loud, with a one-action alternative.

## The one BT path that works end to end

| Direction | Platform | Verdict |
|---|---|---|
| Receiver (this product listens + renders to own output/DAC) | **Linux** (BlueZ `a2dp_sink` + PipeWire media-sink) | ✅ **Supported** — `linux-receiver` |
| Emitter → ordinary BT speaker/headset | any | System behavior, not a product BT receiver |
| Receiver on a **stock phone** | Android / iOS / Windows / macOS | 🚫 **Unsupported by public API** — stock phones cannot act as A2DP sinks to third-party apps (verified in the platform matrix with evidence) |
| Custom product-peer (RFCOMM/L2CAP) | Android / Windows-RFCOMM / Linux | 🟡 low-bitrate **lossy fallback** cell (feature-gated) |

A Bluetooth cell only "counts" when the device **running this product receives**
the emitted audio and renders it to its selected output or DAC (FR-034).
Routing an emitter to an ordinary Bluetooth headset is useful OS behavior, but
it does **not** satisfy the product requirement.

## Why stock phones can't be receivers

- **Android**: the A2DP *sink* service (`BluetoothA2dpSink`) is hidden/removed
  from third-party apps (verified unavailable); LE-Audio generic receive isn't
  exposed to apps either.
- **iOS**: there is no public A2DP sink API; LE-Audio receive is for hearing
  devices only.

These are documented facts (see `docs/planning/PLATFORM_MATRIX.md` §B and
`ADR-009.md`), not something a "Pro" plan can unlock.

## What the apps do about it

- The UI shows an explicit Bluetooth **support matrix** and never implies a
  stock phone can be a receiver.
- Unsupported cells always offer a **one-action path back to free lossy Wi-Fi**
  (FR-033/034) — the reliable way to get audio from an emitter to a phone.
- Custom RFCOMM/L2CAP product-peer links are described as **low-bitrate lossy**
  transport (never "hi-fi"), gated behind the lab (`bt-lab`).

## Get it working (Linux)

1. Install `linux-receiver` on a Linux box with BlueZ + PipeWire.
2. Register the A2DP sink profile; the box becomes discoverable as a BT
   audio receiver.
3. Render to the selected output / USB DAC. Native BlueZ/PipeWire behavior is
   validated in the Bluetooth lab (`bt-lab` gate; runbook:
   `HARDWARE_VALIDATION.md → Bluetooth`).
