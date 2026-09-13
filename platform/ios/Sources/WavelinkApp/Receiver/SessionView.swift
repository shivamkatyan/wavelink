//
//  SessionView.swift
//  WDRReceiverApp
//
//  Live stream-health panel (FR-053): state, peer (verbose only), transport,
//  codec, sample rate, bit depth, channels, frame rate, estimated latency,
//  buffer fill, packet loss, underruns, output route, fidelity. Values use
//  labels + numbers (never colour alone) for a11y (FR-056); the whole panel
//  is VoiceOver-readable and Dynamic-Type scaled.
//

import SwiftUI

struct SessionView: View {
    let model: ReceiverModel
    @Environment(\.colorScheme) private var colorScheme

    var body: some View {
        List {
            healthRow(label: "Session state", value: model.status.state.rawValue,
                      indicator: indicator(for: model.status.state))
            healthRow(label: "Transport", value: model.status.transport.rawValue)
            healthRow(label: "Codec", value: codecLabel)
            healthRow(label: "Sample rate", value: "\(model.status.sampleRateHz) Hz")
            healthRow(label: "Bit depth", value: "\(model.status.bitDepth)-bit")
            healthRow(label: "Channels", value: "\(model.status.channels)")
            healthRow(label: "Frame rate", value: String(format: "%.1f Hz", model.status.fidelity.frameRate))
            healthRow(label: "Est. latency", value: "\(model.status.latencyMs) ms",
                      secondary: "end-to-end capture→render (device-gated measurement)")
            gaugeRow(label: "Buffer fill", fraction: model.status.bufferFill,
                     voiceOver: "\(Int(model.status.bufferFill * 100)) percent of the jitter buffer is filled")
            healthRow(label: "Packet loss", value: String(format: "%.2f %%", model.status.packetLossPct),
                      secondary: "receiver-observed frames")
            healthRow(label: "Underruns", value: "\(model.status.underruns)",
                      secondary: "since session start")
            healthRow(label: "Output route", value: model.status.route)
            healthRow(label: "Fidelity", value: model.status.fidelity.step.title,
                      indicator: fidelityIndicator)

            Section {
                Text(redactionFootnote)
                    .font(.footnote)
                    .foregroundColor(Palette.textSecondary(colorScheme))
                    .accessibilityLabel("Diagnostics note: exported reports redact the peer identity and device names (FR-055).")
            }

            Section(header: Text("Actions").accessibilityAddTraits(.isHeader)) {
                if model.sessionStarted {
                    Button("Stop and close") {
                        model.stopSession()
                    }
                    .foregroundColor(Palette.error(colorScheme))
                    .accessibilityHint("Stops demo audio and returns to the home screen.")
                }
            }
        }
        .listStyle(InsetGroupedListStyle())
        .navigationBarTitle("Stream Health", displayMode: .inline)
        .background(Palette.background(colorScheme).ignoresSafeArea())
    }

    private var codecLabel: String {
        model.status.codec.isEmpty ? "—" : model.status.codec
    }

    private var redactionFootnote: String {
        let peer = model.status.peer.isEmpty ? "not paired" : "paired"
        return "Peer: \(peer). The redacted diagnostic export (FR-055) never includes the peer identity, raw device names, or any audio payload."
    }

    // MARK: - Rows

    private func healthRow(label: String, value: String, secondary: String? = nil,
                           indicator: StatusBadge? = nil) -> some View {
        HStack {
            if let indicator { indicator }
            VStack(alignment: .leading, spacing: 2) {
                Text(label)
                    .font(.subheadline)
                    .foregroundColor(Palette.textSecondary(colorScheme))
                Text(value)
                    .font(.body.bold())
                    .foregroundColor(Palette.textPrimary(colorScheme))
                if let secondary {
                    Text(secondary)
                        .font(.caption)
                        .foregroundColor(Palette.textSecondary(colorScheme))
                }
            }
            Spacer()
        }
        .padding(.vertical, 2)
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(label): \(value)")
        .accessibilityValue([secondary].compactMap { $0 }.joined(separator: ", "))
    }

    /// Buffer fill rendered as a labelled gauge (progress bar) whose value is
    /// also read out as text — never colour-only (FR-056).
    private func gaugeRow(label: String, fraction: Double, voiceOver: String) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(label)
                .font(.subheadline)
                .foregroundColor(Palette.textSecondary(colorScheme))
            ProgressView(value: min(max(fraction, 0), 1))
                .accessibilityLabel(label)
                .accessibilityValue(voiceOver)
            Text("\(Int(min(max(fraction, 0), 1) * 100))%")
                .font(.caption.monospacedDigit())
                .foregroundColor(Palette.textPrimary(colorScheme))
        }
        .padding(.vertical, 2)
    }

    private func indicator(for state: SessionState) -> StatusBadge {
        switch state {
        case .streaming:
            return StatusBadge(shape: .filled, label: "streaming", color: Palette.good(colorScheme))
        case .paused, .connecting, .pairing:
            return StatusBadge(shape: .hollow, label: state.rawValue, color: Palette.warn(colorScheme))
        case .error:
            return StatusBadge(shape: .warning, label: "error", color: Palette.error(colorScheme))
        case .idle, .terminated:
            return StatusBadge(shape: .hollow, label: state.rawValue, color: Palette.textSecondary(colorScheme))
        }
    }

    private var fidelityIndicator: StatusBadge {
        switch model.status.fidelity.step {
        case .lossyTransport:
            return StatusBadge(shape: .filled, label: "lossy", color: Palette.warn(colorScheme))
        case .losslessTransportOutputPathConverted:
            return StatusBadge(shape: .warning, label: "converted", color: Palette.warn(colorScheme))
        case .losslessTransportOutputPathUnverified:
            return StatusBadge(shape: .hollow, label: "unverified", color: Palette.accent(colorScheme))
        case .losslessTransportBitPerfectVerified:
            return StatusBadge(shape: .filled, label: "bit-perfect", color: Palette.good(colorScheme))
        }
    }
}

#if DEBUG
struct SessionView_Previews: PreviewProvider {
    static var previews: some View {
        NavigationView {
            SessionView(model: ReceiverModel())
        }
    }
}
#endif
