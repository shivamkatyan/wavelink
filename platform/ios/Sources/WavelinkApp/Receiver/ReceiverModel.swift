//
//  ReceiverModel.swift
//  WDRReceiverApp
//
//  Single source of truth for the receiver shell UI: tiers policy,
//  live status (FR-053), route reporting (FR-013/FR-015) and the renderer.
//  Confined to the main actor (SwiftUI-facing); the pure core types stay
//  independent so the logic is unit-testable without UIKit.
//

import Combine
import Foundation
import SwiftUI

/// Persisted in-memory tier store (dev/demo switch — FR-045 note: this is a
/// demonstration toggle, NOT tamper-resistant enforcement; commerce backend
/// adapter is a later, separate project per FR-044/FR-048).
final class ReceiverTierStore: PolicyStore {
    private var _tier: EntitlementTier
    init(initial: EntitlementTier = .free) {
        _tier = initial
    }
    func readTier() -> EntitlementTier { _tier }
    func writeTier(_ tier: EntitlementTier) { _tier = tier }
}

/// Buffer profile choice (FR-023).
enum BufferProfile: String, CaseIterable, Identifiable {
    case lowLatency
    case balanced
    case resilient

    var id: String { rawValue }
    var title: String {
        switch self {
        case .lowLatency: return "Low Latency"
        case .balanced: return "Balanced"
        case .resilient: return "Resilient"
        }
    }
}

@MainActor
final class ReceiverModel: ObservableObject {
    // MARK: Tiers & policy
    let policyGate: PolicyGate
    private let tierStore: ReceiverTierStore
    @Published var currentTier: EntitlementTier = .free
    @Published var requiresDowngradeConfirmation = false

    // MARK: Live status (FR-053)
    @Published var status: StatusModel
    @Published var routeSnapshot: RouteSnapshot = .none
    @Published var lastRouteEvent: RouteChangeEvent?
    @Published var selectedProfile: BufferProfile = .balanced

    // MARK: Render
    let renderer: Renderer
    private let usbPlayer: USBAudioPlayer
    @Published var rendererState: RendererState = .stopped
    @Published var routeReportingAvailable: Bool

    // MARK: Permission flow (FR-052)
    @Published var permissionExplained = false
    @Published var sessionStarted = false

    private let outputRouter: OutputRouter
    private var demoHealthTimer: Timer?
    private var cancellables = Set<AnyCancellable>()

    init(appSupportPeersInfoHidden: Void = ()) {
        let store = ReceiverTierStore(initial: .free)
        tierStore = store
        policyGate = PolicyGate(store: store)
        currentTier = policyGate.currentTier

        let renderer = Renderer(sampleRate: 48_000, channels: 2)
        self.renderer = renderer
        usbPlayer = USBAudioPlayer(renderer: renderer)

        status = StatusModel(state: .idle, transport: .wifi, codec: "", route: "none")

        let router = OutputRouter()
        outputRouter = router
        routeSnapshot = router.currentRouteSnapshot()
        routeReportingAvailable = true

        // Observed route events flow into the model on the main queue
        // (OutputRouter registers its observer with .main).
        router.events
            .receive(on: DispatchQueue.main)
            .sink { [weak self] event in
                self?.handleRouteEvent(event)
            }
            .store(in: &cancellables)

        router.startObserving()
    }

    deinit {
        demoHealthTimer?.invalidate()
        outputRouter.stopObserving()
    }
}

// MARK: - Route + renderer coordination

extension ReceiverModel {
    /// Central reaction to a route change (FR-015): translate to the player
    /// decision (pause on NO_ROUTE) and refresh the FR-053 route label.
    func handleRouteEvent(_ event: RouteChangeEvent) {
        lastRouteEvent = event
        let snapshot = outputRouter.currentRouteSnapshot()
        routeSnapshot = snapshot
        status.route = snapshot.routeLabel

        let decision = OutputRouteDetector.decide(snapshot: snapshot)
        usbPlayer.handle(routeDecision: decision)
        rendererState = renderer.state
    }

    func refreshRouteLabel() {
        let snapshot = outputRouter.currentRouteSnapshot()
        routeSnapshot = snapshot
        status.route = snapshot.routeLabel
    }

    /// Resume after a NO_ROUTE pause once a usable route returns.
    func resumeIfPaused() {
        guard case .paused = renderer.state else { return }
        let decision = OutputRouteDetector.decide(snapshot: routeSnapshot)
        usbPlayer.handle(routeDecision: decision)
        rendererState = renderer.state
    }
}

// MARK: - User actions

extension ReceiverModel {
    /// Free/Pro toggle (FR-040/048). Never silently downgrades mid-lossless
    /// (FR-026/FR-047): surfaces REQUIRES_CONFIRM and sets the confirm flag.
    func toggleTier(_ newTier: EntitlementTier) {
        switch policyGate.toggle(to: newTier) {
        case .applied:
            currentTier = policyGate.currentTier
            syncFidelityFromTier()
        case .requiresConfirm:
            requiresDowngradeConfirmation = true
        }
    }

    func confirmDowngradeToFree() {
        requiresDowngradeConfirmation = false
        policyGate.applyConfirmedTier(.free)
        policyGate.setStreamingLossless(false)
        currentTier = policyGate.currentTier
        syncFidelityFromTier()
    }

    func dismissDowngradeConfirmation() {
        requiresDowngradeConfirmation = false
        // Tier unchanged (the request was never applied).
    }

    /// Keep the fidelity label honest relative to the tier (FR-042/043):
    /// Free can never present a lossless path — the fidelity state becomes
    /// lossyTransport when a PRO user drops to Free mid-session.
    private func syncFidelityFromTier() {
        if currentTier == .free {
            _ = status.fidelity.transition(to: .lossyTransport)
            policyGate.setStreamingLossless(false)
        } else {
            policyGate.setStreamingLossless(status.fidelity.transportsLossless)
        }
    }

    /// Start the demo render session after the FR-052 explanation is shown.
    func startSession() {
        guard permissionExplained else { return }
        sessionStarted = true
        status.state = .streaming
        status.codec = "opus" // demo default; flac/pcm on Pro lossless (ADR-005)
        status.fidelity = FidelityStatus(
            step: currentTier == .pro ? .losslessTransportOutputPathUnverified : .lossyTransport,
            sampleRateHz: 48_000, bitDepth: currentTier == .pro ? 24 : 16,
            channels: 2, frameRate: 43, codec: currentTier == .pro ? "flac" : "opus"
        )
        policyGate.setStreamingLossless(status.fidelity.transportsLossless)

        let usesDac = routeSnapshot.isUsbAudioRouted
        switch renderer.start(routeUsesUsbDac: usesDac) {
        case .success:
            rendererState = renderer.state
        case .failure(let error):
            let reason: String
            switch error {
            case .alreadyActive: reason = "renderer already active"
            case .sessionActivation(let m), .engineStart(let m): reason = m
            }
            rendererState = .failed(reason)
            status.state = .error
        }
        startDemoHealthTimer()
    }

    func stopSession() {
        sessionStarted = false
        status.state = .idle
        renderer.stop()
        rendererState = .stopped
        demoHealthTimer?.invalidate()
        demoHealthTimer = nil
    }

    /// Jitter-buffer fill / loss / latency demo values so the health panel is
    /// visibly live (FR-053 display). The network transport task will replace
    /// these with real telemetry.
    private func startDemoHealthTimer() {
        demoHealthTimer?.invalidate()
        demoHealthTimer = Timer.scheduledTimer(withTimeInterval: 1.0, repeats: true) { [weak self] _ in
            Task { @MainActor in
                guard let self, self.sessionStarted else { return }
                var s = self.status
                s.latencyMs = Int.random(in: 20...120)
                s.bufferFill = min(1.0, max(0.05, s.bufferFill + Double.random(in: -0.1...0.12)))
                s.packetLossPct = Double.random(in: 0...1.5)
                if Int.random(in: 0..<40) == 0 { s.underruns += 1 }
                self.status = s
            }
        }
    }
}
