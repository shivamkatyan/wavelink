//! WDR fakes — deterministic synthetic PCM sources, hash sink, null devices
//! and fake adapters (project `R-B0-FAKES`, task `t-B0-fakes`).
//!
//! Mirrors the adapter names from `docs/planning/ARCHITECTURE.md`
//! (§Shared-core / adapter boundary): `CaptureSource`, `RenderSink`,
//! `Discovery`, `PairingUi`, `EntitlementProvider`, `Clock`, `PermissionGate`,
//! `Storage`. Everything here is **deterministic** — no OS entropy anywhere on
//! the data path — so tests can replay byte-identical PCM plus exact hashes.
//!
//! # Canonical byte representation (defined once, reused by codec goldens)
//!
//! * **16-bit** samples are `i16` little-endian, channel-interleaved
//!   `L–R–L–R` for stereo (mono is a single channel, no padding).
//! * **24-bit** samples are `i32`-by-value with the top byte zero (0x00RRGGBB)
//!   — byte order `little-endian i24`, low 3 bytes meaningful, high byte
//!   stripped on the wire. The codec worker (which owns `SampleRepr::I24Packed`
//!   packing) consumes these 3-byte lows; this crate deliberately keeps the
//!   24-bit path as *values* so the interleave rule (L–R–L–R) is the only
//!   ordering the generator must own.
//!
//! # Lossless-equality contract
//!
//! `NullSource → PcmSource → HashSink` hashes the *canonical byte stream* for a
//! fixture (all bytes the lossless codec would encode), so
//! `hash(decoded stream) == golden for fixture` asserts true losslessness.
//!
//! # Golden fixtures (see `TEST_PLAN.md` §Golden audio)
//!
//! `silence`, `impulse_train`, `sine_sweep`, `full_scale_edge`, seeded
//! `pseudo-random PCM`, `mono/stereo channel-id` at 44.1/48 kHz and 16/24-bit.
//! All are produced through a small set of builder helpers on [`Fixture`] and
//! [`PcmSource`]; the recorded golden hashes live in
//! `docs/orchestration/reports/t-B0-fakes.md` (table "Canonical golden hashes").

pub mod adapters;
pub mod hash;
pub mod source;

pub use adapters::{
    CaptureSource, CaptureStem, Clock, Discovery, FakeCaptureSource, FakeCaptureSourceConfig,
    FakeClock, FakeClockError, FakeDiscovery, FakeDiscoveryPeer, FakeEntitlement,
    FakeEntitlementProvider, FakePairingUi, FakePairingUiOutcome, FakePermissionGate,
    FakeRenderSink, FakeRenderSinkStats, FakeStorage, FakeTier, PairingUi, PermissionGate,
    RenderUnderrun,
};
pub use hash::{HashSink, HashSinkState};
pub use source::{
    unpack_i24, Fixture, FixtureConfig, FixtureKind, PcmChunkRef, PcmSource, SampleFormat,
    SourceFormat, SourceSeed, Stereo, SILENCE, SOURCE_SEED,
};
