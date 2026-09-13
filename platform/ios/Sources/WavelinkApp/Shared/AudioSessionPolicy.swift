//
//  AudioSessionPolicy.swift
//  WDRiOSEmitterCore
//
//  Enum-safe models for the iOS audio-session category + background-mode
//  declarations for the EMITTER (PLATFORM_MATRIX.md §A iOS row
//  "`.playback` + `UIBackgroundModes audio` (receiver); `screen-capture`+`audio`
//  (SCK emitter)"). Pure Foundation: this file only holds the enum/constants —
//  the real `AVAudioSession.setCategory(...)` call lives in the App layer where
//  AVAudioSession is importable (iOS-only runtime).
//
//  Honesty contract (see CapturePolicy.backgroundHonesty): capture NEVER starts
//  silently in the background; these declarations only allow an *active*
//  broadcast/session to continue while the app is backgrounded (system red
//  indicator still up) and the self-session tap to keep running while
//  backgrounded (`.audio`), both of which are device-gated.
//

import Foundation

/// Emitter audio-session category choices (PLATFORM_MATRIX iOS row).
public enum AudioSessionCategory: String, CaseIterable, Sendable, Equatable {
    /// Maps to AVAudioSession.Category.playback — for the self-session tap on the
    /// app's OWN output node (no microphone prompt; the app taps what it plays).
    case playback

    /// Maps to AVAudioSession.Category.playAndRecord — used ONLY if a future
    /// input-node branch is added (mic consent would be its own TCC surface,
    /// NOT part of this shell by default).
    case playAndRecord

    /// Encapsulated raw NSString constant the app layer passes to the SDK.
    public var rawAVAudioSessionCategory: String {
        switch self {
        case .playback: return "AVAudioSessionCategoryPlayback"
        case .playAndRecord: return "AVAudioSessionCategoryPlayAndRecord"
        }
    }
}

/// Emitter audio-session mode.
public enum AudioSessionMode: String, CaseIterable, Sendable, Equatable {
    case `default`

    public var rawAVAudioSessionMode: String {
        switch self {
        case .default: return "AVAudioSessionModeDefault"
        }
    }
}

/// The exact set of `UIBackgroundModes` entries the emitter declares in
/// Info.plist. `.audio` keeps an active self-session tap alive while
/// backgrounded; `screen-capture` (iOS 11+) is the standard declaration for a
/// capture/broadcast app whose system-gated capture may continue in the
/// background with the system indicator visible (ReplayKit/SCK extension
/// sessions run in the broadcast host). Nothing here grants silent background
/// capture — start is always foreground + system picker (CapturePolicy).
public enum UIBackgroundModes {
    public static let audio = "audio"
    public static let screenCapture = "screen-capture"

    /// The set that must appear in the built Info.plist under `UIBackgroundModes`.
    public static let declared: Set<String> = [audio, screenCapture]
}

/// Self-describing emitter audio-session + background policy. The App layer
/// derives its `AVAudioSession` configuration from this value and the string
/// constants above, so the policy is testable and a single source of truth.
public struct AudioSessionPolicy: Sendable, Equatable {
    public let category: AudioSessionCategory
    public let mode: AudioSessionMode
    public let backgroundModes: Set<String>

    public init(
        category: AudioSessionCategory,
        mode: AudioSessionMode,
        backgroundModes: Set<String>
    ) {
        self.category = category
        self.mode = mode
        self.backgroundModes = backgroundModes
    }

    /// Canonical policy for the WDR emitter shell.
    public static let emitterCapture = AudioSessionPolicy(
        category: .playback,
        mode: .default,
        backgroundModes: UIBackgroundModes.declared
    )

    /// Canonical policy for the WDR receiver shell.
    public static let receiverPlayback = AudioSessionPolicy(
        category: .playback,
        mode: .default,
        backgroundModes: UIBackgroundModes.declared
    )
}
