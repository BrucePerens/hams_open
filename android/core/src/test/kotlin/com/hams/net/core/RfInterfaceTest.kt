package com.hams.net.core

import kotlin.test.Test
import kotlin.test.assertContentEquals
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/** A virtual TNC: the other end of the serial line, speaking KISS. */
class VirtualTnc : SerialPort {
    val decoder = KissDecoder()
    val received = mutableListOf<KissFrame>()
    override fun write(data: ByteArray) { received += decoder.feed(data) }
}

class RfInterfaceTest {
    private val packet = byteArrayOf(0x60, 0, 0, 0, 0xC0.toByte(), 0xDB.toByte(), 1, 2, 0xC0.toByte())

    @Test fun kissFramesPassBothWaysWithEscapes() {
        val tnc = VirtualTnc()
        var allowed = true
        val rf = RfInterface(tnc, "N0CALL", TxGate { send -> if (allowed) { send(); true } else false })
        rf.enable()
        assertTrue(rf.send(packet))
        assertEquals(1, tnc.received.size)
        assertContentEquals(packet, tnc.received[0].payload)
        assertEquals(0, tnc.received[0].command)
        val air = Kiss.encode(packet)
        val first = rf.receive(air.copyOfRange(0, 4))
        val second = rf.receive(air.copyOfRange(4, air.size))
        assertEquals(0, first.size)
        assertContentEquals(packet, second.single())
    }

    @Test fun nothingKeysUnlessEnabledAndGateAllows() {
        val tnc = VirtualTnc()
        var allowed = false
        val rf = RfInterface(tnc, "N0CALL", TxGate { send -> if (allowed) { send(); true } else false })
        assertFalse(rf.send(packet))
        rf.enable()
        assertFalse(rf.send(packet))
        allowed = true
        assertTrue(rf.send(packet))
        rf.disable()
        assertFalse(rf.send(packet))
        assertEquals(1, tnc.received.size)
    }

    @Test fun rfRefusesWireGuardAndEveryEncryptingSuite() {
        val rf = RfInterface(VirtualTnc(), "N0CALL", TxGate { send -> send(); true })
        rf.attach(LinkSuite.NONE)
        assertFailsWith<RfRefused> { rf.attach(LinkSuite.WIREGUARD) }
        assertFailsWith<RfRefused> { rf.attach(LinkSuite.ENCRYPTED_OTHER) }
        RfPolicy.requireUnencrypted(LinkType.INTERNET, LinkSuite.WIREGUARD)
        assertFailsWith<IllegalArgumentException> {
            PeerTable({ null }, object : WgBackend {
                override fun addPeer(publicKey: String, endpoint: String, allowedIp: String) {}
                override fun removePeer(publicKey: String) {}
            }, rf.link, { 0L })
        }
    }

    @Test fun enablingShowsTheStationIdReminder() {
        val rf = RfInterface(VirtualTnc(), "N0CALL", TxGate { send -> send(); true })
        val text = rf.enable()
        assertTrue(text.contains("N0CALL") && text.contains("97.119") && text.contains("No encryption"))
        assertFailsWith<IllegalArgumentException> { RfInterface(VirtualTnc(), " ", TxGate { send -> send(); true }).enable() }
    }

    @Test fun badEscapeDropsOnlyThatFrame() {
        val d = KissDecoder()
        val frames = d.feed(byteArrayOf(0xC0.toByte(), 0, 0xDB.toByte(), 5, 0xC0.toByte()) + Kiss.encode(byteArrayOf(7)))
        assertEquals(1, frames.size)
        assertContentEquals(byteArrayOf(7), frames[0].payload)
    }
}
