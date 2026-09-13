package dev.wavelink.app

import dev.wavelink.app.emitter.CapturePolicy
import dev.wavelink.app.emitter.CaptureUsage

import android.media.AudioAttributes
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Usage allow-list logic for [CapturePolicy] / [CaptureUsage]. Pure and
 * framework-light: assertions reference ONLY the android.media.AudioAttributes
 * USAGE constants (compile-time ints, inlined into the bytecode — no framework
 * call at runtime), so this runs on the plain JVM with no Robolectric/device.
 * The allow-list decision itself flips no framework switches.
 */
class CapturePolicyTest {

    @Test
    fun defaultPolicyIsMediaOnly() {
        val p = CapturePolicy()
        assertTrue(p.shouldInclude(AudioAttributes.USAGE_MEDIA))
        assertFalse("Game not opted in by default", p.shouldInclude(AudioAttributes.USAGE_GAME))
        assertFalse("Unknown not opted in by default", p.shouldInclude(AudioAttributes.USAGE_UNKNOWN))
    }

    @Test
    fun fullOptInMixMatchesRequestedSelection() {
        val p = CapturePolicy(allowMedia = true, allowGame = true, allowUnknown = true)
        assertEquals(3, p.allowedUsages.size)
        assertTrue(p.shouldInclude(AudioAttributes.USAGE_MEDIA))
        assertTrue(p.shouldInclude(AudioAttributes.USAGE_GAME))
        assertTrue(p.shouldInclude(AudioAttributes.USAGE_UNKNOWN))
    }

    @Test
    fun unlistedUsagesAlwaysExcluded() {
        val p = CapturePolicy(allowMedia = true, allowGame = true, allowUnknown = true)
        // Outside the capturable {MEDIA, GAME, UNKNOWN} subset — always excluded.
        assertFalse(p.shouldInclude(AudioAttributes.USAGE_ALARM))
        assertFalse(p.shouldInclude(AudioAttributes.USAGE_NOTIFICATION))
        assertFalse(p.shouldInclude(AudioAttributes.USAGE_ASSISTANT))
        assertFalse(p.shouldInclude(AudioAttributes.USAGE_ASSISTANCE_NAVIGATION_GUIDANCE))
        assertFalse(p.shouldInclude(AudioAttributes.USAGE_VOICE_COMMUNICATION))
    }

    @Test
    fun unknownUsageExcludedWhenNotOptedIn() {
        val p = CapturePolicy(allowMedia = true, allowGame = true, allowUnknown = false)
        assertFalse(p.shouldInclude(AudioAttributes.USAGE_UNKNOWN))
    }

    @Test
    fun emptyPolicyFailsClosed() {
        val p = CapturePolicy(allowMedia = false, allowGame = false, allowUnknown = false)
        assertTrue(p.isEmpty)
        assertFalse(p.shouldInclude(AudioAttributes.USAGE_MEDIA))
        assertFalse(p.shouldInclude(AudioAttributes.USAGE_GAME))
        assertFalse(p.shouldInclude(AudioAttributes.USAGE_UNKNOWN))
    }

    @Test
    fun uidFilterRestrictsToAllowedUids() {
        assertTrue("Empty allow-uid set = all apps", CapturePolicy().uidAllowed(12_345))

        val restricted = CapturePolicy(allowMedia = true, allowUids = setOf(1_000, 2_000))
        assertTrue(restricted.uidAllowed(1_000))
        assertTrue(restricted.uidAllowed(2_000))
        assertFalse(restricted.uidAllowed(12_345))
        assertFalse(
            "UID gate must block capture from a non-allow-listed app",
            restricted.shouldCapture(uid = 12_345, usage = AudioAttributes.USAGE_MEDIA),
        )
        assertTrue(
            "UID gate must allow capture from an allow-listed app with an allowed usage",
            restricted.shouldCapture(uid = 1_000, usage = AudioAttributes.USAGE_MEDIA),
        )
        assertFalse(
            "Usage gate applies even for an allowed UID",
            restricted.shouldCapture(uid = 1_000, usage = AudioAttributes.USAGE_ALARM),
        )
    }

    @Test
    fun captureUsageConstantsMatchTheSdk() {
        // The pure constants must track android.media.AudioAttributes exactly
        // (compile-time values; inlined, no runtime framework call).
        assertEquals(AudioAttributes.USAGE_MEDIA, CaptureUsage.USAGE_MEDIA)
        assertEquals(AudioAttributes.USAGE_GAME, CaptureUsage.USAGE_GAME)
        assertEquals(AudioAttributes.USAGE_UNKNOWN, CaptureUsage.USAGE_UNKNOWN)
    }
}
