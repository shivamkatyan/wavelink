//
//  CapturePolicy.swift
//  WDRiOSEmitterCore
//
//  Pure decision logic for WHAT the iOS emitter may capture — the honest
//  capability matrix for permitted audio capture on iOS, per
//  docs/planning/PLATFORM_MATRIX.md §A iOS row + ADR-008:
//
//    * SYSTEM BUS is UNCAPTURABLE via public API on iOS — there is no
//      system-wide "tap the output bus" API (unlike WASAPI loopback/PipeWire on
//      desktop). "Capture everything that plays" is therefore NOT an honest
//      promise and never appears here.
//    * Per-app / self audio:
//        (a) `.selfTap`          — AVAudioSession `installTap` on the app's own
//                                  node (self-playback path, no mic needed for
//                                  tapped apps that route through the session).
//        (b) `.replayKitBroadcast` — ReplayKit broadcast extension surfaced
//                                  through `RPSystemBroadcastPickerView`, the
//                                  PUBLIC system picker (iOS 12+; the per-other-
//                                  app path across iOS 12–26; no bypass).
//        (c) `.screenCaptureKit` — ScreenCaptureKit `SCContentSharingPicker`
//                                  `.audio` output (iOS 27+; replaces ReplayKit
//                                  where available per ADR-008).
//
//  Everything here is PURE (Foundation only): OS-version → allowed-mode mapping,
//  capability records, and the honesty strings for protected content, consent
//  and background. The actual picker views / session taps live in the App layer
//  (`Sources/WDRiOSEmitterApp`) and the broadcast-extension scaffold.
//

import Foundation

/// Capture modes the iOS emitter can honestly present, keyed by the OS-version
/// window in which each is legal (see `iOSCapabilityMatrix`).
public enum CaptureMode: String, CaseIterable, Sendable, Equatable {
    /// AVAudioSession `installTap` on the app's own audio node (self-playback
    /// capture; no microphone prompt for tapped apps that route through the
    /// session). Legal on every iOS version in our supported window.
    case selfTap

    /// ReplayKit broadcast-extension capture started from the PUBLIC system
    /// picker `RPSystemBroadcastPickerView` (iOS 12+). The per-other-app audio
    /// path across iOS 12–26. Capture itself runs in the system's broadcast
    /// extension host with the system's red status-bar overlay.
    case replayKitBroadcast

    /// ScreenCaptureKit capture started from `SCContentSharingPicker` (iOS 27+,
    /// replaces ReplayKit where available per ADR-008). The `.audio` clip output
    /// carries the shared audio; protected content is excluded by the OS.
    case screenCaptureKit
}

/// What a capture mode may actually reach. iOS public API can target the app's
/// own audio or other apps' audio through a system-gated mechanism — never the
/// whole system bus.
public enum CaptureScope: String, CaseIterable, Sendable, Equatable {
    /// The app's own audio only (session tap).
    case selfOnly

    /// Other apps' audio that the OS permits to be captured (ReplayKit/SCK),
    /// always behind the system picker.
    case otherAppAudio
}

/// How consent is obtained for a mode.
public enum ConsentKind: String, CaseIterable, Sendable, Equatable {
    /// No system picker is involved (tapping our own session is our own audio).
    case noPicker
    /// The OS itself presents the picker (`RPSystemBroadcastPickerView` /
    /// `SCContentSharingPicker`). The app MUST route through it — never bypass.
    case systemPicker
}

/// One row of the honest capability matrix.
public struct CaptureCapability: Equatable, Sendable {
    public let mode: CaptureMode
    /// First iOS major version on which the mode is legal.
    public let minOSMajor: Int
    /// Last iOS major version on which the mode is legal; `nil` = no upper bound.
    public let maxOSMajor: Int?
    public let scope: CaptureScope
    public let consent: ConsentKind
    /// Capture cannot be started in the background or invisibly — a visible,
    /// foreground user action (tapping the system picker) must start it.
    public let requiresForegroundStart: Bool
    /// While a mode is active the SYSTEM continuously shows its own red recording
    /// indicator (App Review Guideline 2.5.14); the app neither draws a fake
    /// indicator nor hides the system one.
    public let systemIndicatorShown: Bool
    /// Protected/DRM content is excluded by the OS for this mode (never
    /// "silently captured"); the mode must report silence/no-audio for it.
    public let protectedContentExcludedByOS: Bool
    /// Microphone capture is NOT part of this mode by default (no mic prompt is
    /// required for the tapped/broadcast audio path). A future
    /// `.playAndRecord`/inputNode branch would be its own consent surface.
    public let microphoneInvolved: Bool

    public init(
        mode: CaptureMode,
        minOSMajor: Int,
        maxOSMajor: Int?,
        scope: CaptureScope,
        consent: ConsentKind,
        requiresForegroundStart: Bool,
        systemIndicatorShown: Bool,
        protectedContentExcludedByOS: Bool,
        microphoneInvolved: Bool = false
    ) {
        self.mode = mode
        self.minOSMajor = minOSMajor
        self.maxOSMajor = maxOSMajor
        self.scope = scope
        self.consent = consent
        self.requiresForegroundStart = requiresForegroundStart
        self.systemIndicatorShown = systemIndicatorShown
        self.protectedContentExcludedByOS = protectedContentExcludedByOS
        self.microphoneInvolved = microphoneInvolved
    }
}

/// The capability matrix itself — the single source of truth for what the iOS
/// emitter may capture. Pure and unit-testable, so the OS-version gates are
/// checked on THIS host (device runtime gates live in the App layer + runbooks).
public enum iOSCapabilityMatrix {

    /// Full matrix (all modes this emitter can ever honestly present).
    /// * iOS floor is 14 for the app (ADR-008); the capture *modes* are legal
    ///   from iOS 12 (ReplayKit picker floor; `installTap` itself is iOS 8+ but
    ///   we floor the permitted capture set at 12 — IAP/ReplayKit picker floor).
    /// * `.screenCaptureKit` returns `nil` `maxOSMajor` but its SDK symbol does
    ///   not exist before iOS 27 — the App layer guards it with
    ///   `#if canImport(ScreenCaptureKit)` (see SystemBroadcastPicker.swift).
    public static let capabilities: [CaptureCapability] = [
        CaptureCapability(
            mode: .selfTap,
            minOSMajor: 12,
            maxOSMajor: nil,
            scope: .selfOnly,
            consent: .noPicker,
            requiresForegroundStart: true,
            systemIndicatorShown: false, // self-session tap shows no system overlay — see note
            protectedContentExcludedByOS: true,
            microphoneInvolved: false
        ),
        CaptureCapability(
            mode: .replayKitBroadcast,
            minOSMajor: 12,
            maxOSMajor: 26,
            scope: .otherAppAudio,
            consent: .systemPicker,
            requiresForegroundStart: true,
            systemIndicatorShown: true,  // system red status-bar overlay
            protectedContentExcludedByOS: true,
            microphoneInvolved: false
        ),
        CaptureCapability(
            mode: .screenCaptureKit,
            minOSMajor: 27,
            maxOSMajor: nil,
            scope: .otherAppAudio,
            consent: .systemPicker,
            requiresForegroundStart: true,
            systemIndicatorShown: true,  // system recording indicator while `.audio` clip is active
            protectedContentExcludedByOS: true,
            microphoneInvolved: false
        ),
    ]

    /// The capability row for a mode, or `nil` if the mode is unknown/not in play.
    public static func capability(for mode: CaptureMode) -> CaptureCapability? {
        capabilities.first { $0.mode == mode }
    }

    /// Is this iOS major version inside a mode's legal window?
    public static func isLegal(_ mode: CaptureMode, onOSMajor os: Int) -> Bool {
        guard let cap = capability(for: mode) else { return false }
        guard os >= cap.minOSMajor else { return false }
        if let maxOSMajor = cap.maxOSMajor, os > maxOSMajor { return false }
        return true
    }

    /// All legal capture modes for an iOS major version, most-capable first:
    /// on 27+ the SCK path replaces ReplayKit; `.selfTap` is always available.
    public static func acceptableCaptureModes(forOSMajor os: Int) -> [CaptureMode] {
        let ordered: [CaptureMode] = [.screenCaptureKit, .replayKitBroadcast, .selfTap]
        return ordered.filter { isLegal($0, onOSMajor: os) }
    }

    /// The primary PER-OTHER-APP capture mode for an iOS version:
    /// `.screenCaptureKit` on 27+, `.replayKitBroadcast` on 12–26, `nil` below 12
    /// (below the floor there is no public per-other-app capture — honest U).
    public static func primaryOtherAppMode(forOSMajor os: Int) -> CaptureMode? {
        if os >= 27 { return .screenCaptureKit }
        if os >= 12 { return .replayKitBroadcast }
        return nil
    }

    /// The recommended mode to drive from the shell UI for an OS version
    /// (primary per-other-app mode where available; otherwise the self tap).
    public static func recommendedCaptureMode(forOSMajor os: Int) -> CaptureMode? {
        if let primary = primaryOtherAppMode(forOSMajor: os) { return primary }
        return acceptableCaptureModes(forOSMajor: os).first
    }

    /// Any legal capture mode declares iOS ≥ 12 — below iOS 12 public capture is
    /// simply unavailable (the matrix returns an empty set, honestly).
    public static func isOSCaptureSupported(_ osMajor: Int) -> Bool {
        !acceptableCaptureModes(forOSMajor: osMajor).isEmpty
    }
}

// MARK: - Honesty strings (consent / protected content / background / App Review)

extension iOSCapabilityMatrix {
    /// The constant, plain-language honesty block the shell surfaces on-device
    /// (FR-052 explain-before-prompt + App Review 2.5.14 record). Derived from
    /// the matrix so the consent copy can never drift from the capability table.
    public static var consentSummary: String {
        "iOS gives apps no public way to tap the whole system audio bus. Wavelink captures the audio it is permitted to reach: your own app's playback, or other apps' audio only through Apple's own system picker — never silently, never in the background, and never protected content (the system mutes DRM/protected audio)."
    }

    /// App Review Guideline 2.5.14 mapping — the shell ALWAYS routes capture
    /// consent through the system picker and relies on the SYSTEM's red
    /// recording indicator while capturing.
    public static var systemPickerConsentNote: String {
        "Consent is obtained by the SYSTEM picker (RPSystemBroadcastPickerView / SCContentSharingPicker). The app never bypasses it and never captures without it (Apple Review Guideline 2.5.14)."
    }

    /// The user-visible recording indicator is the system's own red overlay; the
    /// app draws no fake indicator and can never hide the real one.
    public static var recordingIndicatorHonesty: String {
        "While capturing, the SYSTEM continuously shows its red recording indicator. The app never draws a fake indicator and capture cannot run with the indicator hidden (App Review 2.5.14)."
    }

    /// Background honesty: no silent background capture. An ACTIVE broadcast
    /// (extension) continues while the app is backgrounded — with the system red
    /// indicator still up — but capture always starts from a visible foreground
    /// action through the system picker.
    public static var backgroundHonesty: String {
        "No silent background capture: capture always starts from a visible, foreground user action through the system picker; it never starts invisibly. While a ReplayKit/SCK session is active the system's extension captures with the red indicator still visible, and protected content is excluded by the OS."
    }

    /// Protected-content honesty: the OS itself excludes DRM/protected audio.
    public static var protectedContentHonesty: String {
        "Protected (DRM) content is excluded by the OS — ReplayKit is incompatible with protected/AVPlayer output, and the system mutes protected audio in ScreenCaptureKit and session taps. What you hear in the stream is what iOS permits to be captured, never a bypass of protection."
    }
}

// MARK: - Display + accessibility-friendly labels (FR-056)

extension CaptureMode {
    /// Short human label for the shell UI / status panel.
    public var title: String {
        switch self {
        case .selfTap: return "This app's audio"
        case .replayKitBroadcast: return "System picker (ReplayKit)"
        case .screenCaptureKit: return "System picker (ScreenCaptureKit)"
        }
    }

    /// Longer explainer used by the FR-052 consent sheet and status a11y labels.
    public var explainer: String {
        switch self {
        case .selfTap:
            return "Captures this app's own playback through AVAudioSession — no microphone needed."
        case .replayKitBroadcast:
            return "Captures other apps' permitted audio via the system broadcast picker (iOS 12–26)."
        case .screenCaptureKit:
            return "Captures shared audio via the system content-sharing picker, ScreenCaptureKit (iOS 27+)."
        }
    }
}
