package com.hams.net.core

import kotlin.test.Test
import kotlin.test.assertEquals

class GuardTest {
    @Test fun onlyInternetTransportsAreInternet() {
        assertEquals(LinkType.INTERNET, LinkGuard.classify(setOf("wifi")))
        assertEquals(LinkType.INTERNET, LinkGuard.classify(setOf("cellular", "validated")))
        assertEquals(LinkType.INTERNET, LinkGuard.classify(setOf("ethernet")))
    }

    @Test fun bluetoothUsbSerialAndUnknownAreRf() {
        assertEquals(LinkType.RF, LinkGuard.classify(setOf("bluetooth")))
        assertEquals(LinkType.RF, LinkGuard.classify(setOf("usb-serial")))
        assertEquals(LinkType.RF, LinkGuard.classify(emptySet()))
        assertEquals(LinkType.RF, LinkGuard.classify(setOf("wifi", "bluetooth")))
    }

    @Test fun caGuidanceNamesTrustingAndNonTrustingApps() {
        val text = CaGuidance.text()
        assertEquals(true, text.contains("Chrome") && text.contains("Firefox"))
        assertEquals(true, text.contains("will not trust"))
    }

    @Test fun privateKeyIsNeverInAnythingSent() {
        val keys = KeyHolder("PRIVATE-SECRET", "PUBLIC")
        assertEquals("PUBLIC", keys.publicKey)
        assertEquals(false, keys.toString().contains("PRIVATE-SECRET"))
    }
}
