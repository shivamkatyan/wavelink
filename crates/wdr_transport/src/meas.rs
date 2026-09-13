//! Frame-size / MTU accounting for the spike measurement table.
//!
//! These are **pure byte-budget calculations** (no I/O): they mirror the
//! `wdr_proto` frame-header sizes and the QUIC overhead bounds so the report's
//! "fits-in-datagram" numbers have an analytic cross-check alongside the
//! measured `conn.max_datagram_size()`.

/// Postcard-encoded header overhead of one `wdr_proto::Frame` (worst-case
/// varints, `Frame::header_max_len = 64`). We use 64 to be conservative.
pub const FRAME_HEADER_MAX_BYTES: usize = 64;

/// Worst-case QUIC + DATAGRAM-frame overhead added to a payload inside one UDP
/// datagram (1-byte frame type + ≤8-byte length + ≤3-byte PN slack; we bound
/// conservatively).
pub const QUIC_DATAGRAM_OVERHEAD: usize = 12;

/// Bytes of raw PCM for N ms at 48 kHz/s/ch, 16-bit stereo.
///
/// `samples_per_channel = 48000 * ms / 1000`; bytes = 2 samples × 2 ch × count.
#[must_use]
pub fn raw_pcm_ms_bytes(ms: u64) -> usize {
    let samples_per_ch = 48_000 * ms / 1_000;
    (samples_per_ch * 2 * 2) as usize
}

/// FLAC of incompressible noise ≈ the raw PCM size (no entropy win), plus a
/// small constant leading block overhead — realistically **≥** the PCM size.
#[must_use]
pub fn flac_noise_ms_bytes(ms: u64) -> usize {
    raw_pcm_ms_bytes(ms)
}

/// Full datagram payload needed for one `Frame` (header + payload + QUIC
/// overhead) so the packet still fits inside `conn.max_datagram_size()`.
#[must_use]
pub fn frame_packet_bytes(payload_bytes: usize) -> usize {
    FRAME_HEADER_MAX_BYTES + payload_bytes + QUIC_DATAGRAM_OVERHEAD
}

/// Whether a `payload`-byte frame fits in a `max_dgram`-byte payload allowance.
#[must_use]
pub fn fits_in_datagram(payload_bytes: usize, max_dgram: usize) -> bool {
    frame_packet_bytes(payload_bytes) <= max_dgram
}

/// Fit matrix for the standard block sizes (@48 kHz, 16-bit stereo):
/// returns `[(block_ms, raw_pcm_bytes, frame_packet_bytes, fits?)]`.
#[must_use]
pub fn fit_matrix(max_dgram: usize) -> Vec<(u64, usize, usize, bool)> {
    [20, 10, 5]
        .iter()
        .map(|&ms| {
            let raw = raw_pcm_ms_bytes(ms);
            let packet = frame_packet_bytes(raw);
            (ms, raw, packet, fits_in_datagram(raw, max_dgram))
        })
        .collect()
}
