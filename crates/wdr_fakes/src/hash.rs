//! [`NullSource`], [`NullSink`], [`HashSink`] and the sample counter.
//!
//! The canonical hash is **blake3** of the exact wire bytes (see the crate
//! docs). "Lossless equality" later asserts `hash(decoded) == hash(source)`.

use crate::source::{SampleFormat, SourceFormat};

pub const HASH_FN: &str = "blake3";

/// Running blake3 over canonical audio bytes. `Default::default()` is the
/// clean starting state; use the same instance to both update and finalize.
#[derive(Debug, Clone)]
pub struct HashSinkState {
    hasher: blake3::Hasher,
}

impl Default for HashSinkState {
    fn default() -> Self {
        Self {
            hasher: blake3::Hasher::new(),
        }
    }
}

impl HashSinkState {
    /// Hash a chunk of **sample bytes** (already canonical i16-le / i24-le).
    pub fn update_bytes(&mut self, bytes: &[u8]) -> &mut Self {
        self.hasher.update(bytes);
        self
    }

    /// Update over interleaved `i16` samples (converts to LE bytes first).
    pub fn update_i16(&mut self, samples: &[i16]) -> &mut Self {
        let mut buf = [0u8; 256];
        for chunk in samples.chunks(128) {
            for (i, &s) in chunk.iter().enumerate() {
                buf[i * 2..i * 2 + 2].copy_from_slice(&s.to_le_bytes());
            }
            self.hasher.update(&buf[..chunk.len() * 2]);
        }
        self
    }

    /// Finalize (idempotent; does not consume the state).
    pub fn finish(&self) -> blake3::Hash {
        self.hasher.finalize()
    }

    /// Const-size lowercase hex, fixed by [`HASH_FN`].
    pub fn hex(&self) -> String {
        self.finish().to_hex().to_string()
    }
}

/// [`PcmSource`] convenience: a chunk-hashing sink around [`HashSinkState`].
/// Push canonical bytes through [`HashSinkState::update_bytes`]; then compare
/// [`HashSink::final_state`] across runs.
#[derive(Debug, Default)]
pub struct HashSink {
    state: HashSinkState,
    samples: u64,
}

impl HashSink {
    pub fn new() -> Self {
        Self::default()
    }

    /// Same final hash as `drain_hash` would (see [`HashSinkState`]).
    pub fn start() -> Self {
        Self::default()
    }

    pub fn push(&mut self, bytes: &[u8]) {
        self.state.update_bytes(bytes);
        self.samples = self.samples.saturating_add(bytes.len() as u64);
    }

    /// Number of sample bytes pushed so far.
    pub fn samples_pushed(&self) -> u64 {
        self.samples
    }

    pub fn final_state(&self) -> &HashSinkState {
        &self.state
    }

    pub fn hex(&self) -> String {
        self.state.hex()
    }
}

/// A source that produces exactly `samples` of silence per chunk. Like
/// [`Fixture`]'s Silence kind, this intentionally streams *zero sample values*
/// — it is a cheap reference point for "no signal" in downstream tests but is
/// NOT a golden fixture (it has no byte-layout guarantee).
#[derive(Debug)]
pub struct NullSource {
    pub format: SourceFormat,
}

impl Default for NullSource {
    fn default() -> Self {
        Self {
            format: SourceFormat::stereo_i16_48k(),
        }
    }
}

impl NullSource {
    pub fn new(format: SourceFormat) -> Self {
        Self { format }
    }
}

impl crate::source::PcmSource for NullSource {
    fn format(&self) -> SourceFormat {
        self.format
    }
    fn next_chunk(&mut self, samples: u32) -> crate::source::PcmChunkRef<'_> {
        // NullSource has no persistent buffer; caller treats length = samples.
        let bytes = match self.format.format {
            SampleFormat::I16 => samples as usize * 2,
            SampleFormat::I24 => samples as usize * 3,
        };
        static ZEROES: [u8; 1024] = [0u8; 1024];
        crate::source::PcmChunkRef {
            bytes: &ZEROES[..core::cmp::min(bytes, ZEROES.len())],
            len: samples as usize,
            per_sample_bytes: self.format.format.bytes_per_sample(),
            format: self.format,
        }
    }
}

/// A sink that drops samples synchronously and counts them (a NullSink).
#[derive(Debug, Default, Clone)]
pub struct NullSink {
    pub samples: u64,
    pub chunks: u64,
}

impl NullSink {
    pub fn new() -> Self {
        Self::default()
    }
    /// Count `count` samples rendered.
    pub fn render_samples(&mut self, _bytes: &[u8], count: u64) {
        self.samples += count;
        self.chunks += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_sink_counts() {
        let mut s = NullSink::new();
        s.render_samples(&[0u8; 16], 8);
        s.render_samples(&[0u8; 16], 8);
        assert_eq!(s.samples, 16);
        assert_eq!(s.chunks, 2);
    }

    #[test]
    fn hash_state_reaches_layout() {
        let mut h = HashSinkState::default();
        h.update_bytes(&[1, 2, 3, 4]);
        assert_eq!(h.hex().len(), 64);
    }
}
