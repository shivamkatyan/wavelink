//
//  Renderer.swift
//  WDRReceiverApp
//
//  AVAudioEngine output plumbing for the iOS receiver (FR-012/FR-016).
//  Attaches an AVAudioPlayerNode, connects it to the output node and starts the
//  engine; plays a looping demo PCM tone so the render path is A/B-testable on a
//  device ("USB DAC audible vs phone speaker") before the network transport
//  lands. The USBAudioPlayer wrapper reflects the selected output route and
//  pauses on NO_ROUTE — but, matching PLATFORM_MATRIX, never claims to force a
//  DAC: AVAudioSession auto-selects the DAC on attach; we only label what the
//  system routed.
//
//  iOS-only runtime (AVAudioSession); this file lives in the App target and is
//  type-checked against the iphonesimulator SDK, not the macOS SPM package.
//

import AVFoundation

/// UI-facing renderer state machine.
enum RendererState: Equatable {
    case stopped
    case starting
    case running(isUsbDac: Bool)
    case paused
    case failed(String)
}

/// Renderer start failures (surfaced as actionable messages, FR-054).
enum RendererError: Error, Equatable {
    case alreadyActive
    case sessionActivation(String)
    case engineStart(String)
}

final class Renderer {
    private let engine = AVAudioEngine()
    private let playerNode = AVAudioPlayerNode()
    private let sampleRate: Double
    private let channels: AVAudioChannelCount
    private var outputFormat: AVAudioFormat?
    /// Underrun counter incremented when the player node reports it fell behind
    /// (FR-053 `underruns`) — sketch-level for the shell; the transport will own
    /// the real count.
    private(set) var underruns: Int = 0

    private(set) var state: RendererState = .stopped

    init(sampleRate: Double = 48_000, channels: AVAudioChannelCount = 2) {
        self.sampleRate = sampleRate
        self.channels = channels
    }

    /// Activate the audio session in `.playback` (FR-012/FR-016 + PLATFORM_MATRIX:
    /// playback category + UIBackgroundModes audio), configure the engine and
    /// begin rendering a looping demo tone.
    func start(routeUsesUsbDac: Bool) -> Result<Void, RendererError> {
        switch state {
        case .stopped, .failed:
            break // may (re)start
        case .starting, .running, .paused:
            return .failure(.alreadyActive)
        }

        let audioSession = AVAudioSession.sharedInstance()
        do {
            // Category enum-safe from the core AudioSessionPolicy (raw constant
            // "AVAudioSessionCategoryPlayback").
            try audioSession.setCategory(
                AVAudioSession.Category(rawValue: AudioSessionPolicy.receiverPlayback.category.rawAVAudioSessionCategory),
                mode: AVAudioSession.Mode(rawValue: AudioSessionPolicy.receiverPlayback.mode.rawAVAudioSessionMode),
                options: []
            )
            try audioSession.setActive(true, options: [])
        } catch {
            let message = "audio session activation failed: \(error.localizedDescription)"
            state = .failed(message)
            return .failure(.sessionActivation(message))
        }

        let format = AVAudioFormat(standardFormatWithSampleRate: sampleRate, channels: channels)
        outputFormat = format

        // Wire: playerNode -> mainMixer -> outputNode (standard AVAudioEngine graph).
        engine.attach(playerNode)
        engine.connect(playerNode, to: engine.mainMixerNode, format: format)
        engine.connect(engine.mainMixerNode, to: engine.outputNode, format: format)

        do {
            try engine.start()
        } catch {
            let message = "AVAudioEngine failed to start: \(error.localizedDescription)"
            state = .failed(message)
            return .failure(.engineStart(message))
        }

        scheduleDemoLoop(format: format)
        playerNode.play()
        state = .running(isUsbDac: routeUsesUsbDac)
        return .success(())
    }

    func pause() {
        guard case .running = state else { return }
        playerNode.pause()
        state = .paused
    }

    func resume(routeUsesUsbDac: Bool) {
        guard case .paused = state else { return }
        playerNode.play()
        state = .running(isUsbDac: routeUsesUsbDac)
    }

    func stop() {
        playerNode.stop()
        engine.stop()
        engine.reset()
        state = .stopped
    }

    /// Make the route visible to the shell/telemetry (FR-053 `route`) without
    /// any claim of forcing: it labels what the OS routed.
    var isRenderingToUsbDac: Bool {
        if case .running(isUsbDac: let usb) = state { return usb }
        return false
    }

    // MARK: - Demo tone

    private func scheduleDemoLoop(format: AVAudioFormat?) {
        guard let format, let buffer = Self.makeDemoToneBuffer(format: format) else { return }
        // `.loops` repeats the buffer indefinitely; a route change that would
        // interrupt playback is handled by the player wrapper (pause/resume).
        playerNode.scheduleBuffer(buffer, at: nil, options: .loops, completionHandler: nil)
    }
}

extension Renderer {
    /// Build a looping stereo sine tone (440 Hz at -18 dBFS) + alternating
    /// stereo-sync impulse so a listener can distinguish "DAC vs speaker" and
    /// left/right mapping by ear. Pure PCM, no synthesis node required.
    static func makeDemoToneBuffer(format: AVAudioFormat, seconds: Int = 2) -> AVAudioPCMBuffer? {
        let frameCount = AVAudioFrameCount(format.sampleRate * Double(seconds))
        guard let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: frameCount) else {
            return nil
        }
        buffer.frameLength = frameCount
        guard let channelData = buffer.floatChannelData else { return nil }
        let frames = Int(frameCount)
        let sr = format.sampleRate
        for frame in 0..<frames {
            let t = Double(frame) / sr
            let left = 0.1 * sin(2 * .pi * 440 * t)
            let right = 0.1 * sin(2 * .pi * 440 * t + (isLeft(t: t) ? 0 : .pi))
            channelData[0][frame] = Float(left)
            channelData[1][frame] = Float(right)
        }
        return buffer
    }

    /// Alternate left/right phase every 0.5 s so the channel identity is
    /// audibly distinct (used only by the demo tone).
    private static func isLeft(t: Double) -> Bool {
        Int(t * 2) % 2 == 0
    }
}

/// Player wrapper that *reflects* the selected route (FR-013/FR-015): starts in
/// render mode, pauses on NO_ROUTE, and exposes an honest `routeLabel`. It never
/// selects hardware — the OS does.
final class USBAudioPlayer {
    let renderer: Renderer

    init(renderer: Renderer) {
        self.renderer = renderer
    }

    func handle(routeDecision: OutputRouteDetector.RouteDecision) {
        switch routeDecision {
        case .renderUsbDac(let port):
            routeLabel = port.displayLabel
            if case .paused = renderer.state { renderer.resume(routeUsesUsbDac: true) }
        case .renderFallback(let port):
            routeLabel = port.displayLabel
            if case .paused = renderer.state { renderer.resume(routeUsesUsbDac: false) }
        case .pause:
            renderer.pause()
            routeLabel = "none"
        }
    }

    private(set) var routeLabel: String = "none"
}
