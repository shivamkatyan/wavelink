package dev.wavelink.app

/**
 * Live status / diagnostics panel dataclass mirroring FR-053 (emitter shell).
 * Pure JVM; populated by the capture service / adapter for UI + telemetry.
 *
 * Fields: state, peer, transport, codec, sample rate, bit depth, channels,
 * estimated end-to-end latency, buffer fill, packet loss, underruns, capture
 * route, fidelity state.
 */
data class StatusModel(
    /** Session state machine value (IDLE/CONNECTING/PAIRING/STREAMING/PAUSED/ERROR...). */
    val state: String = "IDLE",

    /** Peer identity (pairing id / hardware id). */
    val peer: String = "",

    /** Transport label ("wi-fi", "bluetooth"...). */
    val transport: String = "wi-fi",

    /** Codec in use ("opus", "flac", "pcm"...). */
    val codec: String = "",

    /** Sample rate in Hz (44.1k/48k first; higher supported by codec/OS/DAC). */
    val sampleRateHz: Int = 48_000,

    /** Bit depth (16 or 24 for integer lossless). */
    val bitDepth: Int = 16,

    /** Channel count (stereo first). */
    val channels: Int = 2,

    /** Estimated end-to-end capture-to-render latency, ms. */
    val latencyMs: Int = 0,

    /** Jitter-buffer fill as a fraction of capacity 0f..1f. */
    val bufferFill: Float = 0f,

    /** Receiver-observed frame/packet loss, percentage 0f..100f. */
    val packetLossPct: Float = 0f,

    /** Underrun count since session start. */
    val underruns: Long = 0,

    /** Capture/output route ("app-audio-capture" / "system-loopback" / "none"). */
    val route: String = "none",

    /**
     * Fidelity state per FR-022/FR-024. Values: "lossless" (decoded PCM
     * matches agreed encoded PCM), "lossy", "bit-perfect" (only when the full
     * digital path is hardware-verified), or "unknown".
     */
    val fidelity: String = "unknown",
) {

    /**
     * FR-053 + FR-056: a single, always-spoken status line for TalkBack and
     * dynamic-text UIs. Every state is carried by a label + word (never colour
     * alone). Deliberately EXCLUDES [peer] / any id / secret / audio content
     * (FR-055) so it is safe to read aloud or log.
     */
    fun statusLine(): String = listOf(
        "Session ${state.lowercase()}",
        "fidelity ${fidelity.ifBlank { "unknown" }}",
        "codec ${codec.ifBlank { "not set" }}",
        "$sampleRateHz hertz",
        "$bitDepth bit",
        "$channels channel",
        "transport ${transport.ifBlank { "none" }}",
        "route ${route.ifBlank { "none" }}",
        "latency $latencyMs milliseconds",
        "buffer ${(bufferFill * 100).toInt()} percent",
        "packet loss $packetLossPct percent",
        "underruns $underruns",
    ).joinToString(", ")

    /**
     * FR-055 redaction: a compact, loggable description with NO peer identity,
     * NO secrets, NO audio content — only session/format/fidelity fields.
     */
    fun redactedDescription(): String =
        "state=$state fidelity=${fidelity.ifBlank { "unknown" }} " +
            "codec=${codec.ifBlank { "-" }} rate=$sampleRateHz depth=$bitDepth " +
            "ch=$channels loss=$packetLossPct% underruns=$underruns"

    /**
     * FR-056 non-color a11y indicator (shape/icon + spoken word): combined with
     * [statusLine]'s words nothing is ever conveyed by colour alone, and the
     * same shape word reaches TalkBack.
     */
    fun visualIndicator(): VisualIndicator = VisualIndicator.forState(state)
}

/**
 * Shape/icon + spoken-label pair so session status is never colour-only (FR-056,
 * PRODUCT_SPEC: "shape/icon + label + colour triad"; colour is redundant here).
 */
enum class VisualIndicator(val shape: String, val spoken: String) {
    IDLE("circle-outline", "Idle"),
    ACTIVE("pulsing-dot", "Streaming"),
    PAUSED("two-bars", "Paused"),
    ERROR("triangle", "Error"),
    UNKNOWN("question-mark", "Unknown status");

    companion object {
        fun forState(state: String): VisualIndicator = when {
            state.equals("IDLE", ignoreCase = true) -> IDLE
            state.contains("ERROR", ignoreCase = true) -> ERROR
            state.contains("PAUS", ignoreCase = true) -> PAUSED
            state.contains("STREAM", ignoreCase = true) ||
                state.contains("CAPTUR", ignoreCase = true) ||
                state.contains("CONNECT", ignoreCase = true) ||
                state.contains("PAIR", ignoreCase = true) -> ACTIVE
            else -> UNKNOWN
        }
    }
}
