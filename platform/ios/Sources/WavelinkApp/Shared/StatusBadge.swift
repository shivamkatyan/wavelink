import SwiftUI

/// Status pill: a shape (filled/hollow) + label + colour — the shape and text
/// carry meaning, never colour alone (FR-056).
enum BadgeShape {
    case filled
    case hollow
    case warning
}

struct StatusBadge: View {
    let shape: BadgeShape
    let label: String
    let color: Color

    var body: some View {
        HStack(spacing: 5) {
            ZStack {
                Circle()
                    .fill((shape == .hollow ? Color.clear : color))
                    .overlay(Circle().stroke(color, lineWidth: shape == .hollow ? 1.5 : 0))
                    .frame(width: 9, height: 9)
            }
            Text(label)
                .font(.caption.weight(.medium))
                .foregroundColor((shape == .hollow ? Color.secondary : color))
        }
        .accessibilityLabel(label)
    }
}
