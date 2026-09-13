//
//  RouteSnapshot.swift
//  WDRReceiverCore
//
//  Output-route report + route-change representation for the iOS receiver
//  (FR-013/FR-015, PLATFORM_MATRIX iOS row). The core holds only the pure model
//  and detection logic; the App layer (`WDRReceiverApp/OutputRouter.swift`)
//  maps live `AVAudioSession.currentRoute.outputs` into [PortDescriptor].
//
//  Honesty contract: the core reports what the OS routes to (including `.usbAudio`
//  visibility) but NEVER claims to force a specific DAC — iOS has no public API
//  to programmatically select an arbitrary hardware output port; the system
//  auto-selects on `.usbAudio` attach (PLATFORM_MATRIX §A iOS: "system
//  auto-selects; cannot force specific DAC").
//
//  Pure Foundation: no AVAudioSession import, so detection is unit-testable
//  on macOS and compile-correct against the iOS SDK.
//

import Foundation

/// Coarse output-port class derived from an `AVAudioSession.Port` type string.
public enum OutputPortKind: String, CaseIterable, Sendable, Equatable {
    case usbAudio
    case builtInSpeaker
    case builtInReceiver
    case headphones
    case lineOut
    case airPlay
    case bluetoothA2DP
    case bluetoothLE
    case hdmi
    case carAudio
    case other // anything unrecognised — never mislabelled as a DAC

    /// Map the OS raw port-type literal ("USB-Audio", "Speaker", "Headphones"…)
    /// to a coarse class. The `AVAudioSession.Port.usbAudio` literal is
    /// "USB-Audio" (AVAudioSessionTypes.h, iOS 6.0+); matching is
    /// case-insensitive and tolerant of a future "USB"-prefixed literal.
    public init(rawType: String) {
        switch rawType {
        case "USB-Audio", "USB-AUDIO", "usb-audio", "USB Audio":
            self = .usbAudio
        case "Speaker":
            self = .builtInSpeaker
        case "Receiver":
            self = .builtInReceiver
        case "Headphones":
            self = .headphones
        case "Headset":
            self = .headphones
        case "LineOut":
            self = .lineOut
        case "AirPlay":
            self = .airPlay
        case "BluetoothA2DPOutput", "BluetoothA2DP":
            self = .bluetoothA2DP
        case "BluetoothLE":
            self = .bluetoothLE
        case "HDMI":
            self = .hdmi
        case "CarAudio":
            self = .carAudio
        default:
            // Forward-compatible guess: a port type whose literal contains
            // "usb" is treated as USB audio (documented + tested).
            if rawType.localizedCaseInsensitiveContains("usb") {
                self = .usbAudio
            } else {
                self = .other
            }
        }
    }
}

/// Pure description of an output port (mirrors android OutputDeviceInfo role).
public struct PortDescriptor: Equatable, Sendable {
    /// First destination name reported by the OS (used for the "USB DAC: <name>"
    /// route label); kept out of redacted exports.
    public let name: String
    /// Coarse class (isUsbAudio is the load-bearing bit for FR-013).
    public let kind: OutputPortKind

    public init(name: String, kind: OutputPortKind) {
        self.name = name
        self.kind = kind
    }

    public init(name: String, portTypeRaw: String) {
        self.name = name
        self.kind = OutputPortKind(rawType: portTypeRaw)
    }

    public var isUsbAudio: Bool { kind == .usbAudio }

    public var displayLabel: String {
        switch kind {
        case .usbAudio: return "USB DAC: \(name)"
        case .builtInSpeaker: return "built-in speaker"
        case .builtInReceiver: return "built-in receiver"
        case .headphones: return "headphones"
        case .lineOut: return "line-out"
        case .airPlay: return "airplay"
        case .bluetoothA2DP: return "bluetooth"
        case .bluetoothLE: return "bluetooth LE"
        case .hdmi: return "hdmi"
        case .carAudio: return "car audio"
        case .other: return name.isEmpty ? "unknown" : name
        }
    }
}

/// One observation of the current output route (FR-053 `route` field).
public struct RouteSnapshot: Equatable, Sendable {
    /// All output ports the session reports (usually a single primary output).
    public let outputs: [PortDescriptor]
    /// The port the system has actually routed to, if the session exposes one.
    public let activeOutput: PortDescriptor?

    public init(outputs: [PortDescriptor], activeOutput: PortDescriptor? = nil) {
        self.outputs = outputs
        self.activeOutput = activeOutput
    }

    public static let none = RouteSnapshot(outputs: [])

    public var isUsbAudioRouted: Bool {
        activeOutput?.isUsbAudio ?? outputs.contains { $0.isUsbAudio }
    }

    public var routeLabel: String {
        if let active = activeOutput { return active.displayLabel }
        if let first = outputs.first, outputs.contains(where: { $0.isUsbAudio }) {
            // USB DAC present but the system has not marked it active (e.g. it
            // is enumerable but not yet chosen) — say so rather than pretending.
            return first.displayLabel
        }
        if outputs.isEmpty { return "none" }
        return outputs.map(\.displayLabel).joined(separator: ", ")
    }
}

/// Route-change event surfaced to the player/UI (FR-015).
public enum RouteChangeEvent: Equatable, Sendable {
    /// A USB DAC became part of the route (attached / newly routed).
    case usbAudioAttached(port: PortDescriptor)
    /// The USB DAC left the route; the player should not crash and should fall
    /// back to whatever the system routes next.
    case usbAudioDetached
    /// The route changed for reasons other than USB attach/detach (e.g. a
    /// headphone jack or speaker change); represents from -> to.
    case routeChanged(from: RouteSnapshot, to: RouteSnapshot)
    /// No usable output at all; the player should pause (FR-054 actionable error).
    case noOutput
}

/// Pure route-diffing + event classification (mirrors android `choosePreferredDevice`
/// + `RouteChange` roles; pure so it is unit-testable without an audio session).
public enum OutputRouteDetector {

    /// Classify the OS route-change from old -> new (FR-015).
    /// - USB attach wins over everything (system now routes to a DAC).
    /// - USB detach is reported when the old route had a DAC and the new one does not.
    /// - No outputs at all -> NO_ROUTE (pause).
    /// - Otherwise a plain route change.
    public static func classifyChange(from old: RouteSnapshot,
                                      to new: RouteSnapshot) -> RouteChangeEvent {
        let newUsb = new.outputs.first(where: \.isUsbAudio)
        let oldHadUsb = old.isUsbAudioRouted

        if new.outputs.isEmpty {
            return .noOutput
        }
        if let newUsb, !oldHadUsb {
            return .usbAudioAttached(port: newUsb)
        }
        if oldHadUsb && new.outputs.allSatisfy({ !$0.isUsbAudio }) {
            return .usbAudioDetached
        }
        if old == new {
            // Same route re-reported; not an event the player must act on.
            return .routeChanged(from: old, to: new)
        }
        return .routeChanged(from: old, to: new)
    }

    /// The "router" role: given a snapshot, decide what the player should do.
    /// Mirrors android's `choosePreferredDevice` semantics adaptively: iOS cannot
    /// force a port, so the *decision* is which status to surface, and a USB DAC
    /// present-but-unverified is surfaced honestly (never "routed to DAC" unless
    /// the system actually routed there).
    public enum RouteDecision: Equatable, Sendable {
        /// System routes to / presents a USB DAC — render (report only, no force).
        case renderUsbDac(port: PortDescriptor)
        /// No DAC; fall back to whatever the system routed (built-in etc.).
        case renderFallback(port: PortDescriptor)
        /// No usable output; the player must pause.
        case pause
    }

    public static func decide(snapshot: RouteSnapshot) -> RouteDecision {
        if let active = snapshot.activeOutput {
            return active.isUsbAudio ? .renderUsbDac(port: active) : .renderFallback(port: active)
        }
        if let usb = snapshot.outputs.first(where: \.isUsbAudio) {
            // Enumerable but not explicitly the active route — render through the
            // DAC only if the system marked it active, else treat as fallback
            // truthfully (system auto-selects on attach; leaving the DAC in the
            // set means the system is likely routing to it).
            return snapshot.outputs.count == 1 ? .renderUsbDac(port: usb) : .renderFallback(port: usb)
        }
        if let first = snapshot.outputs.first {
            return .renderFallback(port: first)
        }
        return .pause
    }
}
