# Dependency Evaluation

Owner: OSS/Licensing (task `t-B0-ossdep`, root `R-B0-OSSDEP`).
Last rubric pass: **2026-09-06**. Policy below requires the rubric to be re-run on every version bump (see §Maintenance rubric).

> **Product-vs-dependency license (2026-09-13):** this policy governs
> **third-party dependencies** only. The Wavelink product itself is proprietary,
> source-available software (repo-root `LICENSE`) — its workspace crates carry
> `license = "LicenseRef-Proprietary"` and are allowed for the members in
> `deny.toml` without waiving the permissive-only rule for dependencies.

## Policy

- Prefer official OS APIs and actively maintained, permissively-licensed libraries; **no GPL/AGPL/SSPL/source-available/noncommercial** dependency without a recorded decision (uniffi/MPL-2.0 is the only recorded exception).
- No unauthored / unverifiable upstreams; no snapshot code without provenance (RISK_R13).
- Platform/codec/transport/discovery/entitlement live behind testable adapter traits (ADR-001/ARCHITECTURE).
- Pin with committed `Cargo.lock` + checksums; verify vendored C sources; SBOM + attribution notices shipped in every artifact (LICENSE-NOTICES.md, SBOM_POLICY.md).
- Every material dependency must have a row with: purpose+alternatives, repo/package source, license+transitive+link mode, last stable release, maintenance activity (commit/date), unresolved critical issues/security posture, supported targets + cross-compile, API stability + test quality, binary size/runtime/native build requirements, use mode, exact pinned version + update policy.
- Rejection reasons for evaluated-but-not-adopted candidates are recorded in the Explicitly rejected section.
- Unpinned/`latest`/float version references are forbidden in `Cargo.toml`; every direct dep is pinned exactly as a workspace dependency and resolved to an exact lockfile entry.

## Locked dependency list (B0 lockfile → `Cargo.toml` workspace pins)

Exact pins below are the *target* pins for the B0 lockfile. `Cargo.lock` is committed (see root `Cargo.lock` placeholder + SBOM_POLICY.md). Rows marked `[at risk]` have an open caveat tracked in §Maintenance rubric.

### Core (shared `core/` workspace)

| Dependency | Purpose + alternatives | Source (repo/package) | License + transitive + link mode | Last stable (date) | Maintenance (commit/date) | Critical issues / security | Targets + cross-compile | API stability + tests | Binary/runtime/native | Use mode | Exact pin + update policy |
|---|---|---|---|---|---|---|---|---|---|---|---|
| **quinn** | QUIC transport: reliable control stream + unreliable datagrams for lossy media, 0-RTT-off, migration/reconnect (ADR-003). Alt: webrtc-rs RTP/SRTP (rejected for media path, see below), custom UDP (ADR-003 exit criteria). | https://github.com/quinn-rs/quinn (crates.io `quinn`) | MIT/Apache-2.0. Transitive: rustls (+ring or aws-lc-rs; we use `rustls-ring` default so ring), rustcrypto, webpki-roots, subtle, etc. Statically linked into core. | 0.11.x (0.11.11 was current at 2026-09-06) | Active: commits within days/weeks of 2026-09-06; 0.11 line supported | No CRITICAL RustSec advisory applicable at pin; track mobile/`ring` gate. GitHub issue #1778 (mobile CI) tracked (P). Long-running SAFETY comments + advisory process acknowledged | All 5 targets; pure-Rust path cross-compiles iOS/Android via rustls-ring (no OpenSSL). P first-time cross build (RISK_R08) | Stable 0.11 series; semver-respected within 0.11.x, minor bumps non-breaking. Good test coverage upstream (+ our B0/B1 network + netem tests) | Moderate binary size (~1–2 MB debug base); pure-Rust codegen, no native build required; async runtime (tokio) | **direct** (wrapped behind `transport` adapter) | Pin patch `0.11.11`; update policy: same-minor patch updates auto-reviewed at each rubric pass; minor 0.12+ gated behind rubric + B0/B1 revalidation |
| **opus** (Rust `opus` crate + sys via `opusic-sys`) | Lossy encode/decode (ADR-004, default codec). Alt: ivy/*pure-Rust Opus encoders* (none production-grade), AAC/MP3 (patent), Vorbis, OS-native AAC (variance) | https://github.com/xiph/opus (vendored C); https://crates.io/crates/opus (binding); `opusic-sys` from the audioplanes/opusic bindings | Crate MIT/Apache-2.0; bundled **libopus BSD-3-Clause** (vendored object code into binary). Link: static (vendored build via cc/cmake); note libopus is BSD (not LGPL-in-fronted-for-opus) so no exception concerns | opus crate 0.4.0; libopus 1.5/1.6 line (0.7.5 observable in opusic-sys) | Active (xiph). Binding crate maintained, low turnover | None critical. Fixed-point path needed for ARM Android (no float DSP guarantee) | All 5 targets; `opus` crate + fixed-point feature for aarch64-android; P first-time FFI cross compile (RISK_R08) | stable FFI surface (libopus API frozen); binding crate thin; our codec adapter unit + golden tests | Add increment ~100–300 KB; native C build requires proper cc + build toolchain in cross env (cmake/NDK) | **direct adapter** (codec adapter wraps libopus) | `opus 0.4.0` + sys pin; update policy: sync to latest libopus minor after cross-build + audio golden verification |
| **flac-bound** (+ **libflac-sys**) / **claxon** / **hound** | Lossless FLAC encode (flac-bound/libflac-sys, ADR-005) + FLAC decode (claxon) + PCM/WAV I/O (hound). Alt: ALAC (Apple-centric), WavPack (smaller Rust ecosystem), raw PCM profile (in-product alternative) | https://github.com/ruuda/claxon; flac-bound → https://github.com/citizen428/flac-bound (libflac); https://github.com/ruud-v-a/hound | flac-bound MIT; libflac-sys wrapper MIT — **vendored libFLAC: BSD-3-Clause** (Xiph variant; we do NOT use the LGPL option). claxon Apache-2.0; hound Apache-2.0. Static/vendored | flac-bound 0.5.0 (2026), libflac-sys 0.3.4, claxon 0.4.3, hound 3.5.1 | flac-bound actively maintained (2026-ongoing); claxon active; hound active but low churn | None critical. Cross-compile of libFLAC FFI is a known build risk (RISK_R08); flac-bound bundles libFLAC source | All 5; claxon/hound are pure Rust (trivial cross); flac-bound needs C toolchain cross (NDK/Xcode) | flac-bound API stable + tests; claxon decode-only well tested on corpus; hound tested for WAV | libFLAC adds ~200–400 KB + native build; claxon/hound pure-Rust small | **direct adapter** (codec adapter: encode via flac-bound, decode via claxon, PCM via hound) | Pin flac-bound 0.5.0, libflac-sys 0.3.4, claxon 0.4.3, hound 3.5.1; update in lockstep like opus |
| **rubato** | Bounded adaptive resampling on receiver (ADR-007 drift correction). Alt: sample-slip insertion/deletion (audible), emitter-side clock slaving (API-gated), no-op (buffer grows) | https://github.com/HEnquist/rubato | MIT/Apache-2.0, static | 5.0.0 (2026-08) | Active (Henquist) | None critical; Polyphase/FFT resampler CPU + quality measured in ADR-005/007 bench | All 5 (pure Rust; simd optional is Rust, no native) | API choices (Sinc/Tukey) settled; upstream has property tests; our drift tests inject ±ppm | Pure Rust; small | **recorded, NOT adopted for drift (2026-09-14, WS-B/E): the drift path reuses `ResamplerI16` with a dependency-free polyphase Kaiser-windowed-sinc (64-tap/4096-phase, ~92 dB) — no rubato on the drift path.** rubato remains only inside `OpusAdapter` for the unavoidable 44.1k resample | Revisit only if the 44.1k Opus path needs an upgrade; drift stays dependency-free |
| **mdns-sd** | mDNS browse/advertise for discovery (ADR-006). Alt: zeroconf (license ambiguity, rejected), libmdns, manual IP/QR (also shipped as fallback) | https://github.com/keepsimple1/mdns-sd | Apache-2.0/MIT | 0.21.3 (2026-09-13 — land + loopback roundtrip on macOS host; addy/flume/if-addrs transitive all permissive) | Active (commits 2026) | None critical; no async runtime forced in crate (it includes its own select loop; integrate into our tokio executor — B0 integration item) | All 5; pure Rust; iOS local-network permission path validated (PLATFORM_MATRIX) | Stable-ish 0.x minor churn; upstream unit tests exist; our discovery adapter tests + loopback advertise→browse roundtrip | Pure Rust, small; per-interface socket behaviour to validate on each OS | **direct** (wrapped behind `Discovery` adapter) | Pin 0.21.3; update policy: every minor re-run discovery adapter + iOS/Windows path tests |
| **snow** | Noise protocol XX handshake for pairing (ADR-006). Alt: TLS-PSK (rejected), manual psk-codes | https://github.com/mcginty/snow | MIT/Apache-2.0 | 0.10.0 (2026) | Active (McGinty) | P: snow↔curve25519-dalek 5.x coexistence — verify feature unification/build sizes at lockfile time (recorded decision on dalek backend below) | All 5; pure Rust (with optional dalek backends) | Stable 0.10; upstream tests good; our noise golden vectors planned | Pure Rust; moderate (with dalek) | **direct** (crypto module) | Pin 0.10.0; update policy: verify dalek coexistence on every bump; GHSA watch |
| **ed25519-dalek / x25519-dalek** | Identity signing/verification + X25519 key agreement for pairing (ADR-006, THREAT_MODEL #4). Alt: ring ECDSA/Ed25519 (FFI), p256/p384 | https://github.com/dalek-cryptography/curve25519-dalek | **BSD-3-Clause** — requires NOTICE/license shipping | 3.0.0 (2026?) | Active (dalek org) | None critical; RustCrypto audit history; no `-pgp` feature needed | All 5; pure Rust; SIMD/backend feature flags to skim cross-arch | Stable 3.0 line; dalek tests strong; our crypto golden + Fuzz included | Pure Rust; depends on `curve25519-dalek` backends (incl. `fiat` for no_asm targets) | **direct** (crypto module) | Pin 3.0.0; update policy: minor updates require rerun of crypto goldens; watch GHSA |
| **chacha20poly1305** (RustCrypto) | AEAD for control + media; XChaCha20-Poly1305 or counter-sourced nonces (THREAT_MODEL #1, ADR-003/006). Alt: AES-GCM (hw-accelerated on some targets but adds arch variance), libsodium-sys (FFI) | https://github.com/RustCrypto/AEADs | MIT/Apache-2.0 | 0.11.0 | Active (RustCrypto) | None critical; RustCrypto release process mature; use nonce discipline from threat model | All 5; pure Rust; portable asm-free path | 0.11 line stable; upstream CI on all targets; our AEAD + replay-window tests | Pure Rust, small | **direct** (crypto module) | Pin 0.11.0 |
| **postcard + serde** | Compact `no_std` wire encoding for internal messages/control (postcard) + derive (serde). Alt: bincode (rejected, archived), rmp-serde (msgpack), hand-rolled | https://github.com/jamesmunns/postcard; https://github.com/serde-rs/serde | postcard MIT/Apache-2.0; serde MIT/Apache-2.0 | postcard 1.1.3; serde 1.0.229 | postcard active (JmMunns), serde extremely active | None critical; serde has had advisories historically; keep current | All 5; pure Rust, `no_std`-compatible | Stable (postcard 1.0+); serde 1.0 semver-stable; golden ser buffer tests planned | Pure Rust, small; no_std friendly for core | **direct** | Pin postcard 1.1.3 + serde 1.0.229 (+ derives 1.0.x); patch-level updates routine |
| **tokio** | Async runtime for QUIC-heavy core (runtime + io-util + net + time + sync + rt-multi-thread). Alt: async-std (deprecated → smol), mono-thread blocking (inadequate) | https://github.com/tokio-rs/tokio | MIT (also Apache-2.0 historically; current cargo metadata MIT) | 1.53.x (1.53.1 at 2026-09-06) | Very active (tokio-rs) | No critical at pin; tokio advisory track record — keep current patch | All 5 (tokio is desktop/mobile fine; features trimmed for core no_std-compatible parts where possible) | 1.x semver-stable; upstream tests extensive; our session-FSM async tests | Small-moderate w/ features trimmed; no native build | **direct** (runtime layer) | Pin 1.53.1 (patch); minor within 1.x safe |
| **tracing / tracing-subscriber** | Structured observability, bounded logs, redacted diagnostics (ARCHITECTURE §Observability). Alt: log+env_logger, slog | https://github.com/tokio-rs/tracing | MIT (subscriber MIT) | tracing 0.1.44 / subscriber 0.3.23 | Active (tokio-rs) | None critical | All 5 | Stable 0.1/0.3 lines; upstream tests; our redaction + log binding tests | Pure Rust; keep subscriber features trimmed for binary size | **direct** | Pin 0.1.44 / 0.3.23 |
| **clap** | CLI parsing for the headless sim (`cli-sim`, Linux-first). Alt: std env::args, argh, pico-args (if we ever shrink) | https://github.com/clap-rs/clap | MIT/Apache-2.0 | 4.6.6 | Very active | None critical | All (host builds primarily); derive feature | 4.x stable; upstream tests strong | Moderate if full; feature-gate `derive`, no `suggestions` bloat | **direct** (cli-sim only) | Pin 4.6.6 |
| **qrcode** | QR fallback for pairing/revocation offline propagation (ADR-006, FR-004). Alt: OS QR readers/third-party image libs | https://github.com/kennytm/qrcode-rust | MIT/Apache-2.0 | 0.14.1 | Low churn but stable/not archived (2026 OK) | None critical | All 5 (host shells); pure Rust | Stable 0.14; basic test coverage | Pure Rust, tiny | **direct** (PairingUI adapter) | Pin 0.14.1 |
| **proptest** (dev) | Property-based tests (core FSM, parsers, crypto, jitter). Alt: quickcheck, cargo-fuzz (separate suite) | https://github.com/proptest-rs/proptest | MIT/Apache-2.0 | 1.11.0 | Active | n/a | Host (dev) | note: proptest pulls `rand` — dev-only | dev-only | **dev** | Pin 1.11.0 (dev-dep) |
| **criterion** (dev) | Benchmark harness (ADR-005/latency budgets). Alt: divan, iai (later maybe) | https://github.com/bheisler/criterion.rs | MIT/Apache-2.0 | 0.8.2 | Active | n/a | Host (dev) | dev-only | **dev** | Pin 0.8.2 (dev-dep) |

### FFI / bindings generation tools

| Tool | Purpose | Source | License | Pin | Use mode / notes |
|---|---|---|---|---|---|
| **uniffi** (+uniffi_core) | Rust→Kotlin/Swift FFI for non-RT control interfaces (ADR-001/002). | https://github.com/mozilla/uniffi-rs | **MPL-2.0 (weak copyleft)** — recorded decision: generated bindings are ours; we do not modify the MPL runtime; only record license notice; `deny.toml` explicit allow with justification | 0.32.0 | **adapter/build-time + runtime** (control plane only; never RT render callbacks). Re-audit on every bump |
| **cbindgen** | C header generation (Android JNI-adjacent / C interop, build-time only) | mozilla/cbindgen | MPL-2.0 (build-time only; headers/output are ours) | 0.29.4 | **build-time only** |
| **cargo-ndk** | Android NDK cross-build plumbing | https://github.com/bbqsrc/cargo-ndk | MIT/Apache-2.0 | 4.1.2 | **tool/direct (build)** |
| **cargo-zigbuild** | Zig-based cross-compilation (glibc/musl/darwin-ish) for CI/host matrix | https://github.com/rust-cross/cargo-zigbuild | MIT | 0.23.x | **tool (build)** |
| **cross** | Containerized cross-compile for non-zig targets + reproducibility | https://github.com/cross-rs/cross | MIT/Apache-2.0 | 0.2.x | **tool (build)** |

### Platform shells (per-OS; pinned/validated per shell, not in core lockfile)

| Shell dep | Purpose | Source | License | Use mode |
|---|---|---|---|---|
| **windows-rs (windows crate)** | WASAPI loopback capture/rendering, DPAPI secure storage, services (Win) | https://github.com/microsoft/windows-rs | MIT/Apache-2.0 | direct (Win shell); lock `windows` + `windows-core` pins at Win shell |
| **screencapturekit-rs** | macOS ScreenCaptureKit wrapper (13+) | https://github.com/antonfisher/screencapturekit-rs | Apache-2.0 | direct (mac shell), optional vs raw SCK FFI |
| **PipeWire** (libpipewire, C) | Linux capture/render + per-app targeting | https://gitlab.freedesktop.org/pipewire/pipewire (LGPL-2.1+ / MIT for lib) | **MIT for the library binding we use (pipewire-rs + libpipewire is LGPL-2.1)** — record: Linux shell links libpipewire LGPL-2.1 dynamically (per-PW exception); pin an exact 1.6.x in the box image (OPEN_QUESTIONS #5) | direct (Linux shell) |
| **Oboe** (Android, C++) | Low-latency audio I/O wrapper (AAudio fallback) | https://github.com/google/oboe | Apache-2.0 | direct (Android shell) |
| **gtk4-rs** | Linux desktop shell UI | https://github.com/gtk-rs/gtk4-rs | MIT (bindings) / LGPL (gtk4 runtime, dynamic) | direct (Linux shell, later) |
| SwiftUI / AVFoundation / CoreAudio | Apple shells | Apple SDK | Proprietary OS SDK (allowed; not a dependency we ship) | direct (Apple shells) |

## Maintenance rubric (required on every bump)

Every dependency bump OR at least once per release cycle, re-run this rubric and record the outcome in a `rubric <date>` note row. A dependency is **at risk** when ≥2 rubric axes are weak or a critical advisory is open without mitigation.

| Axis | Criteria to check | Weak signal | Evidence source |
|---|---|---|---|
| **Release date** | Last stable release within 12 months | >12–18 months stale for a security-relevant crate | crates.io / release feed |
| **Last commit / activity** | Commits or merged PRs within ~3 months | Quiescent repo, unanswered issues | GitHub/web, `cargo outdated` |
| **CI state** | CI green or known-failing-but-acknowledged | Long-red CI, no fixes | upstream CI page |
| **Security responsiveness** | advisories patched within reasonable window (weeks) | Known GHSA unpatched > a release cycle | RustSec, GHSA, OSSF scorecard |
| **Unresolved advisories** | 0 CRITICAL/HIGH for runtime deps; HIGH allowed only with mitigation + decision | Open CRITICAL on runtime dep | `cargo audit` (CI) |
| **Corrective / replacement plan** | For each at-risk dep, a named fallback + trigger date | No documented fallback | this file, `cargo-deny`, risk register |

**Currently at risk / tracked:**
- `opus`/`flac-bound` FFI cross-build: tracked as RISK_R08 (cross toolchain pins + spice tasks; mitigation: pinned NDK/Xcode, vendored C via cmake).
- `quinn` GH#1778 (mobile CI): track; fallback pin freeze + rustls-ring default.
- `snow`↔dalek backend coexistence: resolve at lockfile time (B0) and record.
- `mdns-sd` 0.2x minor churn: revalidate discovery adapter on every minor.
- `qrcode`: low activity — acceptable (tiny, stable, non-security-critical) but recheck each rubric pass; fallback: `qr_code` crate (MIT) or OS-native QR.

**Policy:** on every `cargo update`/bump and at release gate, run `cargo audit` (gate: no CRITICAL/HIGH on runtime), `cargo deny check`, and update this rubric table. Any new material dependency must earn a full row here before merge (CI license-audit will fail until `deny.toml` reflects it and this table documents it).

## Explicitly rejected / flagged (with reasons)

| Candidate | Why rejected / flagged | Verdict |
|---|---|---|
| **bincode** | Archived upstream repos at time of eval; bus-factor and maintenance risk on a serialization-on-the-wire crate | rejected → postcard |
| **async-std** | Deprecated upstream in favour of smol; ecosystem momentum + support risk | rejected → tokio |
| **legacy standalone webrtc rtp / rtcp / srtp crates** | Several yanked / superseded by the webrtc-rs monorepo; historically unstable | rejected → quinn (ADR-003); revisit only on interop exit-criteria |
| **zeroconf** | License ambiguity at eval time | rejected → mdns-sd |
| **BlackHole / BackgroundMusic** | GPL-3.0 / GPL-2.0 virtual-audio drivers — cannot embed or statically distribute; may only appear as separately-installed external components with a recorded decision | rejected for embedding (PLATFORM_MATRIX C) |
| **eqMac (open snapshot)** | Apache-2.0 snapshot is stale (driver code 2021); core/capture engine ships closed from a private fork; cannot reuse or reverse-engineer closed binaries | rejected → native APIs + permissive wrappers (PLATFORM_MATRIX C; DECISION_LOG entry) |
| **symphonia** | MPL-2.0; decode-only breadth beyond current needs; would add weak-copyleft runtime surface for no current requirement (FLAC significantly + Opus covered) | flagged/rejected now → revisit only if a broad container-decode requirement appears (recorded decision required) |
| **libflac LGPL option** | libFLAC is dual BSD-3/LGPL; we use the BSD-3 variant only (no LGPL burden) | decision recorded — use BSD-3 variant |
| **AAC / MP3 codecs** | Licensing/patent complexity for distribution | rejected → Opus (ADR-004) |
| **unsupported standalone QOI/others** | not applicable | n/a |

## Pending pins / open checks (B0)

- PipeWire exact pin (1.6.x) in the Linux box image — OPEN_QUESTIONS #5; pin at B0/B1.
- `snow`↔`curve25519-dalek` 5.x coexistence + feature unification at lockfile time; record decision in DECISION_LOG.
- `deny.toml` policy file landing with the B0 lockfile; license-audit CI DISABLED until then (see `.github/workflows/license-audit.yml` `if: false`).
- ring/nasm/asm objects enumerated in SBOM (SBOM_POLICY.md).

## Notices & SBOM

Per SBOM_POLICY.md: `cargo-cyclonedx` / `syft` in CI; per-vendor LICENSE/NOTICE shipped in every artifact; `cargo audit` + GHSA watchers; maintenance rubric re-run per bump (above). Attribution template is in LICENSE-NOTICES.md.
