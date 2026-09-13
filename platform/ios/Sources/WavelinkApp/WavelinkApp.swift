import SwiftUI

/// Wavelink — one app, two roles (FR-001). Launches to a role picker:
/// Emitter (capture & stream) or Receiver (render to output / USB DAC).
@main
struct WavelinkApp: App {
    var body: some Scene {
        WindowGroup {
            RolePickerView()
        }
    }
}

enum WDRole: String, CaseIterable, Identifiable {
    case emitter = "Emitter"
    case receiver = "Receiver"
    var id: String { rawValue }
    var blurb: String {
        switch self {
        case .emitter: return "Capture & stream this device's audio over Wi-Fi."
        case .receiver: return "Play Wavelink streams to your output or USB DAC."
        }
    }
    var icon: String {
        switch self {
        case .emitter: return "record.circle"
        case .receiver: return "speaker.wave.2.fill"
        }
    }
}

/// Role picker + the chosen role's home.
struct RolePickerView: View {
    @State private var role: WDRole?

    var body: some View {
        Group {
            if let role {
                roleHome(role)
            } else {
                picker
            }
        }
        .environment(\.colorScheme, .dark)
    }

    /// The shared home wrapper lets the user switch roles back from a toolbar.
    private func roleHome(_ r: WDRole) -> some View {
        NavigationView {
            Group {
                switch r {
                case .emitter: EmitterContentView()
                case .receiver: ReceiverContentView()
                }
            }
            .toolbar {
                ToolbarItem(placement: .navigationBarLeading) {
                    Button {
                        withAnimation { role = nil }
                    } label: {
                        Image(systemName: "arrowshape.turn.up.left.circle")
                    }
                    .accessibilityLabel("Choose role")
                }
            }
        }
    }

    private var picker: some View {
        NavigationView {
            List(WDRole.allCases) { r in
                Button {
                    withAnimation { role = r }
                } label: {
                    HStack(spacing: 14) {
                        Image(systemName: r.icon)
                            .font(.title2)
                            .foregroundColor(Palette.accent(scheme))
                        VStack(alignment: .leading, spacing: 4) {
                            Text(r.rawValue).font(.headline)
                            Text(r.blurb).font(.caption).foregroundColor(Palette.textSecondary(scheme))
                        }
                    }
                    .padding(.vertical, 6)
                }
                .accessibilityElement(children: .combine)
            }
            .navigationTitle("Wavelink")
            .navigationBarTitleDisplayMode(.inline)
        }
        .navigationViewStyle(StackNavigationViewStyle())
    }
    @Environment(\.colorScheme) private var scheme
}
