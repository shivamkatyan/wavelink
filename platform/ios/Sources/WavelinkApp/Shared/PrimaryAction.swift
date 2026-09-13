//  PrimaryAction.swift — WDRShared
//
//  The single source-of-truth primary action button used by every app shell:
//  ONE contextual, mutually-exclusive Start/Stop control (never two buttons
//  always visible). The caller supplies the label + tint for the CURRENT
//  state; the view renders an accessibility-labelled filled rounded button.

import SwiftUI

/// A filled, rounded primary-action button (the "single Stop/Start" control).
struct PrimaryAction: View {
    let title: String
    let systemImage: String?
    let tint: Color
    let action: () -> Void
    var disabled = false

    var body: some View {
        Button(action: action) {
            HStack(spacing: 6) {
                if let systemImage {
                    Image(systemName: systemImage)
                }
                Text(title)
            }
            .font(.headline)
            .frame(maxWidth: .infinity)
            .padding(.vertical, 10)
            .foregroundColor(.white)
            .background(tint.opacity(disabled ? 0.4 : 1))
            .cornerRadius(10)
        }
        .buttonStyle(.plain)
        .disabled(disabled)
    }
}
