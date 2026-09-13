# Getting started

The product concept, end to end:

> 1. Connect a portable DAC to a **receiver** (a phone/tablet).
> 2. Launch the **receiver**, choose the DAC as output, and become discoverable.
> 3. Pair an **emitter** (your computer) to the receiver.
> 4. Listen to the computer's live audio — lossy (Free) or lossless (Pro).

The reference system proves this whole loop over Wi-Fi (lossless FLAC
hash-perfect, clean and under 1% network loss; lossy Opus bounded). The app
shells implement each platform surface (capture, output routing, policy,
consent, status) and are being wired to the transport via the `FrameSink` seam.
Here is the journey as designed, plus what you can exercise today.

## The receiver journey (FR-050)

1. **Pick Receiver**, choose an **output route** (a USB DAC is detected where
   the OS exposes it — e.g. Android `TYPE_USB_DEVICE`, iOS `.usbAudio`, macOS
   CoreAudio transport) and a **buffer profile** (Low Latency / Balanced /
   Resilient).
2. **Become discoverable** and **approve pairing** when an emitter requests it.
3. The health panel (FR-053) shows: connection state, peer, active transport,
   codec, sample rate, bit depth, channels, estimated end-to-end latency, buffer
   fill, packet loss, underruns, output route, and **fidelity status**.
4. Fidelity is honest (FR-022): the panel says whether the output path is
   **converted**, **unverified**, or **bit-perfect** — bit-perfect is ONLY shown
   after a hardware loopback/USB-analyzer verification, never assumed.

> Today the in-app health values are demo/simulated until transport wiring
> lands; the reference CLIs (`ref_emitter`/`ref_receiver`) already report the
> real measured values over the real QUIC path.

## The emitter journey (FR-051)

1. **Pick Emitter**, choose the capture source (system output; per-app where the
   OS supports it, e.g. macOS process taps, Linux PipeWire node targeting).
2. **Discover a receiver** (local network) or enter its address manually (a QR
   fallback is specified for networks that block multicast).
3. Choose an **allowed quality mode** (Free → lossy only; Pro → + lossless;
   unknown tiers fail closed).
4. **Pair** (explicit confirmation / SAS / QR) and **start streaming**.
5. Permission requests are always **explained right before the OS prompt**
   (FR-052): Screen Recording (macOS), MediaProjection consent (Android),
   local network (iOS), etc.

## Live policy changes never lie (FR-026/FR-047)

- Switching **Pro → Free while a lossless stream is running asks for
  confirmation** (REQUIRES_CONFIRM) and never silently downgrades fidelity.
- The active mode is displayed continuously.

## What you can exercise today

- The **reference system** end-to-end (developers):
  ```bash
  source dev/env.sh
  cargo build -p wdr_refsim --release
  bash docker/soak.sh          # 60-min clean soak (WDR_SOAK_SECONDS=N shorter)
  ```
  Receiver completes with underruns=0/fatal=0 and the lossless hash preserved.
- The **shells** (see [Setup & install](setup-and-install.md)): the Android
  app's role picker walks through consent + policy + status; iOS source
  type-checks; macOS/Windows/Linux CLIs enumerate endpoints and report
  permission/capture state; the Linux BT receiver registers as an A2DP sink on
  a lab Linux box.

## The next milestone

Wire `FrameSink` / `AudioFrameSink` (the transport seam) into the app shells so
the reference streaming core drives the real apps end to end.
