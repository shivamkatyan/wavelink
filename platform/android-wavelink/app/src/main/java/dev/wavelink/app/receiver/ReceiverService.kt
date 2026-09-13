package dev.wavelink.app.receiver

import dev.wavelink.app.*

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioTrack
import android.media.session.MediaSession
import android.os.Build
import android.os.IBinder

/**
 * Foreground receiver service (FR-016): hosts the AudioTrack player and a
 * framework [MediaSession] so the receiver keeps rendering in the background
 * and on a locked screen. Sketch-level but API-correct:
 *
 *  - startForeground with type mediaPlayback (always-on type on API 34+);
 *  - on route change (USB DAC attach/detach) the track is re-created and
 *    re-routed via [AudioOutputRouter] — never dropped with a crash (FR-015);
 *  - BIT_PERFECT (media-over-USB) is gated by SDK_INT >= 34 per PLATFORM_MATRIX.
 */
class ReceiverService : Service() {

    private lateinit var outputRouter: AudioOutputRouter
    private lateinit var mediaSession: MediaSession
    private var player: Player? = null
    private var hotplugHandle: AutoCloseable? = null

    override fun onCreate() {
        super.onCreate()
        outputRouter = AudioOutputRouter(this)
        mediaSession = MediaSession(this, "wdr-receiver")
        createNotificationChannel()
        rebuildPlayer()
        hotplugHandle = outputRouter.registerHotplug(::onRouteChange)
    }

    /** FGS started from e.g. the receiver UI after pairing; once started it may outlive it. */
    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        startInForeground()
        ReceiverService.serviceRunning = true
        ReceiverService.serviceListener?.invoke(true)
        return START_STICKY
    }

    private fun startInForeground() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(
                NOTIFICATION_ID,
                buildNotification(),
                ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK,
            )
        } else {
            startForeground(NOTIFICATION_ID, buildNotification())
        }
    }

    /**
     * Create a fresh AudioTrack, attach it to the router, and route it to the
     * best device (USB DAC preferred — FR-013/FR-015). Called on boot and on
     * detach so a hot-unplug never leaves a dead track behind.
     */
    private fun rebuildPlayer() {
        player?.release()
        val track = newTrack()
        outputRouter.attachPlayerTrack(track)
        val chosen = choosePreferredDevice(
            outputRouter.outputDevices(),
            player?.routedDeviceId,
        )
        if (chosen != null) {
            outputRouter.routeTo(chosen)
        }
        player = Player(track, outputRouter)
    }

    private fun newTrack(): AudioTrack {
        val sampleRate = 48000
        val channelMask = AudioFormat.CHANNEL_OUT_STEREO
        val encoding = AudioFormat.ENCODING_PCM_16BIT
        val minBuf = AudioTrack.getMinBufferSize(sampleRate, channelMask, encoding)
        val bufferSizeInBytes = if (minBuf > 0) minBuf * 2 else 1 shl 16
        return AudioTrack.Builder()
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_MEDIA)
                    .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                    .build(),
            )
            .setAudioFormat(
                AudioFormat.Builder()
                    .setSampleRate(sampleRate)
                    .setEncoding(encoding)
                    .setChannelMask(channelMask)
                    .build(),
            )
            .setTransferMode(AudioTrack.MODE_STREAM)
            .setBufferSizeInBytes(bufferSizeInBytes)
            .build()
    }

    /**
     * Route-change handling (FR-015): switching devices while streaming must
     * not crash the track — the player and route are re-created instead.
     */
    private fun onRouteChange(change: RouteChange) {
        when (change) {
            RouteChange.ATTACHED -> {
                val chosen =
                    choosePreferredDevice(outputRouter.outputDevices(), player?.routedDeviceId)
                if (chosen != null) outputRouter.routeTo(chosen)
            }
            RouteChange.DETACHED -> {
                rebuildPlayer()
                val chosen = choosePreferredDevice(outputRouter.outputDevices(), null)
                if (chosen != null) outputRouter.routeTo(chosen)
            }
            RouteChange.NO_ROUTE -> player?.pause()
        }
    }

    /** Not wired end-to-end yet: transport layer (network frames) is a later task. */
    @Suppress("UNUSED_PARAMETER")
    private fun onFrames(frames: ByteArray) {
        player?.write(frames)
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
            .setSmallIcon(android.R.drawable.ic_media_play)
            .setContentTitle("Wavelink")
            .setContentText("Receiver active")
            .setOngoing(true)
            .build()

    override fun onDestroy() {
        ReceiverService.serviceRunning = false
        ReceiverService.serviceListener?.invoke(false)
        hotplugHandle?.close()
        player?.release()
        mediaSession.release()
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    companion object {
        /** Whether the foreground receiver service is currently running (UI state). */
        @Volatile
        @JvmField
        var serviceRunning: Boolean = false

        /** Live push to the Activity (service thread → observer posts to main). */
        @Volatile
        @JvmField
        var serviceListener: ((Boolean) -> Unit)? = null

        private const val CHANNEL_ID = "wdr_receiver_channel"
        private const val NOTIFICATION_ID = 1

        fun start(context: Context) {
            context.startForegroundService(Intent(context, ReceiverService::class.java))
        }
    }
}

/**
 * Minimal sketch of the player owned by the service: owns the [AudioTrack] and
 * forwards decoded PCM. A full implementation (audiocore interaction,
 * adaptive buffers FR-023) is the next receiver task; the bits that matter for
 * FR-016 (background playback) and FR-015 (route-aware track re-creation) live
 * in ReceiverService.
 */
internal class Player(
    private var track: AudioTrack,
    private val router: AudioOutputRouter,
) {

    var routedDeviceId: Int? = null
        private set
    var isPlaying: Boolean = false
        private set

    fun write(data: ByteArray) {
        if (!isPlaying) {
            play()
        }
        track.write(data, 0, data.size, AudioTrack.WRITE_BLOCKING)
    }

    fun play() {
        if (!isPlaying && track.playState == AudioTrack.PLAYSTATE_STOPPED) {
            track.play()
            isPlaying = true
        }
    }

    fun pause() {
        if (isPlaying) {
            track.pause()
            isPlaying = false
        }
    }

    fun release() {
        track.release()
        router.detachPlayerTrack()
    }
}
