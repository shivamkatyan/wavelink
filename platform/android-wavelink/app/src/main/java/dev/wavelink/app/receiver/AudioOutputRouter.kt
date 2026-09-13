package dev.wavelink.app.receiver

import dev.wavelink.app.*

import android.content.Context
import android.media.AudioDeviceInfo
import android.media.AudioDeviceCallback
import android.media.AudioManager
import android.media.AudioTrack
import android.os.Handler
import android.os.Looper
import java.util.concurrent.atomic.AtomicReference

/**
 * Route-change event surfaced to the player (FR-015).
 */
enum class RouteChange {
    /** A USB DAC was attached and should now be preferred. */
    ATTACHED,

    /** The previously routed USB DAC was detached; fall back to built-in output. */
    DETACHED,

    /** No DAC and no usable built-in route at all; the player should pause. */
    NO_ROUTE,
}

/**
 * Pure, JVM-testable description of an output device, so the routing *decision*
 * can be unit-tested without android.media on the classpath.
 */
data class OutputDeviceInfo(
    val id: Int,
    val type: Int,
    val name: String,
) {
    val isUsbDac: Boolean
        get() = type == TYPE_USB_DEVICE || type == TYPE_USB_HEADSET || type == TYPE_USB_ACCESSORY

    companion object {
        // Verified values from android.media.AudioDeviceInfo.
        const val TYPE_UNKNOWN = 0
        const val TYPE_BUILTIN_EARPIECE = 1
        const val TYPE_BUILTIN_SPEAKER = 2
        const val TYPE_WIRED_HEADSET = 3
        const val TYPE_WIRED_HEADPHONES = 4
        const val TYPE_USB_DEVICE = 11
        const val TYPE_USB_ACCESSORY = 12
        const val TYPE_USB_HEADSET = 22
    }
}

/**
 * Pure device-selection logic for the JVM unit tests. Always prefers an
 * attached USB DAC over built-in/wired output; keeps a currently-routed USB
 * DAC stable while it is still attached; returns null when there is no usable
 * route at all. Order of input is honoured (stable, deterministic).
 */
fun choosePreferredDevice(
    devices: List<OutputDeviceInfo>,
    currentDeviceId: Int?,
): OutputDeviceInfo? {
    if (devices.isEmpty()) return null

    val usb = devices.filter { it.isUsbDac }
    if (usb.isNotEmpty()) {
        return usb.firstOrNull { it.id == currentDeviceId } ?: usb.first()
    }

    // No USB DAC: fall back to a sound-producing built-in/wired route.
    return devices.firstOrNull {
        it.type == OutputDeviceInfo.TYPE_BUILTIN_SPEAKER ||
            it.type == OutputDeviceInfo.TYPE_WIRED_HEADSET ||
            it.type == OutputDeviceInfo.TYPE_WIRED_HEADPHONES
    } ?: devices.first()
}

/**
 * Adapts android.media.AudioManager to our routing model (FR-013/FR-015).
 *
 * The attach/detach/switch decision lives in the pure [choosePreferredDevice];
 * this class only maps android types <-> [OutputDeviceInfo]. The current
 * player [AudioTrack] is registered by the service so [routeTo] can apply
 * `AudioTrack#setPreferredDevice` before playback.
 */
class AudioOutputRouter(context: Context) {

    private val audioManager =
        context.getSystemService(Context.AUDIO_SERVICE) as AudioManager

    // Latest enumeration: id -> real AudioDeviceInfo (needed to hand an actual
    // AudioDeviceInfo to AudioTrack#setPreferredDevice, which our pure model
    // must not carry).
    private val currentById: AtomicReference<Map<Int, AudioDeviceInfo>> =
        AtomicReference(emptyMap())

    private val playerTrack: AtomicReference<AudioTrack?> =
        AtomicReference(null)

    /** The AudioTrack the router should route. Call after creating the track, before play(). */
    fun attachPlayerTrack(track: AudioTrack) {
        playerTrack.set(track)
    }

    fun detachPlayerTrack() {
        playerTrack.set(null)
    }

    /** All currently connected output devices (AudioManager#getDevices). */
    fun outputDevices(): List<OutputDeviceInfo> {
        val raw = audioManager.getDevices(AudioManager.GET_DEVICES_OUTPUTS)
        currentById.set(raw.associateBy { it.id })
        return raw.map(OutputDeviceInfo::from)
    }

    /** Subset of [outputDevices] that are USB DACs (FR-013). */
    fun usbDacDevices(): List<OutputDeviceInfo> =
        outputDevices().filter { it.isUsbDac }

    /**
     * Route the attached player track to [device] via AudioTrack#setPreferredDevice.
     * Returns false if the device is gone, no track is attached, or the call failed.
     * Must be called before the track starts playing.
     */
    fun routeTo(device: OutputDeviceInfo): Boolean {
        val track = playerTrack.get() ?: return false
        val real = currentById.get()[device.id] ?: return false
        return track.setPreferredDevice(real)
    }

    /**
     * Register a hotplug listener (registerAudioDeviceCallback). The callback
     * fires on the main thread; route decisions delegating to
     * [choosePreferredDevice] + routeTo may then recreate the track. The
     * returned handle's close() unregisters the callback.
     */
    fun registerHotplug(onChange: (RouteChange) -> Unit): AutoCloseable {
        val callback = object : AudioDeviceCallback() {
            override fun onAudioDevicesAdded(addedDevices: Array<AudioDeviceInfo>) {
                val anyUsb = addedDevices.any { OutputDeviceInfo.from(it).isUsbDac }
                if (anyUsb) {
                    onChange(RouteChange.ATTACHED)
                }
            }

            override fun onAudioDevicesRemoved(removedDevices: Array<AudioDeviceInfo>) {
                val anyUsb = removedDevices.any { OutputDeviceInfo.from(it).isUsbDac }
                if (anyUsb) {
                    onChange(RouteChange.DETACHED)
                }
            }
        }
        @Suppress("DEPRECATION")
        audioManager.registerAudioDeviceCallback(callback, Handler(Looper.getMainLooper()))
        return AutoCloseable { audioManager.unregisterAudioDeviceCallback(callback) }
    }

    /**
     * Confirm the attached track's active route equals [device] via
     * AudioTrack#getRoutedDevice (FR-015 verification, README runbook step).
     */
    fun isRoutedTo(device: OutputDeviceInfo): Boolean =
        playerTrack.get()?.routedDevice?.id == device.id

    companion object {
        val OUTPUT_TYPES = AudioManager.GET_DEVICES_OUTPUTS
    }
}

/** Map android.media.AudioDeviceInfo -> our pure model. */
internal fun OutputDeviceInfo.Companion.from(raw: AudioDeviceInfo): OutputDeviceInfo =
    OutputDeviceInfo(
        id = raw.id,
        type = raw.type,
        name = raw.productName?.toString() ?: "",
    )
