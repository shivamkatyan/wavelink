# Real-Time Audio Contract (RT_CONTRACT)

Governing rule: a platform **RT audio callback** may only move bytes into/out of a
preallocated, lock-free SPSC ring (`wdr_rt::spsc::SpscRing`) and perform trivial
int/pointer/atomic work. Everything else — encode, decode, resample, encrypt,
transport, logging, allocation, locking — lives on dedicated worker threads.

Evidence grounding: `docs/orchestration/reports/t-B0-*.md` (what the codec adapters
actually spend CPU/allocation on), `docs/orchestration/reports/t-P1-bench.md`
(measured encode/decode per-frame cost that must NEVER run inside a callback).

## 1. Why this exists
P1 independent review (findings F9/F10) and ADR-002/ARCHITECTURE: the highest-risk
real-time issue is doing encode/decode or transport on a platform RT thread. A
block > one audio period is an audible dropout. This contract makes the allowed
work per callback mechanically enumerable and testable.

## 2. Platform RT callbacks and their allowed-operation whitelist

| Platform / API | RT entry point | Allowed (whitelist) | Forbidden |
|---|---|---|---|
| Windows WASAPI (capture+render) | `IAudioClient`/`IAudioCaptureClient::GetBuffer` processing loop, event-driven | `SpscRing::try_push`/`try_pop_exact`; atomic flags; pointer math | alloc (`Vec/Box/format!`), locks (Mutex), syscalls, I/O, logging, any codec/encrypt/transport call |
| macOS ScreenCaptureKit | `SCStreamOutput` audio sample handler (its callback thread is RT-ish) | **ONLY** `copy_nonoverlapping` into a caller-preallocated single-producer buffer + atomic fill/overflow stores (`backend::system_capture::PreallocatedCaptureBuffer::push_rt` — functionally the SPSC ring's producer with an atomic fill count), drained off-RT by a worker; **no** other copy | alloc (except nothing), locks (Mutex), syscalls, I/O, logging, any codec/encrypt/transport call; block on AVFoundation locks, do work, allocate per-sample |
| macOS CoreAudio taps / AU render block / HAL IO proc | `AudioUnitRender` / `AudioDeviceIOProc` | `try_pop_exact` into output buffers; atomic; pointer math | as above; note AU render block is strict RT |
| Linux PipeWire | `pw_stream::process()` with `PW_STREAM_FLAG_RT_PROCESS` | `try_push`/`try_pop_exact`; atomics; **no blocking** (explicitly documented as RT data thread) | alloc/locks/syscalls/logging; blocking `pw_*` calls |
| Android Oboe/AAudio | `AudioStreamCallback::onAudioReady` | `try_pop_exact` (render) / `try_push` (capture); atomics | alloc/locks/futex; Java/JNI calls; work > period |
| Android `AudioTrack` (push) | no opaque callback — a worker writes via blocking `write()` | worker thread only; RT discipline applies to Oboe path | — |
| iOS AVAudioEngine/AU | AU render block | `try_pop_exact` into IO buffers; atomics | ObjC/Swift messages, locks, alloc, work > period |

## 3. Thread handoff
- **Capture side:** RT capture callback = producer → `SpscRing.try_push`; a
  dedicated worker consumer drains the ring (`try_pop_exact`), then runs
  encode/encrypt/transport.
- **Render side:** a dedicated worker producer fills the ring (`try_push`) from
  decode/jitter-buffer; RT render callback = consumer → `SpscRing.try_pop_exact`.
- Ring is **lock-free SPSC**; buffer pool sized ≥ 2× worst-case in-flight (see
  ADR-007 sizing inequalities). `SpscRing::with_capacity` allocates once, off-RT.

## 4. Enforcement (not just prose)

### Implemented today (as of 0.0.1)
- **By construction:** the RT-side surface is `SpscRing::try_push` /
  `try_pop_exact` — only atomic loads/stores and byte copies on a slice
  allocated exactly once in `with_capacity` (off-RT). No allocation, locks,
  syscalls or blocking are expressible inside those calls, so a callback routed
  through the ring cannot violate the whitelist without explicitly invoking
  something outside it. The same holds for the macOS SCK row, whose RT handler
  only copies into `backend::system_capture::PreallocatedCaptureBuffer::push_rt`.
- **By contract + tests:** the allowed/forbidden tables above, plus
  `cargo test -p wdr_rt` SPSC stress tests (overflow returns full; no data race
  under a 2-thread producer/consumer hammer).

### Runtime enforcement layer — IMPLEMENTED 2026-09-13 (was RISK_REGISTER R16)
Land with `docs/orchestration/` updates:
- **`panic = "abort"`** — workspace `[profile.release] panic = "abort"` (Cargo
  allows `panic` only as a whole-profile option; dev/test profiles keep
  `unwind`). A panic on an RT thread is a process-level hard fault, never an
  unwind through a realtime callback.
- **`wdr_rt` `rt-guard` feature** — `guard.rs` installs a `#[global_allocator]`
  that forwards to `std::alloc::System`, except while a thread is inside an RT
  callback context (`RtGuard::enter()` — a nesting-aware RAII probe a platform
  callback brackets its body with); an allocation there aborts the process at the
  offending line (cold, never-allocating abort; `RT_DEPTH` is a const-initialized
  `#[thread_local]` so the allocator reads it safely mid-alloc). Out-of-RT
  allocation is completely unaffected. Subprocess-verified: a child that
  allocates inside RT context dies (SIGABRT); out-of-context allocation passes.
  RT-era binaries opt in via the feature; default builds ship zero overhead.
- **Remaining (still specified, NOT implemented):** `wdr_rt` as `#[no_std]` +
  deny-alloc/lint gating of the RT module surface, and CI watchdog/instrumentation
  (a watchdog thread that aborts on callback stall + an alloc detector) per
  TEST_PLAN — those stay on the native-RT evidence path (gate §5).

## 5. Native evidence gate
WSL2/Docker simulation **cannot** satisfy the native RT evidence gate. Linux RT
evidence comes only from a real PipeWire + RTKit run (RTKit scheduling confirmed,
no priority inversion, `pw_stream::process` stays under budget). WASAPI/SCK/Oboe/AU
RT evidence requires native runners or the hardware lab (HARDWARE_VALIDATION.md).
Simulated runs are labeled simulated and never substitute for the native gate.

## 6. What this project proves at each gate
- P1/B0: `wdr_rt` SPSC ring + contract doc + stress test (this gate).
- B2 (Win emitter + Android receiver): the WASAPI/Oboe adapters route through
  `SpscRing`; native-runner RT evidence recorded.
- B6: full hardware-lab RT validation (worst-case, not average) + callback
  stall/corrupted instrumentation; zero critical/high findings.

Risk row: see RISK_REGISTER R16 (RT callback alloc/lock regression) owner Audio,
gate B2 native runner + B6 lab.
