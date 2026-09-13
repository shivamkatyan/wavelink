package dev.wavelink.app

import kotlin.random.Random

/**
 * Deterministic synthetic PCM source for JVM tests and the pre-device demo
 * path (mirrors the portable Fake* pattern in the shared Rust core). No
 * android.* dependency: a fixed seed yields the exact same interleaved
 * little-endian byte stream on every run once sample rate / bit depth /
 * channels are fixed, so tests and the soak harness get reproducible "audio"
 * without a real device.
 */
class FakeSoundSource(
    val sampleRateHz: Int = 48_000,
    val bitDepth: Int = 16,
    val channels: Int = 2,
    seed: Long = 0xC0FFEE,
) {

    private val rng = Random(seed)
    private var frameIndex: Long = 0L
    private val baseAmplitude: Double =
        when (bitDepth) {
            24 -> 0.35 * 8_388_607.0
            16 -> 0.35 * 32_767.0
            else -> throw IllegalArgumentException("bitDepth must be 16 or 24, was $bitDepth")
        }

    /** Bytes per frame (one sample per channel, little-endian). */
    val frameBytes: Int
        get() = channels * bitDepth / 8

    /** Total frames produced so far (monotonic; deterministic per sequence). */
    fun framesProduced(): Long = frameIndex

    /**
     * Next interleaved little-endian PCM block of [frameCount] frames. The
     * sequence is deterministic for a fixed seed + call order: a 440 Hz sine
     * fundamental plus a small deterministic "noise" component per sample.
     */
    fun nextBlock(frameCount: Int = 480): ByteArray {
        require(frameCount > 0) { "frameCount must be > 0" }
        val out = ByteArray(frameCount * frameBytes)
        var p = 0
        for (i in 0 until frameCount) {
            val phase = (frameIndex + i) * 2.0 * Math.PI * 440.0 / sampleRateHz
            val tone = Math.sin(phase)
            val jitter = (rng.nextDouble() - 0.5) * 0.05
            val sample = (tone + jitter) * baseAmplitude
            repeat(channels) {
                when (bitDepth) {
                    16 -> writeLe16(out, p, sample.toInt().coerceIn(-32768, 32767))
                    24 -> writeLe24(out, p, sample.toInt().coerceIn(-8_388_608, 8_388_607))
                }
                p += bitDepth / 8
            }
        }
        frameIndex += frameCount
        return out
    }

    /** One spec-style sentence for status/UI ("fake 16-bit @ 48000Hz 2ch (seed-deterministic)"). */
    fun describe(): String = "fake $bitDepth-bit @ ${sampleRateHz}Hz ${channels}ch (seed-deterministic)"

    companion object {
        /** Write one little-endian 16-bit sample. Exposed for the 24-bit helper/file writers. */
        fun writeLe16(b: ByteArray, offset: Int, sample: Int) {
            b[offset] = (sample and 0xFF).toByte()
            b[offset + 1] = ((sample shr 8) and 0xFF).toByte()
        }

        /** Write one little-endian packed 24-bit sample. */
        fun writeLe24(b: ByteArray, offset: Int, sample: Int) {
            b[offset] = (sample and 0xFF).toByte()
            b[offset + 1] = ((sample shr 8) and 0xFF).toByte()
            b[offset + 2] = ((sample shr 16) and 0xFF).toByte()
        }
    }
}
