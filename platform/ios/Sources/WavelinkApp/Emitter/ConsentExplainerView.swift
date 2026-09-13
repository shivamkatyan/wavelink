//
//  ConsentExplainerView.swift
//  WDRiOSEmitterApp
//
//  FR-052: every OS permission is explained IN-APP immediately BEFORE the OS
//  prompt. For the iOS emitter the OS prompt is the SYSTEM picker for capture
//  (RPSystemBroadcastPickerView on 12–26 / SCContentSharingPicker on 27+). This
//  view is shown first (ContentView gates the picker behind it); the copy is
//  driven by the core honesty strings (CapturePolicy) so it can never drift from
//  the capability matrix.
//
//  Accessibility: all content is VoiceOver-readable, Dynamic-Type scaled
//  (system text styles), and the Continue button has a full label/hint (FR-056).
//

import SwiftUI

struct ConsentExplainerView: View {
    @EnvironmentObject var model: EmissionModel
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.presentationMode) private var presentationMode

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Label("What does capturing audio involve?", systemImage: "record.circle")
                    .font(.title3.bold())
                    .foregroundColor(Palette.textPrimary(colorScheme))
                    .accessibilityAddTraits(.isHeader)

                explainerRow(
                    systemImage: "dot.radiowaves.left.and.right",
                    title: "iOS captures permitted audio only",
                    body: iOSCapabilityMatrix.consentSummary
                )
                explainerRow(
                    systemImage: "hand.raised.fill",
                    title: "Apple's system picker asks for consent",
                    body: iOSCapabilityMatrix.systemPickerConsentNote
                )
                explainerRow(
                    systemImage: "shield.lefthalf.filled",
                    title: "Protected content is excluded",
                    body: iOSCapabilityMatrix.protectedContentHonesty
                )
                explainerRow(
                    systemImage: "circle.badge.checkmark",
                    title: "A red recording indicator appears",
                    body: iOSCapabilityMatrix.recordingIndicatorHonesty
                )

                Divider()

                Button {
                    model.dismissExplanationAndPresentPicker()
                    presentationMode.wrappedValue.dismiss()
                } label: {
                    Label("Continue — show Apple's system picker", systemImage: "arrow.right.circle.fill")
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 8)
                }
                .foregroundColor(Palette.accent(colorScheme))
                .overlay(RoundedRectangle(cornerRadius: 10).stroke(Palette.accent(colorScheme), lineWidth: 2))
                .accessibilityHint("Proceeds to Apple's own picker for choosing what to capture. Capture only starts after you choose there.")

                Button("Not now") {
                    model.closeExplanationWithoutCapture()
                    presentationMode.wrappedValue.dismiss()
                }
                .frame(maxWidth: .infinity)
                .foregroundColor(Palette.textSecondary(colorScheme))
                .accessibilityHint("Closes this explanation without showing the picker or capturing anything.")

                Text("Capture always starts from a visible foreground action through the system picker — never silently, never in the background, and never for protected content.")
                    .font(.footnote)
                    .foregroundColor(Palette.textSecondary(colorScheme))
            }
            .padding()
        }
        .background(Palette.background(colorScheme).ignoresSafeArea())
        .navigationBarTitle("Capture consent", displayMode: .inline)
    }

    private func explainerRow(systemImage: String, title: String, body: String) -> some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: systemImage)
                .font(.body)
                .foregroundColor(Palette.accent(colorScheme))
                .frame(width: 28)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 4) {
                Text(title)
                    .font(.body.bold())
                    .foregroundColor(Palette.textPrimary(colorScheme))
                Text(body)
                    .font(.body)
                    .foregroundColor(Palette.textSecondary(colorScheme))
            }
        }
        .accessibilityElement(children: .combine)
        .padding(.vertical, 2)
    }
}

#if DEBUG
struct ConsentExplainerView_Previews: PreviewProvider {
    static var previews: some View {
        NavigationView {
            ConsentExplainerView().environmentObject(EmissionModel())
        }
    }
}
#endif
