//! Tier + feature policy and the [`EntitlementProvider`] boundary (FR-041/FR-044).
//!
//! [`EntitlementProvider`] is the centralized boundary FR-041 mandates: every
//! session/network/UI feature check goes through this trait. FR-044 reserves it
//! as the *only* seam for future commerce/billing: implementers are the single
//! thing a future billing project swaps out, so nothing above this layer
//! changes when that happens.
//!
//! The shipped implementation [`DevToggleEntitlementProvider`] is a
//! development/demonstration toggle (FR-045) — see the crate root for the
//! honesty caveat: not tamper-resistant, not an enforcement mechanism.

use crate::policy::EffectivePolicy;

/// Product tiers. `Pro` is a strict superset of `Free` (FR-043).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tier {
    /// Free tier: lossy Wi-Fi + supported Bluetooth cells (FR-042).
    Free,
    /// Pro tier: everything in `Free` plus lossless Wi-Fi and bit-perfect
    /// PCM output verification (FR-043).
    Pro,
}

/// Features gated behind a tier. The set is central here so policy and tests
/// share one source of truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Feature {
    /// Free + Pro. Opus over Wi-Fi (FR-020).
    LossyWifi,
    /// Pro only. FLAC/PCM over Wi-Fi (FR-021).
    LosslessWifi,
    /// Free + Pro. Supported Bluetooth cells where public APIs make an
    /// emitter→receiver link viable (FR-030).
    BluetoothSupportedCells,
    /// Free + Pro. Diagnostics panel, bounded local logs, redacted export
    /// (FR-053/FR-055).
    Diagnostics,
    /// Pro only. Bit-perfect PCM output verification, only claimable when the
    /// full digital path is verified (FR-022).
    BitPerfectVerify,
}

/// All [`Feature`]s, order-stable — used by parity tests and lookup helpers.
pub const ALL_FEATURES: [Feature; 5] = [
    Feature::LossyWifi,
    Feature::LosslessWifi,
    Feature::BluetoothSupportedCells,
    Feature::Diagnostics,
    Feature::BitPerfectVerify,
];

/// Static tier→feature grant table (FR-042/FR-043). This is the single
/// normative table; [`DevToggleEntitlementProvider`] and [`ProviderTier`] are
/// parity-checked against it by tests.
pub const DEFAULT_POLICY_TABLE: [(Tier, Feature, bool); 10] = [
    (Tier::Free, Feature::LossyWifi, true),
    (Tier::Free, Feature::LosslessWifi, false),
    (Tier::Free, Feature::BluetoothSupportedCells, true),
    (Tier::Free, Feature::Diagnostics, true),
    (Tier::Free, Feature::BitPerfectVerify, false),
    (Tier::Pro, Feature::LossyWifi, true),
    (Tier::Pro, Feature::LosslessWifi, true),
    (Tier::Pro, Feature::BluetoothSupportedCells, true),
    (Tier::Pro, Feature::Diagnostics, true),
    (Tier::Pro, Feature::BitPerfectVerify, true),
];

/// Feature-level policy lookup over [`DEFAULT_POLICY_TABLE`].
///
/// Unknown feature/tier pairs are treated as **denied** (fail-closed): a newly
/// added `Feature` that is missing from the table is not silently granted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeaturePolicy;

impl FeaturePolicy {
    /// Grant lookup over the static table.
    pub fn granted(tier: Tier, feature: Feature) -> bool {
        DEFAULT_POLICY_TABLE
            .iter()
            .find(|(t, f, _)| *t == tier && *f == feature)
            .map(|(_, _, granted)| *granted)
            .unwrap_or(false)
    }

    /// The single normative table (mirrors [`DEFAULT_POLICY_TABLE`]).
    pub const fn table() -> [(Tier, Feature, bool); 10] {
        DEFAULT_POLICY_TABLE
    }
}

/// Cheap convenience: how strong is `feature` for a given `tier`?
///
/// - `none` — not available in this tier.
/// - `lossy` — available (Opus over Wi-Fi, or a lossy transport).
/// - `lossless` — available losslessly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureLevel {
    None,
    Lossy,
    Lossless,
}

impl FeatureLevel {
    pub fn is_enabled(self) -> bool {
        !matches!(self, FeatureLevel::None)
    }
}

/// The `EntitlementProvider` **is** the commerce/billing border. All tier and
/// feature decisions at runtime go through it; no other code path may.
///
/// See the crate-root docs for the reserved migration boundary (FR-044) and
/// the honesty caveat on the dev toggle (FR-045).
pub trait EntitlementProvider {
    /// The product tier currently effective.
    fn tier(&self) -> Tier;

    /// Whether feature `f` is enabled under the effective policy.
    fn feature_enabled(&self, f: Feature) -> bool;
}

/// Value-style extension: any provider can also answer the bundle of fields
/// derived from its effective policy.
///
/// Implemented on top of the trait so implementers only supply
/// [`EntitlementProvider::tier`] (they may still override). Non-defaulting
/// consumers should prefer [`EffectivePolicy`] semantics directly.
pub trait EntitlementEffective {
    /// Derive the effective transport policy from the provider's tier.
    fn effective_policy(&self) -> EffectivePolicy;

    /// Whether bit-perfect output is verified and enabled at this tier.
    fn bit_perfect_enabled(&self) -> bool;
}

/// Blanket impl: every provider can derive its effective policy from the
/// static table + feature checks.
impl<P: EntitlementProvider + ?Sized> EntitlementEffective for P {
    fn effective_policy(&self) -> EffectivePolicy {
        EffectivePolicy {
            lossy_wifi: self.feature_enabled(Feature::LossyWifi),
            lossless_wifi: self.feature_enabled(Feature::LosslessWifi),
            bluetooth_cells: self.feature_enabled(Feature::BluetoothSupportedCells),
        }
    }

    fn bit_perfect_enabled(&self) -> bool {
        self.feature_enabled(Feature::BitPerfectVerify)
    }
}

/// In-memory provider bound to a concrete [`Tier`] value. Used by tests,
/// demos and by UI to present the *current* toggle selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderTier {
    tier: Tier,
}

impl ProviderTier {
    pub const fn new(tier: Tier) -> Self {
        Self { tier }
    }
}

impl EntitlementProvider for ProviderTier {
    fn tier(&self) -> Tier {
        self.tier
    }

    fn feature_enabled(&self, f: Feature) -> bool {
        FeaturePolicy::granted(self.tier, f)
    }
}

/// Reserved adapter boundary for future commerce (FR-044).
///
/// **Not implemented.** This empty marker trait exists so a future billing
/// backend can live *behind* the [`EntitlementProvider`] seam today: the
/// holder of a purchase unlocks a persistent provider that reads from a
/// commerce backend, while the DevToggle provider is retired or pushed into
/// debug builds. No other part of the product needs to change —
/// `EntitlementProvider` stays the only seam.
pub trait CommerceEntitlementBackend {}

/// Concrete state for a DevTest toggle provider over an in-memory tier (FR-045).
///
/// **Dev/demo switch — not tamper-resistant.** This is the FR-048 development
/// toggle that keeps every shipped artifact functional while billing is
/// deferred. It is `pub` so the UI can drive it, which means it is **not** a
/// security control: a user can flip it freely. Production release must not
/// rely on it as enforcement; replace it via the [`EntitlementProvider`] seam
/// in the separate, approved commerce project (FR-048).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DevToggleEntitlementProvider {
    tier: Tier,
}

impl DevToggleEntitlementProvider {
    pub const fn new(tier: Tier) -> Self {
        Self { tier }
    }
}

impl Default for DevToggleEntitlementProvider {
    fn default() -> Self {
        Self { tier: Tier::Free }
    }
}

impl EntitlementProvider for DevToggleEntitlementProvider {
    fn tier(&self) -> Tier {
        self.tier
    }

    fn feature_enabled(&self, f: Feature) -> bool {
        FeaturePolicy::granted(self.tier, f)
    }
}

impl CommerceEntitlementBackend for DevToggleEntitlementProvider {}

/// Change the effective tier of a toggle-backed provider (dev UI toggle).
///
/// Note: this only flips the in-memory tier; it does **not** itself run a
/// renegotiation. Callers wanting the FR-047 plan must call
/// [`crate::policy::renegotiate_live_change`] explicitly (the UI flow does
/// exactly that: change the toggle → run the plan → apply or pause).
pub fn set_dev_toggle_tier(provider: &mut DevToggleEntitlementProvider, tier: Tier) {
    provider.tier = tier;
}

/// Fail-closed feature level (lossy/lossless/none) without the provider trait.
pub fn feature_level(tier: Tier, f: Feature) -> FeatureLevel {
    match (tier, f) {
        (Tier::Free, Feature::LossyWifi) => FeatureLevel::Lossy,
        (Tier::Free, Feature::BluetoothSupportedCells) | (Tier::Free, Feature::Diagnostics) => {
            FeatureLevel::Lossy
        }
        (Tier::Pro, Feature::LossyWifi)
        | (Tier::Pro, Feature::BluetoothSupportedCells)
        | (Tier::Pro, Feature::Diagnostics)
        | (Tier::Pro, Feature::LosslessWifi)
        | (Tier::Pro, Feature::BitPerfectVerify) => FeatureLevel::Lossless,
        _ => FeatureLevel::None,
    }
}

#[cfg(test)]
mod provider_tests {
    use super::*;

    #[test]
    fn dev_toggle_default_is_free() {
        assert_eq!(DevToggleEntitlementProvider::default().tier(), Tier::Free);
    }

    #[test]
    fn dev_toggle_set_tier_via_helper() {
        let mut p = DevToggleEntitlementProvider::default();
        set_dev_toggle_tier(&mut p, Tier::Pro);
        assert_eq!(p.tier(), Tier::Pro);
        assert!(p.feature_enabled(Feature::LosslessWifi));
        set_dev_toggle_tier(&mut p, Tier::Free);
        assert!(!p.feature_enabled(Feature::LosslessWifi));
    }

    #[test]
    fn dev_toggle_is_a_dev_demo_switch_not_enforcement() {
        // Documentary + structural: the toggle lives behind `CommerceEntitlementBackend`
        // as a marker, and must not be reachable as a *keyed* backend. It obeys the
        // static table but flips with a plain function — it is not a security control.
        let mut p = DevToggleEntitlementProvider::default();
        set_dev_toggle_tier(&mut p, Tier::Pro);
        assert!(p.feature_enabled(Feature::BitPerfectVerify));
        let _marker: &dyn CommerceEntitlementBackend = &p;
    }

    #[test]
    fn factory_level_matches_table() {
        for tier in [Tier::Free, Tier::Pro] {
            for f in ALL_FEATURES {
                let expected = FeaturePolicy::granted(tier, f);
                assert_eq!(
                    feature_level(tier, f).is_enabled(),
                    expected,
                    "feature level parity for {tier:?} × {f:?}"
                );
            }
        }
    }

    #[test]
    fn effective_policy_matches_provider() {
        for tier in [Tier::Free, Tier::Pro] {
            let p = ProviderTier::new(tier);
            let eff = p.effective_policy();
            assert_eq!(eff.lossy_wifi, p.feature_enabled(Feature::LossyWifi));
            assert_eq!(eff.lossless_wifi, p.feature_enabled(Feature::LosslessWifi));
            assert_eq!(
                eff.bluetooth_cells,
                p.feature_enabled(Feature::BluetoothSupportedCells)
            );
        }
    }
}
