# t-B0-ent — EntitlementProvider Crate (Free/Pro Policy, Intersection, Live-Change)

**task_id:** t-B0-ent
**root_task_id:** R-B0-ENT
**owner_role:** Product Analyst
**status:** complete
**date:** 2026-09-06

## Summary

Created `crates/wdr_entitlement/` implementing the centralized entitlement model specified by FR-040…FR-048: the `Tier` enum (`Free`/`Pro`), the `Feature` set, a single normative tier→feature `DEFAULT_POLICY_TABLE` (FR-042/FR-043), the `EntitlementProvider` trait as the **only** seam all session/net code consumes (FR-041), the shipped dev/demo `DevToggleEntitlementProvider` documented as **not** tamper-resistant (FR-045), the reserved `CommerceEntitlementBackend` marker for future billing behind the same trait (FR-044), the `Policy` bitfield + `intersect()` returning a coherent `EffectivePolicy` or a **visible** `NegotiationMismatch` (FR-046), and `renegotiate_live_change()` producing `RenegotiationPlan` with an explicit forbid on silently dropping LosslessWifi while Streaming (FR-047).

## Files changed

| Path | Change |
|------|--------|
| `crates/wdr_entitlement/Cargo.toml` | New crate (no deps; workspace metadata). |
| `crates/wdr_entitlement/src/lib.rs` | Crate root docs (centralized boundary, toggle honesty, commerce migration border) + re-exports. |
| `crates/wdr_entitlement/src/provider.rs` | `Tier`, `Feature`, `ALL_FEATURES`, `DEFAULT_POLICY_TABLE`, `FeaturePolicy` (fail-closed lookup), `EntitlementProvider` trait, `ProviderTier`, `DevToggleEntitlementProvider` (FR-045, non-enforcement), `CommerceEntitlementBackend` marker (FR-044), `EntitlementEffective` blanket impl, `feature_level()`, `set_dev_toggle_tier()`. |
| `crates/wdr_entitlement/src/policy.rs` | `EffectivePolicy`, `MismatchKind`, `NegotiationMismatch`, `Policy` bitset + `intersect()` (FR-046), `SessionMode`, `DowngradePreference`, `RenegotiationPlan`, `renegotiate_live_change()` (FR-047), `effective_of_tier()`. |
| `docs/orchestration/reports/t-B0-ent.md` | This report. |

Root `Cargo.toml` was **not** edited: it already declares `members = ["crates/*"]`, so the new crate is auto-included (verified by `cargo metadata`).

## Decisions

1. **Single normative table:** one `DEFAULT_POLICY_TABLE` is the source of truth; `FeaturePolicy::granted()`, `DevToggleEntitlementProvider`, `ProviderTier`, and `feature_level()` all read the same table and are parity-checked by tests. Unknown (tier, feature) pairs fail **closed** (denied), never silently granted.
2. **FR-042/FR-043 mapping:** Free = LossyWifi + BluetoothSupportedCells + Diagnostics; Pro = superset + LosslessWifi + BitPerfectVerify (specs allow Diagnostics anywhere; only LosslessWifi and BitPerfectVerify are Pro-only).
3. **Intersection = per-axis AND** of both peers' encoded capabilities; incoherent/empty (no lossy Wi-Fi baseline in *either* side) returns `Err(NegotiationMismatch)` so the failure is visible and typed, never a silent empty session. Free∩Pro = Pro∩Free = the Free capability (lossless dropped, session stays viable).
4. **Live-change is explicit (FR-047):** `renegotiate_live_change(Pro→Free, Streaming, AskEachTime) = PauseForConfirm`. With `UseSaved`, plan = `ApplySaved(new_eff)` **only when** the new tier's effective policy has `lossless_wifi == false` — by construction `ApplySaved` can never retain LosslessWifi (a property under test). No lossless lost (e.g. Free→Pro) or `Idle` state → `NoChange`; no silent downgrades anywhere.
5. **Held the DIAGNOSTICS decision as Free+Pro** over the task's "etc." wording — specs permit diagnostics at both tiers; changing this later is a one-cell table edit.

## Commands run

```bash
source dev/env.sh
cargo build -p wdr_entitlement
cargo test  -p wdr_entitlement
cargo clippy -p wdr_entitlement --all-targets --all-features -- -D warnings
rustfmt --edition 2021 --check crates/wdr_entitlement/src/*.rs
cargo metadata --no-deps              # confirms crates/* glob picks up the new crate
```

## Validation results

- `cargo build -p wdr_entitlement` → OK, no warnings.
- `cargo test -p wdr_entitlement` → **22 passed, 0 failed** (plus 0 doc-tests).
- `cargo clippy -p wdr_entitlement --all-targets --all-features -- -D warnings` → clean.
- `rustfmt --check` on the crate's 3 source files → clean.
- Coverage of required test list:
  - feature table Free (FR-042) / Pro superset (FR-043) → `feature_table_free_per_fr_042`, `feature_table_pro_superset_fr_043`, `feature_table_unknown_is_denied`
  - intersection matrix Free∩Free, Pro∩Pro, Free∩Pro and Pro∩Free (both directions, commutativity) → `intersection_matrix`, `intersection_commutative_all_pairs`
  - mismatch visible → `mismatch_is_visible_when_peer_lacks_lossy`, `mismatch_when_self_offers_nothing`
  - live-change Pro→Free during lossless forces PauseForConfirm; UseSaved applies but NEVER retains lossless → `pro_to_free_streaming_lossless_forces_pause`, `apply_saved_never_silently_keeps_lossless`, `paused_and_streaming_both_honor_use_saved`
  - no-panic on arbitrary tier×feature → `no_panic_over_all_tiers_and_features` (exhaustive — the whole 2×5 domain, which is stronger than a sampled proptest for this size; no proptest dep added per dependency-policy discipline)
  - DevToggleProvider parity with the table → `provider_tier_matches_static_table`, `dev_toggle_parity_default`, `factory_level_matches_table`, `effective_policy_matches_provider`

## Acceptance criteria

- [x] `wdr_entitlement` crate builds, tests green (22/22), clippy clean, fmt clean.
- [x] Intersection semantics exactly per FR-046 (per-axis AND, coherent `EffectivePolicy`, visible typed mismatch when either side lacks the baseline).
- [x] Live-change semantics exactly per FR-047 (explicit renegotiation; Pro→Free during lossless → PauseForConfirm unless saved prefs → `ApplySaved`, which never silently keeps lossless; nothing silent).
- [x] Report written.

## Risks / limitations

- **`cargo build --workspace` / `just selftest` currently fail** on the *parallel, in-progress* `crates/wdr_proto` (round-2 `t-B0-proto` worker still constructing its `control.rs`/`golden.rs` mods — errors are in `wdr_proto`, not here). This crate itself is fully green; workspace-wide green resumes once the proto crate lands (per INTEGRATION_STATUS merge order: contracts/schemas first).
- No deps were added; `FeatureLevel`/`Policy` bitfield layout and `MismatchKind::UnknownExtData`/`Incoherent` variants are reserved-but-unused forward-compat stubs (documented in code).
- DevToggle is a non-enforcement demo switch by design (FR-045); product must not treat it as a security control.

## Follow-up tasks

- `t-B0-session` (parallel) will consume `EntitlementProvider` via `agree_common_policy` (PROTOCOL_SPEC) — requires the protocol `Policy` wire encoding to match the bit positions in `Policy::bits`.
- Commerce/billing project (FR-044/FR-048): implement `CommerceEntitlementBackend` and swap it behind `EntitlementProvider`; separate approved project.
- Integration: confirm `cargo build --workspace` once `wdr_proto` lands.

## Blockers

- None for this task. Workspace-wide validation is gated only on the parallel `wdr_proto` crate landing (tracked by `t-B0-proto`).
