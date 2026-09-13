//
//  EmissionStatus.swift
//  WDRiOSEmitterCore
//
//  Live capture status / diagnostics value mirroring FR-053 for the emitter
//  role (and the receiver's StatusModel in role). Matches the Android
//  StatusModel in field coverage and adds the App-Review-relevant
//  `systemIndicatorVisible` honesty field.
//
//  Pure Foundation. Includes an FR-055-aware `redactedExport()` used by exported
//  diagnostics so no raw stable source label / peer identity / audio payload ever
//  leaves the device.
//

import Foundation

/// Emitter session state machine value (FR-053 `state`).
public enum EmissionState: String, CaseIterable, Sendable, Equatable {
    case idle
    case configuringSession
    case awaitingSystemPicker
    case broadcasting
    case paused
    case error
    case terminated
}

/// What the emitter is capturing (coarse scope for FR-055 redaction).
public enum EmissionScope: String, CaseIterable, Sendable, Equatable {
    /// The app's own audio (session tap).
    case selfOnly
    /// Other apps' permitted audio (ReplayKit / SCK system picker).
    case otherApp
}

/// FR-053 live capture-status model. Immutable value type; the app layer
/// publishes new instances as capture/route events arrive.
public struct EmissionStatus: Equatable, Sendable {
    public var state: EmissionState
    /// Capture mode in use (from `CaptureMode`).
    public var mode: CaptureMode
    /// Coarse scope: self-only or other-app audio.
    public var scope: EmissionScope
    /// App Review 2.5.14 honesty: whether the SYSTEM's red recording indicator is
    /// (or would be) visible while capturing. The app never draws its own fake
    /// indicator and never suppresses the system one.
    public var systemIndicatorVisible: Bool
    /// Human-facing capture source label (e.g. "Broadcast extension"). Redacted
    /// on export — never a raw stable identifier.
    public var sourceLabel: String
    /// Sample rate in Hz.
    public var sampleRateHz: Int
    /// Bit depth (16/24).
    public var bitDepth: Int
    /// Channel count.
    public var channels: Int
    /// Captured frames/sec (e.g. a 23.25 ms frame ≈ 43 Hz).
    public var frameRate: Double
    /// Dropped capture frames observed since session start.
    public var droppedBuffers: Int
    /// Estimated capture→transport latency, ms.
    public var latencyMs: Int
    /// Transport jitter-buffer fill as a fraction, 0.0...1.0.
    public var bufferFill: Double
    /// Transport-observed loss, percent 0.0...100.0.
    public var packetLossPct: Double

    public init(
        state: EmissionState = .idle,
        mode: CaptureMode = .replayKitBroadcast,
        scope: EmissionScope = .otherApp,
        systemIndicatorVisible: Bool = false,
        sourceLabel: String = "",
        sampleRateHz: Int = 48_000,
        bitDepth: Int = 16,
        channels: Int = 2,
        frameRate: Double = 0,
        droppedBuffers: Int = 0,
        latencyMs: Int = 0,
        bufferFill: Double = 0,
        packetLossPct: Double = 0
    ) {
        self.state = state
        self.mode = mode
        self.scope = scope
        self.systemIndicatorVisible = systemIndicatorVisible
        self.sourceLabel = sourceLabel
        self.sampleRateHz = sampleRateHz
        self.bitDepth = bitDepth
        self.channels = channels
        self.frameRate = frameRate
        self.droppedBuffers = droppedBuffers
        self.latencyMs = latencyMs
        self.bufferFill = bufferFill
        self.packetLossPct = packetLossPct
    }

    /// Honest indicator state from the matrix: once a broadcast/SCK session is
    /// live the SYSTEM shows the red indicator; a self-session tap does not.
    public static func systemIndicatorVisible(for mode: CaptureMode,
                                              isActive: Bool) -> Bool {
        isActive && (iOSCapabilityMatrix.capability(for: mode)?.systemIndicatorShown ?? false)
    }
}

// MARK: - Redaction (FR-055)

extension EmissionStatus {
    /// All diagnostic fields are public except the redacted ones below. Keeps a
    /// support/dashboard dump readable while excluding anything that could
    /// fingerprint or leak a session: the raw capture source label, and any
    /// encapsulating identifier. Audio payload is absent from this model by
    /// construction (it never transits diagnostics).
    public struct RedactedExport: Equatable, Sendable, CustomStringConvertible {
        public let state: EmissionState
        public let mode: CaptureMode
        /// Coarse scope ("self" / "other-app") — never a raw app/source label.
        public let scope: String
        public let systemIndicatorVisible: Bool
        public let sampleRateHz: Int
        public let bitDepth: Int
        public let channels: Int
        public let frameRate: Double
        public let droppedBuffers: Int
        public let latencyMs: Int
        public let bufferFill: Double
        public let packetLossPct: Double

        public var description: String {
            "Emission[state=\(state.rawValue) mode=\(mode.rawValue) scope=\(scope) "
                + "systemIndicator=\(systemIndicatorVisible) sampleRateHz=\(sampleRateHz) "
                + "bitDepth=\(bitDepth) channels=\(channels) frameRate=\(String(format: "%.1f", frameRate)) "
                + "dropped=\(droppedBuffers) latencyMs=\(latencyMs) "
                + "bufferFill=\(String(format: "%.2f", bufferFill)) "
                + "packetLossPct=\(String(format: "%.2f", packetLossPct))]"
        }
    }

    /// FR-055 redacted export: no `sourceLabel` or any raw identifier; scope is
    /// coarse-classified; no audio payload (there is none by construction).
    public func redactedExport() -> RedactedExport {
        RedactedExport(
            state: state,
            mode: mode,
            scope: scope == .selfOnly ? "self" : "other-app",
            systemIndicatorVisible: systemIndicatorVisible,
            sampleRateHz: sampleRateHz,
            bitDepth: bitDepth,
            channels: channels,
            frameRate: frameRate,
            droppedBuffers: droppedBuffers,
            latencyMs: latencyMs,
            bufferFill: bufferFill,
            packetLossPct: packetLossPct
        )
    }

    /// Human-facing single-line status summary for the shell UI / local logs.
    /// Deliberately EXCLUDES `sourceLabel` and any raw identifiers (FR-055);
    /// scope shown as a coarse class.
    public var summaryLine: String {
        "\(state.rawValue) · \(mode.title) · \(redactedExport().scope) · "
            + "\(sampleRateHz)Hz/\(bitDepth)bit · \(channels)ch · dropped \(droppedBuffers) · "
            + "sys-indicator \(systemIndicatorVisible ? "on" : "off")"
    }
}
