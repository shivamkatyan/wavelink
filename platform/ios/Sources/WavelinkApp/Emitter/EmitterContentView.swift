//
//  ContentView.swift
//  WDRiOSEmitterApp
//
//  Emitter home: the persistent top-level Free/Pro toggle (FR-040/FR-048),
//  the FR-052 explain-before-prompt capture flow ending in the SYSTEM picker
//  (App Review 2.5.14: consent via Apple's picker; the system red indicator is
//  the recording indicator — the app never draws its own), a capture/status
//  summary, and the downgrade-confirmation sheet (FR-047).
//
//  Accessibility (FR-056): labels/values/hints on every control, Dynamic Type
//  via system text styles, Reduce Motion honoured, contrast-safe palette
//  (>=4.5:1), and status is never conveyed by colour alone (shape+label).
//

import SwiftUI

struct EmitterContentView: View {
    @EnvironmentObject var model: EmissionModel
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        NavigationView {
            List {
                tierSection
                roleSection
                captureSection
                indicatorSection
                statusSection
                if model.requiresDowngradeConfirmation {
                    downgradeConfirmationSection
                }
            }
            .listStyle(InsetGroupedListStyle())
            .navigationBarTitle("Wavelink", displayMode: .inline)
            .background(Palette.background(colorScheme).ignoresSafeArea())
            .animation(reduceMotion ? nil : .easeInOut(duration: 0.25),
                       value: model.consentPhase)
            .sheet(isPresented: $model.explanationPresented) {
                ConsentExplainerView()
                    .environmentObject(model)
            }
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
                         ? "Lossless capture enabled (FLAC/PCM)"
                         : "Lossy capture only (Opus). Pro adds lossless.")
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
                .accessibilityHint("Pro enables lossless capture. Switching from Pro to Free while capturing lossless will ask you to confirm before downgrading.")
            }
            .padding(.vertical, 4)
        }
        .accessibilityElement(children: .contain)
    }

    /// This build is the Emitter.
    private var roleSection: some View {
        Section(header: Text("Role").accessibilityAddTraits(.isHeader)) {
            HStack {
                Label("Emitter", systemImage: "record.circle")
                    .accessibilityLabel("Role: Emitter. This device captures permitted audio and would relay it to the receiver.")
                Spacer()
                StatusBadge(shape: model.captureActive ? .filled : .hollow,
                            label: model.captureActive ? "Capturing" : "Idle",
                            color: model.captureActive ? Palette.good(colorScheme) : Palette.textSecondary(colorScheme))
            }
        }
    }

    /// The FR-052 → system-picker capture flow (2.5.14: consent ONLY via the
    /// system picker; never a silent private capture).
    private var captureSection: some View {
        Section(header: Text("Capture").accessibilityAddTraits(.isHeader)) {
            switch model.consentPhase {
            case .needsExplanation:
                Button {
                    model.requestExplanation()
                } label: {
                    Label("Start capture — explain first", systemImage: "play.fill")
                }
                .foregroundColor(Palette.accent(colorScheme))
                .accessibilityHint("Shows a short explanation, then Apple's own system picker. Capture only starts after you choose there.")

            case .awaitingSystemPicker:
                systemPickerRow
                Text("Apple's system picker is shown above. Choose an app/audio onscreen — the system shows a red recording indicator while capturing.")
                    .font(.footnote)
                    .foregroundColor(Palette.textSecondary(colorScheme))

            case .authorized:
                HStack {
                    Label("Broadcasting", systemImage: "record.circle.fill")
                        .foregroundColor(Palette.good(colorScheme))
                    Spacer()
                    Button("Stop", action: { model.stopCapture() })
                        .foregroundColor(Palette.error(colorScheme))
                        .accessibilityHint("Stops capture. The system removes the recording indicator.")
                }
                .accessibilityElement(children: .combine)
                .accessibilityLabel("Broadcasting")

            case .denied:
                VStack(alignment: .leading, spacing: 8) {
                    Label("Capture not started", systemImage: "exclamationmark.triangle.fill")
                        .font(.headline)
                        .foregroundColor(Palette.warn(colorScheme))
                    Text(model.consentDenial?.guidance ?? ConsentDenial(code: .unknown).guidance)
                        .font(.body)
                        .foregroundColor(Palette.textPrimary(colorScheme))
                    HStack {
                        Button("Try again", action: { model.retrySystemPicker() })
                            .padding(.horizontal, 14)
                            .padding(.vertical, 8)
                            .background(RoundedRectangle(cornerRadius: 8).fill(Palette.accent(colorScheme).opacity(0.22)))
                            .foregroundColor(Palette.accent(colorScheme))
                        Button("Reset", action: { model.resetConsent() })
                            .padding(.horizontal, 14)
                            .padding(.vertical, 8)
                            .background(RoundedRectangle(cornerRadius: 8).stroke(Palette.textSecondary(colorScheme), lineWidth: 1))
                            .foregroundColor(Palette.textPrimary(colorScheme))
                    }
                    .font(.body)
                }
                .padding(.vertical, 4)
                .accessibilityElement(children: .contain)
            }
        }
    }

    /// The user-visible recording indicator is the SYSTEM's own (2.5.14) — shown
    /// honestly as a compliance row while capture is active.
    private var indicatorSection: some View {
        Section(header: Text("Recording indicator").accessibilityAddTraits(.isHeader)) {
            HStack {
                Image(systemName: model.status.systemIndicatorVisible ? "circle.fill" : "circle")
                    .foregroundColor(model.status.systemIndicatorVisible ? Palette.good(colorScheme) : Palette.textSecondary(colorScheme))
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 2) {
                    Text(model.status.systemIndicatorVisible ? "System red indicator is visible" : "No capture active — no system indicator")
                        .font(.body)
                        .foregroundColor(Palette.textPrimary(colorScheme))
                    Text("The indicator is Apple's own (status bar / control centre). This app never draws a fake one or hides it.")
                        .font(.caption)
                        .foregroundColor(Palette.textSecondary(colorScheme))
                }
            }
            .accessibilityElement(children: .combine)
            .accessibilityLabel("Recording indicator")
            .accessibilityValue(model.status.systemIndicatorVisible
                                ? "Visible, shown by the system"
                                : "Not visible, no capture active")
        }
    }

    private var statusSection: some View {
        Section(header: Text("Status").accessibilityAddTraits(.isHeader)) {
            if let mode = model.captureMode {
                HStack {
                    Text("Mode")
                        .font(.body)
                        .foregroundColor(Palette.textPrimary(colorScheme))
                    Spacer()
                    Text(mode.title)
                        .font(.caption)
                        .foregroundColor(Palette.textSecondary(colorScheme))
                }
                .accessibilityElement(children: .combine)
                .accessibilityLabel("Capture mode")
                .accessibilityValue(mode.explainer)
            }

            NavigationLink(destination: CaptureStatusView(model: model)) {
                HStack {
                    Label("FR-053 capture status", systemImage: "waveform.path.ecg")
                    Spacer()
                    Text(model.status.state.rawValue)
                        .font(.caption)
                        .foregroundColor(Palette.textSecondary(colorScheme))
                }
                .accessibilityElement(children: .combine)
                .accessibilityLabel("Capture status")
                .accessibilityValue("Session state \(model.status.state.rawValue)")
            }

            Text(model.status.summaryLine)
                .font(.footnote)
                .foregroundColor(Palette.textSecondary(colorScheme))
                .accessibilityLabel("Status summary. \(model.status.summaryLine)")
        }
    }

    /// FR-047 confirmation when dropping PRO -> FREE mid-lossless-capture.
    private var downgradeConfirmationSection: some View {
        Section {
            VStack(alignment: .leading, spacing: 8) {
                Label("You are capturing lossless.", systemImage: "exclamationmark.triangle.fill")
                    .font(.headline)
                    .foregroundColor(Palette.warn(colorScheme))
                    .accessibilityLabel("You are capturing lossless.")

                Text("Switching to Free silently drops capture to lossy. Stop capture first, confirm the downgrade, or stay on Pro.")
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

    // MARK: - Presentation helpers

    /// The captureStart control is chosen by the mode the capability matrix
    /// recommends for THIS device's OS. ReplayKit (12–26) renders the actual
    /// system picker view; SCK (27+) is a documented conditional (SDK-gated).
    @ViewBuilder
    private var systemPickerRow: some View {
        if model.captureMode == .screenCaptureKit {
            ContentSharingPickerButton()
        } else {
            HStack {
                AccessibleSystemPickerButton(preferredExtensionBundleID: nil)
                    .frame(width: 44, height: 44)
                Text("Start Broadcast")
                    .font(.body)
                    .foregroundColor(Palette.textPrimary(colorScheme))
                Spacer()
            }
            .accessibilityElement(children: .contain)
        }
    }

    private var tierAccent: Color {
        model.currentTier == .pro ? Palette.pro(colorScheme) : Palette.textSecondary(colorScheme)
    }
}

/// Non-color status indicator: shape conveys state even without colour
/// (FR-056): a solid/filled form = active/healthy, hollow = not active,
/// triangle = warning/error. The text label always accompanies it.
