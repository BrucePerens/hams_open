package com.hams.net.core

import uniffi.hams_net_ffi.ListChange
import uniffi.hams_net_ffi.ListedKey
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class DeprecationEnforcerTest {
    private class Wg : WgBackend {
        val calls = mutableListOf<String>()
        override fun addPeer(publicKey: String, endpoint: String, allowedIp: String) { calls += "add $publicKey" }
        override fun removePeer(publicKey: String) { calls += "remove $publicKey" }
    }

    private val rogue = PeerRecord("2001:db8:1230::/60", "ROGUE", "198.51.100.7:51820")
    private val wg = Wg()
    private val blocklist = Blocklist()
    private val table = PeerTable({ rogue }, wg, LinkType.INTERNET, { 0L }, blocklist = blocklist)
    private val enforcer = DeprecationEnforcer(table, blocklist)

    private fun listed(vararg k: ListedKey) = ListChange(k.toList(), emptyList())
    private val rogueKey = ListedKey("ROGUE", listOf("2001:db8:1230::/60"))

    @Test fun aListedPeerIsEvictedAndNotMadeAgain() {
        assertEquals("ROGUE", table.onPacket("2001:db8:1230::5"))
        enforcer.apply(listed(rogueKey))
        assertEquals("remove ROGUE", wg.calls.last())
        assertNull(table.onPacket("2001:db8:1230::5"))
        assertEquals(listOf("add ROGUE", "remove ROGUE"), wg.calls)
    }

    @Test fun aListedKeyIsRefusedEvenUnderAnotherPrefix() {
        enforcer.apply(listed(ListedKey("ROGUE", emptyList())))
        assertNull(table.onPacket("2001:db8:1230::5"))
        assertTrue(wg.calls.isEmpty())
    }

    @Test fun removalFromTheListLiftsTheBlock() {
        enforcer.apply(listed(rogueKey))
        enforcer.apply(ListChange(emptyList(), listOf(rogueKey)))
        assertEquals("ROGUE", table.onPacket("2001:db8:1230::5"))
    }

    @Test fun revokedCertificatesAreRecordedAndLifted() {
        val cert = ListedKey("cert:ISSUERHASH:1A2B", emptyList())
        enforcer.apply(listed(cert))
        assertTrue(blocklist.isCertRevoked("ISSUERHASH", "1A2B"))
        assertFalse(blocklist.isCertRevoked("ISSUERHASH", "9"))
        enforcer.apply(ListChange(emptyList(), listOf(cert)))
        assertFalse(blocklist.isCertRevoked("ISSUERHASH", "1A2B"))
    }

    @Test fun aSignedDisableDropsEveryPeerUntilEnabled() {
        table.onPacket("2001:db8:1230::5")
        enforcer.disable()
        assertEquals("remove ROGUE", wg.calls.last())
        assertNull(table.onPacket("2001:db8:1230::5"))
        enforcer.enable()
        assertEquals("ROGUE", table.onPacket("2001:db8:1230::5"))
    }
}
