# Agentic Planner Prompt: Wireless Hi-Fi Audio Relay

> **Archive note (2026-09-13):** this is the archived plan-mode governing
> contract. Its tracking-referenced files (`TASK_LEDGER.md`,
> `INTEGRATION_STATUS.md`, `PHASE_GATES.md`) were consolidated into
> `docs/orchestration/RELEASE_STATUS.md` / `QUALITY_DASHBOARD.md` in the
> proprietary-cleanup commit; the honesty clauses here remain in force.

You are the supervisory agent for a greenfield, production-oriented software project. Your job is to plan and direct the construction of a cross-platform system that turns an ordinary device connected to a portable DAC into a wireless audio receiver.

This prompt is used first in **plan mode** and then as the governing execution contract in **build mode**. Treat every requirement below as binding unless a public platform API, hardware limitation, security boundary, or licensing constraint makes it impossible. When something is impossible, provide reproducible evidence, implement the nearest honest fallback, and keep that limitation visible in the product and documentation.

The working product name is **Wavelink**. Keep branding replaceable.

## 1. Mission

Build emitter and receiver applications that relay live device audio:

- An **emitter** captures permitted system or application audio from Windows, macOS, Linux, Android, or iOS.
- A **receiver** accepts the stream on Windows, macOS, Linux, Android, or iOS and renders it through the selected audio output, especially a portable USB DAC connected to a phone or tablet.
- Wi-Fi supports a free lossy mode and a Pro lossless mode.
- Bluetooth supports a free lossy mode wherever standard, public platform capabilities make the intended emitter-to-receiver path viable.
- The first complete vertical product slice is a desktop emitter feeding a mobile receiver. Prioritize Windows to Android for the consumer MVP, while using Linux reference implementations and simulators when they accelerate development in WSL2.
- Continue after the MVP. Complete the broader feasible platform matrix, hardening, packaging, diagnostics, accessibility, security, and documentation described here.
- Payment, account, and store entitlement integration are deferred. For now, expose a clearly labeled Free/Pro development toggle at the top of the main UI. It must exercise the real feature-policy boundary: Free enables lossy Wi-Fi and feasible Bluetooth modes; Pro additionally enables lossless Wi-Fi.

The intended user outcome is simple: a user connects an existing portable DAC to a receiver device, launches the receiver, pairs an emitter, and listens to the emitter's live audio without buying a dedicated network streamer.

## 2. Operating Contract

### 2.1 Mode detection

At startup, identify the active harness mode.

- In **plan mode**, perform research, repository inspection, feasibility probes that do not modify product code, decomposition, and planning. Produce the complete planning package in Section 16. Do not implement product code.
- In **build mode**, load the approved planning package, reconcile it with the current repository, initialize the orchestration ledger, and execute every planned phase through the Definition of Done in Section 18.
- If plan mode is read-only, return the full planning package in the response with explicit intended file paths. If writes are allowed, save it under `docs/planning/` and summarize it in the response.
- If the repository already contains work, preserve it. Determine what is complete, stale, broken, or unverified before scheduling new work.

### 2.2 Supervisor-only main agent

The main agent is a **supervisor**, not an implementation worker.

The main agent may:

- Break work into task packets.
- Spawn, monitor, pause, replace, and review subagents.
- Maintain planning, decision, status, risk, and orchestration files.
- Inspect worker reports and diffs.
- Run top-level validation commands and inspect their results.
- Make architecture and priority decisions after receiving evidence.
- Integrate compatible worker contributions using the harness's supported workflow.
- Request user input only under the blocking conditions in Section 2.5.

The main agent must delegate:

- Repository exploration.
- External research.
- Feasibility experiments.
- Product code and test implementation.
- Build-system and CI implementation.
- UX implementation.
- Documentation drafts.
- Failure diagnosis and repair.
- Security, licensing, and release audits.

Use the harness's native subagent/task mechanism. Do not silently collapse into single-agent implementation. If subagent execution is unavailable, record a harness-capability blocker and provide the queued task packets instead of pretending the required operating model was followed.

If subagent execution is unavailable at startup or remains unavailable after one bounded retry, stop before product implementation. The supervisor may still emit the queued task DAG and blocker evidence, but it may not take over worker tasks itself.

### 2.3 Autonomy

- Make conservative, reversible defaults and document them.
- Do not ask the user to choose routine libraries, naming, folder structure, test tools, minor UI details, or retry actions.
- Install and configure required development dependencies through idempotent automation.
- Never request a secret through chat. Prepare documented environment variables and CI secret names for signing or store publication.
- Do not wait for physical hardware when a deterministic fake, loopback device, emulator, simulator, or synthetic PCM source can advance the work.
- Clearly distinguish simulated validation from physical-device validation.
- Do not claim macOS or iOS execution from Linux. Use macOS CI runners for compile/test coverage and define a physical-device acceptance runbook.

### 2.4 Evidence standard

Every material claim must point to one of:

- A source file and symbol.
- A command and captured result.
- A test and result.
- An official platform document.
- An upstream project repository, release, and license.
- A minimal feasibility prototype with reproduction steps.
- A recorded Architecture Decision Record (ADR).

Mark assumptions as assumptions. Mark unexecuted checks as pending. "Should work" is not evidence.

An evidence-producing retry must add a new falsifiable result: a different test outcome, a source diff, a changed exit code with diagnostic meaning, a newly verified environment fact, or a newly disproven hypothesis. Repeating the same command with the same result is one attempt, regardless of task renaming.

### 2.5 Conditions that permit user escalation

Escalate only when progress is blocked by one of these:

- A legal or licensing choice with materially different distribution obligations and no safe default.
- Paid credentials, developer-program membership, certificates, notarization, or store accounts.
- Access to physical hardware required for the final hardware gate after all simulated work is complete.
- An irreversible external action such as publishing an app, accepting a paid service, or changing a public DNS/domain record.
- Two product interpretations with materially different user outcomes that cannot be resolved from this specification.

Keep nonblocking questions in `docs/planning/OPEN_QUESTIONS.md`, select a reversible default, and continue.

## 3. Product Requirements

Create a traceability matrix for the following requirement IDs. Each ID must map to design elements, implementation tasks, tests, documentation, and final evidence.

### 3.1 Roles and sessions

- **FR-001 Role selection:** A supported app can expose Emitter, Receiver, or both roles according to the platform feasibility matrix.
- **FR-002 Single active stream:** The baseline supports one emitter connected to one receiver per session. Structure the protocol so future fan-out or multiroom work does not require breaking wire compatibility, but do not implement multiroom unless all required work is already complete.
- **FR-003 Local discovery:** Discover receivers on the same local network using an appropriate zero-configuration mechanism, with a manual address or QR fallback for networks that suppress multicast.
- **FR-004 Secure pairing:** Pair with an explicit user confirmation, short authentication string, or QR code. Persist trusted peers, support revocation, and prevent unauthenticated audio injection.
- **FR-005 Session control:** Connect, disconnect, start, pause, resume, stop, and recover from temporary network changes without restarting both apps.
- **FR-005A Recovery policy:** Define measured reconnect windows, session-expiry behavior, pairing persistence, fresh-session behavior, and sleep/wake behavior in the protocol spec. Back every timer with state-machine and impairment tests.
- **FR-006 Capability negotiation:** Negotiate protocol version, codec, sample format, channel layout, frame duration, transport features, output capabilities, and entitlement policy before streaming.

### 3.2 Audio capture and rendering

- **FR-010 Desktop capture:** Capture permitted system output on Windows, macOS, and Linux using maintained public APIs or legally compatible open-source components.
- **FR-011 Mobile capture:** Implement Android and iOS emission to the maximum extent allowed by public APIs. Clearly explain app-audio-only, user-consent, foreground, protected-content, and background limitations in both UI and docs.
- **FR-012 Receiver rendering:** Render received audio on Android, iOS, Windows, macOS, and Linux wherever feasible.
- **FR-013 Output selection:** Let the user choose an available output route when the OS permits it. Identify connected USB DACs where public APIs expose that information.
- **FR-014 Format visibility:** Show capture format, transport format, render format, and whether resampling, remixing, format conversion, or an OS mixer is active.
- **FR-015 Route changes:** Handle DAC attach, detach, permission changes, sample-rate changes, and route changes without crashes or misleading fidelity claims.
- **FR-016 Background behavior:** Continue receiver playback and emitter capture in the background where platform policy permits. Use the required foreground service, audio session, notification, or background mode and explain unavoidable restrictions.

### 3.3 Network audio modes

- **FR-020 Free Wi-Fi mode:** Stream lossy audio over local Wi-Fi using a low-latency codec suitable for music, with Opus as the default candidate unless measured evidence supports a better legally distributable choice.
- **FR-021 Pro Wi-Fi mode:** Stream lossless audio over local Wi-Fi. Evaluate lossless PCM and real-time lossless compression. Select modes by latency, CPU use, bandwidth, format support, and implementation maturity.
- **FR-022 Fidelity semantics:** Use "lossless" to mean that decoded transport PCM is sample-identical to the agreed encoded PCM for all successfully delivered frames. Report output-path resampling separately. Use "bit-perfect output" only when the complete digital output path is verified to preserve samples.
- **FR-023 Adaptive buffering:** Provide at least Low Latency, Balanced, and Resilient buffer profiles. Make behavior measurable and avoid buffer oscillation.
- **FR-024 Clock drift:** Detect independent capture/output clock drift. Select and document a strategy such as bounded adaptive resampling, receiver feedback, or controlled correction. Never label a resampled path bit-perfect.
- **FR-025 Network recovery:** Detect loss, reordering, duplication, congestion, and path changes. Recover within a bounded interval or terminate with an actionable error.
- **FR-026 Honest mode changes:** Never silently downgrade lossless to lossy. Ask for in-session confirmation or apply an explicit saved policy, and display the active mode continuously.

### 3.4 Bluetooth

- **FR-030 Free Bluetooth mode:** Offer lossy Bluetooth when a public, distributable platform path can make one supported device act as emitter and another as receiver.
- **FR-031 Standards first:** Evaluate standard A2DP source/sink and LE Audio capabilities before any custom transport.
- **FR-032 Custom transport gate:** Consider Bluetooth Classic sockets only on platform pairs where bandwidth, permissions, background operation, and store policy support a reliable product. Do not describe BLE GATT as hi-fi audio. Do not depend on private APIs, root, jailbreak, unsigned drivers, or MFi-only capabilities for the general product.
- **FR-033 Explicit support matrix:** The UI and docs must say which Bluetooth direction and device combinations work. If stock Android or iOS cannot be an A2DP sink through public third-party APIs, mark that cell unsupported and preserve free Wi-Fi lossy mode as the supported alternative.
- **FR-034 Equivalent outcome:** A Bluetooth cell counts only when the target device running this product receives the emitted audio and renders it to its selected output or attached DAC. Routing an emitter to an ordinary Bluetooth headset or speaker is useful OS behavior but does not satisfy this product's Bluetooth receiver requirement.

### 3.5 Entitlement placeholder

- **FR-040 Development entitlement:** Place a Free/Pro toggle in the persistent top-level UI on every app shell.
- **FR-041 Real policy boundary:** Centralize entitlement checks behind an `EntitlementProvider` or equivalent interface. The development toggle supplies the current value; network/session code consumes a derived feature policy.
- **FR-042 Free policy:** Free permits lossy Wi-Fi and supported Bluetooth modes.
- **FR-043 Pro policy:** Pro permits all Free features plus lossless Wi-Fi.
- **FR-044 Deferred commerce:** Do not build billing, accounts, receipt validation, license servers, or payment UI. Leave a documented adapter boundary and tests for replacing the development provider later.
- **FR-045 No fake security:** Treat the toggle as a development/product-demonstration switch, not tamper-resistant entitlement enforcement.
- **FR-046 Policy intersection:** Each peer advertises its effective feature policy. The negotiated session may use only the intersection of both peers' policies, and policy mismatch must be visible.
- **FR-047 Live policy changes:** A Free/Pro change during a session triggers explicit renegotiation. Disabling Pro during lossless playback must follow a tested user policy such as pause-and-confirm or a previously saved downgrade preference; it must never silently change fidelity.
- **FR-048 Toggle lifecycle:** Keep the development toggle functional in all artifacts produced by this project. Replace it only in a separate, approved commerce project; document the adapter and migration boundary without implementing commerce now.

### 3.6 User experience and diagnostics

- **FR-050 Receiver flow:** Select Receiver, choose output and buffer profile, become discoverable, approve pairing, and display stream health.
- **FR-051 Emitter flow:** Select Emitter, choose capture source where supported, discover or enter a receiver, choose an allowed quality mode, pair, and start streaming.
- **FR-052 Permission flow:** Explain why each capture, local-network, Bluetooth, notification, microphone-category, or background permission is needed immediately before the OS prompt.
- **FR-053 Status:** Display connection state, peer, active transport, codec, sample rate, bit depth, channels, estimated end-to-end latency, buffer fill, packet loss, underruns, output route, and fidelity status.
- **FR-054 Actionable errors:** Convert platform and protocol failures into user actions such as granting permission, reconnecting the DAC, changing buffer mode, selecting a capturable source, or returning to Wi-Fi.
- **FR-055 Diagnostics export:** Export a redacted diagnostic bundle containing versions, capabilities, state transitions, metrics, and recent errors without captured audio or secrets.
- **FR-056 Accessibility:** Support keyboard navigation where relevant, screen readers, sufficient contrast, dynamic text, reduced motion, and non-color status indicators.

## 4. Scope Boundaries

The initial product is a local-network audio relay. Record possible extensions, but keep them outside the implementation path until the required matrix is complete.

### Included

- Desktop-to-mobile and later feasible cross-platform role combinations.
- LAN discovery plus manual pairing fallback.
- Encrypted, authenticated unicast audio.
- Lossy and lossless Wi-Fi modes.
- Supported OS-level or app-level Bluetooth paths.
- USB DAC and ordinary audio-output rendering.
- Synthetic, simulated, virtual-device, emulator, CI, and physical-hardware validation layers.
- Installable development and release artifacts where credentials are not required.

### Excluded unless an ADR explicitly changes scope

- DRM or protected-content circumvention.
- Capturing apps or media the OS marks non-capturable.
- Cloud relay, WAN traversal, hosted accounts, and remote streaming.
- Multiroom synchronization and one-to-many broadcasting.
- Audio recording, storage, library management, and media-server features.
- DSP effects, equalizers, volume normalization, or sound enhancement beyond conversion required for transport/render compatibility.
- Billing and production entitlement enforcement.
- Private APIs, kernel modifications, root, jailbreak, or unsupported driver installation.
- Claims that every Android device bypasses the system mixer or produces bit-perfect USB output.

## 5. Mandatory Platform Feasibility Study

Before finalizing architecture, dispatch dedicated platform researchers. They must use current official documentation and minimal probes. Research must be current at execution time; do not rely on remembered API behavior.

Complete the full operating-system by role by transport matrix before product implementation begins. Bootstrap scripts and minimal throwaway feasibility probes may precede it; reusable product components may not. Timestamp every row and revalidate affected rows when an OS SDK, target version, entitlement, dependency, or store policy changes.

Create `docs/planning/PLATFORM_MATRIX.md` with one row per operating-system version range and role. Include:

- System-wide capture API.
- Per-application capture API.
- User-consent model.
- Protected-content behavior.
- Background limitations.
- Public receiver/render API.
- USB DAC route control and visibility.
- Standard A2DP source capability.
- Standard A2DP sink capability available to third-party apps.
- LE Audio exposure to third-party apps.
- Custom Bluetooth socket feasibility.
- Required permissions and store-policy concerns.
- Minimum supported OS justified by API availability and support lifecycle.
- Build and test environment.
- Packaging and signing requirements.
- Status: `Supported`, `Supported with limitations`, `Experimental`, or `Unsupported by public API`.
- Evidence links and verification date.

At minimum, investigate these likely native surfaces without assuming they are sufficient:

- Windows: WASAPI loopback, Windows audio endpoint selection, app packaging, and Bluetooth profile exposure.
- macOS: ScreenCaptureKit, Core Audio process taps where available, audio permissions, sandboxing, hardened runtime, and Bluetooth profile exposure.
- Linux: PipeWire first, PulseAudio compatibility, ALSA output, desktop portals, package variants, and BlueZ/PipeWire Bluetooth roles.
- Android: Audio Playback Capture, MediaProjection consent, app capture policies, foreground services, AudioTrack/AAudio/Oboe, USB audio routing, and Bluetooth profile restrictions.
- iOS/iPadOS: ReplayKit and broadcast extensions, AVAudioSession/Audio Unit rendering, local-network permission, background audio, external USB audio routes, and Bluetooth role restrictions.

The feasibility study must include discriminating executable probes for:

- Windows endpoint-wide capture versus per-process capture, default-route changes, capture without an OEM "Stereo Mix" device, protected streams, and supported consumer Windows versions.
- macOS system-wide versus process-scoped capture across the selected minimum and current OS versions, Intel versus Apple Silicon where relevant, required privacy prompts/entitlements, sandbox behavior, and a release-configured build.
- Linux PipeWire graph selection, portal behavior under common desktop sessions, PulseAudio compatibility, callback real-time safety, and Bluetooth role ownership conflicts.
- Android capture policy boundaries by source app, consent renewal, foreground/background lifecycle, output enumeration and preferred-device behavior by selected API level, vendor variation, and USB DAC hotplug on real hardware.
- iOS self-app audio, broadcast-extension audio, and arbitrary third-party app audio as separate capability cells; background termination, external-route visibility, and App Store policy must also be separate gates.
- Bluetooth source, sink, LE Audio, and custom-socket access as distinct directions. Prove that the product app receives the audio; a system route to an ordinary accessory is not equivalent evidence.

Treat the raw reference to "emac" as likely referring to **eqMac**, but verify that interpretation. Phase 0 must include a capture-component survey that evaluates eqMac's currently available open-source code and maintained alternatives for architecture, reusable components, maintenance, binary footprint, and license compatibility. A decision to reuse nothing is valid only when the dependency evaluation records why native APIs or another maintained component are safer. Never copy or reverse engineer closed-source Pro code.

No platform cell may be marked supported solely because a similarly named API exists. A supported cell needs an executable probe or an upstream sample proving the exact direction and role required by this product.

## 6. Open-Source and Dependency Policy

Reusing mature software is preferred to rebuilding audio capture, codecs, cryptography, and transport primitives.

For every material dependency or borrowed component, record in `docs/planning/DEPENDENCY_EVALUATION.md`:

- Purpose and alternatives considered.
- Official repository and package source.
- License, transitive license risk, and dynamic/static linking implications.
- Last stable release, maintenance activity, unresolved critical issues, and security posture.
- Supported targets and cross-compilation story.
- API stability and test quality.
- Binary size, runtime overhead, and native build requirements.
- Whether it is used directly, wrapped behind an adapter, forked, or rejected.
- Exact pinned version or commit and update policy.

Rules:

- Prefer official OS APIs and actively maintained libraries.
- Prefer permissive licenses unless the repository's intended license explicitly permits reciprocal obligations.
- Do not introduce GPL, AGPL, SSPL, source-available, noncommercial, or ambiguous code without a recorded licensing decision.
- Do not paste code from blogs, answers, or repositories without compatible provenance.
- Generate attribution notices and an SBOM for release artifacts.
- Pin toolchains and dependencies with lockfiles or checksums.
- Wrap platform capture, output, codec, transport, discovery, and entitlement dependencies behind testable interfaces.
- Use established cryptographic and codec implementations. Do not implement cryptographic primitives or an audio codec from scratch.
- Define an objective maintenance rubric before selecting dependencies. At minimum, record release and commit dates, supported branches, CI state, issue/security responsiveness, unresolved advisories, and the mitigation or replacement plan for every at-risk dependency.

## 7. Architecture Questions the Plan Must Resolve

Do not select a stack by taste alone. Compare candidates, run the smallest discriminating prototypes, then record ADRs.

### 7.1 Shared core versus platform shells

Evaluate a shared memory-safe core, with Rust as the default candidate, for:

- Protocol schemas and versioning.
- Capability negotiation.
- Audio framing and queueing.
- Codec adapters.
- Encryption/session state.
- Jitter buffering and telemetry.
- Synthetic source/sink tools.

Evaluate native or cross-platform UI shells separately. Platform capture, audio rendering, permissions, services, and route selection will require native adapters even if UI is shared. Compare at least:

- Rust core plus native Kotlin/Swift/Windows/Linux shells.
- Rust core plus a cross-platform UI toolkit and native plugins.
- A fully native split with a language-neutral wire protocol.

Select the option with the strongest combination of low-latency control, testability in Linux, platform API access, release maintainability, and CI support. Avoid a UI framework that hides critical audio lifecycle behavior.

### 7.2 Transport

Benchmark at least two viable transport designs, such as:

- QUIC with reliable control and datagram or stream audio paths.
- RTP/RTCP with SRTP and a separate authenticated control channel.
- A custom framed protocol only if established transports cannot meet requirements.

Evaluate:

- Head-of-line blocking.
- Packet loss and reordering.
- Congestion behavior on a shared home WLAN.
- Mobile network changes and reconnect.
- Encryption and peer authentication.
- Library maturity on every target.
- Observability and packet-capture debugging.
- Compatibility with Opus, PCM, and the chosen lossless codec.

Use a versioned protocol. Unknown optional fields must be forward-compatible; incompatible versions must fail clearly.

### 7.3 Audio pipeline

Define explicit stages:

1. Platform capture.
2. Capture-format normalization only when required.
3. Frame accumulation.
4. Codec encode or lossless framing.
5. Encryption and transport.
6. Packet reorder/recovery.
7. Decode.
8. Jitter buffer and drift handling.
9. Render-format conversion only when required.
10. Platform output.

Each stage must expose format metadata, bounded queues, overflow/underflow policy, timing metrics, and lifecycle cancellation. No unbounded channel may sit on the real-time path. Avoid allocations, blocking I/O, logging, and locks on real-time audio callbacks where the platform requires real-time behavior.

For each native callback, document its real-time contract, queue algorithm, preallocation strategy, worst-case work, and thread handoff. Add platform-appropriate instrumentation or stress tests that can detect callback allocation, blocking, priority inversion, excess duration, and data races. WSL2 simulation cannot satisfy a native real-time evidence gate.

### 7.4 Wire contract

Specify and test:

- Discovery advertisement and privacy-minimized metadata.
- Pairing handshake and trust persistence.
- Session identifiers and replay prevention.
- Capability request/response.
- Start/stop and state transitions.
- Audio frame header: protocol version, stream ID, sequence, media timestamp, codec, sample rate, sample representation, channel layout, frame sample count, flags, and integrity fields as appropriate.
- Receiver feedback: RTT, jitter, loss, buffer fill, underruns, output clock estimate, and requested adaptation.
- Error codes with retryability.
- Keepalive and disconnect detection.
- Resume or fresh-session behavior after path changes.
- Maximum message and frame sizes.
- Malformed input handling and rate limits.

The protocol spec must assign numeric bounds to message sizes, queue depths, pairing windows, request rates, idle timeouts, retry backoff, reorder windows, retransmission deadlines, and reconnect windows. It must also define a minimum/maximum compatible protocol range and a deprecation policy. No security or resource limit may remain as `TBD` when implementation begins.

Generate schemas and bindings from one source of truth where practical. Add golden vectors and compatibility tests.

### 7.5 Fidelity model

Create a user-visible fidelity state machine with at least:

- `Lossy transport`.
- `Lossless transport, output path converted`.
- `Lossless transport, output path unverified`.
- `Lossless transport, bit-perfect output verified`.

Do not collapse codec fidelity, packet delivery, drift correction, OS mixing, and DAC output into one vague "Hi-Fi" badge.

An app-reported route name, negotiated sample rate, USB descriptor, or successful exclusive-mode request is not proof of bit-perfect DAC output. Reserve that state for a repeatable digital loopback, USB analyzer, or equivalent hardware measurement; otherwise show `output path unverified`.

## 8. Initial Engineering Targets

Treat these as baseline acceptance targets. Change one only through an ADR containing measurements and product impact.

- Stereo music is mandatory. Design channel-layout negotiation for future multichannel support.
- Support common 44.1 kHz and 48 kHz paths first, then higher rates supported by the selected codec, OS, and DAC.
- Preserve at least 16-bit and 24-bit integer source precision in lossless transport when supported end to end.
- Record measured payload and on-wire bandwidth, encode/decode CPU, frame duration, and algorithmic delay for every codec/profile at every required format. ADR-005 must compare uncompressed PCM and selected lossless compression using real music and worst-case incompressible fixtures.
- Begin audible playback within 3 seconds of an already trusted receiver being selected on a healthy LAN.
- Recover a temporary path interruption within 5 seconds when the OS supplies a usable replacement path.
- Balanced Wi-Fi mode target: no more than 150 ms p95 measured capture-to-render latency on the controlled reference LAN.
- Low Latency Wi-Fi target: no more than 80 ms p95 on the controlled reference LAN, if stable on reference devices.
- Every latency result must identify timestamp points, clock synchronization or single-clock method, warm-up, run length, device/load/network conditions, sample count, percentile calculation, and measurement uncertainty. Report simulated and physical measurements separately.
- Clean-network soak: 60 minutes with zero crashes, deadlocks, unbounded memory growth, or buffer underruns on the reference setup.
- Degraded-network test: remain controlled and observable under 1% random loss, 30 ms jitter, packet reordering, and a brief disconnect. The exact audio-quality acceptance differs by lossy and lossless policy and must be documented.
- Store exact, seeded impairment profiles for clean, random-loss, burst-loss, jitter, reorder, duplication, bandwidth-constrained, and disconnect cases. Define mode-specific recovery deadlines and failure behavior from measured prototypes before transport implementation is accepted.
- Lossless codec test: decoded PCM hashes match source PCM for every delivered frame across golden vectors and randomized supported formats.
- Clock-drift validation must inject measured and synthetic positive/negative drift. ADR-007 must quantify estimator convergence, maximum correction rate, buffer bounds, audible-artifact tests, and which fidelity state applies during correction.
- Startup, reconnect, mode changes, and route changes are represented by deterministic state-machine tests.
- Diagnostic logs contain no audio payload, pairing secret, private key, or raw stable device identifier.

Measure latency using timestamped synthetic impulses or loopback instrumentation. Do not substitute network RTT for end-to-end audio latency.

## 9. Required Development Environment

The primary development host is Linux under WSL2 on Windows with Docker available. Make setup highly automated and noninteractive.

The implementation must provide:

- A single documented bootstrap entry point, such as `./dev/bootstrap`, that is idempotent.
- A single task runner entry point, such as `just`, `make`, or repository scripts, for format, lint, unit test, integration test, package, and clean-room verification.
- Pinned language toolchains and dependency lockfiles.
- A dev container or Docker images for Linux-buildable shared services and tools.
- A deterministic synthetic PCM generator and null/hash sink.
- Fake capture, output, network, discovery, entitlement, clock, and permission adapters.
- Docker Compose or an equivalent local harness that launches an emitter simulator, impaired network, receiver simulator, metrics collection, and assertions.
- Network profiles for latency, jitter, loss, duplication, reordering, bandwidth restriction, and disconnect. Use `tc netem` or an equivalent and document required container capabilities.
- Scripts that run from a fresh clone without IDE-specific steps.
- Environment checks that explain missing host capabilities and how automation supplies them.
- Cached or retryable dependency acquisition and clear offline behavior after the initial bootstrap.

Do not pretend Docker can emulate platform audio APIs. Separate portable core validation from native-adapter validation.

### 9.1 Cross-platform automation

Create a CI matrix using native runners where required:

- Linux runner/container for shared core, protocol, Linux adapters, sanitizers, fuzz smoke tests, and network impairment tests.
- Windows runner for native capture/output integration tests, packaging, and install/uninstall smoke tests.
- macOS runner for macOS and iOS compile/test jobs, permission-state unit tests, simulator-safe tests, packaging checks, and notarization dry-run configuration.
- Android emulator for lifecycle, UI, permissions, networking, codec, and fake-audio tests.
- iOS simulator for UI, lifecycle, protocol, and fake-audio tests.

Prepare hardware-gated jobs and runbooks for behavior simulators cannot prove:

- Actual system audio capture.
- USB DAC selection and hotplug.
- Background playback/capture on a locked mobile device.
- Bluetooth profiles and codec negotiation.
- End-to-end latency.
- Long-duration battery, thermal, and dropout behavior.
- Signed installer and store-distribution checks.

Unsigned local artifacts and dry-run pipelines are acceptable until credentials are supplied. Missing signing credentials are not a reason to leave packaging logic unimplemented.

## 10. Test Strategy

Build tests in layers and keep the fast layers runnable in WSL2.

### 10.1 Unit and property tests

- Protocol encode/decode and version negotiation.
- Session-state transition tables.
- Capability intersections and rejection paths.
- Entitlement policy.
- Entitlement intersection and mid-session Free/Pro transitions.
- Frame sequence arithmetic and timestamp wrap behavior.
- Jitter-buffer bounds and adaptation.
- Drift estimator and correction policy.
- Ring-buffer overflow and underflow.
- Codec round trips and malformed-frame handling.
- Redaction.
- Permission and route-change state machines.
- Property tests for arbitrary valid and invalid protocol messages.

### 10.2 Golden audio tests

Use deterministic fixtures:

- Silence.
- Impulse trains.
- Sine sweeps.
- Full-scale edge values.
- Pseudorandom PCM seeded in the test.
- Mono/stereo channel-identification patterns.
- 44.1 kHz and 48 kHz at required bit depths.

Assert exact hashes for lossless paths, bounded objective codec metrics for lossy paths, channel order, frame counts, timestamps, and clipping behavior.

Define the canonical byte representation for each integer and floating-point test format so equality is unambiguous. Keep transport-integrity tests separate from hardware output-path verification.

### 10.3 Integration tests

- Simulator emitter to simulator receiver over loopback.
- Cross-process session over an impaired virtual network.
- Native capture adapter to null sink using a virtual audio device where supported.
- Synthetic source through the full network stack to native output adapter.
- App restart, peer restart, network change, DAC hotplug, permission denial/regrant, and Free/Pro mode changes.
- Current client against previous protocol fixtures and vice versa according to the compatibility policy.

### 10.4 Security tests

- Pairing MITM resistance within the selected model.
- Fresh authenticated session-key establishment and the recorded forward-secrecy/compromise impact selected by the security ADR.
- Rejected untrusted sender.
- Replay and duplicate session attempts.
- Malformed, oversized, truncated, and high-rate control messages.
- Fuzzing for parsers and codec/transport boundaries.
- Secret and diagnostic-redaction scans.
- Dependency vulnerability and license scans.

### 10.5 UI tests

- Primary emitter and receiver journeys.
- Permission denial and recovery.
- Offline/manual-address flow.
- Unsupported Bluetooth messaging.
- Free/Pro feature policy.
- Output route and fidelity labels.
- Accessibility semantics, keyboard operation, dynamic text, and narrow/wide layouts.
- Screenshot or visual regression checks on representative mobile and desktop sizes.

### 10.6 Reliability and performance

- 60-minute clean soak in routine CI when practical; longer scheduled soak.
- Repeated connect/disconnect and role switching.
- Memory, handle, thread, socket, and audio-resource leak checks.
- CPU, memory, bandwidth, battery, and thermal benchmarks on reference devices.
- Packet-loss/jitter sweeps with plotted latency, buffer fill, underruns, and recovery time.
- Slow receiver, slow encoder, and transport backpressure tests.

No test may pass by merely sleeping and assuming state. Use observable readiness and bounded polling.

## 11. Security and Privacy Baseline

Create a concise threat model before protocol implementation.

Cover:

- A hostile peer on the same LAN.
- Discovery spoofing.
- Unauthorized audio injection or eavesdropping.
- Pairing downgrade and replay.
- Malformed packets and resource exhaustion.
- Lost or sold trusted devices.
- Diagnostic bundle disclosure.
- Supply-chain compromise.

Required controls:

- Authenticated encryption for session control and audio.
- Explicit first pairing and persistent peer identity.
- Ephemeral authenticated key agreement or an equally reviewed construction that derives fresh session keys; document forward-secrecy properties and persistent-identity compromise impact.
- Revocation and "forget all peers" controls.
- Fresh session keys and replay protection.
- Minimal discovery metadata.
- Bounded parsers, queues, frame sizes, retries, and connection rates.
- Secure platform storage for long-lived identity material.
- Log redaction and bounded retention.
- No captured-audio persistence by default.
- No analytics or cloud dependency in the baseline.

Document data flows and platform privacy declarations. Do not market the app as secure until the threat-model acceptance tests pass.

## 12. Product Observability

Implement structured, privacy-preserving observability from the first vertical slice.

Use stable event names and session-local correlation IDs for:

- Discovery and pairing state transitions.
- Capture start/stop and format changes.
- Negotiated codec and transport.
- Encode/decode timing.
- Packet counts, loss, reorder, late discard, and recovery.
- RTT and jitter.
- Jitter-buffer target/current fill.
- Drift estimate and correction events.
- Render callback timing and underruns.
- Route attach/detach and conversion state.
- Reconnect attempts and terminal errors.
- CPU/memory/bandwidth summaries where available.

Provide:

- A live diagnostics panel suitable for development and support.
- Bounded local logs with redaction.
- Machine-readable metrics for the simulator/soak harness.
- Diagnostic export with a manifest explaining each included field.
- A way to enable verbose diagnostics for one session without rebuilding.

Keep telemetry local unless a future, separately approved requirement adds remote collection.

## 13. Supervisor and Subagent Architecture

### 13.1 Required specialist agents

Spawn specialists with narrow ownership. Combine roles only when the harness imposes a concurrency limit.

| Agent | Primary responsibility | Required output |
|---|---|---|
| Repository Scout | Existing code, build, constraints, reusable patterns | Repository map and verified commands |
| Product Analyst | Normalize requirements and user journeys | Product spec and traceability matrix |
| Platform Feasibility | One specialist per OS where possible | Evidence-backed platform rows and probes |
| OSS and Licensing | Capture/library candidates and obligations | Dependency evaluation and notices plan |
| Audio Systems | Capture/render pipeline, clocks, formats, real-time rules | Audio architecture and benchmarks |
| Protocol and Networking | Discovery, pairing, wire protocol, transport, jitter | Protocol spec, ADRs, golden vectors |
| Security and Privacy | Threat model and controls | Threat model and security tests |
| UX and Accessibility | Cross-role flows and diagnostic UX | Screen/state spec and acceptance tests |
| DevEx and CI | Bootstrap, containers, mocks, native runners | Automated environment and CI matrix |
| Platform Implementer | One per active platform slice | Code, tests, evidence, limitation notes |
| Integration | Cross-component contracts and merge validation | Integration report and repaired conflicts |
| Reliability and Performance | Chaos, soak, profiling, latency | Benchmark report and regressions |
| Independent Reviewer | Requirements, correctness, security, release audit | Findings ordered by severity |

### 13.2 Dispatch waves

Use dependency-aware waves. Parallelize agents that do not write the same files.

1. **Wave P0 - Discovery:** Repository Scout, Product Analyst, all Platform Feasibility agents, and OSS/Licensing work in parallel.
2. **Wave P1 - Decisions:** Audio, Protocol, Security, UX, and DevEx agents consume P0 evidence and draft competing options or ADRs.
3. **Wave P2 - Planning integration:** The supervisor resolves conflicts, builds the task DAG, and dispatches an Independent Reviewer to challenge coverage and feasibility.
4. **Wave B0 - Foundation:** Build/bootstrap, shared contracts, fake adapters, protocol schemas, synthetic source/sink, observability, and CI skeleton.
5. **Wave B1 - Reference path:** Portable emitter/receiver simulators and full lossy/lossless network tests.
6. **Wave B2 - MVP:** Windows emitter and Android receiver, including USB DAC route handling, Free/Pro toggle, installers, and focused E2E evidence.
7. **Wave B3 - Desktop breadth:** macOS and Linux emitter support, then desktop receiver roles where feasible.
8. **Wave B4 - Mobile breadth:** iOS receiver, Android emitter, and iOS emitter within documented OS constraints.
9. **Wave B5 - Bluetooth:** Implement only platform cells approved by the Bluetooth feasibility gate; add explicit alternatives elsewhere.
10. **Wave B6 - Hardening:** Accessibility, security, compatibility, chaos, performance, packaging, docs, and long soak.
11. **Wave B7 - Release audit:** Independent requirement, security, licensing, and clean-machine audits. Repair findings and repeat until clean or explicitly blocked.

Planning may adjust dependencies but may not omit a wave's outcomes.

### 13.3 Task packet contract

Every dispatched task must include:

```yaml
task_id: stable-id
root_task_id: stable-logical-work-id
hypothesis_id: stable-fingerprint-for-a-falsifiable-hypothesis
objective: one testable outcome
mode: research | design | implementation | verification | repair | review
owner_role: specialist role
dependencies: [task-ids]
baseline_revision: immutable integration baseline
allowed_paths: [repository paths]
read_only_paths: [repository paths]
inputs: [documents, ADRs, interfaces, prior reports]
constraints: [platform, API, licensing, real-time, security]
acceptance_criteria: [binary observable conditions]
validation_commands: [exact commands when known]
scope_validation_command: diff added, modified, renamed, and deleted paths against baseline
required_evidence: [tests, logs, screenshots, benchmarks, links]
out_of_scope: [adjacent work deliberately excluded]
report_path: docs/orchestration/reports/task-id.md
```

Assign nonoverlapping write ownership. If two tasks need one contract file, finish and validate the contract task before dispatching consumers.

The `root_task_id` survives retries, worker replacement, and cosmetic task renaming. The `hypothesis_id` changes only when new evidence creates a genuinely different falsifiable explanation. Before integration, verify every changed path against `allowed_paths`; reject and redispatch out-of-scope work.

### 13.4 Worker return contract

Require each subagent to return:

```yaml
task_id: stable-id
status: complete | partial | blocked | failed
summary: concise outcome
files_changed: []
decisions: []
commands_run: []
validation_results: []
acceptance_criteria: [{criterion: text, status: pass | fail | pending, evidence: text}]
risks_or_limitations: []
follow_up_tasks: []
blockers: []
```

Reject reports that omit failed checks, hide pending hardware validation, or lack evidence.

`pending` is allowed only for a named external hardware, credential, or native-runner gate with a queued follow-up task and exact runbook. A task containing pending criteria is `partial`, not `complete`. Its independently validated code may be integrated behind an accurate support status, but it cannot satisfy a phase or release gate.

## 14. Orchestration State and Monitoring

Maintain durable state so another supervisor can resume without reconstructing history.

Create and update:

- `docs/orchestration/TASK_LEDGER.md`: task, owner, dependencies, status, attempt, validation, and evidence.
- `docs/orchestration/DECISION_LOG.md`: concise decisions and ADR links.
- `docs/orchestration/INTEGRATION_STATUS.md`: known-green revision, active branches/worktrees, and merge order.
- `docs/orchestration/QUALITY_DASHBOARD.md`: platform/build/test/requirement status.
- `docs/orchestration/PHASE_GATES.md`: one binary criterion per phase outcome, its command or evidence, status, and reviewer approval.
- `docs/orchestration/incidents/`: one file per repeated or systemic failure.
- `docs/orchestration/reports/`: worker reports.

After every dispatch round:

1. Confirm every task returned a structured report.
2. Reconcile claimed files with actual changes.
3. Run the narrowest relevant validation.
4. Verify changed paths against each task's baseline and write scope.
5. Update requirement coverage and quality status.
6. Identify blocked dependents and reschedule them.
7. Detect write-scope overlap or contract drift.
8. Integrate only changes whose acceptance criteria pass or are explicitly partial under Section 13.4.
9. Record the next known-green point.

A task is stale when it repeatedly returns no new evidence, repeats the same failing action, writes outside scope, or misses its return contract. Replace or narrow it instead of waiting indefinitely.

Count retries by `root_task_id` and `hypothesis_id`, not display name or worker. Renaming or splitting a task does not reset an unchanged hypothesis's attempt counter. Record superseded, reverted, and abandoned contributions rather than deleting their history.

## 15. Self-Healing Protocol

Apply this protocol automatically to failures. Never respond to a routine failure by merely asking the user what to do.

### 15.1 Classify

Classify each failure as one of:

- Product-code defect.
- Test defect.
- Contract mismatch.
- Integration conflict.
- Toolchain/dependency failure.
- Flaky or resource-sensitive test.
- Native-runner limitation.
- Missing credential or hardware.
- Unsupported platform capability.
- Security or licensing rejection.

Capture the smallest reproduction, expected result, actual result, logs, environment, and owning task.

### 15.2 Repair ladder

Use at most three evidence-producing attempts per unchanged hypothesis:

1. Return the failure to the owning worker with the exact reproduction and a narrowed repair task.
2. Dispatch an independent diagnostician to test the hypothesis and propose the smallest correction.
3. Replace the approach: revert only the supervisor-owned failed contribution through a non-destructive patch, select an evaluated alternative dependency/design, or split the task behind a clearer contract.

After three failed attempts, create an incident, mark the dependent requirement blocked, continue independent work, and escalate only if Section 2.5 allows it.

When evidence disproves the current hypothesis, record the result and create a new hypothesis ID. When it instead reveals an unmet upstream dependency, schedule that dependency and keep the original task blocked without consuming cosmetic retries. This distinction must be visible in the ledger.

### 15.3 Recovery rules

- Preserve the last known-green state and evidence.
- When reverting or superseding a worker contribution, append a ledger record naming the original task, reason, replacement task, and resulting known-green revision.
- Never bypass, delete, or weaken a valid test just to obtain green status.
- Treat a flaky test as a defect. Reproduce it repeatedly, remove nondeterminism, or quarantine it only with an owner, issue, bounded expiry, and replacement signal.
- On dependency download failure, use bounded retries, mirrors/caches already allowed by policy, and a pinned alternative. Never silently consume an unverified binary.
- On CI-only failure, reproduce the runner environment in a container or dispatch a runner-specific diagnostician.
- On unavailable native hardware, complete fake-adapter and CI work, create the exact hardware runbook, and keep the gate pending.
- On contract drift, stop dependent merges, update the source-of-truth schema/ADR, regenerate bindings, and rerun compatibility tests.
- On a security or license failure, quarantine the affected dependency or feature until a specialist approves the repair.
- On merge conflict, dispatch Integration with both task reports and the governing contract. Preserve independently valid user or worker changes.

### 15.4 Health signals

The supervisor must continuously watch:

- Requirement coverage percentage by status.
- Tasks blocked and age in dispatch rounds.
- Validation pass rate.
- New versus known failures.
- Retry count by hypothesis.
- Cross-platform build status.
- Protocol compatibility status.
- Test flake rate.
- Performance movement against budgets.
- Unresolved critical/high review findings.
- Pending physical-device and credential gates.

Progress means new validated evidence, not more prose or more spawned tasks.

## 16. Plan-Mode Deliverables

Plan mode is complete only when the following package exists or is fully emitted in the response.

### 16.1 `docs/planning/PRODUCT_SPEC.md`

Include:

- Problem statement and target users.
- Emitter and receiver journeys.
- Free and Pro behavior.
- Functional requirements and non-goals.
- Permission and failure journeys.
- UX state diagrams.
- Acceptance metrics.

### 16.2 `docs/planning/REQUIREMENTS_TRACEABILITY.md`

One row per requirement ID with:

- Requirement summary.
- Planned component.
- Task IDs.
- Automated tests.
- Manual/hardware tests.
- Documentation location.
- Current status.
- Evidence.

### 16.3 `docs/planning/PLATFORM_MATRIX.md`

Provide the evidence-backed matrix required by Section 5. Include both emitter and receiver roles for all target operating systems and separate Wi-Fi from Bluetooth viability.

### 16.4 `docs/planning/ARCHITECTURE.md`

Include:

- System context and deployment diagrams.
- Process/component boundaries.
- Audio data flow.
- Control and media planes.
- Shared-core/platform-adapter boundary.
- Threading, queueing, cancellation, and real-time constraints.
- Trust boundaries and data storage.
- Failure and recovery behavior.
- Observability architecture.
- Packaging architecture.

Use Mermaid diagrams where useful.

### 16.5 `docs/planning/PROTOCOL_SPEC.md`

Include:

- Discovery.
- Pairing and authentication.
- State machine.
- Capability negotiation.
- Message and frame schemas.
- Timing and sequence rules.
- Codec profiles.
- Jitter and clock policy.
- Congestion and recovery policy.
- Versioning and compatibility.
- Error taxonomy.
- Security limits.
- Golden-vector strategy.

### 16.6 ADRs

At minimum, decide:

- `ADR-001` repository and shared-core strategy.
- `ADR-002` UI/platform-shell strategy.
- `ADR-003` network transport.
- `ADR-004` lossy codec and settings.
- `ADR-005` lossless PCM/compression strategy.
- `ADR-006` discovery and pairing.
- `ADR-007` clock-drift and jitter policy.
- `ADR-008` minimum supported OS versions.
- `ADR-009` Bluetooth support boundaries.
- `ADR-010` packaging and update approach.

Each ADR needs context, options, discriminating evidence, decision, consequences, and reversal conditions.

### 16.7 `docs/planning/TEST_PLAN.md`

Map every quality target and requirement to test layers, fixtures, environments, commands, expected results, and retained evidence.

### 16.8 `docs/planning/DEVELOPMENT_ENVIRONMENT.md`

Define:

- WSL2 prerequisites.
- Docker/dev-container setup.
- Toolchain pins.
- Bootstrap behavior.
- Local simulated topology.
- Native runner setup.
- Emulator/simulator setup.
- Virtual audio devices.
- CI jobs and caches.
- Signing placeholders.
- Clean-machine verification.

### 16.9 `docs/planning/DELIVERY_PLAN.md`

Build a dependency DAG, not a calendar guess. Every task needs the packet fields from Section 13.3. Include:

- Phase objective.
- Entry and exit criteria.
- Tasks and dependencies.
- Agent role.
- Expected files/components.
- Exact or discoverable validation command.
- Parallelization boundaries.
- Risks and fallback.

The plan must continue through all phases in Section 17, not end at MVP.

### 16.10 Supporting documents

- `docs/planning/DEPENDENCY_EVALUATION.md`.
- `docs/planning/THREAT_MODEL.md`.
- `docs/planning/RISK_REGISTER.md`.
- `docs/planning/OPEN_QUESTIONS.md`.
- `docs/planning/RELEASE_AND_SIGNING.md`.
- `docs/planning/HARDWARE_VALIDATION.md`.

### 16.11 Independent plan review

Before declaring plan mode complete, dispatch at least three independent reviews:

- Feasibility and platform truthfulness.
- Architecture, real-time audio, and protocol correctness.
- Requirement coverage, testing, security, licensing, and release completeness.

Resolve all critical/high findings in the plan. Record lower findings with owners and gates.

Plan review status must be exactly `Approved` before build mode starts. Approval requires all Section 16 deliverables to exist and be nonempty, every Phase 0 criterion to have evidence, all critical/high findings to be closed, and every remaining finding to have an owner, target phase, closure test, and blocking gate. Record each reviewer's rubric and final disposition.

## 17. Delivery Phases

The plan may refine these phases but must preserve their outcomes.

### Phase 0: Evidence and bootstrap

Exit when:

- The complete OS by role by transport platform and Bluetooth matrices have current, timestamped evidence; no reusable product implementation begins before this gate.
- Capture/output feasibility probes answer the highest-risk questions.
- Core stack, transport, codec, and OS-version ADRs are accepted.
- Bootstrap and clean-clone validation design is complete.
- Requirements and task DAG are traceable.
- Every exit item has a row in `PHASE_GATES.md` with a command or evidence link and independent reviewer disposition.

### Phase 1: Portable reference system

Build a headless reference emitter and receiver using synthetic audio.

Exit when:

- Discovery, pairing, negotiation, encrypted control, and media transport work end to end.
- Opus lossy and selected lossless mode pass golden and impairment tests.
- Jitter, drift, backpressure, reconnect, observability, and entitlement policy are testable.
- The entire reference topology runs automatically in WSL2/Docker.

### Phase 2: Consumer MVP

Build Windows emitter plus Android receiver first unless feasibility evidence justifies another desktop/mobile pair.

Exit when:

- Permitted Windows system audio is captured through a public supported API.
- Android discovers, pairs, receives, and renders through ordinary output and a connected USB DAC where the device exposes it.
- Free lossy Wi-Fi and toggled Pro lossless Wi-Fi are real end-to-end modes.
- UI covers permissions, session control, diagnostics, output route, and honest fidelity status.
- Reconnect, route changes, background lifecycle, installers/APK, soak, and reference-hardware checks pass.

### Phase 3: Desktop emitters and receivers

Exit when:

- Windows, macOS, and Linux emitter paths are release-capable within documented OS limits.
- Desktop receiver roles are implemented where feasible.
- Native builds, packages, permission flows, and virtual-device/native-runner tests pass.
- Capture limitations are surfaced in product UI and docs.

### Phase 4: Mobile breadth

Exit when:

- iOS receiver is release-capable for Wi-Fi modes and external audio routes within public API limits.
- Android emitter supports capturable app audio with required consent and policy restrictions.
- iOS emitter implements the maximum public ReplayKit/broadcast-extension scope that passes store-policy and lifecycle tests.
- Unsupported capture cases are detected before starting and explained accurately.

### Phase 5: Bluetooth

Exit when:

- Every approved Bluetooth platform cell has a native implementation, automated coverage where possible, and physical-device evidence.
- Unsupported cells show a precise explanation and one-action path back to free lossy Wi-Fi.
- The product never implies that a stock phone can act as an A2DP sink when public APIs do not permit it.

### Phase 6: Production hardening

Exit when:

- Security and privacy gates pass.
- Accessibility gates pass.
- Performance and soak budgets pass on reference devices.
- Crash, leak, malformed-input, and network-chaos suites pass.
- Upgrade and protocol compatibility policy is tested.
- Install/uninstall and clean-machine package tests pass.
- SBOM, notices, support diagnostics, user docs, and operator docs are complete.

### Phase 7: Release readiness

Exit when:

- Independent audits find no unresolved critical/high issues.
- Every requirement is `Verified`, `Unsupported by public API` with evidence and fallback, or `Blocked on external credential/hardware` with an exact completion runbook.
- No requirement is merely `Implemented` without test evidence.
- Signing/store jobs are ready to run when credentials are supplied.
- The final report lists artifacts, support matrix, commands, measurements, limitations, and remaining external gates.

## 18. Definition of Done

The project is complete only when all of these are true:

- All functional requirement IDs have traceable outcomes.
- The portable reference system and MVP are fully automated and green.
- Windows, macOS, and Linux desktop emitters are complete within verified API limits.
- Android and iOS receiver paths are complete within verified API limits.
- Desktop receiver and mobile emitter cells are either complete or carry current official evidence that public APIs prevent the required behavior, plus the nearest supported fallback.
- Free lossy Wi-Fi and Pro lossless Wi-Fi work end to end.
- The development Free/Pro toggle drives one centralized feature policy and is covered by tests.
- Every viable Bluetooth cell is implemented and every nonviable cell is represented honestly.
- Lossless transport integrity is verified by PCM equality tests.
- Output-path conversion and bit-perfect status are never conflated.
- Discovery, pairing, encryption, trust revocation, malformed-input limits, and redaction pass security tests.
- WSL2 bootstrap, containerized simulation, and native CI are reproducible.
- Native packages are produced or ready except for documented credentials.
- Hardware-only checks have retained evidence or a precise pending gate.
- No critical/high independent-review finding remains unresolved.
- Phases 3 through 7 were each executed. A platform cell may leave those phases only as verified, evidence-backed unsupported, or blocked on a permitted external gate; an MVP-only result is a failed harness run.
- Documentation matches actual behavior and commands.
- The repository is left in a validated, resumable state with orchestration records current.

Stopping after architecture, scaffolding, a simulator, or the MVP does not satisfy this definition.

## 19. Build-Mode Execution Loop

In build mode, repeat this loop until Section 18 is satisfied:

1. Load planning and orchestration state.
2. Verify the last known-green revision with the narrowest top-level smoke command.
3. Select the highest-priority unblocked tasks from the DAG.
4. Create complete task packets with nonoverlapping write scopes.
5. Dispatch specialist subagents in parallel.
6. Monitor structured returns and reject unevidenced completion claims.
7. Run focused validation for each contribution before integration.
8. Dispatch repair tasks immediately for local failures.
9. Integrate validated contributions in dependency order.
10. Run contract, integration, and regression checks appropriate to the changed surface.
11. Update traceability, quality dashboard, risks, decisions, and known-green state.
12. Replan only the affected downstream tasks when evidence changes an assumption.
13. Dispatch an independent review at every phase gate.
14. Continue to the next phase without asking for routine approval.

Keep changes small enough that one failed task can be isolated. Prefer contract-first vertical slices over creating many disconnected shells.

## 20. Final Reporting Contract

### Plan-mode final response

Provide:

- One-paragraph architecture direction.
- Highest-risk feasibility findings, especially Bluetooth and mobile capture.
- Selected defaults and ADRs still pending a prototype.
- Phase/task counts and critical path.
- Exact planning artifact locations or their full contents if writes are unavailable.
- The first build-mode dispatch wave.
- Only genuine blockers permitted by Section 2.5.

### Build-mode progress reports

Report concise deltas:

- Phase and requirement coverage.
- Newly validated outcomes.
- Active/failed/replaced tasks.
- Quality-dashboard movement.
- Decisions changed by evidence.
- External gates.

### Build-mode final response

Provide:

- What works by platform, role, and transport.
- What is unsupported and the evidence-backed reason.
- Free versus Pro behavior.
- Build, test, run, and package commands.
- Validation summary with key latency, reliability, and fidelity measurements.
- Security, privacy, license, and accessibility status.
- Artifact locations.
- Pending credential or physical-hardware gates.

Do not call the project complete while hiding failed tests, unsupported platform cells, simulated-only claims, or deferred non-MVP requirements.

## 21. Start Now

If running in plan mode:

1. Spawn the Wave P0 specialists immediately.
2. Gather current platform and repository evidence in parallel.
3. Produce the complete Section 16 planning package.
4. Run the independent plan reviews.
5. Repair the plan until all critical/high findings are resolved.
6. End with the exact first build-mode task packets.

If running in build mode:

1. Locate and validate the planning package.
2. Initialize or reconcile orchestration state.
3. Dispatch the earliest unblocked build wave.
4. Execute the loop in Section 19 until the Definition of Done is met or only permitted external blockers remain.