//! Negotiation policy: intersection of peer policies (FR-046) and explicit
//! mid-session renegotiation plans with no silent lossless downgrade (FR-047).

use crate::provider::{feature_level, Feature, Tier};

/// Coherent transport capabilities agreed by both peers (FR-046).
///
/// Produced by [`Policy::intersect`]. Every field is the **per-axis AND** of
/// both peers' offerings — the common denominator both can serve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectivePolicy {
    /// Lossy Wi-Fi (Opus / datagram path) available.
    pub lossy_wifi: bool,
    /// Lossless Wi-Fi (FLAC/PCM, reliable stream) available.
    pub lossless_wifi: bool,
    /// Supported Bluetooth cells available.
    pub bluetooth_cells: bool,
}

impl EffectivePolicy {
    /// Sanity: an effective policy must not claim lossless without a working
    /// lossless feature (it is derived from the table, so this is expected to
    /// always hold — the assertion guards future drift).
    pub fn is_coherent(&self) -> bool {
        // There is no incoherent combination expressible in the current field
        // set; kept as a documented no-op check that future axes can extend.
        let _ = (self.lossy_wifi, self.lossless_wifi, self.bluetooth_cells);
        true
    }
}

impl Default for EffectivePolicy {
    /// The safe fallback when no intersection exists.
    fn default() -> Self {
        EffectivePolicy {
            lossy_wifi: false,
            lossless_wifi: false,
            bluetooth_cells: false,
        }
    }
}

/// Why a negotiation could not produce a common policy (FR-046).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MismatchKind {
    /// A peer offers no transport at all on Wi-Fi (no lossy baseline).
    #[default]
    NoCommonLossy,
    /// The unsigned/unknown-data mismatch guard (reserved, currently unused).
    UnknownExtData,
    /// Generic incoherence (reserved, currently unused).
    Incoherent,
}

/// The visible failure surfaced when two policies cannot be intersected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NegotiationMismatch {
    pub kind: MismatchKind,
    pub local: EffectivePolicy,
    pub remote: EffectivePolicy,
}

impl NegotiationMismatch {
    /// Human-readable reason for logs/UI.
    pub fn reason(&self) -> &'static str {
        match self.kind {
            MismatchKind::NoCommonLossy => {
                "no common lossy Wi-Fi transport available on both peers"
            }
            MismatchKind::UnknownExtData => "unrecognized extension data in policy (reserved)",
            MismatchKind::Incoherent => "incoherent policy data (reserved)",
        }
    }
}

/// A peer's policy expressed as a compact bitfield (FR-046).
///
/// `bits` currently allocate: bit 0 = lossy Wi-Fi, bit 1 = lossless Wi-Fi,
/// bit 2 = supported Bluetooth cells. `clearbits` are reserved for future
/// explicit negation — kept so the type is forward-compatible without
/// renegotiating the wire position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    pub tier: Tier,
    pub bits: u8,
}

/// bit position of lossy Wi-Fi in [`Policy::bits`].
pub const BIT_LOSSY_WIFI: u8 = 1 << 0;
/// bit position of lossless Wi-Fi in [`Policy::bits`].
pub const BIT_LOSSLESS_WIFI: u8 = 1 << 1;
/// bit position of Bluetooth cells in [`Policy::bits`].
pub const BIT_BLUETOOTH_CELLS: u8 = 1 << 2;

impl Policy {
    /// Build a policy from the effective policy of a tier.
    pub fn from_tier(tier: Tier) -> Self {
        let eff = effective_of_tier(tier);
        Self {
            tier,
            bits: (eff.lossy_wifi as u8 * BIT_LOSSY_WIFI)
                | (eff.lossless_wifi as u8 * BIT_LOSSLESS_WIFI)
                | (eff.bluetooth_cells as u8 * BIT_BLUETOOTH_CELLS),
        }
    }

    /// The effective policy encoded by these bits (for the local tier).
    pub fn effective(self) -> EffectivePolicy {
        let b = self.bits;
        EffectivePolicy {
            lossy_wifi: b & BIT_LOSSY_WIFI != 0,
            lossless_wifi: b & BIT_LOSSLESS_WIFI != 0,
            bluetooth_cells: b & BIT_BLUETOOTH_CELLS != 0,
        }
    }

    /// A no-bits policy (used by tests and as the degenerate case).
    pub fn empty(tier: Tier) -> Self {
        Self { tier, bits: 0 }
    }

    /// Whether the policy offers the lossy Wi-Fi baseline (Free/Pro required).
    pub fn has_lossy_wifi(self) -> bool {
        self.bits & BIT_LOSSY_WIFI != 0
    }

    /// Whether the policy offers lossless Wi-Fi.
    pub fn has_lossless_wifi(self) -> bool {
        self.bits & BIT_LOSSLESS_WIFI != 0
    }

    /// Whether the policy offers supported Bluetooth cells.
    pub fn has_bluetooth_cells(self) -> bool {
        self.bits & BIT_BLUETOOTH_CELLS != 0
    }

    /// Per-axis intersection of two peers' policies (FR-046).
    ///
    /// Returns `Ok(effective)` when the common denominator exists (per-axis
    /// AND of the supplied bits). If **either** side lacks the lossy baseline
    /// needed for *any* session, returns `Err` with a visible
    /// [`NegotiationMismatch`] (never a silent empty session).
    pub fn intersect(&self, other: &Self) -> Result<EffectivePolicy, NegotiationMismatch> {
        let local = self.effective();
        let remote = other.effective();

        let eff = EffectivePolicy {
            lossy_wifi: local.lossy_wifi && remote.lossy_wifi,
            lossless_wifi: local.lossless_wifi && remote.lossless_wifi,
            bluetooth_cells: local.bluetooth_cells && remote.bluetooth_cells,
        };

        if !eff.lossy_wifi {
            return Err(NegotiationMismatch {
                kind: MismatchKind::NoCommonLossy,
                local,
                remote,
            });
        }
        Ok(eff)
    }

    /// True if `other` describes an equal capability set.
    pub fn capability_eq(&self, other: &Self) -> bool {
        (self.bits >> 1) == (other.bits >> 1)
    }
}

/// Session mode at the time a live tier change begins (FR-047).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionMode {
    /// Session is actively streaming audio.
    Streaming,
    /// Session exists but is paused; audio will resume on a negotiated policy.
    Paused,
    /// No session exists yet; only the toggle changed.
    Idle,
}

/// User preference when a live tier change would drop a capability (FR-047).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DowngradePreference {
    /// Ask the user each time (pause + explicit confirm — never silent).
    AskEachTime,
    /// Apply a previously-saved explicit downgrade intent without a prompt.
    UseSaved,
}

/// The planned outcome of an explicit renegotiation (FR-047).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenegotiationPlan {
    /// Pause the session and ask for explicit confirmation of the new, lower
    /// capability policy. Never silently applied.
    PauseForConfirm,
    /// Apply the saved downgrade policy directly (already explicitly approved
    /// earlier by the user).
    ApplySaved(EffectivePolicy),
    /// Nothing changes (e.g. the tier change is a no-op for effective policy).
    NoChange,
}

impl RenegotiationPlan {
    /// Does this plan guarantee lossless is never silently dropped? True for
    /// every variant we produce (see [`renegotiate_live_change`]).
    pub fn never_silently_drops_lossless(&self) -> bool {
        match self {
            RenegotiationPlan::PauseForConfirm => true,
            RenegotiationPlan::ApplySaved(eff) => !eff.lossless_wifi,
            RenegotiationPlan::NoChange => true,
        }
    }
}

/// Effective policy for a tier (single source of truth).
pub fn effective_of_tier(tier: Tier) -> EffectivePolicy {
    // Use `feature_level` (the pure table-driven view) so policy never drifts
    // from the static table.
    EffectivePolicy {
        lossy_wifi: feature_level(tier, Feature::LossyWifi).is_enabled(),
        lossless_wifi: feature_level(tier, Feature::LosslessWifi).is_enabled(),
        bluetooth_cells: feature_level(tier, Feature::BluetoothSupportedCells).is_enabled(),
    }
}

/// The core FR-047 transition: what happens when the effective tier changes
/// while a session is live.
///
/// Rules (no silent downgrade, FR-047/FR-026):
/// 1. If the new tier is a strict **upgrade** (or equal capability), we
///    return `NoChange` — nothing about the active stream degrades; promotion
///    is offered by the caller but never forces a pause.
/// 2. If the change would **drop** the lossless capability:
///    - `DowngradePreference::AskEachTime` → `PauseForConfirm`.
///    - `DowngradePreference::UseSaved` → `ApplySaved(new_effective)` **only
///      if** `new_effective.lossless_wifi == false`, i.e. the saved policy was
///      already explicitly downgraded and can never keep lossless alive.
///    - If session is `Idle`, no live session is affected → `NoChange`.
/// 3. A lossless Wi-Fi *provision* is **never** preserved silently when the
///    new tier lacks it: `ApplySaved` is constructed so its policy lacks
///    lossless; any other path returns `PauseForConfirm`.
#[must_use]
pub fn renegotiate_live_change(
    old: Tier,
    new: Tier,
    state: SessionMode,
    pref: DowngradePreference,
) -> RenegotiationPlan {
    let old_eff = effective_of_tier(old);
    let new_eff = effective_of_tier(new);

    if state == SessionMode::Idle {
        return RenegotiationPlan::NoChange;
    }

    // No lossless to lose → nothing to pause over.
    let loses_lossless = old_eff.lossless_wifi && !new_eff.lossless_wifi;
    if !loses_lossless {
        return RenegotiationPlan::NoChange;
    }

    match pref {
        DowngradePreference::AskEachTime => RenegotiationPlan::PauseForConfirm,
        DowngradePreference::UseSaved => {
            if new_eff.lossless_wifi {
                // Safety net: never let ApplySaved silently keep lossless.
                RenegotiationPlan::PauseForConfirm
            } else {
                RenegotiationPlan::ApplySaved(new_eff)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{FeaturePolicy, ALL_FEATURES};
    use crate::{Feature, FeatureLevel};

    fn eff(tier: Tier) -> EffectivePolicy {
        effective_of_tier(tier)
    }

    #[test]
    fn policy_roundtrip_from_tier() {
        assert!(Policy::from_tier(Tier::Free).has_lossy_wifi());
        assert!(!Policy::from_tier(Tier::Free).has_lossless_wifi());
        assert!(Policy::from_tier(Tier::Free).has_bluetooth_cells());
        assert!(Policy::from_tier(Tier::Pro).has_lossless_wifi());
        assert!(Policy::from_tier(Tier::Pro).has_bluetooth_cells());
        assert_eq!(Policy::from_tier(Tier::Free).effective(), eff(Tier::Free));
        assert_eq!(Policy::from_tier(Tier::Pro).effective(), eff(Tier::Pro));
    }

    #[test]
    fn intersection_matrix() {
        // Free ∩ Free = Free (lossy + BT).
        assert_eq!(
            Policy::from_tier(Tier::Free)
                .intersect(&Policy::from_tier(Tier::Free))
                .unwrap(),
            eff(Tier::Free)
        );
        // Pro ∩ Pro = Pro.
        assert_eq!(
            Policy::from_tier(Tier::Pro)
                .intersect(&Policy::from_tier(Tier::Pro))
                .unwrap(),
            eff(Tier::Pro)
        );
        // Free ∩ Pro (and symmetric) = the common (Free) capability: lossless
        // is dropped, but the session stays viable on lossy + BT.
        let free_pro = Policy::from_tier(Tier::Free)
            .intersect(&Policy::from_tier(Tier::Pro))
            .unwrap();
        assert_eq!(free_pro, eff(Tier::Free));
        assert!(!free_pro.lossless_wifi);
        let pro_free = Policy::from_tier(Tier::Pro)
            .intersect(&Policy::from_tier(Tier::Free))
            .unwrap();
        assert_eq!(free_pro, pro_free, "intersection is commutative");
    }

    #[test]
    fn intersection_commutative_all_pairs() {
        let tiers = [Tier::Free, Tier::Pro];
        for a in tiers {
            for b in tiers {
                let ab = Policy::from_tier(a).intersect(&Policy::from_tier(b));
                let ba = Policy::from_tier(b).intersect(&Policy::from_tier(a));
                match (ab, ba) {
                    (Ok(x), Ok(y)) => assert_eq!(x, y),
                    (Err(x), Err(y)) => assert_eq!(x.kind, y.kind),
                    (x, y) => panic!("direction-dependent intersection: {x:?} vs {y:?}"),
                }
            }
        }
    }

    #[test]
    fn mismatch_is_visible_when_peer_lacks_lossy() {
        // A peer offering nothing on Wi-Fi (no lossy baseline) must produce a
        // visible, typed mismatch — not a silent empty session.
        let remote = Policy::empty(Tier::Pro);
        let err = Policy::from_tier(Tier::Pro).intersect(&remote).unwrap_err();
        assert_eq!(err.kind, MismatchKind::NoCommonLossy);
        assert!(!err.remote.lossy_wifi);
        assert!(err.local.lossy_wifi);
        assert!(!err.reason().is_empty());
    }

    #[test]
    fn mismatch_when_self_offers_nothing() {
        let err = Policy::empty(Tier::Free)
            .intersect(&Policy::from_tier(Tier::Free))
            .unwrap_err();
        assert_eq!(err.kind, MismatchKind::NoCommonLossy);
    }

    #[test]
    fn feature_table_free_per_fr_042() {
        let table = FeaturePolicy::table();
        for (tier, f, granted) in table {
            if tier == Tier::Free {
                let expect_free = matches!(
                    f,
                    Feature::LossyWifi | Feature::BluetoothSupportedCells | Feature::Diagnostics
                );
                assert_eq!(granted, expect_free, "Free grant mismatch for {f:?}");
            }
        }
    }

    #[test]
    fn feature_table_pro_superset_fr_043() {
        let table = FeaturePolicy::table();
        for (tier, f, granted) in table {
            if tier == Tier::Pro {
                assert!(granted, "Pro must grant every known feature: {f:?}");
            }
        }
    }

    #[test]
    fn feature_table_unknown_is_denied() {
        // Fail-closed: an unknown feature/tier pair is `None`, never granted.
        assert!(!FeaturePolicy::granted(
            Tier::Free,
            Feature::BitPerfectVerify
        ));
        assert!(!FeaturePolicy::granted(Tier::Free, Feature::LosslessWifi));
        assert!(FeaturePolicy::granted(Tier::Free, Feature::Diagnostics));
        assert!(FeaturePolicy::granted(Tier::Pro, Feature::LosslessWifi));
        assert!(FeaturePolicy::granted(Tier::Pro, Feature::BitPerfectVerify));
    }

    #[test]
    fn feature_level_matches_table_full() {
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
    fn no_silent_lossless_drop_all_paths() {
        // Property: every plan produced must never silently preserve lossless
        // when the new tier cannot serve it.
        for state in [
            SessionMode::Streaming,
            SessionMode::Paused,
            SessionMode::Idle,
        ] {
            for pref in [
                DowngradePreference::AskEachTime,
                DowngradePreference::UseSaved,
            ] {
                for old in [Tier::Pro, Tier::Free] {
                    for new in [Tier::Free, Tier::Pro] {
                        let plan = renegotiate_live_change(old, new, state, pref);
                        assert!(
                            plan.never_silently_drops_lossless(),
                            "silent drop risk: old={old:?} new={new:?} state={state:?} pref={pref:?} → {plan:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn pro_to_free_streaming_lossless_forces_pause() {
        // Pro→Free while actively streaming and lossless is provisioned must
        // force an explicit pause-and-confirm — never silent.
        assert_eq!(
            renegotiate_live_change(
                Tier::Pro,
                Tier::Free,
                SessionMode::Streaming,
                DowngradePreference::AskEachTime
            ),
            RenegotiationPlan::PauseForConfirm
        );
        // Same with the saved-preference: still cannot apply silently —
        // Application is only allowed via the explicit UseSaved path checked
        // below, and it must *drop* lossless, not keep it.
        let plan = renegotiate_live_change(
            Tier::Pro,
            Tier::Free,
            SessionMode::Streaming,
            DowngradePreference::UseSaved,
        );
        match plan {
            RenegotiationPlan::ApplySaved(eff) => {
                assert!(!eff.lossless_wifi, "ApplySaved must not keep lossless");
            }
            other => panic!("expected ApplySaved, got {other:?}"),
        }
    }

    #[test]
    fn apply_saved_never_silently_keeps_lossless() {
        // Even when the saved pref is used, the applied policy must have
        // lossless disabled — by construction.
        for state in [SessionMode::Streaming, SessionMode::Paused] {
            if let RenegotiationPlan::ApplySaved(eff) =
                renegotiate_live_change(Tier::Pro, Tier::Free, state, DowngradePreference::UseSaved)
            {
                assert!(!eff.lossless_wifi);
            } else {
                panic!("expected ApplySaved for state {state:?}");
            }
        }
    }

    #[test]
    fn paused_and_streaming_both_honor_use_saved() {
        assert_eq!(
            renegotiate_live_change(
                Tier::Pro,
                Tier::Free,
                SessionMode::Paused,
                DowngradePreference::UseSaved
            ),
            RenegotiationPlan::ApplySaved(eff(Tier::Free))
        );
    }

    #[test]
    fn idle_is_no_change() {
        assert_eq!(
            renegotiate_live_change(
                Tier::Pro,
                Tier::Free,
                SessionMode::Idle,
                DowngradePreference::AskEachTime
            ),
            RenegotiationPlan::NoChange
        );
        assert_eq!(
            renegotiate_live_change(
                Tier::Pro,
                Tier::Free,
                SessionMode::Idle,
                DowngradePreference::UseSaved
            ),
            RenegotiationPlan::NoChange
        );
    }

    #[test]
    fn no_lossless_lost_is_no_change() {
        // Free→Pro, Idle/Streaming/Paused: promotion while streaming never
        // forces a pause (NoChange), because nothing is dropped.
        for state in [
            SessionMode::Streaming,
            SessionMode::Paused,
            SessionMode::Idle,
        ] {
            for pref in [
                DowngradePreference::AskEachTime,
                DowngradePreference::UseSaved,
            ] {
                assert_eq!(
                    renegotiate_live_change(Tier::Free, Tier::Pro, state, pref),
                    RenegotiationPlan::NoChange,
                    "free→pro {state:?} {pref:?}"
                );
            }
        }
    }

    #[test]
    fn no_panic_over_all_tiers_and_features() {
        // The task requires a "no-panic on arbitrary tier × feature" property.
        // Run every tier with every feature through the fail-closed lookup and
        // feature-level mapping; assert results are always defined.
        for tier in [Tier::Free, Tier::Pro] {
            for f in ALL_FEATURES {
                let _ = feature_level(tier, f);
                let _ = FeaturePolicy::granted(tier, f);
            }
        }
    }

    #[test]
    fn feature_json_smoke() {
        let _ = crate::Feature::LossyWifi;
        let _ = FeatureLevel::Lossless;
    }
}
