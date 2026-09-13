package dev.wavelink.app.emitter

import dev.wavelink.app.*

import android.media.AudioFormat
import android.media.AudioRecord
import android.media.projection.MediaProjection
import android.os.Build
import java.util.concurrent.atomic.AtomicBoolean

/** Pure, JVM-testable description of the captured PCM format. */
data class Format(
    val sampleRateHz: Int = 48_000,
    val channelCount: Int = 2,
    val bitDepth: Int = 16,
    /** android.media.AudioFormat.ENCODING_* compile-time constant (2 = PCM 16-bit int). */
    val encoding: Int = ENCODING_PCM_16BIT,
) {
    fun describe(): String = "$bitDepth-bit ${sampleRateHz}Hz ${channelCount}ch"

    /** Little-endian bytes per frame (one sample per channel). */
    val frameSizeBytes: Int
        get() = channelCount * bitDepth / 8

    companion object {
        // Verified values from android.media.AudioFormat (compile-time inlined).
        const val ENCODING_PCM_16BIT = 2
        const val ENCODING_PCM_FLOAT = 4
    }
}

/** An active capture handle returned by an adapter; stop() tears the source down. */
interface SourceStarted {
    val format: Format
    fun stop()
}

/** Pure metadata for one delivered PCM block. */
data class FrameMeta(
    val frameIndex: Long,
    val captureTimeNanos: Long,
)

/** Frame listener a consumer registers to receive captured PCM (integration point). */
fun interface FramesAvailable {
    fun onFrames(data: ByteArray, meta: FrameMeta)
}

/**
 * Platform seam for app-audio capture — the Android instance of the shared
 * core's `CaptureSource` adapter boundary (ARCHITECTURE.md). A concrete adapter
 * turns a granted per-session [MediaProjection] + [CapturePolicy] into an
 * [AudioRecord] fed by an [AudioPlaybackCaptureConfiguration] (built by
 * [PlaybackCaptureConfigFactory]) running on a dedicated capture thread.
 *
 * PLATFORM_MATRIX facts encoded in this class (honest):
 *  - Consent is RE-ASKED EVERY session from a visible activity (MainActivity),
 *    never from the background service.
 *  - Single-use token: on API 34+ the projection token is single-use — once the
 *    source is stopped (or the OS reclaims the projection) it cannot be
 *    restarted; the next session re-requests consent. On API <= 33 a projection
 *    *could* be reused, but this shell deliberately re-requests each session
 *    for honest per-session consent (the caller owns that; see API branches in
 *    EmitService/MainActivity).
 *  - Protected content and non-opted apps are SILENCED by the OS — this
 *    adapter never sees that audio, and there is no public API to override it.
 *  - The capture thread only copies PCM out (mirrors the RT-copy discipline);
 *    encode/encrypt/transport live downstream of [FramesAvailable].
 */
class MediaProjectionCaptureAdapter(
    private val projection: MediaProjection,
    private val policy: CapturePolicy,
    private val requestedFormat: Format = Format(),
) {

    private val active = AtomicBoolean(false)
    private var record: AudioRecord? = null
    private var thread: Thread? = null
    private var frameIndex: Long = 0L

    /**
     * Validate the allow-list, build the OS configuration, and start reading on
     * a dedicated thread. Returns a [SourceStarted] handle, or null when the
     * policy allows nothing (fails closed — nothing to capture).
     */
    fun start(onFrames: FramesAvailable): SourceStarted? {
        val config = PlaybackCaptureConfigFactory.build(projection, policy) ?: return null

        val channelMask =
            if (requestedFormat.channelCount >= 2) AudioFormat.CHANNEL_IN_STEREO else AudioFormat.CHANNEL_IN_MONO
        val minBuf = AudioRecord.getMinBufferSize(requestedFormat.sampleRateHz, channelMask, requestedFormat.encoding)
        val bufBytes = if (minBuf > 0) minBuf * 2 else 16 * 1024

        val recorder = AudioRecord.Builder()
            .setAudioPlaybackCaptureConfig(config)
            .setAudioFormat(
                AudioFormat.Builder()
                    .setSampleRate(requestedFormat.sampleRateHz)
                    .setChannelMask(channelMask)
                    .setEncoding(requestedFormat.encoding)
                    .build(),
            )
            .build()

        val buffer = ByteArray(bufBytes)
        active.set(true)
        record = recorder
        var startedOk = false
        try {
            recorder.startRecording()
            startedOk = true
        } catch (e: Exception) {
            active.set(false)
            runCatching { recorder.release() }
            record = null
            throw e
        }

        if (startedOk) {
            val t = Thread {
                while (active.get()) {
                    val n = runCatching { recorder.read(buffer, 0, buffer.size) }.getOrDefault(-1)
                    if (n > 0) {
                        val pcm = if (n == buffer.size) buffer else buffer.copyOf(n)
                        onFrames.onFrames(pcm, FrameMeta(frameIndex, System.nanoTime()))
                        frameIndex += n / requestedFormat.frameSizeBytes
                    }
                }
            }
            t.name = "wdr-capture"
            t.start()
            thread = t
        } else {
            runCatching { recorder.release() }
            record = null
            return null
        }

        return object : SourceStarted {
            override val format = requestedFormat
            override fun stop() {
                this@MediaProjectionCaptureAdapter.stop()
            }
        }
    }

    /**
     * Stop the read loop and release the recorder. NOTE: on API 34+ this
     * invalidates the single-use projection token; the next session must
     * re-request consent (the projection's MediaProjection.Callback onStop is
     * owned by EmitService).
     */
    private fun stop() {
        if (!active.getAndSet(false)) return
        thread?.join(500)
        runCatching { record?.stop() }
        runCatching { record?.release() }
        record = null
        thread = null
    }
}
