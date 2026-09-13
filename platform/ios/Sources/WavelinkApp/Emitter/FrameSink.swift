//
//  FrameSink.swift
//  WDRiOSEmitterApp
//
//  WS4 seam-level wiring: the Swift analogue of the shared core's
//  `wdr_refsim::sink::FrameSink` (and the Android `FrameSink.kt`).
//
//  onFormat once -> onBlock (whole interleaved LE i16 blocks) -> finish.
//  The Rust core (encode -> AEAD -> QUIC) is reached through a uniffi/JNI
//  control-plane bridge (ADR-001/ADR-002; non-RT interaces only) wired by the
//  mobile-backbone task. Today the shell fills the seam with a real,
//  host-testable FixtureFrameSink: real counts + CRC32 over the same canonical
//  byte stream the Rust hash sink verifies, so FR-053 status is derived from
//  genuinely-observed blocks rather than demo randomness.
//

import Foundation

/// The emitter frame seam (protocol mirror of `FrameSink.kt` /
/// `wdr_refsim::sink::FrameSink`).
protocol FrameSink: AnyObject {
    /// Capture format is stable and delivered once, before the first block.
    func onFormat(channels: Int, bitDepth: Int, sampleRateHz: Int)

    /// One captured PCM block (interleaved little-endian i16 bytes). The sink
    /// accumulates to whole frames and counts what it accepts.
    func onBlock(data: Data)

    /// End of session: flush accumulation; no calls after.
    func finish()
}

/// Concrete host-testable [FrameSink] the shell uses today: accumulates
/// whole-frame-aligned canonical bytes, counts samples/blocks, and computes a
/// deterministic CRC-32 over the accepted byte stream (the lossless
/// bytes-out==bytes-in readout the Rust hash sink provides).
final class FixtureFrameSink: FrameSink {
    private let channels: Int
    private let bitDepth: Int
    private let sampleRateHz: Int

    private(set) var blockCount: Int = 0
    private(set) var samplesAccepted: Int = 0
    private(set) var bytesAccepted: Int = 0
    private(set) var formatAnnounced: Bool = false
    private(set) var finished: Bool = false
    private lazy var crc = CRC32()

    init(channels: Int = 2, bitDepth: Int = 16, sampleRateHz: Int = 48_000) {
        self.channels = channels
        self.bitDepth = bitDepth
        self.sampleRateHz = sampleRateHz
    }

    /// Whole interleaved frame size in bytes (one sample per channel).
    var frameSizeBytes: Int { channels * bitDepth / 8 }

    func onFormat(channels: Int, bitDepth: Int, sampleRateHz: Int) {
        precondition(
            channels == self.channels && bitDepth == self.bitDepth && sampleRateHz == self.sampleRateHz,
            "FixtureFrameSink format mismatch (\(channels)ch/\(bitDepth)bit/\(sampleRateHz)Hz)"
        )
        precondition(!formatAnnounced, "FixtureFrameSink.onFormat delivered twice")
        formatAnnounced = true
    }

    func onBlock(data: Data) {
        precondition(!finished, "FrameSink.onBlock after finish() — contract violation")
        precondition(formatAnnounced, "FrameSink.onBlock before onFormat() — contract violation")
        let whole = data.count - (data.count % frameSizeBytes)
        guard whole > 0 else { return }
        data.prefix(whole).withUnsafeBytes { buf in
            crc.update(buf)
        }
        bytesAccepted += whole
        samplesAccepted += whole / 2
        blockCount += 1
    }

    func finish() {
        finished = true
    }

    /// Deterministic canonical fixture: interleaved L-R little-endian i16
    /// frames (left = +564·n pattern A, right = −904·n pattern B — the same
    /// shape `wdr_fakes` generates), for host seam validation. No OS entropy.
    static func makeFixture(frames: Int, seed: Int32 = 1) -> Data {
        var buf = Data()
        buf.reserveCapacity(frames * 4)
        for f in 0..<frames {
            var l = Int16(truncatingIfNeeded: (seed + Int32(f)) * 564).littleEndian
            var r = Int16(truncatingIfNeeded: (seed + Int32(f)) * -904).littleEndian
            withUnsafeBytes(of: &l) { buf.append(contentsOf: $0) }
            withUnsafeBytes(of: &r) { buf.append(contentsOf: $0) }
        }
        return buf
    }

    /// Deterministic CRC-32 over every accepted byte (as uppercase hex).
    var contentCrcHex: String { String(format: "%08X", crc.value) }
}

/// Minimal CRC-32 (IEEE 802.3) — mirrors the Rust `frame_crc32` polynomial.
private final class CRC32 {
    private var crc: UInt32 = 0xFFFF_FFFF
    private static let table: [UInt32] = {
        (0..<256).map { i in
            var c = UInt32(i)
            for _ in 0..<8 { c = (c & 1) != 0 ? (c >> 1) ^ 0xEDB8_8320 : c >> 1 }
            return c
        }
    }()

    var value: UInt32 { crc ^ 0xFFFF_FFFF }

    func update(_ bytes: UnsafeRawBufferPointer) {
        var c = crc
        for b in bytes {
            c = (c >> 8) ^ Self.table[Int((c ^ UInt32(truncatingIfNeeded: b)) & 0xFF)]
        }
        crc = c
    }
}
