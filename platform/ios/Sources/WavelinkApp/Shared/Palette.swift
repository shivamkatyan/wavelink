//
//  Palette.swift
//  WDRiOSEmitterApp
//
//  Contrast-safe palette for the emitter shell (FR-056 accessibility).
//  Every pair used in the app meets WCAG AA >= 4.5:1. Ratios below were
//  computed by the WCAG relative-luminance formula and re-verified on this
//  host; the numbers are recorded here and in README.md so the claim is
//  evidence-backed, not prose. See build-check.md for the checker command.
//
//  Design rule: status is NEVER conveyed by colour alone — every colour is a
//  companion to a shape/icon + descriptive label (non-color indicators).
//

import SwiftUI

extension Color {
    /// Create a fixed sRGB color from a 0xRRGGBB integer (no dynamic-provider
    /// ambiguity, so the documented contrast ratios stay exact).
    init(hex: UInt32) {
        self.init(
            red: Double((hex >> 16) & 0xFF) / 255.0,
            green: Double((hex >> 8) & 0xFF) / 255.0,
            blue: Double(hex & 0xFF) / 255.0
        )
    }
}

/// Fixed sRGB palette with documented AA ratios (see README.md accessibility
/// section for the full table).
enum Palette {
    // MARK: Surfaces & text
    static let backgroundDark = Color(hex: 0x1C1C1E)
    static let backgroundLight = Color(hex: 0xF5F5F7)
    static let textPrimaryDark = Color(hex: 0xF2F2F7)     // 15.25:1 on dark bg
    static let textPrimaryLight = Color(hex: 0x1C1C1E)    // 15.63:1 on light bg
    static let textSecondaryDark = Color(hex: 0xC7C7CC)   // 10.10:1 on dark bg
    static let textSecondaryLight = Color(hex: 0x545458)  // 6.92:1 on light bg

    // MARK: Accent (action buttons / link)
    static let accentDark = Color(hex: 0x0A84FF)          // 4.66:1 on dark bg
    static let accentLight = Color(hex: 0x0040DD)         // 6.94:1 on light bg

    // MARK: Status (ALWAYS paired with a shape/icon + label, never colour alone)
    static let goodDark = Color(hex: 0x32D74B)            // 8.88:1 on dark bg
    static let goodLight = Color(hex: 0x00754C)           // 5.29:1 on light bg
    static let warnDark = Color(hex: 0xFFD60A)            // 12.05:1 on dark bg
    static let warnLight = Color(hex: 0x8A5A00)           // 5.44:1 on light bg
    static let errorDark = Color(hex: 0xFF453A)           // 4.99:1 on dark bg
    static let errorLight = Color(hex: 0xC0262B)          // 5.43:1 on light bg
    static let freeDark = Color(hex: 0x8E8E93)            // 5.22:1 on dark bg
    static let freeLight = Color(hex: 0x6E6E73)           // 4.66:1 on light bg

    // MARK: Free/Pro tier colours (paired with text label, see toggle)
    static let proDark = Color(hex: 0xFF9F0A)             // 8.28:1 on dark bg
    static let proLight = Color(hex: 0x9A5B00)            // 4.98:1 on light bg

    // Convenience resolved by the ambient color scheme.
    static func background(_ scheme: ColorScheme) -> Color {
        scheme == .dark ? backgroundDark : backgroundLight
    }
    static func textPrimary(_ scheme: ColorScheme) -> Color {
        scheme == .dark ? textPrimaryDark : textPrimaryLight
    }
    static func textSecondary(_ scheme: ColorScheme) -> Color {
        scheme == .dark ? textSecondaryDark : textSecondaryLight
    }
    static func accent(_ scheme: ColorScheme) -> Color {
        scheme == .dark ? accentDark : accentLight
    }
    static func good(_ scheme: ColorScheme) -> Color {
        scheme == .dark ? goodDark : goodLight
    }
    static func warn(_ scheme: ColorScheme) -> Color {
        scheme == .dark ? warnDark : warnLight
    }
    static func error(_ scheme: ColorScheme) -> Color {
        scheme == .dark ? errorDark : errorLight
    }
    static func free(_ scheme: ColorScheme) -> Color {
        scheme == .dark ? freeDark : freeLight
    }
    static func pro(_ scheme: ColorScheme) -> Color {
        scheme == .dark ? proDark : proLight
    }
}
