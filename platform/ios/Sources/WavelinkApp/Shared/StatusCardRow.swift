//  StatusCardRow.swift — WDRShared
//
//  A reusable status "card row": icon + tinted title + value, used on every
//  platform's metrics cards (FR-053). Icon is decorative (a11y-hidden); the
//  text carries the meaning (FR-056: never colour/shape alone).

import SwiftUI

/// One row of a status/metrics card: `[icon]  title  ·····  value`.
struct StatusCardRow: View {
    let icon: String
    let tint: Color
    let title: String
    let value: String
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: icon)
                .foregroundColor(tint)
                .accessibilityHidden(true)
            Text(title)
                .font(.body)
                .foregroundColor(Palette.textSecondary(scheme))
            Spacer()
            Text(value)
                .font(.body.weight(.medium))
                .foregroundColor(Palette.textPrimary(scheme))
        }
    }
}
