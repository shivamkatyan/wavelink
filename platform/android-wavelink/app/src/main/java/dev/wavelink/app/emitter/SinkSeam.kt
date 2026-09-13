package dev.wavelink.app.emitter

import java.util.zip.CRC32

/**
 * Frame-transport seam for the emitter core wiring task — the Kotlin analogue
 * of the shared core's `wdr_refsim::sink::FrameSink`:
 *
 *   onFormat — capture format is stable for the session (called once, at start).
 *   onBlock  — one captured PCM block (interleaved little-endian i16 bytes).
 *   finish   — end of session; the sink commits/flushes (mirrors the Rust
 *
 * The core (encode -> AEAD -> QUIC datagram/stream) is reached via a JNI/binder
 * seam added by the transport integration task; this interface is what that
 * task implements. Deliberately no network/FFI code here — JVM-testable.
 */
interface FrameSink {
    /** Format is stable and delivered once, before the first block. */
    fun onFormat(format: Format)

    /**
     * One captured PCM block (interleaved little-endian i16 bytes, any length).
     * The sink accumulates to whole frames / wraps for the network.
     */
    fun onBlock(data: ByteArray, meta: FrameMeta)

    /** End of session: flush accumulation and commit any end-of-stream marker. */
    fun finish()
}

/**
 * A concrete, host-testable [FrameSink] that the shell uses **today** so the
 * seam is real (not null): it accumulates blocks in whole-frame alignment,
 * counts bytes/samples, and computes a deterministic integrity CRC-32 over the
 * canonical little-endian byte stream — the same "bytes-out must equal
 * bytes-in lossless" readout the Rust core's hash sink provides. When the
 * JNI/binder transport wiring lands, a network sink replaces it behind this
 * exact interface and the tests here stay valid.
 */
class FixtureFrameSink(
    /** Expected capture format; used to size/reject malformed blocks. */
    private val expectedFormat: Format,
) : FrameSink {

    private var formatSeen: Format? = null
    private var byteCount: Long = 0L
    private var blockCount: Long = 0L
    private var finished = false
    private val crc = CRC32()

    /** Bytes accepted since onFormat (whole-frame accumulated). */
    val bytesAccepted: Long get() = byteCount

    /** Blocks delivered via onBlock. */
    val blocksReceived: Long get() = blockCount

    /** Little-endian 16-bit samples accepted since onFormat. */
    val samplesAccepted: Long get() = byteCount / 2

    /** True once format has been announced. */
    val formatAnnounced: Boolean get() = formatSeen != null

    /** True after finish() (a second onBlock must be rejected). */
    val isFinished: Boolean get() = finished

    override fun onFormat(format: Format) {
        require(format == expectedFormat) {
            "FixtureFrameSink format mismatch: expected $expectedFormat, got $format"
        }
        formatSeen = format
    }

    override fun onBlock(data: ByteArray, meta: FrameMeta) {
        check(!finished) { "FrameSink.onBlock after finish() — contract violation" }
        check(formatSeen != null) { "FrameSink.onBlock before onFormat() — contract violation" }
        val frameBytes = expectedFormat.frameSizeBytes
        // Whole-frame invariant: a truncated block tail is not silently
        // encoded (mirrors the Rust seam's "partial tail is dropped/reported").
        val whole = data.size - (data.size % frameBytes)
        if (whole > 0) {
            crc.update(data, 0, whole)
            byteCount += whole
            blockCount++
        }
    }

    override fun finish() {
        finished = true
    }

    /** Deterministic CRC-32 over every accepted canonical byte (lossless readout). */
    fun contentCrcHex(): String = crc.value.toUInt().toString(16)
}
