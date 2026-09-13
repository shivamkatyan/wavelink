package dev.wavelink.app

import dev.wavelink.app.emitter.FixtureFrameSink
import dev.wavelink.app.emitter.FrameMeta
import dev.wavelink.app.emitter.Format
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * JVM tests for the emitter frame seam (WS4 seam-level wiring). The same
 * console: onFormat once → onBlock whole frames → finish, mirroring the shared
 * core's wdr_refsim::sink::FrameSink, and the lossless "bytes-out == bytes-in"
 * CRC readout the Rust hash sink provides.
 */
class SinkSeamTest {

    private val fmt = Format(sampleRateHz = 48_000, channelCount = 2, bitDepth = 16)

    /** Deterministic canonical fixture: interleaved L-R i16 LE bytes. */
    private fun fixtureBytes(frames: Int, seed: Int = 1): ByteArray {
        val b = ByteArray(frames * fmt.frameSizeBytes)
        for (f in 0 until frames) {
            val l = ((seed + f) * 564).toShort()   // left  = pattern A
            val r = ((seed + f) * -904).toShort()  // right = pattern B
            b[f * 4 + 0] = (l.toInt() and 0xFF).toByte()
            b[f * 4 + 1] = ((l.toInt() shr 8) and 0xFF).toByte()
            b[f * 4 + 2] = (r.toInt() and 0xFF).toByte()
            b[f * 4 + 3] = ((r.toInt() shr 8) and 0xFF).toByte()
        }
        return b
    }

    @Test
    fun onFormatThenBlocks_sumsWholeFrames() {
        val sink = FixtureFrameSink(fmt)
        sink.onFormat(fmt)
        assertTrue(sink.formatAnnounced)
        assertEquals(0L, sink.samplesAccepted)

        sink.onBlock(fixtureBytes(100), FrameMeta(0, 0))
        sink.onBlock(fixtureBytes(250), FrameMeta(1, 0))
        sink.finish()

        assertEquals(2L, sink.blocksReceived)
        // 350 frames * 2 ch = 700 16-bit samples = 1400 canonical bytes.
        assertEquals(700L, sink.samplesAccepted)
        assertEquals(1400L, sink.bytesAccepted)
        assertTrue(sink.isFinished)
        // Deterministic integrity: recompute the same CRC32 independently.
        val expected = java.util.zip.CRC32().apply {
            update(fixtureBytes(100)); update(fixtureBytes(250))
        }.value.toUInt().toString(16)
        assertEquals(expected, sink.contentCrcHex())
    }

    @Test
    fun truncatedTailIsCountedWholeOnly_neverPartiallyEncoded() {
        val sink = FixtureFrameSink(fmt)
        sink.onFormat(fmt)
        // 99.5 frames -> only the whole 99 frames enter the pipeline.
        val truncated = fixtureBytes(99) + ByteArray(2)
        sink.onBlock(truncated, FrameMeta(0, 0))
        sink.finish()
        assertEquals(99L * 2, sink.samplesAccepted)
        assertEquals(99L * fmt.frameSizeBytes, sink.bytesAccepted)
    }

    @Test
    fun finishThenOnBlock_isContractViolation() {
        val sink = FixtureFrameSink(fmt)
        sink.onFormat(fmt)
        sink.finish()
        assertThrows(IllegalStateException::class.java) {
            sink.onBlock(fixtureBytes(10), FrameMeta(0, 0))
        }
    }

    @Test
    fun formatMismatch_isRejectedTyped_notSilent() {
        val sink = FixtureFrameSink(fmt)
        assertThrows(IllegalArgumentException::class.java) {
            sink.onFormat(Format(sampleRateHz = 44_100, channelCount = 2, bitDepth = 16))
        }
    }
}
