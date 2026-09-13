package dev.wavelink.app.receiver

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * Pure-JVM tests for the route-selection core [choosePreferredDevice]. Standard
 * JUnit 4; no android.* types on the classpath (only the pure
 * [OutputDeviceInfo] model + compile-time device-type constants).
 */
class AudioOutputRouterTest {

    private fun usb(id: Int) = OutputDeviceInfo(id = id, type = OutputDeviceInfo.TYPE_USB_DEVICE, name = "usb-$id")
    private fun headphone(id: Int) = OutputDeviceInfo(id = id, type = OutputDeviceInfo.TYPE_WIRED_HEADPHONES, name = "hp-$id")
    private fun speaker(id: Int) = OutputDeviceInfo(id = id, type = OutputDeviceInfo.TYPE_BUILTIN_SPEAKER, name = "spk-$id")

    @Test
    fun picksUsbDacOverBuiltIn() {
        val devices = listOf(speaker(3), usb(11), headphone(4))
        val chosen = choosePreferredDevice(devices, currentDeviceId = null)
        assertEquals("USB DAC must win over built-in/wired output (FR-013)", usb(11), chosen)
    }

    @Test
    fun keepsCurrentlyRoutedUsbStable() {
        val devices = listOf(usb(11), usb(12))
        val chosen = choosePreferredDevice(devices, currentDeviceId = 12)
        // Last-attached DAC stays even though it appears after the first.
        assertEquals("Currently routed USB DAC must stay stable while attached", usb(12), chosen)
    }

    @Test
    fun fallsBackToFirstUsbWhenNoCurrentRoute() {
        val devices = listOf(usb(11), usb(12))
        val chosen = choosePreferredDevice(devices, currentDeviceId = null)
        assertEquals("First USB DAC is the deterministic default", usb(11), chosen)
    }

    @Test
    fun noDeviceReturnsNull() {
        assertNull(
            "Empty output set -> no preferred device (NO_ROUTE)",
            choosePreferredDevice(emptyList(), currentDeviceId = null),
        )
    }

    @Test
    fun builtInSpeakerUsedWhenNoUsb() {
        val devices = listOf(speaker(3))
        val chosen = choosePreferredDevice(devices, currentDeviceId = null)
        assertEquals(speaker(3), chosen)
    }

    @Test
    fun orderStabilityFirstElementWins() {
        val devices = listOf(usb(1), usb(2), usb(3))
        val chosen = choosePreferredDevice(devices, currentDeviceId = null)
        assertEquals("Must be deterministic regardless of enumeration order", usb(1), chosen)
    }

    @Test
    fun usbHeadsetAndAccessoryCountAsUsb() {
        val headset = OutputDeviceInfo(7, OutputDeviceInfo.TYPE_USB_HEADSET, "usb-headset")
        val accessory = OutputDeviceInfo(9, OutputDeviceInfo.TYPE_USB_ACCESSORY, "usb-acc")
        val chosen = choosePreferredDevice(listOf(speaker(2), headset), currentDeviceId = null)
        assertEquals(headset, chosen)
        val chosen2 = choosePreferredDevice(listOf(speaker(2), accessory), currentDeviceId = null)
        assertEquals(accessory, chosen2)
    }
}
