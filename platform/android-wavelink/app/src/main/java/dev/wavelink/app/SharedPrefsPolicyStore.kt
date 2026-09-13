package dev.wavelink.app

import android.content.Context

/** Android persistence for [PolicyStore] (shared by both roles). */
class SharedPrefsPolicyStore(context: Context) : PolicyStore {

    private val prefs = context.getSharedPreferences("wdr_emitter", Context.MODE_PRIVATE)

    override fun readTier(): EntitlementTier = when (prefs.getString(KEY_TIER, null)) {
        "PRO" -> EntitlementTier.PRO
        "FREE" -> EntitlementTier.FREE
        else -> EntitlementTier.UNKNOWN // unresolved -> gate fails closed
    }

    override fun writeTier(tier: EntitlementTier) {
        if (tier == EntitlementTier.UNKNOWN) return // never persist the sentinel
        prefs.edit().putString(KEY_TIER, if (tier == EntitlementTier.PRO) "PRO" else "FREE").apply()
    }

    private companion object {
        const val KEY_TIER = "entitlement_tier"
    }
}
