//! Wavelink — entitlement model (t-B0-ent).
//!
//! Codifies the Free/Pro tier policy (FR-042/FR-043), the single
//! `EntitlementProvider` seam all session/net code consumes (FR-041), the
//! development/demonstration toggle that backs it today (FR-045), the
//! `Policy` intersection used during peer negotiation (FR-046), and the
//! explicit renegotiation plan for mid-session tier changes with no silent
//! lossless downgrade (FR-047).
//!
//! # Centralized boundary
//!
//! [`EntitlementProvider`] is the *only* seam through which session, network
//! and UI code asks "what tier am I" and "is feature X enabled". Nothing else
//! in the product may hard-code a tier or feature grant. This is the boundary
//! FR-041 mandates and FR-044 reserves as the single migration point: when a
//! real commerce/billing backend lands, it is implemented behind this same
//! trait, not bolted on elsewhere.
//!
//! # Honesty about the toggle
//!
//! The shipped [`DevToggleEntitlementProvider`] is a development/demonstration
//! switch (FR-045). It is **not** tamper-resistant and **not** an enforcement
//! mechanism: it keeps the whole product functional in every shipped artifact
//! (FR-048) while accounting/billing is deferred. Replacing it with a real
//! commerce backend is a separate, approved project (FR-048) and must be
//! layered behind the [`EntitlementProvider`] / [`CommerceEntitlementBackend`]
//! boundary defined here. Production release must not rely on the toggle as a
//! security control.

pub mod policy;
pub mod provider;

pub use policy::{
    renegotiate_live_change, EffectivePolicy, NegotiationMismatch, Policy, RenegotiationPlan,
};
pub use provider::{
    feature_level, set_dev_toggle_tier, CommerceEntitlementBackend, DevToggleEntitlementProvider,
    EntitlementProvider, Feature, FeatureLevel, FeaturePolicy, ProviderTier, Tier, ALL_FEATURES,
    DEFAULT_POLICY_TABLE,
};
