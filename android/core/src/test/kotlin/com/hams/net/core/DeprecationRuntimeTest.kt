package com.hams.net.core

import org.json.JSONArray
import org.json.JSONObject
import uniffi.hams_net_ffi.DeprecationList
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class DeprecationRuntimeTest {
    private val signer = TestSigner()
    private val rogue = PeerRecord("2001:db8:1230::/60", "ROGUE", "198.51.100.7:51820")

    private class Wg : WgBackend {
        val calls = mutableListOf<String>()
        override fun addPeer(publicKey: String, endpoint: String, allowedIp: String) { calls += "add $publicKey" }
        override fun removePeer(publicKey: String) { calls += "remove $publicKey" }
    }

    private val wg = Wg()
    private val blocklist = Blocklist()
    private val table = PeerTable({ rogue }, wg, LinkType.INTERNET, { 0L }, blocklist = blocklist)
    private val enrolment = Enrolment("tok", signer.publicKeyLine, "2001:db8:1200::/60")

    private fun fullList(seq: Long) = signer.sign(JSONObject().put("kind", "full").put("seq", seq).put("entries", JSONArray()
        .put(JSONObject().put("key", "ROGUE").put("prefixes", JSONArray().put("2001:db8:1230::/60")))))

    @Test fun pollEvictsTheListedPeer() {
        table.onPacket("2001:db8:1230::5")
        val (payload, signature) = fullList(1)
        val post = { path: String, _: String ->
            if (path == "/api/ipv6/status") signer.envelope(JSONObject().put("kind", "status").put("state", "ok").put("list_seq", 1))
            else JSONObject().put("result", JSONObject().put("payload", payload).put("signature", signature)).toString()
        }
        DeprecationRuntime(enrolment, JvmCrypto, post, table, blocklist, null).runOnce()
        assertEquals("remove ROGUE", wg.calls.last())
        assertNull(table.onPacket("2001:db8:1230::5"))
    }

    @Test fun aNeighbourDeliversTheListWithHamsComUnreachable() {
        table.onPacket("2001:db8:1230::5")
        val neighbour = DeprecationList(JvmCrypto, signer.publicKeyLine)
        val (payload, signature) = fullList(1)
        neighbour.apply(payload, signature)
        val server = GossipServer(neighbour, 0, "127.0.0.1").also { it.start() }
        try {
            DeprecationRuntime(enrolment, JvmCrypto, { _, _ -> null }, table, blocklist, null,
                neighbourPort = server.port, extraNeighbours = { listOf("127.0.0.1") }).runOnce()
        } finally {
            server.stop()
        }
        assertEquals("remove ROGUE", wg.calls.last())
        assertNull(table.onPacket("2001:db8:1230::5"))
    }

    @Test fun enrolmentNeedsTokenAndKey() {
        assertEquals(Enrolment("t", "k", "p"), Enrolment.fromJson("""{"status":"success","token":"t","deprecation_key":"k","prefix60":"p"}"""))
        assertNull(Enrolment.fromJson("""{"status":"success","token":"t"}"""))
        assertNull(Enrolment.fromJson("not json"))
    }

    @Test fun firstHostOfAPrefix() {
        assertEquals("2001:db8:1230:0:0:0:0:1", Ipv6Prefix.parse("2001:db8:1230::/60")!!.firstHost())
    }
}
