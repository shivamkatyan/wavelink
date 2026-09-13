//
//  PermissionExplainerView.swift
//  WDRReceiverApp
//
//  FR-052: every OS permission is explained IN-APP immediately BEFORE the OS
//  prompt. For the iOS receiver the relevant permission is the local-network
//  TCC prompt (iOS 14+, ADR-008) that the transport will hit when it starts
//  browsing `_wdr._tcp` on the LAN. This view is shown first (ContentView gates
//  the actual session start behind it); the bundle also declares
//  NSLocalNetworkUsageDescription + NSBonjourServices(_wdr._tcp) in Info.plist.
//
//  Accessibility: all content is VoiceOver-readable, Dynamic-Type scaled
//  (system text styles), and the Continue button has a full label/hint.
//

import SwiftUI

struct PermissionExplainerView: View {
    @EnvironmentObject var model: ReceiverModel
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.presentationMode) private var presentationMode

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Label("Why does Wavelink need Local Network?", systemImage: "network")
                    .font(.title3.bold())
                    .foregroundColor(Palette.textPrimary(colorScheme))
                    .accessibilityAddTraits(.isHeader)

                explainerRow(
                    systemImage: "wifi",
                    title: "Audio arrives over your Wi-Fi network",
                    body: "Your computer relays audio to this iPhone directly, over your home Wi-Fi — no cloud server is involved."
                )
                explainerRow(
                    systemImage: "point.3.connected.trianglepath.dotted",
                    title: "We find your relaying computer by browsing `_wdr._tcp`",
                    body: "That is a local service advertisement. It only ever reaches devices on the same Wi-Fi network."
                )
                explainerRow(
                    systemImage: "hand.raised.fill",
                    title: "Nothing leaves your network",
                    body: "No audio is recorded, streamed to the Internet, or collected. If you deny Local Network access, we simply cannot find your computer — you can still use the app; streaming just will not start."
                )

                Divider()

                // The one-action path: continue -> the OS TCC local-network
                // prompt appears (FR-052 last step, FR-054 guidance on deny).
                Button {
                    model.permissionExplained = true
                    model.startSession()
                    presentationMode.wrappedValue.dismiss()
                } label: {
                    Label("Continue — ask iOS for Local Network access", systemImage: "arrow.right.circle.fill")
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 8)
                }
                .foregroundColor(Palette.accent(colorScheme))
                .overlay(RoundedRectangle(cornerRadius: 10).stroke(Palette.accent(colorScheme), lineWidth: 2))
                .accessibilityHint("Proceeds to the iOS Local Network permission prompt, then starts demo audio.")

                Button("Not now") {
                    model.permissionExplained = true
                    model.sessionStarted = false
                    presentationMode.wrappedValue.dismiss()
                }
                .frame(maxWidth: .infinity)
                .foregroundColor(Palette.textSecondary(colorScheme))
                .accessibilityHint("Closes this explanation without starting audio or asking for permission.")

                Text("Later, change your choice in Settings > Privacy > Local Network. Denied permission shows a clear in-app message with a Retry action (FR-054).")
                    .font(.footnote)
                    .foregroundColor(Palette.textSecondary(colorScheme))
            }
            .padding()
        }
        .background(Palette.background(colorScheme).ignoresSafeArea())
        .navigationBarTitle("Local Network", displayMode: .inline)
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
struct PermissionExplainerView_Previews: PreviewProvider {
    static var previews: some View {
        NavigationView {
            PermissionExplainerView().environmentObject(ReceiverModel())
        }
    }
}
#endif
