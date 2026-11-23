package com.hams.net.core

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNull

private class FakeWg : WgBackend {
    val calls = mutableListOf<String>()
    override fun addPeer(publicKey: String, endpoint: String, allowedIp: String) { calls += "add $publicKey $endpoint $allowedIp" }
    override fun removePeer(publicKey: String) { calls += "remove $publicKey" }
}

class PeerTableTest {
    private val record = PeerRecord("2001:db8:1230::/60", "KEYA", "198.51.100.7:51820")
    private var now = 1000L
    private var asked = 0
    private val wg = FakeWg()
    private fun table(answer: PeerRecord? = record, link: LinkType = LinkType.INTERNET) =
        PeerTable({ asked++; answer }, wg, link, { now })

    @Test fun firstPacketMakesPeerAndLaterOnesReuseIt() {
        val t = table()
        assertEquals("KEYA", t.onPacket("2001:db8:1230:5::1"))
        assertEquals("KEYA", t.onPacket("2001:db8:1230:f::9"))
        assertEquals(1, asked)
        assertEquals(listOf("add KEYA 198.51.100.7:51820 2001:db8:1230::/60"), wg.calls)
    }

    @Test fun peerIdleTenMinutesIsRemoved() {
        val t = table()
        t.onPacket("2001:db8:1230::1")
        now += 600
        assertEquals(emptyList(), t.expire())
        now += 1
        assertEquals(listOf("KEYA"), t.expire())
        assertEquals("remove KEYA", wg.calls.last())
    }

    @Test fun trafficRefreshesIdleTime() {
        val t = table()
        t.onPacket("2001:db8:1230::1")
        now += 500
        t.onPacket("2001:db8:1230::2")
        now += 500
        assertEquals(emptyList(), t.expire())
    }

    @Test fun unknownDestinationIsNotAskedAgainForAMinute() {
        val t = table(answer = null)
        assertNull(t.onPacket("2001:db8:9::1"))
        now += 59
        assertNull(t.onPacket("2001:db8:9::1"))
        assertEquals(1, asked)
        now += 1
        assertNull(t.onPacket("2001:db8:9::1"))
        assertEquals(2, asked)
    }

    @Test fun recordNotCoveringTheAddressIsRefused() {
        val t = table()
        assertNull(t.onPacket("2001:db8:9999::1"))
        assertEquals(emptyList(), wg.calls)
    }

    @Test fun rfLinkNeverGetsATable() {
        assertFailsWith<IllegalArgumentException> { table(link = LinkType.RF) }
    }
}
