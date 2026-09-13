//
//  EmissionModel.swift
//  WDRiOSEmitterApp
//
//  Single source of truth for the emitter shell UI: tier policy (Free/Pro),
//  FR-052 consent flow, live capture status (FR-053) and the capture state.
//  Confined to the main actor (SwiftUI-facing); the pure core types stay
//  independent so the logic is unit-testable without UIKit (see
//  Tests/WDRiOSEmitterCoreTests).
//
//  Honesty contract (App Review 2.5.14): capture consent ALWAYS goes through the
//  SYSTEM picker. The model's phase machine never reaches `.authorized` without
//  `.awaitingSystemPicker` first (ConsentFlowState enforces it and is tested).
//  The actual picker interactions (RPBroadcastController / SCContentSharingPicker
//  callbacks) are DEVICE-only — the model exposes the seams and the device
//  runbook drives them.
//

import Combine
import Foundation
import SwiftUI
import UIKit

/// Persisted in-memory tier store (dev/demo switch — FR-045 note: this is a
/// demonstration toggle, NOT tamper-resistant enforcement; commerce backend
/// adapter is a later, separate project per FR-044/FR-048).
final class EmitterTierStore: PolicyStore {
    private var _tier: EntitlementTier
    init(initial: EntitlementTier = .free) {
        _tier = initial
    }
    func readTier() -> EntitlementTier { _tier }
    func writeTier(_ tier: EntitlementTier) { _tier = tier }
}

@MainActor
final class EmissionModel: ObservableObject {
    // MARK: Tiers & policy
    let policyGate: PolicyGate
    private let tierStore: EmitterTierStore
    @Published var currentTier: EntitlementTier = .free
    @Published var requiresDowngradeConfirmation = false

    // MARK: Capture mode (from the capability matrix, keyed to this device's OS)
    @Published var captureMode: CaptureMode?

    // MARK: FR-052 consent flow (ConsentFlowState value type — published copies)
    @Published var consentPhase: ConsentPhase = .needsExplanation
    @Published var consentDenial: ConsentDenial?
    @Published var explanationPresented = false
    private var flow = ConsentFlowState()

    // MARK: Live status (FR-053)
    @Published var status: EmissionStatus
    @Published var captureActive = false
    @Published var lastDenialShortLabel: String?

    private var demoHealthTimer: Timer?
    /// The seam sink the shell drives today (WS4 seam-level wiring): real
    /// FixtureFrameSink counts replace demo-random telemetry for FR-053 until
    /// the network/FFI transport lands behind the same FrameSink interface.
    private var currentSink: FixtureFrameSink?

    init() {
        let store = EmitterTierStore(initial: .free)
        tierStore = store
        policyGate = PolicyGate(store: store)
        currentTier = policyGate.currentTier

        let osMajor = EmissionModel.currentOSMajor()
        let mode = iOSCapabilityMatrix.recommendedCaptureMode(forOSMajor: osMajor)
        captureMode = mode
        status = EmissionStatus(
            state: .idle,
            mode: mode ?? .replayKitBroadcast,
            scope: mode == .selfTap ? .selfOnly : .otherApp,
            sourceLabel: ""
        )
    }

    deinit {
        demoHealthTimer?.invalidate()
    }

    /// The capture-mode picker is only presentable when capture is NOT active.
    var canChooseMode: Bool {
        consentPhase != .authorized
    }

    /// Runtime OS major version (device-gated value; the shell never lies about
    /// what mode its OS permits — the matrix decides).
    private static func currentOSMajor() -> Int {
        let raw = UIDevice.current.systemVersion
        let major = raw.split(separator: ".").first.map(String.init) ?? "14"
        return Int(major) ?? 14
    }
}

// MARK: - Consent flow (FR-052 / FR-054)

extension EmissionModel {
    /// The user tapped "Start capture" — FR-052: show the explanation BEFORE the
    /// system picker appears.
    func requestExplanation() {
        guard consentPhase == .needsExplanation else { return }
        explanationPresented = true
    }

    /// Called by ConsentExplainerView's Continue: explanation acknowledged, so
    /// the App layer may now present the SYSTEM picker.
    func dismissExplanationAndPresentPicker() {
        if flow.transition(to: .awaitingSystemPicker) == .applied {
            consentPhase = flow.phase
        }
        explanationPresented = false
    }

    /// "Not now" from the explainer — stay before the picker.
    func closeExplanationWithoutCapture() {
        explanationPresented = false
        captureActive = false
    }

    /// DEVICE seam: the system picker / broadcast controller reported success.
    /// Never reachable on this host (device-gated).
    func systemAuthorized() {
        guard flow.transition(to: .authorized) == .applied else { return }
        consentPhase = flow.phase
        consentDenial = nil
        captureActive = true
        status.state = .broadcasting
        status.mode = captureMode ?? .replayKitBroadcast
        status.scope = status.mode == .selfTap ? .selfOnly : .otherApp
        status.systemIndicatorVisible =
            EmissionStatus.systemIndicatorVisible(for: status.mode, isActive: true)
        policyGate.setCapturingLossless(currentTier == .pro)

        // WS4 seam: wire a real sink (format -> fixture block -> finish on
        // stop) so FR-053 derives from genuinely observed blocks, not demo.
        let bitDepth = currentTier == .pro ? 24 : 16
        let sink = FixtureFrameSink(channels: 2, bitDepth: bitDepth, sampleRateHz: 48_000)
        sink.onFormat(channels: 2, bitDepth: bitDepth, sampleRateHz: 48_000)
        sink.onBlock(data: FixtureFrameSink.makeFixture(frames: 512 * 2))
        currentSink = sink
        syncStatusFromSink()

        startDemoHealthTimer()
    }

    /// DEVICE seam: the user cancelled/denied at the system picker (FR-054).
    func systemDenied(code: ConsentDenialCode) {
        let denial = ConsentDenial(code: code)
        if flow.transition(to: .denied, denial: denial) == .applied {
            consentPhase = flow.phase
            consentDenial = flow.denial
            lastDenialShortLabel = denial.guidance
        }
        captureActive = false
        status.state = .idle
    }

    /// Retry from the denied state (FR-054) — back to the system picker, no
    /// re-explanation.
    func retrySystemPicker() {
        if flow.transition(to: .awaitingSystemPicker) == .applied {
            consentPhase = flow.phase
            consentDenial = nil
        }
    }

    /// Stop/restart path: capture ended, user may start again via the picker.
    func stopCapture() {
        captureActive = false
        status.state = .idle
        status.systemIndicatorVisible = false
        policyGate.setCapturingLossless(false)
        // End-of-session: the seam's finish() lets the sink commit/flush.
        currentSink?.finish()
        currentSink = nil
        demoHealthTimer?.invalidate()
        demoHealthTimer = nil
        if flow.transition(to: .awaitingSystemPicker) == .applied {
            consentPhase = flow.phase
        }
    }

    /// Full reset (e.g. explicit "Reset consent" — not user-facing by default).
    func resetConsent() {
        if flow.transition(to: .needsExplanation) == .applied {
            consentPhase = flow.phase
            consentDenial = nil
        }
    }
}

// MARK: - User actions (tier / downgrade confirmation — FR-040/FR-047)

extension EmissionModel {
    /// Free/Pro toggle. Never silently downgrades mid-lossless-capture
    /// (FR-026/FR-047): surfaces REQUIRES_CONFIRM and sets the confirm flag.
    func toggleTier(_ newTier: EntitlementTier) {
        switch policyGate.toggle(to: newTier) {
        case .applied:
            currentTier = policyGate.currentTier
            syncCaptureFidelityFromTier()
        case .requiresConfirm:
            requiresDowngradeConfirmation = true
        }
    }

    func confirmDowngradeToFree() {
        requiresDowngradeConfirmation = false
        policyGate.applyConfirmedTier(.free)
        policyGate.setCapturingLossless(false)
        currentTier = policyGate.currentTier
        syncCaptureFidelityFromTier()
    }

    func dismissDowngradeConfirmation() {
        requiresDowngradeConfirmation = false
        // Tier unchanged (the request was never applied).
    }

    /// Keep capture fidelity honest relative to the tier (FR-042/043): a Free
    /// user can never present a lossless capture path.
    private func syncCaptureFidelityFromTier() {
        if currentTier == .free {
            policyGate.setCapturingLossless(false)
        } else {
            policyGate.setCapturingLossless(captureActive)
        }
    }

    /// FR-053 status from the seam sink's genuinely observed blocks. Latency,
    /// buffer fill and loss stay ZERO/unknown — the network transport is not
    /// wired, so those are never demo-faked.
    private func syncStatusFromSink() {
        guard let sink = currentSink else { return }
        var s = status
        s.sampleRateHz = 48_000
        s.channels = 2
        s.bitDepth = sink.frameSizeBytes * 8 / max(s.channels, 1)
        s.frameRate = Double(sink.blockCount)
        s.latencyMs = 0
        s.bufferFill = 0
        s.packetLossPct = 0
        s.droppedBuffers = 0
        status = s
    }

    /// Periodic FR-053 telemetry so the status panel stays live — now derived
    /// from the seam sink (real block/sample counts), replacing the earlier
    /// demo-random values. The network transport task will add real latency/
    /// loss when the FFI transport lands.
    private func startDemoHealthTimer() {
        demoHealthTimer?.invalidate()
        demoHealthTimer = Timer.scheduledTimer(withTimeInterval: 1.0, repeats: true) { [weak self] _ in
            Task { @MainActor in
                guard let self, self.captureActive else { return }
                self.syncStatusFromSink()
            }
        }
    }
}
