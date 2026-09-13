package dev.wavelink.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * FR-053 field surface + FR-055 redaction + FR-056 non-color a11y indicator.
 * Pure JVM, no Android.
 */
class StatusModelTest {

    @Test
    fun statusLineCoversEveryFieldButPeer() {
        val s = StatusModel(
            state = "STREAMING",
            peer = "device-abc-123",
            codec = "pcm",
            sampleRateHz = 48_000,
            bitDepth = 16,
            channels = 2,
            latencyMs = 25,
            bufferFill = 0.5f,
            packetLossPct = 0.2f,
            underruns = 3,
            route = "app-audio-capture",
            fidelity = "lossless",
        )
        val line = s.statusLine()
        assertTrue("state spoken", line.contains("streaming"))
        assertTrue("fidelity spoken", line.contains("lossless"))
        assertTrue("codec spoken", line.contains("pcm"))
        assertTrue("rate spoken", line.contains("48000 hertz"))
        assertTrue("depth spoken", line.contains("16 bit"))
        assertTrue("channels spoken", line.contains("2 channel"))
        assertTrue("latency spoken", line.contains("25 milliseconds"))
        assertTrue("buffer spoken", line.contains("buffer 50 percent"))
        assertTrue("loss spoken", line.contains("0.2 percent"))
        assertTrue("underruns spoken", line.contains("underruns 3"))
        assertTrue("transport spoken", line.contains("wi-fi"))
        assertTrue("route spoken", line.contains("app-audio-capture"))

        assertFalse("FR-055: status line must NEVER expose the peer id", line.contains("device-abc-123"))
        assertFalse("FR-055: status line must never mention the peer at all", line.contains("peer"))
    }

    @Test
    fun redactedDescriptionOmitsPeerAndContainsDiagnostics() {
        val s = StatusModel(
            state = "STREAMING",
            peer = "peer-secret-42",
            codec = "pcm",
            sampleRateHz = 44_100,
            bitDepth = 24,
            packetLossPct = 0.5f,
            underruns = 7,
        )
        val d = s.redactedDescription()
        assertTrue(d.contains("state=STREAMING"))
        assertTrue(d.contains("codec=pcm"))
        assertTrue(d.contains("rate=44100"))
        assertTrue(d.contains("depth=24"))
        assertTrue(d.contains("loss=0.5%"))
        assertTrue(d.contains("underruns=7"))

        assertFalse("FR-055: redacted description must omit the peer identity", d.contains("peer-secret-42"))
        assertFalse("FR-055: redacted description must omit any peer= field", d.contains("peer="))
    }

    @Test
    fun peerNeverAppearsInAnyOutput() {
        val withPeer = StatusModel(state = "STREAMING", peer = "x-via-garbage-9", codec = "pcm")
        val without = StatusModel(state = "STREAMING", codec = "pcm")
        assertEquals("Peer must not influence the status line", withPeer.statusLine(), without.statusLine())
        assertEquals(
            "Peer must not influence the redacted description",
            withPeer.redactedDescription(),
            without.redactedDescription(),
        )
    }

    @Test
    fun visualIndicatorProvidesTextualNonColorStatus() {
        assertEquals(VisualIndicator.ACTIVE, StatusModel(state = "STREAMING").visualIndicator())
        assertEquals(VisualIndicator.ACTIVE, StatusModel(state = "CONNECTING").visualIndicator())
        assertEquals(VisualIndicator.ERROR, StatusModel(state = "ERROR").visualIndicator())
        assertEquals(VisualIndicator.PAUSED, StatusModel(state = "PAUSED").visualIndicator())
        assertEquals(VisualIndicator.IDLE, StatusModel(state = "IDLE").visualIndicator())
        assertEquals(VisualIndicator.UNKNOWN, StatusModel(state = "SOMETHING_ELS").visualIndicator())

        // Every indicator pairs a shape/icon with a spoken word (FR-056).
        for (v in VisualIndicator.entries) {
            assertFalse("shape must be set", v.shape.isBlank())
            assertFalse("spoken label must be set (non-color)", v.spoken.isBlank())
        }
    }
}
