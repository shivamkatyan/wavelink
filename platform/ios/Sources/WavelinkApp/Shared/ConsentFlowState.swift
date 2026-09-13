//
//  ConsentFlowState.swift
//  WDRiOSEmitterCore
//
//  FR-052 consent state machine for the iOS emitter, mirroring the contract of
//  the receiver's PermissionExplainerView (explain-before-prompt) as a
//  testable pure value type:
//
//      .needsExplanation ──(user reads explanation + taps Continue)──► .awaitingSystemPicker
//      .awaitingSystemPicker ──(system grants / broadcast active)──► .authorized
//      .awaitingSystemPicker ──(user cancels/denies at the system picker)──► .denied
//      .authorized ──(broadcast stopped)──► .awaitingSystemPicker
//      .denied ──(retry, FR-054)──► .awaitingSystemPicker
//      .authorized / .denied ──(full reset)──► .needsExplanation
//
//  The system picker itself (`RPSystemBroadcastPickerView` / `SCContentSharingPicker`)
//  is a DEVICE-only runtime object — it is NOT on this host. The core models the
//  transitions; the App layer (`Sources/WDRiOSEmitterApp/ConsentExplainerView.swift`)
//  drives them and the device runbook verifies the real picker.
//
//  Any illegal transition is REJECTED with a reason — FR-052's invariant is that
//  `.authorized` is unreachable unless the explanation was shown
//  (`.needsExplanation` → `.awaitingSystemPicker` happened first). The app can
//  never silently skip to a capture-authorised state.
//

import Foundation

/// Phases of the FR-052 consent flow.
public enum ConsentPhase: String, CaseIterable, Sendable, Equatable {
    /// The app is about to show the in-app explanation (before any OS prompt).
    case needsExplanation
    /// The explanation was acknowledged; the next step is presenting the SYSTEM
    /// picker (on device). No capture may start before this phase is reached.
    case awaitingSystemPicker
    /// The system granted capture (broadcast active / picker confirmed).
    case authorized
    /// The user denied or cancelled at the system picker — with a redacted reason.
    case denied
}

/// Coarse, redaction-aware reason for a denial (FR-054 actionable errors). Never
/// carries raw system/probe strings — only a stable code + generic text.
public enum ConsentDenialCode: String, CaseIterable, Sendable, Equatable {
    case userDeniedAtSystemPicker
    case broadcastCancelled
    case protectedContent
    case unsupportedOS
    case unknown
}

/// A redacted consent denial: stable code + generic human text. The `reason`
/// string is deliberately minimal so no raw detail ever enters logs/exports.
public struct ConsentDenial: Equatable, Sendable, CustomStringConvertible {
    public let code: ConsentDenialCode

    public init(code: ConsentDenialCode) {
        self.code = code
    }

    /// Human-facing guidance (a11y-friendly, FR-056) shown on the denied screen.
    public var guidance: String {
        switch code {
        case .userDeniedAtSystemPicker:
            return "Capture was not allowed. You can open Settings to review access, then try again."
        case .broadcastCancelled:
            return "Broadcast was cancelled. Nothing was captured."
        case .protectedContent:
            return "Protected content cannot be captured by iOS. Nothing from protected apps was included."
        case .unsupportedOS:
            return "This iOS version does not support this capture mode."
        case .unknown:
            return "Capture could not start. Please try again."
        }
    }

    public var description: String {
        "ConsentDenial(\(code.rawValue))"
    }
}

/// Result of a consent-flow transition — mirrors `FidelityTransitionResult`.
public enum ConsentTransitionResult: Equatable, Sendable {
    case applied
    case rejected(String)
}

/// Pure value-type consent FSM. Mutable only through `transition(to:denial:)`;
/// see the type doc for the transition graph. Tests in
/// `Tests/WDRiOSEmitterCoreTests/ConsentFlowStateTests.swift`.
public struct ConsentFlowState: Equatable, Sendable {
    public private(set) var phase: ConsentPhase
    public private(set) var denial: ConsentDenial?

    public init(phase: ConsentPhase = .needsExplanation) {
        self.phase = phase
        self.denial = nil
    }

    /// Has the FR-052 explanation been acknowledged (so the system picker may be
    /// presented on device)?
    public var canPresentSystemPicker: Bool {
        phase == .awaitingSystemPicker
    }

    /// Capture may only be considered active when the system authorised it.
    public var isCaptureAuthorised: Bool {
        phase == .authorized
    }

    /// Request a documented transition. Illegal moves are rejected with a reason
    /// — a consent FSM corruption must be a caller bug, not silent state change.
    @discardableResult
    public mutating func transition(to newPhase: ConsentPhase,
                                    denial: ConsentDenial? = nil) -> ConsentTransitionResult {
        switch (phase, newPhase) {
        case (.needsExplanation, .awaitingSystemPicker):
            phase = newPhase
            self.denial = nil
            return .applied

        case (.awaitingSystemPicker, .authorized):
            phase = newPhase
            self.denial = nil
            return .applied

        case (.awaitingSystemPicker, .denied):
            phase = newPhase
            // A denial must carry a reason code (FR-054 actionable errors).
            self.denial = denial ?? ConsentDenial(code: .unknown)
            return .applied

        case (.authorized, .awaitingSystemPicker):
            // Broadcast ended/stopped; user may restart via the picker. Still
            // explained — no re-explanation required (FR-052 is one-shot per start).
            phase = newPhase
            self.denial = nil
            return .applied

        case (.authorized, .needsExplanation), (.denied, .needsExplanation):
            // Full reset (e.g. app reinstall flow / explicit "restart").
            phase = newPhase
            self.denial = nil
            return .applied

        case (.denied, .awaitingSystemPicker):
            // Retry via the system picker (FR-054). Explanation already shown.
            phase = newPhase
            self.denial = nil
            return .applied

        default:
            return .rejected("no legal consent transition from \(phase.rawValue) to \(newPhase.rawValue)")
        }
    }
}

// MARK: - Accessibility-friendly phase label (FR-056)

extension ConsentFlowState {
    /// Human-facing message for the shell's consent status, never raw.
    public func phaseLabel(hasExplanation: Bool = false) -> String {
        switch phase {
        case .needsExplanation:
            return hasExplanation ? "Ready to explain what capture means" : "Needs an explanation before capture"
        case .awaitingSystemPicker:
            return "Awaiting the system picker"
        case .authorized:
            return "Capture authorised by the system"
        case .denied:
            return denial?.guidance ?? "Capture was not allowed"
        }
    }
}
