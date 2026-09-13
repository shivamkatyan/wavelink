//
//  ContentView.swift
//  WDRReceiverApp
//
//  Receiver home: role, output route (FR-013/FR-015), buffer profile (FR-023),
//  fidelity label (FR-014/FR-022) and the persistent top-level Free/Pro toggle
//  (FR-040/FR-048). Accessibility (FR-056): labels/values/hints on every
//  control, Dynamic Type via system text styles, Reduce Motion honoured, and a
//  contrast-safe palette (>=4.5:1 — recorded in Palette.swift/README).
//  Status is never communicated by colour alone (non-color indicators).
//

import SwiftUI

struct ReceiverContentView: View {
    @EnvironmentObject var model: ReceiverModel
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        NavigationView {
            List {
                tierSection
                roleSection
                outputSection
                bufferSection
                fidelitySection
                sessionSection
                if model.requiresDowngradeConfirmation {
                    downgradeConfirmationSection
                }
            }
            .listStyle(InsetGroupedListStyle())
            .navigationBarTitle("Wavelink", displayMode: .inline)
            .background(Palette.background(colorScheme).ignoresSafeArea())
            // Reduce Motion (FR-056): no crossfade/pulse animations when the
            // user opts out; content still updates instantly.
            .animation(reduceMotion ? nil : .easeInOut(duration: 0.25), value: model.sessionStarted)
            .animation(reduceMotion ? nil : .easeInOut(duration: 0.25), value: model.currentTier)
            .sheet(isPresented: Binding(
                get: { model.sessionStarted && !model.permissionExplained },
                set: { _ in }
            )) {
                PermissionExplainerView()
                    .environmentObject(model)
            }
            .onAppear { model.refreshRouteLabel() }
        }
        .navigationViewStyle(StackNavigationViewStyle())
        .preferredColorScheme(nil)
    }

    // MARK: - Sections

    /// FR-040: Free/Pro toggle stays at the top of the shell UI, on every screen.
    private var tierSection: some View {
        Section(header: Text("Tier").accessibilityAddTraits(.isHeader)) {
            HStack(spacing: 12) {
                Image(systemName: model.currentTier == .pro ? "checkmark.seal.fill" : "seal")
                    .foregroundColor(tierAccent)
                    .accessibilityHidden(true)

                VStack(alignment: .leading, spacing: 2) {
                    Text(model.currentTier == .pro ? "Pro" : "Free")
                        .font(.headline)
                        .foregroundColor(Palette.textPrimary(colorScheme))
                    Text(model.currentTier == .pro
                         ? "Lossless Wi-Fi enabled (FLAC/PCM)"
                         : "Lossy Wi-Fi only (Opus). Pro adds lossless.")
                        .font(.caption)
                        .foregroundColor(Palette.textSecondary(colorScheme))
                }
                .accessibilityElement(children: .combine)
                .accessibilityLabel("Current tier: \(model.currentTier == .pro ? "Pro" : "Free")")

                Spacer()

                Picker("Tier", selection: Binding(
                    get: { model.currentTier },
                    set: { model.toggleTier($0) }
                )) {
                    Text("Free").tag(EntitlementTier.free)
                    Text("Pro").tag(EntitlementTier.pro)
                }
                .pickerStyle(SegmentedPickerStyle())
                .frame(width: 150)
                .accessibilityLabel("Free or Pro tier")
                .accessibilityValue(model.currentTier == .pro ? "Pro selected" : "Free selected")
                .accessibilityHint("Pro enables lossless Wi-Fi audio. Switching from Pro to Free while streaming lossless will ask you to confirm before downgrading.")
            }
            .padding(.vertical, 4)
        }
        .accessibilityElement(children: .contain)
    }

    /// FR-001 / shell role: this build is the Receiver.
    private var roleSection: some View {
        Section(header: Text("Role").accessibilityAddTraits(.isHeader)) {
            HStack {
                Label("Receiver", systemImage: "wave.3.right.circle.fill")
                    .accessibilityLabel("Role: Receiver. This device renders the relayed audio stream to the selected output.")
                Spacer()
                StatusBadge(shape: .filled,
                            label: "Render",
                            color: Palette.good(colorScheme))
            }
        }
    }

    /// FR-013/FR-015: output route report. Honest: we report, never force.
    private var outputSection: some View {
        Section(header: Text("Output").accessibilityAddTraits(.isHeader)) {
            HStack {
                statusIcon(forRoute: model.routeSnapshot.isUsbAudioRouted)
                    .foregroundColor(routeColor)
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 2) {
                    Text(routeTitle)
                        .font(.body)
                        .foregroundColor(Palette.textPrimary(colorScheme))
                    Text(routeSubtitle)
                        .font(.caption)
                        .foregroundColor(Palette.textSecondary(colorScheme))
                }
            }
            .accessibilityElement(children: .combine)
            .accessibilityLabel("Output route")
            .accessibilityValue(voiceOverRoute)

            if model.routeSnapshot.isUsbAudioRouted,
               let dac = model.routeSnapshot.outputs.first(where: { $0.isUsbAudio }) {
                Label("USB DAC visible: \(dac.name)", systemImage: "cable.connector")
                    .font(.footnote)
                    .foregroundColor(Palette.textSecondary(colorScheme))
                    .accessibilityLabel("USB DAC attached. Its name is \(dac.name).")
            }

            Text("iOS routes to your USB DAC automatically when it is attached. This app reports the route; it cannot force a specific DAC.")
                .font(.footnote)
                .foregroundColor(Palette.textSecondary(colorScheme))
                .accessibilityHint("The system chooses the hardware output; the app only reports what is routed.")
        }
    }

    /// FR-023: buffer profile.
    private var bufferSection: some View {
        Section(header: Text("Buffer Profile").accessibilityAddTraits(.isHeader)) {
            Picker("Buffer Profile", selection: $model.selectedProfile) {
                ForEach(BufferProfile.allCases) { profile in
                    Text(profile.title).tag(profile)
                }
            }
            .pickerStyle(SegmentedPickerStyle())
            .accessibilityLabel("Buffer profile")
            .accessibilityValue(model.selectedProfile.title)
            .accessibilityHint("Low Latency minimizes delay. Resilient tolerates network jitter with higher latency. Balanced is in between.")

            Text(bufferProfileDescription)
                .font(.caption)
                .foregroundColor(Palette.textSecondary(colorScheme))
        }
    }

    /// FR-014/FR-022: honest fidelity label, never overclaimed.
    private var fidelitySection: some View {
        Section(header: Text("Fidelity").accessibilityAddTraits(.isHeader)) {
            HStack {
                fidelityIcon
                    .foregroundColor(fidelityColor)
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 2) {
                    Text(model.status.fidelity.step.title)
                        .font(.body)
                        .foregroundColor(Palette.textPrimary(colorScheme))
                    Text(fidelitySubtitle)
                        .font(.caption)
                        .foregroundColor(Palette.textSecondary(colorScheme))
                }
            }
            .accessibilityElement(children: .combine)
            .accessibilityLabel("Fidelity")
            .accessibilityValue(voiceOverFidelity)

            if model.status.fidelity.isBitPerfectVerified {
                Label("Measured by hardware loopback", systemImage: "checkmark.shield.fill")
                    .font(.footnote)
                    .foregroundColor(Palette.good(colorScheme))
                    .accessibilityLabel("Bit perfect verified by hardware loopback measurement.")
            }
        }
    }

    /// FR-053 live panel entry point + demo session controls.
    private var sessionSection: some View {
        Section(header: Text("Session").accessibilityAddTraits(.isHeader)) {
            NavigationLink(destination: SessionView(model: model)) {
                HStack {
                    Label("Stream health", systemImage: "waveform.path.ecg")
                    Spacer()
                    Text(model.status.state.rawValue)
                        .font(.caption)
                        .foregroundColor(Palette.textSecondary(colorScheme))
                }
                .accessibilityElement(children: .combine)
                .accessibilityLabel("Stream health")
                .accessibilityValue("Session state \(model.status.state.rawValue)")
            }

            if model.sessionStarted {
                Button(action: { model.stopSession() }) {
                    Label("Stop session", systemImage: "stop.fill")
                }
                .foregroundColor(Palette.error(colorScheme))
                .accessibilityHint("Stops demo rendering and resets the session state.")
            } else {
                Button {
                    // FR-052: show the why-this-permission sheet BEFORE any
                    // local-network access is attempted.
                    triggerPermissionFlow()
                } label: {
                    Label("Start streaming demo", systemImage: "play.fill")
                }
                .foregroundColor(Palette.accent(colorScheme))
                .accessibilityHint("Shows a short explanation, then starts a demo audio session over the current output route.")
            }
        }
    }

    /// FR-047 confirmation when dropping PRO -> FREE mid-lossless.
    private var downgradeConfirmationSection: some View {
        Section {
            VStack(alignment: .leading, spacing: 8) {
                Label("You are streaming lossless.", systemImage: "exclamationmark.triangle.fill")
                    .font(.headline)
                    .foregroundColor(Palette.warn(colorScheme))
                    .accessibilityLabel("You are streaming lossless.")

                Text("Switching to Free silently drops quality to lossy. Stop the session first, confirm the downgrade, or stay on Pro.")
                    .font(.body)
                    .foregroundColor(Palette.textPrimary(colorScheme))

                HStack {
                    Button("Stay Pro", action: { model.dismissDowngradeConfirmation() })
                        .padding(.horizontal, 14)
                        .padding(.vertical, 8)
                        .background(RoundedRectangle(cornerRadius: 8).fill(Palette.pro(colorScheme).opacity(0.22)))
                        .foregroundColor(Palette.pro(colorScheme))
                    Button("Confirm Free", action: { model.confirmDowngradeToFree() })
                        .padding(.horizontal, 14)
                        .padding(.vertical, 8)
                        .background(RoundedRectangle(cornerRadius: 8).stroke(Palette.textSecondary(colorScheme), lineWidth: 1))
                        .foregroundColor(Palette.textPrimary(colorScheme))
                }
                .font(.body)
            }
            .padding(.vertical, 4)
        }
        .accessibilityElement(children: .contain)
    }

    private func triggerPermissionFlow() {
        // FR-052: the explanatory sheet opens first; the actual local-network
        // access (and its OS prompt) only happens after the user reads and
        // continues from PermissionExplainerView.
        model.sessionStarted = true
        model.permissionExplained = false
    }

    // MARK: - Presentation helpers

    private var routeTitle: String {
        if model.routeSnapshot.isUsbAudioRouted { return "USB DAC" }
        if model.routeSnapshot.outputs.isEmpty { return "No output found" }
        return model.routeSnapshot.routeLabel
    }

    private var routeSubtitle: String {
        if model.routeSnapshot.isUsbAudioRouted {
            return "System routed to a USB DAC — audible output should come from the DAC."
        }
        if model.routeSnapshot.outputs.isEmpty {
            return "No usable output. The session will pause (FR-054)."
        }
        return "Not a USB DAC — system output falls back to \(model.routeSnapshot.routeLabel)."
    }

    private var voiceOverRoute: String {
        if model.routeSnapshot.isUsbAudioRouted {
            return "USB DAC, routed by the system"
        }
        if model.routeSnapshot.outputs.isEmpty { return "No output" }
        return model.routeSnapshot.routeLabel
    }

    private var routeColor: Color {
        if model.routeSnapshot.isUsbAudioRouted { return Palette.good(colorScheme) }
        if model.routeSnapshot.outputs.isEmpty { return Palette.error(colorScheme) }
        return Palette.warn(colorScheme)
    }

    private func statusIcon(forRoute hasUsb: Bool) -> some View {
        Group {
            if hasUsb {
                Image(systemName: "cable.connector")
            } else if model.routeSnapshot.outputs.isEmpty {
                Image(systemName: "exclamationmark.triangle.fill")
            } else {
                Image(systemName: "speaker.wave.2.fill")
            }
        }
    }

    private var fidelityIcon: some View {
        Group {
            switch model.status.fidelity.step {
            case .lossyTransport:
                Image(systemName: "waveform")
            case .losslessTransportOutputPathConverted:
                Image(systemName: "arrow.triangle.swap")
            case .losslessTransportOutputPathUnverified:
                Image(systemName: "questionmark.circle")
            case .losslessTransportBitPerfectVerified:
                Image(systemName: "checkmark.shield.fill")
            }
        }
    }

    private var fidelityColor: Color {
        switch model.status.fidelity.step {
        case .lossyTransport: return Palette.warn(colorScheme)
        case .losslessTransportOutputPathConverted: return Palette.warn(colorScheme)
        case .losslessTransportOutputPathUnverified: return Palette.accent(colorScheme)
        case .losslessTransportBitPerfectVerified: return Palette.good(colorScheme)
        }
    }

    private var voiceOverFidelity: String {
        switch model.status.fidelity.step {
        case .lossyTransport:
            return "Lossy transport, Free tier"
        case .losslessTransportOutputPathConverted:
            return "Lossless transport, but the output path converts audio. Not bit perfect."
        case .losslessTransportOutputPathUnverified:
            return "Lossless transport, output path not yet verified"
        case .losslessTransportBitPerfectVerified:
            return "Lossless transport, bit perfect verified by hardware loopback"
        }
    }

    private var fidelitySubtitle: String {
        let f = model.status.fidelity
        switch f.step {
        case .lossyTransport:
            return "Opus over Wi-Fi — Free tier (FR-042)."
        case .losslessTransportOutputPathConverted:
            return "\(f.codec) @ \(f.sampleRateHz / 1000)k/\(f.bitDepth)-bit; output path converts (never bit-perfect, FR-024)."
        case .losslessTransportOutputPathUnverified:
            return "\(f.codec) @ \(f.sampleRateHz / 1000)k/\(f.bitDepth)-bit; pass-through not yet measured."
        case .losslessTransportBitPerfectVerified:
            return "\(f.codec) @ \(f.sampleRateHz / 1000)k/\(f.bitDepth)-bit; measured bit-identical via loopback."
        }
    }

    private var bufferProfileDescription: String {
        switch model.selectedProfile {
        case .lowLatency: return "Targets lowest latency (device-gated latency probe required before enabling — FR-023)."
        case .balanced: return "Default: balanced latency and resilience."
        case .resilient: return "Larger jitter buffer tolerates network hiccups at higher latency."
        }
    }

    private var tierAccent: Color {
        model.currentTier == .pro ? Palette.pro(colorScheme) : Palette.textSecondary(colorScheme)
    }
}

/// Non-color status indicator: shape conveys state even without colour
/// (FR-056): a solid/filled form = active/healthy, hollow = not active,
/// triangle = warning/error. The text label always accompanies it.
