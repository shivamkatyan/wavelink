package dev.wavelink.app.emitter

/**
 * Loads the Rust bridge engine (`libwdr_bridge.so`) when it is present.
 *
 * WS-G (BRIDGE_PLAN step 1): the app stays fully functional without the
 * library — host `assembleDebug` builds ship no `.so` (no NDK downloaded), and
 * the engine-backed sink replaces the `FixtureFrameSink` seam only once the
 * android-ci gate produces the real `.so` for the pinned ABIs. Never crashes
 * when the library is absent.
 *
 * ADR-001/002: this load is control-plane only; the engine's `on_block` is
 * driven from a capture worker thread, never a realtime callback.
 */
object WdrEngineLoader {
    /** Whether the Rust bridge library loaded successfully this run. */
    val loaded: Boolean by lazy {
        try {
            System.loadLibrary("wdr_bridge")
            true
        } catch (_: UnsatisfiedLinkError) {
            false
        }
    }
}
