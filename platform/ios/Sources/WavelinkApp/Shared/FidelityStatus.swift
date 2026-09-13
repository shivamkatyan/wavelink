//
//  FidelityStatus.swift
//  WDRReceiverCore
//
//  Honest fidelity ladder for the receiver (FR-014/FR-022, ADR-005).
//  "lossless" = decoded PCM sample-identical to the agreed encoded PCM; a
//  resampled/reformatted output path is NEVER labelled bit-perfect (FR-024).
//  Bit-perfect is ONLY ever set externally by a hardware-loopback / USB-analyzer
//  gate (ADR-005); ordinary transport code cannot reach it.
//
//  Pure Foundation: no audio framework imports, so the transitions are
//  unit-testable on macOS and compile against the iOS SDK unchanged.
//

import Foundation

/// Rungs on the fidelity ladder, most honest-first. The `Comparable` rank is a
/// state-machine aid only ("how close to measured bit-perfect"); it is never
/// rendered to the user as a scalar quality score.
public enum FidelityLadder: String, CaseIterable, Sendable, Equatable {
    /// Free tier, or a lossy codec in use (e.g. Opus over Wi-Fi) — FR-020.
    case lossyTransport

    /// Lossless transport reached the output path, but the path is known to
    /// convert (resample / reformat). Terminal honest cap: it can never advance
    /// to bit-perfect while the path converts (FR-024).
    case losslessTransportOutputPathConverted

    /// Lossless transport with an unverified output path: the path *claims*
    /// pass-through but has not been measured end to end.
    case losslessTransportOutputPathUnverified

    /// Measured bit-perfect through the full digital path by a hardware
    /// loopback / USB analyzer (ADR-005). Reachable ONLY via a
    /// LoopbackVerificationToken; ordinary transport code can never assign it.
    case losslessTransportBitPerfectVerified

    /// Rank used ONLY by the state-machine transitions below.
    fileprivate var rank: Int {
        switch self {
        case .lossyTransport: return 0
        case .losslessTransportOutputPathConverted: return 1
        case .losslessTransportOutputPathUnverified: return 2
        case .losslessTransportBitPerfectVerified: return 3
        }
    }

    /// Human-facing short label (used by the SwiftUI shell + telemetry).
    public var title: String {
        switch self {
        case .lossyTransport: return "Lossy transport"
        case .losslessTransportOutputPathConverted: return "Lossless / output path converted"
        case .losslessTransportOutputPathUnverified: return "Lossless / output path unverified"
        case .losslessTransportBitPerfectVerified: return "Lossless / bit-perfect verified"
        }
    }
}

/// Result of requesting a fidelity state transition.
public enum FidelityTransitionResult: Equatable, Sendable {
    /// The transition was applied.
    case applied
    /// The transition was rejected; the payload explains why (fixable, not a crash).
    case rejected(String)
}

/// Ordering for state-machine purposes ONLY (see FidelityLadder.rank).
extension FidelityLadder: Comparable {
    public static func < (lhs: FidelityLadder, rhs: FidelityLadder) -> Bool {
        lhs.rank < rhs.rank
    }
}

/// A one-time-use capability whose only public factory names the
/// hardware-loopback gate. Holders of this token are trusted to have measured
/// bit-identical samples into the DAC via a digital loopback / USB analyzer
/// (ADR-005) — no application code on the streaming path may mint it.
public struct LoopbackVerificationToken: Sendable {
    /// Provenance string recorded for auditability (not a security boundary).
    public let source: String

    fileprivate init(source: String) {
        self.source = source
    }

    /// The only sanctioned minting point: the hardware validation team's gate.
    /// Requires an explicit non-empty source description so every bit-perfect
    /// assignment is attributable to a loopback/analyzer measurement.
    public static func hardwareLoopbackGate(source: String) -> LoopbackVerificationToken {
        precondition(!source.isEmpty, "bit-perfect assignment requires a source description")
        return LoopbackVerificationToken(source: "hardware-loopback:\(source)")
    }
}

/// Live fidelity + format telemetry (FR-014/FR-053 fields). A pure value type;
/// the app layer mutates it in response to transport and route events.
public struct FidelityStatus: Equatable, Sendable {
    public var step: FidelityLadder
    public var sampleRateHz: Int
    public var bitDepth: Int
    public var channels: Int
    /// Encoded transport frame rate (frames/sec), derived from the negotiated
    /// frame duration (e.g. 43.0 Hz for a 23.25 ms frame).
    public var frameRate: Double
    /// Codec label ("opus", "flac", "pcm", "pcm-f32"...) — informational.
    public var codec: String

    public init(
        step: FidelityLadder,
        sampleRateHz: Int = 48_000,
        bitDepth: Int = 16,
        channels: Int = 2,
        frameRate: Double = 0,
        codec: String = ""
    ) {
        self.step = step
        self.sampleRateHz = sampleRateHz
        self.bitDepth = bitDepth
        self.channels = channels
        self.frameRate = frameRate
        self.codec = codec
    }

    /// Request a documented transition. The transition graph (closed on purpose):
    ///
    ///   lossy ──► outputPathConverted
    ///      └────► outputPathUnverified ──► bitPerfectVerified (token required)
    ///                │
    ///                └────► outputPathConverted   (path later detected converting)
    ///   outputPathConverted ──terminal, never advances; only a full reap/reset.
    ///   bitPerfectVerified  ──► outputPathUnverified (evidence invalidated)
    ///
    /// Anything else is rejected with a reason — a mis-transition must be a
    /// caller bug, not a silent state corruption.
    public mutating func transition(to newStep: FidelityLadder,
                                    token: LoopbackVerificationToken? = nil) -> FidelityTransitionResult {
        // Bit-perfect is gated behind the hardware token (ADR-005).
        if newStep == .losslessTransportBitPerfectVerified {
            guard let token, token.source.hasPrefix("hardware-loopback:") else {
                return .rejected("bit-perfect requires a hardware-loopback gate token")
            }
            guard step == .losslessTransportOutputPathUnverified
                || step == .losslessTransportBitPerfectVerified else {
                return .rejected("bit-perfect requires an unverified lossless path verified by hardware loopback; a lossy or known-converting path can never qualify (FR-022/FR-024)")
            }
            step = newStep
            return .applied
        }

        switch (step, newStep) {
        case (.lossyTransport, .losslessTransportOutputPathUnverified),
             (.lossyTransport, .losslessTransportOutputPathConverted):
            step = newStep
            return .applied
        case (.losslessTransportOutputPathUnverified, .losslessTransportOutputPathConverted),
             (.losslessTransportOutputPathUnverified, .losslessTransportOutputPathUnverified),
             (.losslessTransportBitPerfectVerified, .losslessTransportOutputPathUnverified):
            step = newStep
            return .applied
        default:
            return .rejected("no legal transition from \(step.rawValue) to \(newStep.rawValue)")
        }
    }

    /// Convenience: mark the output path as known-to-convert (FR-024). Honest
    /// cap — the path can then never advance to bit-perfect.
    @discardableResult
    public mutating func markOutputPathConverts() -> FidelityTransitionResult {
        switch step {
        case .lossyTransport, .losslessTransportOutputPathConverted:
            step = .losslessTransportOutputPathConverted
            return .applied
        case .losslessTransportOutputPathUnverified,
             .losslessTransportBitPerfectVerified:
            step = .losslessTransportOutputPathConverted
            return .applied
        }
    }

    /// Is the current rung semantically "lossless-capable" for the Free/Pro gate
    /// (FR-042/FR-043: hardware/transport losslessness, NOT bit-perfectness)?
    public var transportsLossless: Bool {
        step != .lossyTransport
    }

    /// Is the full digital path measured bit-perfect (ADR-005)? Only true when a
    /// LoopbackVerificationToken set it; never true from transport alone.
    public var isBitPerfectVerified: Bool {
        step == .losslessTransportBitPerfectVerified
    }
}
