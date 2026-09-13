//! Deterministic synthetic PCM generators and the [`PcmSource`] trait.
//!
//! Every generator seeds a ChaCha12-based RNG (`rand_chacha::ChaCha12Rng`)
//! from an explicit 32-byte seed, so a fixed seed produces a byte-identical,
//! replayable chunk stream across runs and machines.

use rand_chacha::rand_core::{RngCore, SeedableRng};
use rand_chacha::ChaCha12Rng;

use crate::hash::HashSinkState;

/// Uniform 32-byte seed (the `Seed` type of `ChaCha12Rng`).
pub type SourceSeed = [u8; 32];

/// The canonical seed anchoring every golden-vector fixture. Changing it
/// invalidates all recorded golden hashes (they must stay in sync with the
/// report's golden table).
pub const SOURCE_SEED: SourceSeed = *b"wdr-b0-fakes-2026-09-06=seed^123";

/// Global zero (silence) sample level, canonical for both i16 and 24-bit.
pub const SILENCE: i32 = 0;

/// Derive a stable 32-byte seed for `feature` from [`SOURCE_SEED`] (blake3 mix,
/// dependency-free and deterministic). Each fixture type streams from an
/// independent PRNG lane so no two fixtures drift into correlation.
pub fn source_seed(feature: &[u8]) -> SourceSeed {
    let mut seed = SOURCE_SEED;
    let mix = blake3::hash(feature);
    let mb = mix.as_bytes();
    for (dst, src) in seed.iter_mut().zip(mb.iter()) {
        *dst = dst.wrapping_add(*src);
    }
    seed
}

/// Sample container (mirror of `wdr_proto::SampleRepr`, kept local).
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum SampleFormat {
    /// 16-bit signed little-endian, 2 bytes per sample.
    I16,
    /// 24-bit samples packed in the low 3 bytes of an `i32` (i24-le), 3 bytes.
    I24,
}

impl SampleFormat {
    pub fn bytes_per_sample(&self) -> usize {
        match self {
            SampleFormat::I16 => 2,
            SampleFormat::I24 => 3,
        }
    }
    pub fn size_bytes(&self, sample_count: usize) -> usize {
        sample_count * self.bytes_per_sample()
    }
}

/// Channel layout + interleave rule.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum ChannelKind {
    /// Single channel, no padding.
    Mono,
    /// `L–R–L–R` interleave — canonical for stereo fixtures.
    Stereo,
}

impl core::fmt::Display for ChannelKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ChannelKind::Mono => f.write_str("mono"),
            ChannelKind::Stereo => f.write_str("stereo"),
        }
    }
}

/// Which channel-ID pattern a lane carries (`LeftPattern` = pattern A).
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Stereo {
    LeftPattern,
    RightPattern,
}

impl Stereo {
    /// Distinct fixed DC levels used by the channel-id fixtures so channel
    /// order is unambiguous and verifiable on decode.
    pub const fn level(&self) -> i32 {
        match self {
            Stereo::LeftPattern => 564,
            Stereo::RightPattern => -904,
        }
    }
    pub fn is_left(&self) -> bool {
        matches!(self, Stereo::LeftPattern)
    }
}

/// Interleave-level channel id for the mono/stereo channel-id fixtures.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct ChannelId {
    pub kind: ChannelKind,
    pub index: usize,
    pub pattern: Stereo,
}

impl ChannelId {
    /// The lane (`0`-based channel index) this id denotes.
    pub const fn lane(&self) -> usize {
        match self.kind {
            ChannelKind::Mono => 0,
            ChannelKind::Stereo => self.index,
        }
    }
}

/// Resolved format descriptor of a source (format/rate/repr/channels).
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct SourceFormat {
    pub rate_hz: u32,
    pub format: SampleFormat,
    pub channels: ChannelKind,
}

/// A read-only link to the most recently generated chunk (see [`PcmSource`]).
/// The link's `bytes` are valid until the next call on the source.
#[derive(Clone, Copy)]
pub struct PcmChunkRef<'a> {
    pub bytes: &'a [u8],
    pub len: usize,
    pub per_sample_bytes: usize,
    pub format: SourceFormat,
}

impl<'a> PcmChunkRef<'a> {
    /// Canonical wire bytes of this chunk.
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }
    pub fn sample_count(&self) -> usize {
        self.len
    }
    pub fn format(&self) -> SourceFormat {
        self.format
    }
}

impl core::fmt::Debug for PcmChunkRef<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "PcmChunkRef {{ {} samples @ {} {}/{:?} }}",
            self.len, self.format.rate_hz, self.format.channels, self.format.format
        )
    }
}

/// Trait for sources that produce canonical PCM sample streams.
///
/// `next_chunk(samples)` advances by up to `samples` **samples** (per format,
/// per-channel) and returns a link to the produced byte chunk. A lossless
/// `hash(source) == hash(decoded)` assertion hashes the exact `bytes` of every
/// chunk through a [`HashSink`](crate::hash::HashSinkState).
///
/// Implementers with a bounded stream return `len == 0` once exhausted; the
/// default [`PcmSource::drain_hash`] relies on that.
pub trait PcmSource {
    fn format(&self) -> SourceFormat;
    fn next_chunk(&mut self, samples: u32) -> PcmChunkRef<'_>;

    /// Drain the whole (bounded) stream in `chunk`-size pieces and return the
    /// running blake3 hash of the canonical bytes.
    fn drain_hash(&mut self, chunk: u32) -> HashSinkState {
        let mut sink = HashSinkState::default();
        if chunk == 0 {
            return sink;
        }
        loop {
            let got = self.next_chunk(chunk);
            if got.len == 0 {
                break;
            }
            sink.update_bytes(got.bytes);
        }
        sink
    }
}

impl SourceFormat {
    pub const fn stereo_i16_48k() -> Self {
        Self {
            rate_hz: 48_000,
            format: SampleFormat::I16,
            channels: ChannelKind::Stereo,
        }
    }
    pub const fn mono_i24_44_1k() -> Self {
        Self {
            rate_hz: 44_100,
            format: SampleFormat::I24,
            channels: ChannelKind::Mono,
        }
    }
    pub const fn stereo_i16_44_1k() -> Self {
        Self {
            rate_hz: 44_100,
            format: SampleFormat::I16,
            channels: ChannelKind::Stereo,
        }
    }
    pub const fn stereo_i24_48k() -> Self {
        Self {
            rate_hz: 48_000,
            format: SampleFormat::I24,
            channels: ChannelKind::Stereo,
        }
    }
    pub fn channel_count(&self) -> usize {
        match self.channels {
            ChannelKind::Mono => 1,
            ChannelKind::Stereo => 2,
        }
    }
}

/// A fixture generator implementing [`PcmSource`] with an independently seeded,
/// deterministic ChaCha12 RNG (or no RNG at all for deterministic shapes).
#[derive(Debug, Clone)]
pub struct Fixture {
    cfg: FixtureConfig,
    rng: ChaCha12Rng,
    generated: u64,
    buffer: Vec<i32>,
    bytebuf: Vec<u8>,
    phase: f64,
    seed: SourceSeed,
}

#[derive(Debug, Clone)]
pub struct FixtureConfig {
    pub kind: FixtureKind,
    pub format: SampleFormat,
    pub rate_hz: u32,
    pub channels: ChannelKind,
    pub total_samples: u64,
    pub sweep: (f64, f64),
}

impl FixtureConfig {
    pub fn channel_count(&self) -> usize {
        match self.channels {
            ChannelKind::Mono => 1,
            ChannelKind::Stereo => 2,
        }
    }
}

impl PartialEq for FixtureConfig {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.format == other.format
            && self.rate_hz == other.rate_hz
            && self.channels == other.channels
            && self.total_samples == other.total_samples
    }
}
impl Eq for FixtureConfig {}

#[derive(Debug, Clone)]
pub enum FixtureKind {
    Silence,
    /// + FULL_SCALE impulse every `period` samples.
    ImpulseTrain {
        period: u32,
    },
    /// Linear frequency sweep `f0 → f1` over the whole stream.
    SineSweep {
        f0: f64,
        f1: f64,
    },
    /// ±32767 alternating sample-wise (per frame).
    FullScaleEdge,
    /// Seeded pseudo-random PCM (uniform u32 mapped into i16/i24 range).
    PseudoRandomPcm,
    /// Mono channel-id: constant `pattern` level.
    ChannelId {
        lane: Stereo,
    },
    /// Mono with an explicit level (default silence-level when absent).
    ChannelIdMono {
        level: i32,
    },
}

impl PartialEq for FixtureKind {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (FixtureKind::Silence, FixtureKind::Silence) => true,
            (FixtureKind::ImpulseTrain { period: a }, FixtureKind::ImpulseTrain { period: b }) => {
                a == b
            }
            (FixtureKind::SineSweep { f0, f1 }, FixtureKind::SineSweep { f0: g0, f1: g1 }) => {
                f0.to_bits() == g0.to_bits() && f1.to_bits() == g1.to_bits()
            }
            (FixtureKind::FullScaleEdge, FixtureKind::FullScaleEdge) => true,
            (FixtureKind::PseudoRandomPcm, FixtureKind::PseudoRandomPcm) => true,
            (FixtureKind::ChannelId { lane: a }, FixtureKind::ChannelId { lane: b }) => a == b,
            (FixtureKind::ChannelIdMono { level: a }, FixtureKind::ChannelIdMono { level: b }) => {
                a == b
            }
            _ => false,
        }
    }
}
impl Eq for FixtureKind {}

impl FixtureKind {
    fn tag(&self) -> String {
        match self {
            FixtureKind::Silence => "silence".to_string(),
            FixtureKind::ImpulseTrain { period } => format!("impulse-train-{period}"),
            FixtureKind::SineSweep { f0, f1 } => format!("sine-sweep-{f0:.1}-{f1:.1}"),
            FixtureKind::FullScaleEdge => "full-scale-edge".to_string(),
            FixtureKind::PseudoRandomPcm => "pseudo-random-pcm".to_string(),
            FixtureKind::ChannelId { lane } => format!("channel-id-{lane:?}"),
            FixtureKind::ChannelIdMono { level } => format!("channel-id-mono-{level}"),
        }
    }
}

impl PcmSource for Fixture {
    fn format(&self) -> SourceFormat {
        SourceFormat {
            rate_hz: self.cfg.rate_hz,
            format: self.cfg.format,
            channels: self.cfg.channels,
        }
    }

    fn next_chunk(&mut self, samples: u32) -> PcmChunkRef<'_> {
        self.buffer.clear();
        self.bytebuf.clear();
        let mut wanted = samples as usize;
        if self.cfg.total_samples > 0 {
            let remain = (self.cfg.total_samples - self.generated) as usize;
            wanted = core::cmp::min(wanted, remain);
        }
        if wanted > 0 {
            self.fill_batch(wanted);
            self.generated += wanted as u64;
        }
        self.encode_buffered();
        PcmChunkRef {
            bytes: &self.bytebuf,
            len: self.buffer.len(),
            per_sample_bytes: self.cfg.format.bytes_per_sample(),
            format: self.format(),
        }
    }
}

impl Fixture {
    /// Create a fixture over `format`/`rate`/`channels`; `total_samples == 0`
    /// means unbounded (drainers must bound). The RNG (where used) is seeded
    /// from [`FixtureKind::tag`] via [`source_seed`], so every fresh instance
    /// of the same kind is already deterministic.
    pub fn new(
        kind: FixtureKind,
        format: SampleFormat,
        rate_hz: u32,
        channels: ChannelKind,
        total_samples: u64,
    ) -> Self {
        let sweep = match &kind {
            FixtureKind::SineSweep { f0, f1 } => (*f0, *f1),
            _ => (0.0, 0.0),
        };
        let cfg = FixtureConfig {
            kind,
            format,
            rate_hz,
            channels,
            total_samples,
            sweep,
        };
        let seed = source_seed(cfg.kind.tag().as_bytes());
        let rng = ChaCha12Rng::from_seed(seed);
        Self {
            cfg,
            rng,
            generated: 0,
            buffer: Vec::new(),
            bytebuf: Vec::new(),
            phase: 0.0,
            seed,
        }
    }

    /// In-place seed override (returned for chaining). Deterministic; does not
    /// change the format fields.
    pub fn with_seed(&mut self, seed: SourceSeed) -> &mut Self {
        self.rng = ChaCha12Rng::from_seed(seed);
        self.seed = seed;
        self
    }

    /// Builder-style: consume and return a new fixture with `seed` applied.
    pub fn seed(mut self, seed: SourceSeed) -> Self {
        self.with_seed(seed);
        self
    }

    /// Effective 32-byte seed (for fingerprints / reporting).
    pub fn seed_bytes(&self) -> &SourceSeed {
        &self.seed
    }

    /// Total sample frames generated so far.
    pub fn generated(&self) -> u64 {
        self.generated
    }

    fn fill_batch(&mut self, want: usize) {
        let count = self.cfg.channel_count();
        match &self.cfg.kind {
            FixtureKind::Silence | FixtureKind::ChannelIdMono { .. } => {
                let level = match self.cfg.kind {
                    FixtureKind::ChannelIdMono { level } => level,
                    _ => SILENCE,
                };
                self.buffer.extend(core::iter::repeat_n(level, want));
            }
            FixtureKind::ChannelId { lane } => {
                let lane = *lane;
                let mut i = 0usize;
                while i + count <= want {
                    for ch in 0..count {
                        let is_left = ch == 0;
                        let level = if is_left == lane.is_left() {
                            lane.level()
                        } else {
                            -lane.level()
                        };
                        self.buffer.push(level);
                        i += 1;
                    }
                }
                while i < want {
                    self.buffer.push(SILENCE);
                    i += 1;
                }
            }
            FixtureKind::ImpulseTrain { period } => {
                let period = *period as u64;
                for k in 0..want as u64 {
                    let idx = self.generated + k;
                    self.buffer.push(if idx.is_multiple_of(period) {
                        32767
                    } else {
                        SILENCE
                    });
                }
            }
            FixtureKind::FullScaleEdge => {
                for k in 0..want as u64 {
                    let idx = self.generated + k;
                    self.buffer.push(if (idx / count as u64).is_multiple_of(2) {
                        32767
                    } else {
                        -32767
                    });
                }
            }
            FixtureKind::SineSweep { .. } => {
                let (f0, f1) = self.cfg.sweep;
                let total = self.cfg.total_samples;
                let mut ph = self.phase;
                let start = self.generated;
                let mut samples: Vec<i32> = Vec::with_capacity(want);
                for k in 0..want {
                    let idx = start + k as u64;
                    let frac = if total > 0 {
                        idx as f64 / total as f64
                    } else {
                        idx as f64 / (self.cfg.rate_hz as f64)
                    };
                    let freq = f0 + (f1 - f0) * frac;
                    ph += freq / self.cfg.rate_hz as f64;
                    let value = (ph * std::f64::consts::TAU).sin();
                    samples.push((value * 24_000.0).round() as i32);
                }
                // Spread each per-frame value across all channels so every
                // lane produces an identical waveform (deterministic decode).
                let frames = want / count;
                for &v in samples.iter().take(frames) {
                    for _ in 0..count {
                        self.buffer.push(v);
                    }
                }
                let produced = frames * count;
                for _ in produced..want {
                    self.buffer.push(SILENCE);
                }
                self.phase = ph;
            }
            FixtureKind::PseudoRandomPcm => {
                for _ in 0..want {
                    // Uniform over the full i32 range; each channel the same
                    // draw keeps lanes correlated for determinism checks.
                    self.buffer.push(self.rng.next_u32() as i32);
                }
            }
        }
    }

    /// Append canonical bytes for the buffered samples to `self.bytebuf`.
    fn encode_buffered(&mut self) {
        match self.cfg.format {
            SampleFormat::I16 => {
                for &v in &self.buffer {
                    let s = v.clamp(-32768, 32767) as i16;
                    self.bytebuf.extend_from_slice(&s.to_le_bytes());
                }
            }
            SampleFormat::I24 => {
                for &v in &self.buffer {
                    let c = v.clamp(-(1 << 23) + 1, (1 << 23) - 1);
                    let b = c.to_le_bytes();
                    self.bytebuf.extend_from_slice(&b[..3]);
                }
            }
        }
    }
}

/// Decode a 3-byte little-endian 24-bit group back into a signed `i32`.
pub fn unpack_i24(bytes: &[u8]) -> i32 {
    assert!(bytes.len() >= 3, "i24 unpack needs >= 3 bytes");
    let v = bytes[0] as i32 | ((bytes[1] as i32) << 8) | ((bytes[2] as i32) << 16);
    if v & (1 << 23) != 0 {
        v | !((1 << 24) - 1)
    } else {
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn determinism_two_runs_identical_hash() {
        let mk = || {
            Fixture::new(
                FixtureKind::PseudoRandomPcm,
                SampleFormat::I16,
                48_000,
                ChannelKind::Stereo,
                10_000,
            )
        };
        let mut a = mk();
        let mut b = mk();
        assert_eq!(a.drain_hash(512).hex(), b.drain_hash(512).hex());
    }

    #[test]
    fn stereo_channel_id_interleaves_left_pattern_first() {
        let mut s = Fixture::new(
            FixtureKind::ChannelId {
                lane: Stereo::LeftPattern,
            },
            SampleFormat::I16,
            48_000,
            ChannelKind::Stereo,
            4,
        );
        let ch = s.next_chunk(4);
        let a = i16::from_le_bytes([ch.bytes[0], ch.bytes[1]]);
        let b = i16::from_le_bytes([ch.bytes[2], ch.bytes[3]]);
        let c = i16::from_le_bytes([ch.bytes[4], ch.bytes[5]]);
        let d = i16::from_le_bytes([ch.bytes[6], ch.bytes[7]]);
        let level = Stereo::LeftPattern.level() as i16;
        assert_eq!(a, level);
        assert_eq!(b, -level);
        assert_eq!(c, level);
        assert_eq!(d, -level);
    }
}
