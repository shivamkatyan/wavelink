//
//  StatusModel.swift
//  WDRReceiverCore
//
//  Live status / diagnostics value mirroring FR-053 for the receiver role.
//  Matches the Android StatusModel in field coverage:
//  state, peer, transport, codec, sample rate, bit depth, channels, estimated
//  end-to-end latency, buffer fill, packet loss, underruns, output route,
//  fidelity state (+ encoded transport frame rate).
//
//  Pure Foundation. Includes an FR-055-aware redactedDescription used by the
//  exported diagnostics so no pairing secret / raw stable device identifier /
//  audio payload ever leaves the device.
//

import Foundation

/// Receiver session state machine value (FR-053 `state`).
public enum SessionState: String, CaseIterable, Sendable, Equatable {
    case idle
    case connecting
    case pairing
    case streaming
    case paused
    case error
    case terminated
}

/// Transport label (FR-053 `transport`).
public enum Transport: String, CaseIterable, Sendable, Equatable {
    case wifi
    case bluetooth
    case loopback
    case unknown
}

/// Integer-lossless bit depths the receiver will surface honestly (16/24 per
/// §8 of the contract; 24 supported in codec terms, measured end-to-end later).
public enum LosslessBitDepth: Int, CaseIterable, Sendable {
    case sixteen = 16
    case twentyFour = 24

    /// A device-observable bit depth we cannot yet claim integer-lossless.
    case unknown = 0
}

/// FR-053 live-status model. Immutable value type; the app layer publishes new
/// instances as telemetry/route events arrive (ObservedObject pattern).
public struct StatusModel: Equatable, Sendable {
    public var state: SessionState
    /// Peer identity (pairing/hardware id) — populate from the pairing step only.
    public var peer: String
    public var transport: Transport
    /// Codec in use ("opus", "flac", "pcm"...).
    public var codec: String
    /// Sample rate in Hz (44.1k/48k first; higher when codec/OS/DAC support it).
    public var sampleRateHz: Int
    /// Bit depth (16/24 for integer lossless).
    public var bitDepth: Int
    /// Channel count (stereo first).
    public var channels: Int
    /// Estimated end-to-end capture-to-render latency, ms.
    public var latencyMs: Int
    /// Jitter-buffer fill as a fraction of capacity, 0.0...1.0.
    public var bufferFill: Double
    /// Receiver-observed frame/packet loss, percent 0.0...100.0.
    public var packetLossPct: Double
    /// Underrun count since session start.
    public var underruns: Int
    /// Output route label ("USB DAC: <name>" / "built-in" / "none").
    public var route: String
    /// Fidelity + format (FR-014/FR-022/FR-053, ADR-005 ladder).
    public var fidelity: FidelityStatus

    public init(
        state: SessionState = .idle,
        peer: String = "",
        transport: Transport = .wifi,
        codec: String = "",
        sampleRateHz: Int = 48_000,
        bitDepth: Int = 16,
        channels: Int = 2,
        latencyMs: Int = 0,
        bufferFill: Double = 0,
        packetLossPct: Double = 0,
        underruns: Int = 0,
        route: String = "none",
        fidelity: FidelityStatus = FidelityStatus(
            step: .lossyTransport, sampleRateHz: 48_000, bitDepth: 16, channels: 2,
            frameRate: 0, codec: "opus"
        )
    ) {
        self.state = state
        self.peer = peer
        self.transport = transport
        self.codec = codec
        self.sampleRateHz = sampleRateHz
        self.bitDepth = bitDepth
        self.channels = channels
        self.latencyMs = latencyMs
        self.bufferFill = bufferFill
        self.packetLossPct = packetLossPct
        self.underruns = underruns
        self.route = route
        self.fidelity = fidelity
    }
}

// MARK: - Redaction (FR-055)

extension StatusModel {
    /// All diagnostic fields are public except the redacted ones joined below.
    /// Keeps the live-diagnostics value readable for support/dashboard while
    /// excluding anything that could fingerprint or leak a session:
    /// peer, and any raw stable device identifier baked into `route` (a route
    /// label from the OS is stable device info, so redaction replaces it with a
    /// coarse class when it is carried in a redacted export).
    public struct RedactedExport: Equatable, Sendable, CustomStringConvertible {
        public let state: SessionState
        public let transport: Transport
        public let codec: String
        public let sampleRateHz: Int
        public let bitDepth: Int
        public let channels: Int
        public let latencyMs: Int
        public let bufferFill: Double
        public let packetLossPct: Double
        public let underruns: Int
        /// Coarse output-route class ("usb-audio" / "built-in-other") — never a raw device name.
        public let outputRouteClass: String
        public let fidelityStep: FidelityLadder

        public var description: String {
            "Status[state=\(state.rawValue) transport=\(transport.rawValue) codec=\(codec) "
                + "sampleRateHz=\(sampleRateHz) bitDepth=\(bitDepth) channels=\(channels) "
                + "latencyMs=\(latencyMs) bufferFill=\(String(format: "%.2f", bufferFill)) "
                + "packetLossPct=\(String(format: "%.2f", packetLossPct)) underruns=\(underruns) "
                + "route=\(outputRouteClass) fidelity=\(fidelityStep.rawValue)]"
        }
    }

    /// FR-055 redacted export: no `peer`, no raw route device name, no audio
    /// payload (there is none in this model by construction).
    public func redactedExport() -> RedactedExport {
        let routeClass: String
        if route.localizedCaseInsensitiveContains("usb") {
            routeClass = "usb-audio"
        } else if route == "none" {
            routeClass = "none"
        } else {
            routeClass = "built-in-other"
        }
        return RedactedExport(
            state: state,
            transport: transport,
            codec: codec,
            sampleRateHz: sampleRateHz,
            bitDepth: bitDepth,
            channels: channels,
            latencyMs: latencyMs,
            bufferFill: bufferFill,
            packetLossPct: packetLossPct,
            underruns: underruns,
            outputRouteClass: routeClass,
            fidelityStep: fidelity.step
        )
    }

    /// Human-facing single-line status summary for the shell UI / local logs.
    /// Deliberately EXCLUDES `peer` and raw device names so casual logs never
    /// carry pairing identity (FR-055); route shown as coarse class.
    public var summaryLine: String {
        "\(state.rawValue) · \(transport.rawValue) · \(codec) @ \(sampleRateHz)Hz/\(bitDepth)bit "
            + "· buf \(Int(bufferFill * 100))% · loss \(String(format: "%.1f", packetLossPct))% "
            + "· underruns \(underruns) · route \(redactedExport().outputRouteClass) · \(fidelity.step.title)"
    }
}
