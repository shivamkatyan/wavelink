package dev.wavelink.app

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Test

/**
 * Determinism contract for [FakeSoundSource] — the pre-device demo / soak path
 * needs byte-for-byte reproducible PCM. Pure JVM, no Android.
 */
class FakeSoundSourceTest {

    @Test
    fun sameSeedProducesIdenticalBlocks() {
        val a = FakeSoundSource(48_000, 16, 2, seed = 7L)
        val b = FakeSoundSource(48_000, 16, 2, seed = 7L)
        assertArrayEquals("same seed + same call order must be identical", a.nextBlock(480), b.nextBlock(480))
        assertArrayEquals("... for a second block too", a.nextBlock(480), b.nextBlock(480))
    }

    @Test
    fun differentSeedsProduceDifferentBlocks() {
        val a = FakeSoundSource(48_000, 16, 2, seed = 1L)
        val b = FakeSoundSource(48_000, 16, 2, seed = 2L)
        assertNotEquals("different seed must diverge", a.nextBlock(480).toList(), b.nextBlock(480).toList())
    }

    @Test
    fun blockSizeMatchesFormat() {
        val src = FakeSoundSource(48_000, 16, 2)
        assertEquals(480 * 2 * 2, src.nextBlock(480).size) // frames * channels * 2 bytes
        assertEquals(480, src.framesProduced())

        val mono24 = FakeSoundSource(48_000, 24, 1)
        assertEquals(480 * 1 * 3, mono24.nextBlock(480).size) // frames * 1 * 3 bytes
    }

    @Test
    fun producedSequenceIsContinuous() {
        val src = FakeSoundSource(48_000, 16, 2, seed = 99L)
        val first = src.nextBlock(10)
        val second = src.nextBlock(10)
        assertEquals(20, src.framesProduced())
        assertNotEquals("consecutive blocks must not be identical", first.toList(), second.toList())
    }
}
