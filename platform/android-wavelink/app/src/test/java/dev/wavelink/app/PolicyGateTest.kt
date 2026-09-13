package dev.wavelink.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Pure-JVM tests for the Free/Pro entitlement gate (emitter side). Standard
 * JUnit 4, no Android/Robolectric — PolicyGate only talks to [PolicyStore].
 * Same harness as the receiver; plus the "unknown fails closed" tier.
 */
class PolicyGateTest {

    private class InMemoryStore(initial: EntitlementTier = EntitlementTier.FREE) : PolicyStore {
        var tier: EntitlementTier = initial
        override fun readTier(): EntitlementTier = tier
        override fun writeTier(newTier: EntitlementTier) {
            tier = newTier
        }
    }

    @Test
    fun freeRejectsLossless() {
        val gate = PolicyGate(InMemoryStore(EntitlementTier.FREE))
        assertFalse("Free tier must never allow lossless (FR-042)", gate.allowLossless())
        assertEquals(EntitlementTier.FREE, gate.currentTier())
        assertFalse(gate.policy().lossless)
    }

    @Test
    fun proAllowsLossless() {
        val gate = PolicyGate(InMemoryStore(EntitlementTier.PRO))
        assertTrue("Pro tier must allow lossless (FR-043)", gate.allowLossless())
        assertTrue(gate.policy().lossless)
        assertEquals(EntitlementTier.PRO, gate.currentTier())
    }

    @Test
    fun upgradeFreeToProPermitsLossless() {
        val gate = PolicyGate(InMemoryStore(EntitlementTier.FREE))
        assertEquals(RenegotiationRequest.APPLIED, gate.toggle(EntitlementTier.PRO))
        assertTrue(gate.allowLossless())
        assertEquals(EntitlementTier.PRO, gate.currentTier())
    }

    @Test
    fun proDowngradeWhileIdleApplies() {
        val gate = PolicyGate(InMemoryStore(EntitlementTier.PRO))
        gate.setStreamingLossless(false)
        assertEquals(RenegotiationRequest.APPLIED, gate.toggle(EntitlementTier.FREE))
        assertEquals(EntitlementTier.FREE, gate.currentTier())
    }

    @Test
    fun proDowngradeWhileLosslessRequiresConfirmAndDoesNotApply() {
        val gate = PolicyGate(InMemoryStore(EntitlementTier.PRO))
        gate.setStreamingLossless(true)
        assertTrue(
            "requiresConfirmForDowngrade must fire for PRO->FREE mid-lossless (FR-047)",
            gate.requiresConfirmForDowngrade(EntitlementTier.FREE),
        )
        assertEquals(
            "PRO->FREE mid-lossless must NOT apply silently (FR-026/FR-047)",
            RenegotiationRequest.REQUIRES_CONFIRM,
            gate.toggle(EntitlementTier.FREE),
        )
        assertEquals(
            "Tier must be untouched until the caller confirms the renegotiation",
            EntitlementTier.PRO,
            gate.currentTier(),
        )
        assertTrue("Lossless must keep being allowed until confirmed downgrade", gate.allowLossless())
    }

    @Test
    fun confirmedDowngradeApplies() {
        val gate = PolicyGate(InMemoryStore(EntitlementTier.PRO))
        gate.setStreamingLossless(true)
        assertEquals(RenegotiationRequest.REQUIRES_CONFIRM, gate.toggle(EntitlementTier.FREE))
        // Caller pauses/confirms, then re-applies:
        gate.applyConfirmedTier(EntitlementTier.FREE)
        gate.setStreamingLossless(false)
        assertEquals(EntitlementTier.FREE, gate.currentTier())
        assertFalse(gate.allowLossless())
    }

    @Test
    fun toggleToSameTierIsNoOp() {
        val store = InMemoryStore(EntitlementTier.PRO)
        val gate = PolicyGate(store)
        assertEquals(RenegotiationRequest.APPLIED, gate.toggle(EntitlementTier.PRO))
        assertEquals(EntitlementTier.PRO, store.tier)
    }

    @Test
    fun unknownTierFailsClosedToFree() {
        val gate = PolicyGate(InMemoryStore(EntitlementTier.UNKNOWN))
        assertEquals("Store sentinel must stay visible to the caller", EntitlementTier.UNKNOWN, gate.currentTier())
        assertFalse("Unresolved/unknown tier must fail closed -> no lossless (FR-042)", gate.allowLossless())
        assertFalse(gate.policy().lossless)
        // And it must keep failing closed while actually streaming lossless:
        gate.setStreamingLossless(true)
        assertFalse(gate.allowLossless())
    }

    @Test
    fun unknownResolvesLikeFreeForDowngradeGuard() {
        val gate = PolicyGate(InMemoryStore(EntitlementTier.UNKNOWN))
        gate.setStreamingLossless(true)
        // Unknown == Free for the guard: upgrading to PRO is never a "downgrade".
        assertFalse(gate.requiresConfirmForDowngrade(EntitlementTier.PRO))
    }
}
