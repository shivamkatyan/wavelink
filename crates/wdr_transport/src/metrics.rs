//! Transport metrics counters.
//!
//! What is *actually measurable* in `quinn 0.11` for a datagram media path:
//!
//! * Per-datagram **ack is NOT exposed app-side** — datagrams are unreliable
//!   (RFC 9221); quinn gives no `datagram_acked` signal. So `datagram_lost` is
//!   **not** measured from ack; we report the closest honest proxies:
//!   * send-side congestion drops we induced (`dropped_congestion`) — `send`
//!     returning `Blocked`, which we count by choice (never silently),
//!   * transport path stats quinn *does* expose: `lost_packets`, `congestion_events`,
//!     `rtt`, `cwnd`, `lost_bytes` (`ConnectionStats.path`),
//!   * `frame_tx`/`frame_rx` datagram-frame counts (UDP datagrams carrying
//!     DATAGRAM frames) and UDP datagram counts (`udp_tx.datagrams`).
//!
//! The production B1 system asserts the "datagram credit high-water" metric from
//! PROTOCOL_SPEC §Numeric bounds via the send-buffer space; the spike keeps the
//! counters minimal and documents the gap.

use crate::wire::SendDatagramOutcome;

/// App-side counters for the loopback spike.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Metrics {
    /// Reliable frames ACKed (stream path; datagrams have no app ack).
    pub frames_sent: u64,
    /// Datagrams we dropped because the QUIC send buffer was full (congestion).
    pub dropped_congestion: u64,
    /// Datagrams rejected as too large (path MTU bound).
    pub dropped_too_large: u64,
    /// Datagrams delivered by the receiver.
    pub frames_received: u64,
}

impl Metrics {
    /// Bump counters from an outcome.
    pub fn note_send(&mut self, outcome: SendDatagramOutcome) {
        match outcome {
            SendDatagramOutcome::Sent => self.frames_sent += 1,
            SendDatagramOutcome::BufferFull => self.dropped_congestion += 1,
            SendDatagramOutcome::TooLarge { .. } => self.dropped_too_large += 1,
            _ => {}
        }
    }
}

/// Snapshot of what `quinn::ConnectionStats` exposes for one connection epoch.
#[derive(Debug, Clone, Copy, Default)]
pub struct PathReport {
    /// Best RTT estimate (`ConnectionStats.path.rtt`).
    pub rtt_micros: u64,
    /// Congestion window (bytes).
    pub cwnd: u64,
    /// Our observed lost packets on this path.
    pub lost_packets: u64,
    /// Our observed lost bytes on this path.
    pub lost_bytes: u64,
    /// Congestion events on this path.
    pub congestion_events: u64,
    /// ACK frames we received (`frame_rx.acks`).
    pub ack_frames: u64,
    /// UDP datagrams we transmitted (all frames, not just DATAGRAM).
    pub udp_tx_datagrams: u64,
    /// Current path MTU (UDP payload bytes).
    pub current_mtu: u16,
}

impl PathReport {
    /// Build from the quinn stats types.
    pub fn from_path_stats(path: quinn::PathStats, ack_frames: u64, udp_tx_datagrams: u64) -> Self {
        Self {
            rtt_micros: path.rtt.as_micros() as u64,
            cwnd: path.cwnd,
            lost_packets: path.lost_packets,
            lost_bytes: path.lost_bytes,
            congestion_events: path.congestion_events,
            ack_frames,
            udp_tx_datagrams,
            current_mtu: path.current_mtu,
        }
    }
}
