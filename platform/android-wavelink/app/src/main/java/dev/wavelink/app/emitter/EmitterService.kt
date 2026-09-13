package dev.wavelink.app.emitter

import dev.wavelink.app.*

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.media.projection.MediaProjection
import android.media.projection.MediaProjectionManager
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper

/**
 * Foreground emitter/capture service (FR-011 / FR-016): hosts the app-audio
 * capture adapter as a mediaProjection FGS. PLATFORM_MATRIX Android row facts
 * encoded here (honest):
 *
 *  - MediaProjection consent is RE-ASKED every session; the consent prompt is
 *    launched from a visible activity (MainActivity), never from this service.
 *  - On API 34+ the projection token is single-use and this service MUST run
 *    with `foregroundServiceType="mediaProjection"` (declared in the manifest;
 *    the FOREGROUND_SERVICE_MEDIA_PROJECTION permission is enforced from
 *    API 34). On API 29..33 the type is informational — we start with
 *    FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK instead (no mediaProjection type
 *    existed before 34).
 *  - Frame transport to the emitter core is NOT implemented here: [FrameSink]
 *    is the integration seam the core-wiring task connects (currently null —
 *    blocks are only counted for FR-053 status).
 */
class EmitterService : Service() {

    private val mainHandler = Handler(Looper.getMainLooper())

    /** Free/Pro gate persisted in SharedPreferences via [SharedPrefsPolicyStore]. */
    private val policyGate by lazy { PolicyGate(SharedPrefsPolicyStore(this)) }

    private var capture: SourceStarted? = null
    private var projection: MediaProjection? = null
    private var projectionCallback: MediaProjection.Callback? = null
    private var sessionStartNanos: Long = 0L
    private var blocksCaptured: Long = 0L

    @Volatile
    private var lastStatus: StatusModel = StatusModel()

    /**
     * INTEGRATION POINT for the core wiring task. The shared Rust core's
     * emitter path (encode -> AEAD -> QUIC datagram/stream) will be reached
     * here via a JNI/binder seam; this shell leaves it null and only counts
     * blocks for FR-053 status. Network transport is deliberately OUT of scope.
     */
    private val frameSink: FrameSink? = null

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_START_CAPTURE -> {
                startInForeground()
                handleStartCapture(intent)
            }
            ACTION_STOP_CAPTURE -> {
                stopCapture()
                stopForeground(STOP_FOREGROUND_REMOVE)
                stopSelf()
            }
            else -> {
                // Sticky restart or plain start: keep the foreground service up,
                // waiting for an explicit start/stop action.
                startInForeground()
            }
        }
        return START_STICKY
    }

    /**
     * Recreate the projection from the consent result and start the adapter.
     * Per PLATFORM_MATRIX, on API 34+ [MediaProjectionManager#getMediaProjection]
     * consumes the single-use token — one grant == one session. On API <= 33 we
     * still let the activity re-request every session for honest consent.
     */
    private fun handleStartCapture(intent: Intent) {
        if (capture != null) return // already capturing this session

        val projectionManager = getSystemService(MEDIA_PROJECTION_SERVICE) as MediaProjectionManager
        val resultCode = intent.getIntExtra(EXTRA_RESULT_CODE, 0)
        val data: Intent? = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            intent.getParcelableExtra(EXTRA_DATA, Intent::class.java)
        } else {
            @Suppress("DEPRECATION")
            intent.getParcelableExtra(EXTRA_DATA)
        }
        if (data == null) {
            updateStatus(lastStatus.copy(state = "ERROR", route = "missing-projection-data"))
            return
        }
        val proj = try {
            projectionManager.getMediaProjection(resultCode, data)
        } catch (e: Exception) {
            updateStatus(lastStatus.copy(state = "ERROR", route = "projection-denied:${e.javaClass.simpleName}"))
            return
        } ?: run {
            updateStatus(lastStatus.copy(state = "ERROR", route = "projection-null"))
            return
        }

        // OS may reclaim the projection (capture indicator / policy) at any
        // time; stop capture cleanly instead of reading stale PCM.
        val callback = object : MediaProjection.Callback() {
            override fun onStop() {
                mainHandler.post { stopCapture() }
            }
        }
        proj.registerCallback(callback, mainHandler)
        projection = proj
        projectionCallback = callback

        val capPolicy = CapturePolicy(
            allowMedia = intent.getBooleanExtra(EXTRA_ALLOW_MEDIA, true),
            allowGame = intent.getBooleanExtra(EXTRA_ALLOW_GAME, true),
            allowUnknown = intent.getBooleanExtra(EXTRA_ALLOW_UNKNOWN, true),
        )
        val adapter = MediaProjectionCaptureAdapter(proj, capPolicy)
        sessionStartNanos = System.nanoTime()
        blocksCaptured = 0
        capture = adapter.start { pcm, meta ->
            blocksCaptured++
            // INTEGRATION POINT: frameSink (null here) forwards PCM to the
            // emitter core. Until wired, blocks are only counted for status.
            frameSink?.onBlock(pcm, meta)
            if (blocksCaptured % STATUS_EVERY_BLOCKS == 1L) refreshStatus()
        }
        if (capture == null) {
            updateStatus(lastStatus.copy(state = "ERROR", route = "empty-capture-policy"))
        } else {
            updateStatus(
                lastStatus.copy(
                    state = "STREAMING",
                    transport = "wi-fi",
                    codec = "pcm",
                    sampleRateHz = capture!!.format.sampleRateHz,
                    bitDepth = capture!!.format.bitDepth,
                    channels = capture!!.format.channelCount,
                    route = "app-audio-capture",
                    fidelity = if (policyGate.allowLossless()) "lossless" else "lossy",
                ),
            )
        }
    }

    private fun refreshStatus() {
        val fmt = capture?.format ?: return
        val elapsedMs = (System.nanoTime() - sessionStartNanos) / 1_000_000L
        updateStatus(
            lastStatus.copy(
                state = "STREAMING",
                sampleRateHz = fmt.sampleRateHz,
                bitDepth = fmt.bitDepth,
                channels = fmt.channelCount,
                latencyMs = elapsedMs.toInt(),
                route = "app-audio-capture",
                fidelity = if (policyGate.allowLossless()) "lossless" else "lossy",
            ),
        )
    }

    private fun updateStatus(updated: StatusModel) {
        lastStatus = updated
        EmitterService.displayStatus = updated
        // Live UI push (replace the onResume poll): the callback posts to the
        // main looper so the Activity can render without polling.
        EmitterService.displayStatusListener?.invoke(updated)
    }

    private fun stopCapture() {
        runCatching { capture?.stop() }
        capture = null
        projectionCallback?.let { cb -> projection?.unregisterCallback(cb) }
        projectionCallback = null
        projection?.let { p ->
            // API 34+: token is single-use anyway (spent once obtained/stopped);
            // API <= 33: we still discard it by policy to force per-session
            // consent on every (re)start.
            runCatching { p.stop() }
        }
        projection = null
        updateStatus(lastStatus.copy(state = "IDLE", route = "none", fidelity = "unknown"))
    }

    private fun startInForeground() {
        val notification = buildNotification()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE /* 34 */) {
            startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PROJECTION)
        } else {
            // Pre-34: no mediaProjection FGS type existed; mediaPlayback is the
            // closest always-on audio type permitted and keeps the capture path
            // in the foreground on API 29..33.
            startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK)
        }
    }

    private fun createNotificationChannel() {
        val channel = NotificationChannel(
            CHANNEL_ID,
            "Wavelink",
            NotificationManager.IMPORTANCE_LOW,
        )
        getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
    }

    private fun buildNotification(): Notification =
        Notification.Builder(this, CHANNEL_ID)
            .setSmallIcon(android.R.drawable.ic_btn_speak_now)
            .setContentTitle("Wavelink")
            .setContentText("Emitting app audio")
            .setOngoing(true)
            .build()

    override fun onDestroy() {
        stopCapture()
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    companion object {
        private const val CHANNEL_ID = "wdr_emitter_channel"
        private const val NOTIFICATION_ID = 2
        private const val STATUS_EVERY_BLOCKS = 50L

        const val ACTION_START_CAPTURE = "dev.wavelink.app.action.START_CAPTURE"
        const val ACTION_STOP_CAPTURE = "dev.wavelink.app.action.STOP_CAPTURE"
        const val EXTRA_RESULT_CODE = "dev.wavelink.app.extra.RESULT_CODE"
        const val EXTRA_DATA = "dev.wavelink.app.extra.DATA"
        const val EXTRA_ALLOW_MEDIA = "dev.wavelink.app.extra.ALLOW_MEDIA"
        const val EXTRA_ALLOW_GAME = "dev.wavelink.app.extra.ALLOW_GAME"
        const val EXTRA_ALLOW_UNKNOWN = "dev.wavelink.app.extra.ALLOW_UNKNOWN"

        /**
         * Latest FR-053 status, shared with the UI shell (framework-only: no
         * binder yet — a proper bound-interface is a follow-up).
         */
        @Volatile
        @JvmField
        var displayStatus: StatusModel = StatusModel()

        /**
         * Live FR-053 push: the Activity sets this (on resume) and clears it (on
         * pause). Called on the service thread — the observer must post to the
         * main looper itself.
         */
        @Volatile
        @JvmField
        var displayStatusListener: ((StatusModel) -> Unit)? = null

        /** Start capture from the consent result (extra Intent from the system prompt). */
        fun startCapture(context: Context, data: Intent, resultCode: Int, policy: CapturePolicy) {
            val i = Intent(context, EmitterService::class.java).apply {
                action = ACTION_START_CAPTURE
                putExtra(EXTRA_RESULT_CODE, resultCode)
                putExtra(EXTRA_DATA, data)
                putExtra(EXTRA_ALLOW_MEDIA, policy.allowMedia)
                putExtra(EXTRA_ALLOW_GAME, policy.allowGame)
                putExtra(EXTRA_ALLOW_UNKNOWN, policy.allowUnknown)
            }
            context.startForegroundService(i)
        }

        fun stopCapture(context: Context) {
            context.startService(Intent(context, EmitterService::class.java).apply { action = ACTION_STOP_CAPTURE })
        }
    }
}

/**
 * Frame-transport seam for the emitter core wiring task:
 *
 *   onFormat  — capture format is stable for the session (once, at start).
 *   onBlock   — one captured PCM block (from FramesAvailable).
 *
 * The core (encode -> AEAD -> QUIC datagram/stream) consumes blocks via a JNI/
 * binder seam that the integration task adds. Deliberately no network code here.
 */
interface FrameSink {
    fun onFormat(format: Format)
    fun onBlock(data: ByteArray, meta: FrameMeta)
}
