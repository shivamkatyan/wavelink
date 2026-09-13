# Risk Register

Owner, phase, mitigation, trigger, status. Updated continuously; incidents beyond 3 retries → `docs/orchestration/incidents/`.

| # | Risk | Owner | Phase | Mitigation | Status |
|---|---|---|---|---|---|
| R01 | QUIC CUBIC CWND collapse under 1% loss starves audio datagrams | Transport | P1/B0 | BBR evaluation in netem spike; redundancy/FEC; UDP fallback | open → spike |
| R02 | Lossless frames don't fit datagram MTU (incompressible worst case) | Audio/Codec | P1/B0 | ADR-005: reliable stream for lossless; small FLAC blocks; frame-size CI calculator | open → spike |
| R03 | RT-callback encode/decode/transport violates real-time | Audio/RT | B0/B1 | RT contract whitelist; SPSC rings; abort-on-alloc; worst-case benches | open |
| R04 | Bit-perfect claim without measurement | Fidelity | B2/B6 | hardware loopback rig; bounded-slip semantics; receiver-side only | plan |
| R05 | iOS App Review rejects capture features | iOS | B4 | system pickers; 2.5.14 compliance record; `screen-capture`/`audio` modes | plan |
| R06 | Hosted-runner audio absence (no audio endpoint on CI) | DevEx | B0 | probe runner; virtual WASAPI endpoint; self-hosted/lab fallback | plan |
| R07 | Android USB latency variance breaks 80 ms low-latency | Reliability | B2 | device-gated Low-Latency via latency probe | mitigated by design |
| R08 | libopus/FLAC cross-compile friction (iOS/Android) | DevEx | B0/B2 | pinned NDK/Xcode; spice tasks; vendored C via cmake | open |
| R09 | Bluetooth over-promise (stock phone A2DP sink) | BT/UX | B5 | two-axis truth model; explicit matrix; fallback to free lossy Wi-Fi | mitigated by design |
| R10 | mDNS spoofing / injection | Security | B0/B1 | fingerprint-pinned pairing; SAS/QR pubkey binding; 0-RTT off; bounds | mitigated by design |
| R11 | Clock-drift estimator oscillation | Audio | P1/B0 | WLS + outlier rejection + deadband + slew bound; ADR-007 spike | open → spike |
| R12 | Credential gates stall release if unplanned | Release | B6/B7 | gated signing jobs ready; unsigned artifacts + dry-runs now | plan |
| R13 | Snapshot third-party code without provenance | OSS | all | pre-deps approved review gates; attribution/SBOM CI | open |
| R14 | Flaky timing-based acceptance | Testing | B0-B7 | virtual clocks in core; percentile envelopes; deterministic FSM tests | mitigated by design |
| R15 | Latency acceptance flaky / device-dependent | Reliability | P1/B1-B7 | LATENCY_MEASUREMENT.md: p95+uncertainty upper-bound gate, warm-up exclusion, ≥5-min×≥5-run method, offset-estimator/§3.2 clock method; device-gated Low-Latency probe on connect (ADR-007/R07); simulated vs physical never merged; gate = device-gate probe qualified + p95+U ≤ budget | open |
| R16 | RT callback alloc/lock/blocking regression | Audio | B2/B6 | RT_CONTRACT.md allowed-op whitelist; wdr_rt no-alloc/no-lock SPSC; rt-guard abort-on-alloc feature (**designed, not yet implemented** — RT_CONTRACT.md §4); CI stress+watchdog; native-runner RT evidence gate (WSL2 cannot substitute); hardware-lab worst-case validation | open |
