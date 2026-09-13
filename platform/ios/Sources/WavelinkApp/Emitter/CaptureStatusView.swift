//
//  CaptureStatusView.swift
//  WDRiOSEmitterApp
//
//  FR-053 live capture-status panel for the emitter shell. Values come from
//  EmissionModel.status (demo telemetry until the network transport task wires
//  real capture telemetry — recorded honestly in README). FR-055 redaction: the
//  model's `redactedExport()` is what an exported/logged diagnostic carries;
//  this view renders the local summary without raw source identity.
//
//  Accessibility (FR-056): every row is a combined VoiceOver element with a
//  label/value; sections are headers; status badges use shape+label (never
//  colour alone).
//

import SwiftUI

struct CaptureStatusView: View {
    @ObservedObject var model: EmissionModel
    @Environment(\.colorScheme) private var colorScheme

    var body: some View {
        List {
            stateSection
            formatSection
            healthSection
            complianceSection
        }
        .listStyle(InsetGroupedListStyle())
        .navigationBarTitle("Capture status", displayMode: .inline)
        .background(Palette.background(colorScheme).ignoresSafeArea())
    }

    /// FR-053 `state` + consent phase + App-Review indicator honesty.
    private var stateSection: some View {
        Section(header: Text("State").accessibilityAddTraits(.isHeader)) {
            row(icon: stateIcon, tint: stateColor,
                title: "State",
                value: stateLabel,
                accessibilityValue: stateLabel)

            row(icon: "hand.raised.fill", tint: Palette.textSecondary(colorScheme),
                title: "Consent",
                value: model.consentPhase.rawValue,
                accessibilityValue: model.consentPhase.rawValue)

            row(icon: model.status.systemIndicatorVisible ? "circle.fill" : "circle",
                tint: model.status.systemIndicatorVisible ? Palette.good(colorScheme) : Palette.textSecondary(colorScheme),
                title: "System recording indicator",
                value: model.status.systemIndicatorVisible ? "Visible (system)" : "Not active",
                accessibilityValue: model.status.systemIndicatorVisible
                    ? "The system red recording indicator is visible"
                    : "No capture is active; no system indicator")

            if let denial = model.consentDenial {
                row(icon: "exclamationmark.triangle.fill", tint: Palette.warn(colorScheme),
                    title: "Last denial",
                    value: denial.guidance,
                    accessibilityValue: denial.guidance)
            }
        }
    }

    /// FR-053 format + scope (redacted class, never a raw source label).
    private var formatSection: some View {
        Section(header: Text("Format").accessibilityAddTraits(.isHeader)) {
            row(icon: "waveform", tint: Palette.accent(colorScheme),
                title: "Capture mode",
                value: model.status.mode.title,
                accessibilityValue: model.status.mode.explainer)
            row(icon: "scope", tint: Palette.accent(colorScheme),
                title: "Scope",
                value: model.status.scope == .selfOnly ? "This app" : "Other apps (system picker)",
                accessibilityValue: model.status.scope == .selfOnly
                    ? "Capturing this app's own audio"
                    : "Capturing other apps' permitted audio through Apple's system picker")
            row(icon: "slider.horizontal.3", tint: Palette.textSecondary(colorScheme),
                title: "PCM",
                value: "\(model.status.sampleRateHz) Hz · \(model.status.bitDepth)-bit · \(model.status.channels) ch",
                accessibilityValue: "\(model.status.sampleRateHz) hertz, \(model.status.bitDepth) bit, \(model.status.channels) channels")
        }
    }

    /// FR-053 health fields (demo-simulated today).
    private var healthSection: some View {
        Section(header: Text("Health").accessibilityAddTraits(.isHeader)) {
            row(icon: "timer", tint: Palette.textSecondary(colorScheme),
                title: "Latency (demo)",
                value: "\(model.status.latencyMs) ms",
                accessibilityValue: "\(model.status.latencyMs) milliseconds")
            row(icon: "cylinder.split.1x2", tint: Palette.textSecondary(colorScheme),
                title: "Buffer fill (demo)",
                value: "\(Int(model.status.bufferFill * 100))%",
                accessibilityValue: "\(Int(model.status.bufferFill * 100)) percent")
            row(icon: "bolt.horizontal", tint: Palette.textSecondary(colorScheme),
                title: "Loss (demo)",
                value: String(format: "%.1f%%", model.status.packetLossPct),
                accessibilityValue: String(format: "%.1f percent", model.status.packetLossPct))
            row(icon: "arrow.down.circle", tint: Palette.textSecondary(colorScheme),
                title: "Dropped buffers",
                value: "\(model.status.droppedBuffers)",
                accessibilityValue: "\(model.status.droppedBuffers) dropped buffers")
        }
    }

    /// App Review 2.5.14 honesty, always visible while the panel is open.
    private var complianceSection: some View {
        Section(header: Text("Compliance").accessibilityAddTraits(.isHeader)) {
            Text("""
            Consent: Apple's system picker only.
            Indicator: the SYSTEM red recording indicator, shown continuously while capturing.
            Background: capture never starts silently in the background; protected content is excluded by iOS.
            """)
                .font(.caption)
                .foregroundColor(Palette.textSecondary(colorScheme))
                .accessibilityLabel("Compliance summary. \(iOSCapabilityMatrix.systemPickerConsentNote) \(iOSCapabilityMatrix.backgroundHonesty)")
        }
    }

    // MARK: - Presentation helpers

    private var stateLabel: String {
        switch model.status.state {
        case .broadcasting: return "Broadcasting"
        case .paused: return "Paused"
        case .error: return "Error"
        case .terminated: return "Terminated"
        case .awaitingSystemPicker: return "Awaiting picker"
        case .configuringSession: return "Configuring"
        case .idle: return "Idle"
        }
    }

    private var stateIcon: String {
        switch model.status.state {
        case .broadcasting: return "record.circle.fill"
        case .error: return "exclamationmark.triangle.fill"
        case .idle, .terminated: return "circle"
        default: return "circle.dotted"
        }
    }

    private var stateColor: Color {
        switch model.status.state {
        case .broadcasting: return Palette.good(colorScheme)
        case .paused: return Palette.warn(colorScheme)
        case .error: return Palette.error(colorScheme)
        default: return Palette.textSecondary(colorScheme)
        }
    }

    private func row(icon: String, tint: Color, title: String, value: String,
                     accessibilityValue: String) -> some View {
        HStack(spacing: 12) {
            Image(systemName: icon)
                .foregroundColor(tint)
                .frame(width: 24)
                .accessibilityHidden(true)
            Text(title)
                .font(.body)
                .foregroundColor(Palette.textPrimary(colorScheme))
            Spacer()
            Text(value)
                .font(.caption)
                .multilineTextAlignment(.trailing)
                .foregroundColor(Palette.textSecondary(colorScheme))
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(title)
        .accessibilityValue(accessibilityValue)
    }
}
