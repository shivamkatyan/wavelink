package dev.wavelink.app.emitter

import dev.wavelink.app.*

import android.media.AudioPlaybackCaptureConfiguration
import android.media.projection.MediaProjection

/**
 * Capture allow-list policy for the Android emitter (PLATFORM_MATRIX Android
 * row: "AudioPlaybackCaptureConfiguration, opt-in USAGE_MEDIA/GAME/UNKNOWN mix,
 * per-UID/usage filter").
 *
 * Mirrors the receiver's file pattern (pure decision + framework composition in
 * one file): the *decision* is a pure JVM model + functions ([shouldInclude],
 * [uidAllowed], [shouldCapture]) that JUnit exercises with no android.* at
 * runtime; only [PlaybackCaptureConfigFactory] touches the framework to build
 * the [AudioPlaybackCaptureConfiguration] the OS enforces.
 *
 * The allowed usage set is a strict subset of the three opt-in
 * android.media.AudioAttributes usages the OS exposes to playback capture:
 * USAGE_MEDIA, USAGE_GAME, USAGE_UNKNOWN. Every other usage (notifications,
 * ringtone, navigation, assistant, ...) is excluded by [shouldInclude] — and by
 * the OS itself — for all apps.
 */
data class CapturePolicy(
    val allowMedia: Boolean = true,
    val allowGame: Boolean = false,
    val allowUnknown: Boolean = false,
    /** Restrict capture to specific app UIDs; empty set = all capturable apps. */
    val allowUids: Set<Int> = emptySet(),
) {

    /** The allowed usage ints, exactly as [AudioPlaybackCaptureConfiguration#addMatchingUsage] sees them. */
    val allowedUsages: List<Int>
        get() = buildList {
            if (allowMedia) add(CaptureUsage.USAGE_MEDIA)
            if (allowGame) add(CaptureUsage.USAGE_GAME)
            if (allowUnknown) add(CaptureUsage.USAGE_UNKNOWN)
        }

    /** Fails closed: nothing opted in = an empty allow-list and no capture. */
    val isEmpty: Boolean
        get() = allowedUsages.isEmpty()

    /**
     * Pure allow-list check — NO framework dependency: a usage outside the
     * {MEDIA, GAME, UNKNOWN} subset is never included, regardless of opt-in.
     */
    fun shouldInclude(usage: Int): Boolean = usage in allowedUsages

    /** Per-UID filter (FR-011 / PLATFORM_MATRIX "per-UID/usage filter"): empty allow-set = all apps. */
    fun uidAllowed(uid: Int): Boolean = allowUids.isEmpty() || uid in allowUids

    /** Combined per-frame-source decision (uid + usage). Pure. */
    fun shouldCapture(uid: Int, usage: Int): Boolean =
        uidAllowed(uid) && shouldInclude(usage)

    /** Human/UI label of the allow-list ("capture[media+game] (all apps)"). */
    fun describe(): String = buildString {
        append("capture[")
        append(
            buildList {
                if (allowMedia) add("media")
                if (allowGame) add("game")
                if (allowUnknown) add("unknown")
            }.joinToString("+").ifEmpty { "none" },
        )
        append(']')
        if (allowUids.isEmpty()) append(" (all apps)") else append(" (apps ${allowUids.sorted()} only)")
    }
}

/**
 * android.media.AudioAttributes USAGE constants as compile-time ints, so the
 * pure allow-list logic ([CapturePolicy.shouldInclude]) and its JVM tests never
 * touch android.* at runtime. Values verified against android.media.AudioAttributes
 * (the OS allows playback-capture of exactly this subset per PLATFORM_MATRIX).
 */
object CaptureUsage {
    /** android.media.AudioAttributes.USAGE_UNKNOWN = 0 */
    const val USAGE_UNKNOWN = 0

    /** android.media.AudioAttributes.USAGE_MEDIA = 1 */
    const val USAGE_MEDIA = 1

    /** android.media.AudioAttributes.USAGE_GAME = 14 */
    const val USAGE_GAME = 14
}

/**
 * Framework-facing construction — the only Android-touching part of the
 * capture policy. Builds the [AudioPlaybackCaptureConfiguration] the OS
 * enforces from a granted per-session [MediaProjection] + [CapturePolicy].
 *
 * PLATFORM_MATRIX facts encoded (honest):
 *  - One addMatchingUsage(...) per allow-listed opt-in usage.
 *  - addMatchingUid(...) only when the user restricted to specific apps.
 *  - An EMPTY policy returns null — fails closed, nothing is capturable.
 *  - "allowAudioPlaybackCapture" (AudioAttributes#setAllowedCapturePolicy) is
 *    the *captured apps'* opt-in; the OS silences protected / non-opted content
 *    and the emitter has no public API to override that (protected content is
 *    never capturable).
 */
object PlaybackCaptureConfigFactory {

    fun build(
        projection: MediaProjection,
        policy: CapturePolicy,
    ): AudioPlaybackCaptureConfiguration? {
        if (policy.isEmpty) return null
        val builder = AudioPlaybackCaptureConfiguration.Builder(projection)
        policy.allowedUsages.forEach(builder::addMatchingUsage)
        policy.allowUids.forEach(builder::addMatchingUid)
        return builder.build()
    }
}
