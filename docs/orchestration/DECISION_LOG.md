# DECISION LOG / DECISION_LOG.md

> **Note (2026-09-13):** the session-planning/ledger docs cited below
> (`HANDOFF.md`, `MACOS_SESSION_PLAN.md`, `PHASE_GATES.md`, `TASK_LEDGER.md`,
> `INTEGRATION_STATUS.md`) were removed in the proprietary-cleanup commit — the
> entries below remain valid as a chronological record and their substance lives
> on in `RELEASE_STATUS.md` / `QUALITY_DASHBOARD.md`.

| Date | Decision | Reference |
|------|----------|-----------|
| 2026-09-06 | Plan approved after 3 independent reviews; 0 unresolved critical/high | docs/planning/* |
| 2026-09-06 | Transport: QUIC (quinn) control + datagrams (lossy) + reliable stream (lossless), 0-RTT media off, BBR/CUBIC spike | ADR-003 |
| 2026-09-06 | Lossy: Opus default. Lossless: FLAC-default w/ PCM profile, ADR-005 bench pending | ADR-004/005 |
| 2026-09-06 | Fidelity ladder incl. bit-perfect-only-when-hardware-verified + bounded-slip semantics | ADR-005 |
| 2026-09-06 | Discovery/pairing: mDNS (privacy-min) + manual/QR; Noise XX + ed25519; revocation w/ offline QR | ADR-006 |
| 2026-09-06 | Drift: receiver bounded adaptive resampling + WLS estimator, slew/deadband bounds (spike pending) | ADR-007 |
| 2026-09-06 | Min OS floors per OS (Win11 24H2+, macOS 13, Linux glibc≥2.35/PW≥1.4, Android 29/34, iOS 14) | ADR-008 |
| 2026-09-06 | Bluetooth two-axis model: Linux standard sink; custom product-peer transport on And/Win-RFCOMM/Linux; no stock-phone sink claim | ADR-009 |
| 2026-09-06 | Packaging: per-platform native; gated signing jobs; unsigned artifacts now | ADR-010 |
| 2026-09-06 | eqMac not reused (Apache snapshot stale 2021; core closed); native APIs + permissive wrappers | DEPENDENCY_EVALUATION |

New entries appended as ADRs/spikes resolve.

| 2026-09-06 | ADR-003 spike evidence (measured, loopback): QUIC datagram max = 1162 B; raw PCM 20 ms (3840 B) & 10 ms (1920 B) DO NOT fit datagram → lossless MUST use reliable stream (confirmed measure); 5 ms PCM fits. Opus 20 ms ≈400 B frames viable over datagrams (p50 149 µs / p99 857 µs loopback). 0-RTT-off verified. BBR vs CUBIC: indistinguishable on loss-free loopback → deferred to netem (B1). quinn datagram-drop overflow under sustained over-budget sends (track upstream; re-verify at pin before B1 soak) | ADR-003, t-B0-transport |
| 2026-09-06 | INC-001 resolved: session FSM split behind a clearer contract (single-file crate, zero deps) on new hypothesis H-SESSION-2 → complete; 7/7 tests. Root cause: oversized/ambiguous contract + missing return discipline, not a code defect (no code existed). Records attempt tax under H-SESSION-1; retries do NOT reset unchanged-hypothesis count | INC-001 |
| 2026-09-06 | Golden lossless equality wired to SHARED canonical fixtures (wdr_fakes goldens → codec): PCM+FLAC i16/48k/stereo, 9 fixtures, tolerance-free hash equality + channel order; i24 explicitly skipped (adapter stubs, ADR-005 24-bit follow-up) | R-B0-WIRE |
| 2026-09-06 | Telemetry H-OBS-1 void → H-OBS-2 single-file std-only crate complete (redaction deny-list, bounded ring 4096, export manifest, correlation ID separation) | R-B0-OBS |

| 2026-09-10 | macOS-host intake: host revalidated as materially more capable (Xcode 26.6, iOS 26.5 sim; Rust 1.98.1; Docker 29.4.0; JDK17+Android SDK installed). Story: B3 macOS emitter + B4 iOS receiver became the session's primary deliverables; Android emitter compiled on this host too; Windows/WASAPI, USB-DAC device, BT-lab, iOS real-device, macOS TCC/SCK/tap physical session, bit-perfect loopback, signing creds remain honest external gates | HANDOFF, MACOS_SESSION_PLAN |
| 2026-09-10 | INC-003 closed: ref_e2e lossless golden desync from rev-8 (per-channel budget change never propagated to dependent e2e) repaired by restoring the canonical 4096-value drain (`total_samples=2048` per-channel); rustfmt style-edition drift reconciled (formatting-only). Verified workspace 215/0 green | INC-003 |
| 2026-09-10 | Android toolchain on macOS: JDK via `brew install openjdk@17` (keg-only; cask temurin@17 needs sudo unavailable non-interactively); SDK via `brew install --cask android-commandlinetools` + `sdkmanager` (platforms;android-34, build-tools;34.0.0, platform-tools, ndk;27.x). Agents use JAVA_HOME/ANDROID_HOME explicitly | MACOS_SESSION_PLAN |

| 2026-09-10 | B5 dispatch on macOS host: only the ADR-009-supported full public receive+render cell is implemented as a shell — `platform/linux-receiver` (BlueZ a2dp_sink + PipeWire media-sink), with honest per-cell matrix (stock mobile/desktop A2DP-sink = unsupported by public API + free-lossy-Wi-Fi fallback); RFCOMM/L2CAP low-bitrate product-peer = feature-gated cell. Native BlueZ/PipeWire evidence = Linux-runner/bt-lab gate; clones of the shell pattern shipped elsewhere | ADR-009, t-B5-linux-receiver |
