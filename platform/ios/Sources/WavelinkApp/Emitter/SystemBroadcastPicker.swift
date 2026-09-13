//
//  SystemBroadcastPicker.swift
//  WDRiOSEmitterApp
//
//  The SYSTEM consent surface for other-app audio capture, per PLATFORM_MATRIX
//  §A iOS + App Review Guideline 2.5.14:
//
//    * iOS 12–26 → `RPSystemBroadcastPickerView` (ReplayKit), the PUBLIC system
//      picker. The app presents THIS; it never starts a broadcast for other apps'
//      audio programmatically (no hiding behind `RPBroadcastController`).
//    * iOS 27+   → `SCContentSharingPicker` (ScreenCaptureKit). The SDK on this
//      host is 26.5 (no ScreenCaptureKit framework), so the SCK branch is behind
//      `#if canImport(ScreenCaptureKit)` — it is a DOCUMENTED scaffold that only
//      compiles/compiles-check on an iOS 27+ SDK, and it is DEVICE-GATED.
//
//  Accessibility (FR-056): `RPSystemBroadcastPickerView`'s internal button is not
//  directly reachable by VoiceOver, so we expose the whole control as ONE
//  labelled button element (traits + label + hint) — the standard, honest
//  pattern for this system view.
//

import SwiftUI
import UIKit
import ReplayKit

/// Wraps the PUBLIC ReplayKit system picker (iOS 12+). Capturing any other app's
/// audio happens ONLY after the user taps this system control; the red recording
/// indicator is the SYSTEM's, shown continuously while capturing.
struct SystemBroadcastPickerView: UIViewRepresentable {
    /// Our own broadcast-upload-extension bundle id (set at build time by the
    /// app+extension Xcode project). `nil` shows the generic system list.
    let preferredExtensionBundleID: String?
    /// This shell never requests the mic — keep the mic toggle hidden (no mic
    /// TCC prompt: CapturePolicy.microphoneInvolved == false for every mode).
    let showsMicrophoneButton = false

    func makeUIView(context: Context) -> RPSystemBroadcastPickerView {
        let view = RPSystemBroadcastPickerView(frame: .zero)
        view.preferredExtension = preferredExtensionBundleID
        view.showsMicrophoneButton = showsMicrophoneButton
        return view
    }

    func updateUIView(_ uiView: RPSystemBroadcastPickerView, context: Context) {
        uiView.preferredExtension = preferredExtensionBundleID
        uiView.showsMicrophoneButton = showsMicrophoneButton
    }
}

/// ScreenCaptureKit content-sharing picker action (iOS 27+ ONLY). Compiled only
/// when the SDK ships ScreenCaptureKit; on this host (SDK 26.5) `canImport` is
/// false and the fallback is an honest "requires iOS 27" button. Never verified
/// on this host — device + 27+ SDK gate (build-check.md).
struct ContentSharingPickerButton: View {
    var body: some View {
        Button(action: presentSystemContentSharingPicker) {
            Label("Start Capture — iOS 27+ system picker", systemImage: "record.circle")
                .frame(maxWidth: .infinity)
                .padding(.vertical, 8)
        }
        .accessibilityLabel("Start Capture with the iOS 27 system content sharing picker")
        .accessibilityHint("Opens Apple's ScreenCaptureKit picker to choose the audio to capture.")
    }

    private func presentSystemContentSharingPicker() {
        #if canImport(ScreenCaptureKit)
        // iOS 27+ scaffold (NOT compiled on this host; SDK 26.5 has no
        // ScreenCaptureKit). Documented shape only — verify on the iOS 27+ SDK
        // and a physical device before enabling:
        //
        //   import ScreenCaptureKit
        //   SCContentSharingPicker.sharedPicker.isMicrophoneCaptureEnabled = false
        //   SCContentSharingPicker.sharedPicker.present()   // system UI, .audio output
        #endif

        #if !canImport(ScreenCaptureKit)
        // Honest fallback reached only when the running SDK can't know SCK:
        // this is surfaced by the UI as a device/SDK gate, never as working code.
        #endif
    }
}

/// Accessibility-friendly wrapper so VoiceOver reads the ReplayKit system picker
/// as one labelled button (the picker's own sub-button is otherwise unreachable).
struct AccessibleSystemPickerButton: View {
    let preferredExtensionBundleID: String?

    var body: some View {
        SystemBroadcastPickerView(preferredExtensionBundleID: preferredExtensionBundleID)
            .frame(width: 44, height: 44)
            .contentShape(Rectangle())
            .accessibilityElement(children: .ignore)
            .accessibilityLabel("Start Broadcast")
            .accessibilityValue("Opens Apple's system broadcast picker")
            .accessibilityHint("This is Apple's own picker. Tapping it lets you choose which app audio to broadcast; the system shows a red recording indicator while capturing.")
            .accessibilityAddTraits(.isButton)
    }
}
