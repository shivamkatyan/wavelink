//
//  PolicyGate.swift
//  WDRReceiverCore
//
//  Free/Pro entitlement toggle for the iOS app (FR-040..FR-048).
//  Mirrors the Android PolicyGate in role (verbatim logic):
//  Free = lossy Wi-Fi only (FR-042); Pro adds lossless Wi-Fi (FR-043);
//  a mid-session PRO -> FREE drop while streaming lossless is NEVER applied
//  silently (FR-026/FR-047) — surfaced to the caller as REQUIRES_CONFIRM.
//
//  Pure Foundation by design: persistence lives behind PolicyStore, so this
//  type (and its tests) never touch UIKit/AVFAudio/AVAudioSession. That keeps
//  the core `swift test`-runnable on this macOS host and type-checkable for the
//  iOS simulator SDK with the same sources.
//

import Foundation

/// Entitlement tier (FR-040). Free and Pro are the only *known* tiers; see
/// Decision on "fails closed on unknown" in PolicyGate.
public enum EntitlementTier: String, CaseIterable, Sendable, Equatable {
    case free
    case pro
}

/// Result of requesting a tier change when a session may be affected (FR-026/FR-047).
public enum RenegotiationRequest: Equatable, Sendable {
    /// The tier switch is safe to apply now.
    case applied

    /// The session is currently lossless and the request would silently
    /// downgrade it to Free. The caller must pause-and-confirm (or honour a
    /// saved downgrade preference) before applying; the gate does NOT change
    /// the tier in this case.
    case requiresConfirm
}

/// Minimal persistence seam so [PolicyGate] itself holds no OS dependencies.
public protocol PolicyStore: AnyObject {
    func readTier() -> EntitlementTier
    func writeTier(_ tier: EntitlementTier)
}

/// Capability set a peer uses during FR-006 negotiation.
public struct Policy: Equatable, Sendable {
    public let lossless: Bool

    public init(lossless: Bool) {
        self.lossless = lossless
    }
}

/// Pure-Foundation policy gate. "Currently streaming lossless" is session state
/// the caller drives via setStreamingLossless; the gate refuses to silently
/// downgrade a lossless session PRO -> FREE.
///
/// Concurrency: this class is a small mutable state machine. In the app it is
/// confined to the main actor via ReceiverModel (AppKit/SwiftUI contention is
/// not a concern for a shell); tests use it synchronously.
public final class PolicyGate {
    private let store: PolicyStore
    private var tier: EntitlementTier
    private var streamingLossless = false

    public init(store: PolicyStore) {
        self.store = store
        self.tier = store.readTier()
    }

    public var currentTier: EntitlementTier { tier }

    /// Free tier may never request a lossless session (FR-042). Fails closed:
    /// any tier other than exactly `.pro` (including a hypothetically
    /// deserialised/unknown value) returns false, so a bad/unknown tier can
    /// never grant lossless.
    public func allowsLossless() -> Bool {
        tier == .pro
    }

    /// Advertised capabilities for capability negotiation (FR-006/FR-046).
    public func policy() -> Policy {
        Policy(lossless: allowsLossless())
    }

    /// Called by the session layer whenever fidelity state changes.
    /// Receiver-role name for the live-lossless flag.
    public func setStreamingLossless(_ streaming: Bool) {
        streamingLossless = streaming
    }

    /// Emitter-role alias (same underlying flag): a lossless capture is live.
    public func setCapturingLossless(_ capturing: Bool) {
        streamingLossless = capturing
    }

    public func isStreamingLossless() -> Bool {
        streamingLossless
    }

    /// Toggle the tier. Never silently downgrades a lossless session: when
    /// currently streaming lossless and switching PRO -> FREE, the request is
    /// surfaced to the caller as `.requiresConfirm` and the tier is left
    /// unchanged (the caller re-applies only after confirmed renegotiation).
    @discardableResult
    public func toggle(to newTier: EntitlementTier) -> RenegotiationRequest {
        if newTier == tier {
            return .applied
        }
        let droppingWhileLossless =
            tier == .pro && streamingLossless && newTier == .free
        if droppingWhileLossless {
            return .requiresConfirm
        }
        applyTier(newTier)
        return .applied
    }

    /// Unconditional transitional switch used after the caller has confirmed.
    public func applyConfirmedTier(_ newTier: EntitlementTier) {
        applyTier(newTier)
    }

    private func applyTier(_ newTier: EntitlementTier) {
        tier = newTier
        store.writeTier(newTier)
    }
}
