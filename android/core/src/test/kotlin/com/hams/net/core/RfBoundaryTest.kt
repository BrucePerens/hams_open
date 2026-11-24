package com.hams.net.core

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

/** Builds IPv6 packets for tests. */
object Pkt {
    fun make(next: Int, l4: ByteArray): ByteArray {
        val p = ByteArray(40 + l4.size)
        p[0] = 0x60; p[6] = next.toByte(); p[7] = 64
        p[8] = 0x20; p[23] = 1; p[24] = 0x20; p[39] = 2
        l4.copyInto(p, 40)
        return p
    }
    private fun ports(src: Int, dst: Int) = byteArrayOf((src shr 8).toByte(), src.toByte(), (dst shr 8).toByte(), dst.toByte())
    fun udp(dst: Int, payload: ByteArray = byteArrayOf(1, 2, 3), src: Int = 40000) = make(17, ports(src, dst) + payload)
    fun tcp(dst: Int, src: Int = 40000) = make(6, ports(src, dst) + ByteArray(16))
    fun icmp(type: Int) = make(58, byteArrayOf(type.toByte(), 0, 0, 0))
    /** A readable packet whose payload holds the KISS special bytes. */
    val readable get() = udp(53, byteArrayOf(0xC0.toByte(), 0xDB.toByte(), 1, 0xC0.toByte()))
}

class RfBoundaryTest {
    @Test fun encryptedFlowsAreRefusedAndCounted() {
        val b = RfBoundary()
        assertEquals(RfRefusal.ENCRYPTED, b.check(Pkt.udp(51820)))
        assertEquals(RfRefusal.ENCRYPTED, b.check(Pkt.udp(500)))
        assertEquals(RfRefusal.ENCRYPTED, b.check(Pkt.tcp(22)))
        assertEquals(RfRefusal.ENCRYPTED, b.check(Pkt.tcp(443)))
        assertEquals(RfRefusal.ENCRYPTED, b.check(Pkt.make(50, ByteArray(16))))
        assertEquals(5L, b.refusedEncrypted)
    }

    @Test fun onlyReadableServicesAndEchoStartAFlow() {
        val b = RfBoundary()
        assertNull(b.check(Pkt.tcp(80)))
        assertNull(b.check(Pkt.tcp(8443)))
        assertNull(b.check(Pkt.udp(53)))
        assertNull(b.check(Pkt.icmp(128)))
        assertEquals(RfRefusal.NOT_ALLOWED, b.check(Pkt.tcp(8080)))
        assertEquals(RfRefusal.NOT_ALLOWED, b.check(Pkt.udp(9999)))
        assertEquals(RfRefusal.NOT_ALLOWED, b.check(Pkt.icmp(135)))
        assertEquals(RfRefusal.NOT_ALLOWED, b.check(Pkt.make(99, ByteArray(8))))
        assertEquals(RfRefusal.NOT_ALLOWED, b.check(byteArrayOf(1, 2, 3)))
        assertEquals(5L, b.refusedOther)
    }

    @Test fun laterPacketsOfAnAllowedFlowPass() {
        val b = RfBoundary()
        assertNull(b.check(Pkt.tcp(80, src = 5000)))
        assertNull(b.check(Pkt.tcp(80, src = 5000)))
        assertEquals(RfRefusal.NOT_ALLOWED, b.check(Pkt.tcp(9000, src = 5000)))
    }
}
