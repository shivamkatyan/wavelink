//
//  OutputRouter.swift
//  WDRReceiverApp
//
//  Bridges live AVAudioSession route state into the pure core model
//  (RouteSnapshot / PortDescriptor) and handles route-change notifications
//  (FR-013/FR-015). Mirrors the android AudioOutputRouter role: enumeration +
//  hotplug reporting.
//
//  HARD HONESTY RULE: iOS offers NO public API to force a specific hardware
//  output port — `AVAudioSession` lets us *observe* the current route (incl.
//  `.usbAudio` visibility) but the system auto-selects the port
//  (PLATFORM_MATRIX §A iOS: "`.usbAudio` visibility; system auto-selects;
//  cannot force specific DAC"). This class therefore only REPORTS the route; it
//  never claims to route/force a DAC.
//

import AVFoundation
import Combine
import Foundation

/// Observed route from the OS.
enum RouteObservationResult {
    case snapshot(RouteSnapshot)
    case noSession // AVAudioSession not available (should not happen on iOS)
}

/// Observes AVAudioSession.currentRoute and re-emits core RouteChangeEvents
/// (FR-013/FR-015). Injected dependencies (session + notification center) keep
/// it testable on the host in a later task if desired.
final class OutputRouter {
    private let session: AVAudioSession
    private let notificationCenter: NotificationCenter
    private var routeChangeObserver: NSObjectProtocol?
    private var lastSnapshot: RouteSnapshot

    /// Combine stream of classified route changes (FR-015 hotplug events).
    let events = PassthroughSubject<RouteChangeEvent, Never>()

    init(
        session: AVAudioSession = .sharedInstance(),
        notificationCenter: NotificationCenter = .default,
        initial snapshot: RouteSnapshot? = nil
    ) {
        self.session = session
        self.notificationCenter = notificationCenter
        self.lastSnapshot = snapshot ?? .none
    }

    deinit {
        stopObserving()
    }

    /// Map the current OS route into the core model. Returns .none when there
    /// is no output at all.
    func currentRouteSnapshot() -> RouteSnapshot {
        let route = session.currentRoute
        let ports = route.outputs.map { port in
            PortDescriptor(name: port.portName, portTypeRaw: port.portType.rawValue)
        }
        guard !ports.isEmpty else { return .none }
        return RouteSnapshot(outputs: ports, activeOutput: ports.first)
    }

    /// Begin observing `AVAudioSession.routeChangeNotification` (FR-015 hotplug).
    func startObserving() {
        guard routeChangeObserver == nil else { return }
        routeChangeObserver = notificationCenter.addObserver(
            forName: AVAudioSession.routeChangeNotification,
            object: session,
            queue: .main
        ) { [weak self] _ in
            guard let self else { return }
            let newSnapshot = self.currentRouteSnapshot()
            // Re-diff against the last snapshot we reported.
            let event = OutputRouteDetector.classifyChange(from: self.lastSnapshot, to: newSnapshot)
            self.lastSnapshot = newSnapshot
            self.events.send(event)
        }
    }

    func stopObserving() {
        if let routeChangeObserver {
            notificationCenter.removeObserver(routeChangeObserver)
            self.routeChangeObserver = nil
        }
    }

    /// True when the current route includes a USB DAC (FR-013 visibility).
    var isUsbAudioPresent: Bool {
        currentRouteSnapshot().isUsbAudioRouted
    }
}
