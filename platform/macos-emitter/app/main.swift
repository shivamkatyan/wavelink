// Wavelink — macOS Emitter UI.
// A polished AppKit window app (+ menu-bar extra) built without storyboards or
// external deps. It shells out to the bundled Rust CLI
// (Contents/Resources/bin/macos-emitter) — the `--stream` engine — and renders
// the newline-delimited JSON status events into a live, structured UI.
//
// Design notes:
// * `static func main()` (not the bare synthesized `NSApplicationMain`) so the
//   delegate — and therefore the window — always appears (fixed "no UI" bug).
// * Closing the window does NOT quit: the app keeps running in the menu bar
//   (background-streaming app semantics); Quit (Cmd-Q / menu) SIGTERMs the
//   streaming child so the receiver still gets its end-of-stream marker.
import AppKit
import Foundation

private let cliPath = { () -> String in
    if let res = Bundle.main.resourceURL {
        let p = res.appendingPathComponent("bin/macos-emitter").path
        if FileManager.default.isExecutableFile(atPath: p) { return p }
    }
    return "macos-emitter" // fallback: PATH (dev runs)
}()

// MARK: - Palette (semantic tokens; dark/light via NSAppearance)

private extension NSColor {
    /// Fixed sRGB hex, ignored alpha.
    convenience init(wdrHex: UInt32) {
        self.init(
            srgbRed: CGFloat((wdrHex >> 16) & 0xFF) / 255,
            green: CGFloat((wdrHex >> 8) & 0xFF) / 255,
            blue: CGFloat(wdrHex & 0xFF) / 255,
            alpha: 1
        )
    }

    /// A dynamic color resolving light/dark from the current appearance.
    static func wdr(_ light: UInt32, _ dark: UInt32) -> NSColor {
        NSColor(name: nil) { appearance in
            let isDark = appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
            return NSColor(wdrHex: isDark ? dark : light)
        }
    }
}

private enum Wd {
    // The exact iOS Palette hex pairs (dark, light), WCAG-AA contrast-checked.
    static let background = NSColor.wdr(0xF5F5F7, 0x1C1C1E)
    static let textPrimary = NSColor.wdr(0x1C1C1E, 0xF2F2F7)
    static let textSecondary = NSColor.wdr(0x545458, 0xC7C7CC)
    static let accent = NSColor.wdr(0x0040DD, 0x0A84FF)
    static let good = NSColor.wdr(0x00754C, 0x32D74B)
    static let warn = NSColor.wdr(0x8A5A00, 0xFFD60A)
    static let error = NSColor.wdr(0xC0262B, 0xFF453A)
    static let pro = NSColor.wdr(0x9A5B00, 0xFF9F0A)
    static let free = NSColor.wdr(0x6E6E73, 0x8E8E93)
    static let card = NSColor.wdr(0xFFFFFF, 0x26262A)
}

// MARK: - App

@main
final class AppDelegate: NSObject, NSApplicationDelegate {
    // Shared streaming state (single source of truth for the window + menubar).
    private enum State {
        case idle
        case connecting
        case streaming
        case stopping
        case error(String)
    }

    private var state: State = .idle {
        didSet { renderState() }
    }
    private var metrics: [String: String] = [:]
    private var activityLines: [(String, String, NSColor)] = [] // time, text, color
    private var lastFatal: String?

    private var window: NSWindow!
    private var pillDot: NSView!
    private var pillLabel: NSTextField!
    private var addrField: NSTextField!
    private var actionButton: NSButton!
    private var tierControl: NSSegmentedControl!
    private var metricsLabels: [String: NSTextField] = [:]
    private var permissionRow: NSStackView!
    private var permissionLabel: NSTextField!
    private var logView: NSTextView!
    private var logToggle: NSButton!
    private var statusItem: NSStatusItem!

    /// The streaming child process (macos-emitter --stream).
    private var streamProc: Process?
    private var streamPipe: Pipe?
    private var tierIsPro: Bool { UserDefaults.standard.bool(forKey: "pro") }

    // MARK: Entry
    static func main() {
        let app = NSApplication.shared
        app.setActivationPolicy(.regular)   // Dock + window; menubar extra also
        let delegate = AppDelegate()
        app.delegate = delegate
        app.run()
    }

    func applicationDidFinishLaunching(_ note: Notification) {
        buildMenuBar()
        buildUI()
        refreshPermission()
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }

    func applicationWillTerminate(_ note: Notification) {
        // Don't orphan a streaming child: SIGTERM flushes its end-of-stream
        // marker so a receiver still completes cleanly.
        if let p = streamProc, p.isRunning { p.terminate() }
    }

    // MARK: - Menu bar (background glance/control)

    private func buildMenuBar() {
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        statusItem.button?.image = NSImage(systemSymbolName: "waveform", accessibilityDescription: "Wavelink")
        let menu = NSMenu()
        let titleItem = NSMenuItem(title: "Wavelink — Idle", action: nil, keyEquivalent: "")
        titleItem.isEnabled = false
        menu.addItem(titleItem)
        menu.addItem(NSMenuItem(title: "Start Stream", action: #selector(menuToggleStream), keyEquivalent: ""))
        menu.addItem(.separator())
        menu.addItem(NSMenuItem(title: "Open Window", action: #selector(showWindow), keyEquivalent: ""))
        menu.addItem(NSMenuItem(title: "Quit", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q"))
        statusItem.menu = menu
    }

    @objc private func menuToggleStream() {
        if case .streaming = state {
            stopStream()
        } else {
            startStream()
        }
    }

    @objc private func showWindow() {
        NSApp.activate(ignoringOtherApps: true)
        window.makeKeyAndOrderFront(nil)
    }

    // MARK: - Window layout

    private func buildUI() {
        let root = NSStackView()
        root.orientation = .vertical
        root.alignment = .leading
        root.spacing = 12
        root.edgeInsets = NSEdgeInsets(top: 18, left: 18, bottom: 18, right: 18)

        // Header: title + live status pill.
        let header = NSStackView()
        header.orientation = .horizontal
        header.spacing = 8
        let title = NSTextField(labelWithString: "Wavelink")
        title.font = .boldSystemFont(ofSize: 18)
        title.setAccessibilityLabel("Wavelink, macOS emitter")
        pillDot = NSView(frame: NSRect(x: 0, y: 0, width: 10, height: 10))
        pillDot.wantsLayer = true
        pillDot.layer?.cornerRadius = 5
        pillDot.setAccessibilityLabel("Streaming state indicator")
        pillLabel = NSTextField(labelWithString: "Idle")
        pillLabel.textColor = Wd.textSecondary
        header.addArrangedSubview(title)
        header.addArrangedSubview(pillDot)
        header.addArrangedSubview(pillLabel)
        // Wavelink roles (FR-001): Emitter active; desktop Receiver is staged.
        let roleControl = NSSegmentedControl(labels: ["Emitter", "Receiver"], trackingMode: .selectOne,
                                             target: self, action: #selector(roleChanged))
        roleControl.selectedSegment = 0
        roleControl.setAccessibilityLabel("Wavelink role")
        header.addArrangedSubview(roleControl)

        // "Streaming to" receiver section.
        let addrLabel = NSTextField(labelWithString: "Receiver address:")
        addrField = NSTextField(string: "127.0.0.1:9100")
        addrField.setAccessibilityLabel("Receiver address")
        addrField.placeholderString = "127.0.0.1:9100"
        let addrRow = NSStackView(views: [addrLabel, addrField])
        addrRow.orientation = .horizontal
        addrRow.spacing = 8

        // Tier + single contextual Stop/Start action.
        tierControl = NSSegmentedControl(labels: ["Free", "Pro"], trackingMode: .selectOne, target: self, action: #selector(tierChanged))
        tierControl.selectedSegment = tierIsPro ? 1 : 0
        tierControl.setAccessibilityLabel("Free or Pro entitlement")
        actionButton = NSButton(title: "Start Stream", target: self, action: #selector(actionPressed))
        actionButton.bezelStyle = .rounded
        actionButton.keyEquivalent = "\r"
        actionButton.setAccessibilityLabel("Start or stop streaming")
        let controlRow = NSStackView(views: [tierControl, actionButton])
        controlRow.orientation = .horizontal
        controlRow.spacing = 12

        // Session metrics card.
        let card = NSBox()
        card.titlePosition = .noTitle
        card.fillColor = Wd.card
        card.cornerRadius = 10
        card.contentViewMargins = NSSize(width: 12, height: 12)
        let metricGrid = NSGridView(views: [
            [metricRow("Codec", "—"), metricRow("Capture rate", "—")],
            [metricRow("Wire rate", "—"), metricRow("Resampled", "no")],
            [metricRow("Frames sent", "0"), metricRow("Bytes sent", "0")],
            [metricRow("Est. send latency", "—"), metricRow("Last error", "—")],
        ])
        metricGrid.rowSpacing = 6
        metricGrid.columnSpacing = 20
        card.contentView = metricGrid

        // Permission row (hidden once Authorized).
        permissionLabel = NSTextField(labelWithString: "Screen Recording not granted.")
        permissionLabel.textColor = Wd.error
        let openPrefs = NSButton(title: "Open System Settings", target: self, action: #selector(openScreenRecordingPrefs))
        openPrefs.bezelStyle = .inline
        permissionRow = NSStackView(views: [permissionLabel, openPrefs])
        permissionRow.orientation = .horizontal
        permissionRow.spacing = 8

        // Activity log (collapsible).
        logToggle = NSButton(title: "Activity", target: self, action: #selector(toggleLog))
        logToggle.setButtonType(.toggle)
        logToggle.state = .on
        let scroll = NSScrollView()
        scroll.hasVerticalScroller = true
        scroll.borderType = .bezelBorder
        logView = NSTextView(frame: NSRect(x: 0, y: 0, width: 420, height: 160))
        logView.isEditable = false
        logView.font = .monospacedSystemFont(ofSize: 11, weight: .regular)
        logView.setAccessibilityLabel("Activity log")
        scroll.documentView = logView
        scroll.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            scroll.widthAnchor.constraint(equalToConstant: 420),
            scroll.heightAnchor.constraint(equalToConstant: 160),
        ])
        let logButtons = NSStackView(views: [
            logToggle,
            NSButton(title: "Clear", target: self, action: #selector(clearLog)),
            NSButton(title: "Copy Diagnostics", target: self, action: #selector(copyDiagnostics)),
        ])
        logButtons.orientation = .horizontal
        logButtons.spacing = 12

        let views: [NSView] = [
            header, addrRow, controlRow, card, permissionRow, logButtons, scroll,
        ]
        for v in views {
            root.addArrangedSubview(v)
        }

        window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 460, height: 560),
            styleMask: [.titled, .closable, .miniaturizable],
            backing: .buffered, defer: false)
        window.title = "Wavelink"
        window.contentView = root
        window.center()
        window.isReleasedWhenClosed = false
        window.backgroundColor = Wd.background
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    private func metricRow(_ title: String, _ value: String) -> NSView {
        let t = NSTextField(labelWithString: title)
        t.textColor = Wd.textSecondary
        t.font = .systemFont(ofSize: 11)
        let v = NSTextField(labelWithString: value)
        v.font = .systemFont(ofSize: 12, weight: .medium)
        metricsLabels[title] = v
        let row = NSStackView(views: [t, v])
        row.orientation = .horizontal
        row.spacing = 6
        return row
    }

    // MARK: - Rendering state

    private func renderState() {
        let (label, color): (String, NSColor)
        switch state {
        case .idle: (label, color) = ("Idle", Wd.textSecondary)
        case .connecting: (label, color) = ("Connecting…", Wd.accent)
        case .streaming: (label, color) = ("Streaming", Wd.good)
        case .stopping: (label, color) = ("Stopping…", Wd.warn)
        case .error: (label, color) = ("Error", Wd.error)
        }
        pillLabel.stringValue = label
        pillLabel.textColor = color
        pillDot.layer?.backgroundColor = color.cgColor

        switch state {
        case .idle, .error:
            actionButton.title = "Start Stream"
            actionButton.isEnabled = true
            addrField.isEnabled = true
        case .connecting, .stopping:
            actionButton.title = "…"
            actionButton.isEnabled = false
            addrField.isEnabled = false
        case .streaming:
            actionButton.title = "Stop Stream"
            actionButton.isEnabled = true
            addrField.isEnabled = false
        }
        if let item = statusItem.menu?.item(at: 0) {
            item.title = "Wavelink — \(label)"
        }
        statusItem.button?.contentTintColor = color
        if let s = lastFatal, stateIsError {
            setMetric("Last error", s)
        }
    }

    private var stateIsError: Bool {
        if case .error = state { return true }
        return false
    }

    private func setMetric(_ title: String, _ value: String) {
        metrics[title] = value
        metricsLabels[title]?.stringValue = value
    }

    private func log(_ text: String, color: NSColor = Wd.textSecondary) {
        let formatter = DateFormatter()
        formatter.dateFormat = "HH:mm:ss"
        let stamp = formatter.string(from: Date())
        activityLines.append((stamp, text, color))
        if activityLines.count > 500 { activityLines.removeFirst(activityLines.count - 500) }
        renderLog()
    }

    private func renderLog() {
        let out = NSMutableAttributedString()
        for (stamp, text, color) in activityLines {
            let line = NSAttributedString(
                string: "\(stamp)  \(text)\n",
                attributes: [.foregroundColor: color, .font: logView.font ?? .monospacedSystemFont(ofSize: 11, weight: .regular)]
            )
            out.append(line)
        }
        logView.textStorage?.setAttributedString(out)
        logView.scrollToEndOfDocument(nil)
    }

    @objc private func toggleLog() { logView.enclosingScrollView?.isHidden = logToggle.state != .on }
    @objc private func clearLog() { activityLines.removeAll(); renderLog() }
    @objc private func copyDiagnostics() {
        // FR-055 redacted: no peer identity or addresses beyond the summary both
        // the user typed and the app already shows in-window.
        let body = activityLines.map { "\($0.0)  \($0.1)" }.joined(separator: "\n")
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        pasteboard.setString(body, forType: .string)
    }

    // MARK: - Actions

    @objc private func roleChanged(_ sender: NSSegmentedControl) {
        if sender.selectedSegment == 1 {
            sender.selectedSegment = 0
            let alert = NSAlert()
            alert.messageText = "Desktop receiver render path is wired at the seam"
            alert.informativeText = "The macOS Receiver role now runs the QUIC → decode → RenderSink pipeline (`macos-emitter --receive`): with `--sink null` it is verified hash-perfect on this host. Playing through an actual output device / USB DAC (`--sink audio`) is the documented device gate (`usb-dac-device`) yet to be validated on real hardware."
            alert.addButton(withTitle: "Got it")
            alert.runModal()
        }
    }

    @objc private func actionPressed() {
        if case .streaming = state { stopStream() } else { startStream() }
    }

    @objc private func tierChanged(_ sender: NSSegmentedControl) {
        let wantPro = sender.selectedSegment == 1
        if !wantPro && isStreamingOrConnecting {
            // FR-047: Pro→Free while a lossless (Pro) stream is active asks first.
            let alert = NSAlert()
            alert.messageText = "Confirm lossless → lossy downgrade?"
            alert.informativeText = "You are streaming lossless as Pro. Switching to Free will only send lossy (Opus) audio from now on."
            alert.addButton(withTitle: "Keep Pro")
            alert.addButton(withTitle: "Downgrade to Free")
            if alert.runModal() == .alertFirstButtonReturn {
                sender.selectedSegment = 1
                return
            }
        }
        UserDefaults.standard.set(wantPro, forKey: "pro")
        log(wantPro ? "Tier set to Pro (lossless enabled)" : "Tier set to Free (lossy only)", color: Wd.pro)
    }

    private var isStreamingOrConnecting: Bool {
        if case .streaming = state { return true }
        if case .connecting = state { return true }
        return false
    }

    @objc private func openScreenRecordingPrefs() {
        let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")!
        NSWorkspace.shared.open(url)
    }

    // MARK: - Permission

    private func refreshPermission() {
        permissionRow.isHidden = true
        runCli(args: ["--permission-state"], prefix: "PERMISSION", parsePermission: true)
    }

    private func handlePermission(_ line: String) {
        // Line: "screen-recording-tcc=Authorized capture_allowed=true"
        let allowed = line.contains("capture_allowed=true")
        permissionRow.isHidden = allowed
        if !allowed {
            permissionLabel.stringValue = "Screen Recording not granted — streaming will not capture audio."
        }
    }

    // MARK: - Streaming (macos-emitter --stream)

    /// Toolchain Swift runtime dir, so the child can load the SCK shim's
    /// libswift_Concurrency (not in the OS dyld cache — build-check.md).
    private func swiftLibRpath() -> String? {
        let candidates = [
            "/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift-5.5/macosx",
            "/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift/macosx",
            "/usr/lib/swift",
        ]
        return candidates.first {
            FileManager.default.fileExists(atPath: $0 + "/libswift_Concurrency.dylib")
        }
    }

    @objc private func startStream() {
        if let p = streamProc, p.isRunning {
            log("Already streaming — press Stop first.", color: Wd.warn)
            return
        }
        let p = Process()
        p.executableURL = URL(fileURLWithPath: cliPath)
        let tierArg = tierIsPro ? "pro" : "free"
        p.arguments = ["--stream", "--addr", addrField.stringValue, "--tier", tierArg]
        var env = ProcessInfo.processInfo.environment
        if let rp = swiftLibRpath() { env["DYLD_LIBRARY_PATH"] = rp }
        if !env.isEmpty { p.environment = env }
        let pipe = Pipe()
        p.standardOutput = pipe
        p.standardError = pipe
        streamProc = p
        streamPipe = pipe
        do {
            try p.run()
        } catch {
            streamProc = nil
            streamPipe = nil
            state = .error(error.localizedDescription)
            log("Stream failed to start: \(error.localizedDescription)", color: Wd.error)
            return
        }
        state = .connecting
        log("Starting stream to \(addrField.stringValue) (\(tierArg))…", color: Wd.accent)
        pipe.fileHandleForReading.readabilityHandler = { [weak self] fh in
            guard let self = self else { return }
            let data = fh.availableData
            guard !data.isEmpty, let s = String(data: data, encoding: .utf8) else { return }
            DispatchQueue.main.async { self.consumeStreamOutput(s, proc: p) }
        }
    }

    @objc private func stopStream() {
        if let p = streamProc, p.isRunning {
            // SIGTERM → the CLI flushes the end-of-stream marker → receiver completes.
            p.terminate()
            state = .stopping
            log("Stopping stream… (flushing end-of-stream)", color: Wd.warn)
        } else {
            log("No stream running.", color: Wd.textSecondary)
        }
        streamProc = nil
        streamPipe = nil
    }

    /// Parse `--stream` status JSON lines into the state machine + logs.
    private func consumeStreamOutput(_ s: String, proc: Process) {
        for line in s.split(separator: "\n") {
            guard let j = try? JSONSerialization.jsonObject(with: Data(line.utf8)),
                let d = j as? [String: Any]
            else { continue }
            switch d["ev"] as? String {
            case "start":
                state = .connecting
                let c = d["codec"] ?? "?", l = d["lane"] ?? "?"
                log("Streaming \(c) over \(l)", color: Wd.accent)
            case "format":
                let r = d["rate"] ?? "?", wire = d["wire_rate"] ?? "?"
                let resampled = (d["resampled"] as? Bool) ?? false
                setMetric("Capture rate", "\(r) Hz")
                setMetric("Wire rate", "\(wire) Hz")
                setMetric("Resampled", resampled ? "yes (worker)" : "no")
            case "stats":
                if !sinkIsStreaming(proc) { break }
                state = .streaming
                let pk = d["packets_sent"] ?? "0", by = d["bytes_sent"] ?? "0"
                setMetric("Frames sent", "\(pk)")
                setMetric("Bytes sent", "\(by)")
                if let ms = d["send_ms"] as? Int { setMetric("Est. send latency", "\(ms) ms") }
                if let ov = d["overflowed"] as? Bool, ov {
                    state = .error("Capture fell behind — dropped audio; reduce load.")
                }
            case "warning":
                log("\(d["message"] ?? "warning")", color: Wd.warn)
            case "end":
                state = .idle
                let st = d["status"] ?? "?", pk = d["packets_sent"] ?? "0"
                log("Stream ended (\(st)) — \(pk) frames", color: Wd.textSecondary)
            case "fatal":
                let msg = d["message"] as? String ?? "unknown error"
                lastFatal = msg
                state = .error(msg)
                log("Fatal: \(msg)", color: Wd.error)
                if msg.contains("Screen Recording") || msg.contains("TCC") {
                    permissionRow.isHidden = false
                    permissionLabel.stringValue = msg
                }
            default:
                break
            }
        }
        if !proc.isRunning { streamProc = nil; streamPipe = nil }
    }

    private func sinkIsStreaming(_ p: Process) -> Bool { p.isRunning }

    // MARK: - Short queries

    @objc private func listInfo() {
        runCli(args: ["--list-format"], prefix: "ENDPOINTS", parsePermission: false)
        refreshPermission()
        log("Refreshed endpoints + permission.", color: Wd.textSecondary)
    }

    private func runCli(args: [String], prefix: String, parsePermission: Bool) {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: cliPath)
        p.arguments = args
        var env = ProcessInfo.processInfo.environment
        if let rp = swiftLibRpath() { env["DYLD_LIBRARY_PATH"] = rp }
        if !env.isEmpty { p.environment = env }
        let pipe = Pipe()
        p.standardOutput = pipe
        p.standardError = pipe
        do {
            try p.run()
            p.waitUntilExit()
            let data = pipe.fileHandleForReading.readDataToEndOfFile()
            let text = String(data: data, encoding: .utf8) ?? "(no output)"
            for line in text.split(separator: "\n") {
                if parsePermission && line.contains("screen-recording-tcc") {
                    handlePermission(String(line))
                } else {
                    log("\(prefix): \(line)", color: Wd.textSecondary)
                }
            }
        } catch {
            log("\(prefix): could not run CLI: \(error)", color: Wd.error)
        }
    }
}
