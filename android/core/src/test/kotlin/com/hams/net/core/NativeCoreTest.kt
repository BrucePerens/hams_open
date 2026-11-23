package com.hams.net.core

import uniffi.hams_net_ffi.DirectoryLookup
import uniffi.hams_net_ffi.DirectoryRecord
import uniffi.hams_net_ffi.NetException
import uniffi.hams_net_ffi.PeerTable
import uniffi.hams_net_ffi.WireGuard
import uniffi.hams_net_ffi.siitEmbed
import uniffi.hams_net_ffi.siitExtract
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNull

/** Calls the shared Rust core (hams_net_core through UniFFI): SIIT map and the on-demand peer table. */
class NativeCoreTest {
    private val record = DirectoryRecord("W1AW", "2001:db8:1:10::/60", "KEY1", "198.51.100.7:51820")

    private class Directory(val record: DirectoryRecord) : DirectoryLookup {
        override fun lookup(destination: String): DirectoryRecord? = record
    }

    private class Wg : WireGuard {
        val added = mutableListOf<String>()
        val removed = mutableListOf<String>()
        override fun addPeer(interfaceName: String, publicKey: String, endpoint: String, prefix60: String) {
            added += "$interfaceName $publicKey $endpoint $prefix60"
        }
        override fun removePeer(interfaceName: String, publicKey: String) {
            removed += publicKey
        }
    }

    // Tests [@ANCHOR: hams_net_ffi_siit_embed]
    // Tests [@ANCHOR: hams_net_ffi_siit_extract]
    @Test
    fun siitMapRoundTrips() {
        val v6 = siitEmbed("2001:db8:1::/48", 7u, "10.1.2.3")
        assertEquals("2001:db8:1:7::a01:203", v6)
        val back = siitExtract("2001:db8:1::/48", v6)
        assertEquals(7u.toUShort(), back.domain)
        assertEquals("10.1.2.3", back.ipv4)
    }

    // Tests [@ANCHOR: hams_net_ffi_peer_table_new]
    // Tests [@ANCHOR: hams_net_ffi_peer_table_on_packet]
    // Tests [@ANCHOR: hams_net_ffi_peer_table_expire]
    @Test
    fun peerTableMakesAndExpiresPeers() {
        val wg = Wg()
        val table = PeerTable(Directory(record), wg, "wg0", false)
        assertEquals("KEY1", table.onPacket("2001:db8:1:10::5", 0.0))
        assertEquals(listOf("wg0 KEY1 198.51.100.7:51820 2001:db8:1:10::/60"), wg.added)
        assertEquals(emptyList(), table.expire(100.0))
        assertEquals(listOf("KEY1"), table.expire(700.0))
        assertEquals(listOf("KEY1"), wg.removed)
    }

    @Test
    fun peerTableRefusesAnRfLinkAndUnknownDestinations() {
        assertFailsWith<NetException> { PeerTable(Directory(record), Wg(), "wg0", true) }
        val table = PeerTable(Directory(record), Wg(), "wg0", false)
        assertNull(table.onPacket("2001:db8:2::1", 0.0))
    }
}
