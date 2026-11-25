package com.hams.net.core

import org.json.JSONObject
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

class DirectoryClientTest {
    @Test fun lookupPostsJsonRpcAndParsesRecord() {
        var path = ""
        var body = ""
        val client = DirectoryClient { p, b ->
            path = p; body = b
            """{"jsonrpc":"2.0","id":1,"result":{"status":"success","callsign":"K6BP","prefix60":"2001:db8:1230::/60",
               "public_key":"KEYA","endpoint":"198.51.100.7:51820","interest":["a"]}}"""
        }
        val r = client.lookup("2001:db8:1230::1")
        assertEquals(PeerRecord("2001:db8:1230::/60", "KEYA", "198.51.100.7:51820"), r)
        assertEquals("/api/ipv6/directory/lookup", path)
        assertTrue(body.contains("\"query\":\"2001:db8:1230::1\""))
    }

    @Test fun notFoundAndTransportFailureGiveNull() {
        assertNull(DirectoryClient { _, _ -> """{"result":{"status":"error","message":"not found"}}""" }.lookup("K6BP"))
        assertNull(DirectoryClient { _, _ -> null }.lookup("K6BP"))
        assertNull(DirectoryClient { _, _ -> "not json" }.lookup("K6BP"))
    }

    @Test fun successWithMissingKeyIsNull() {
        assertNull(DirectoryClient { _, _ -> """{"result":{"status":"success","prefix60":"2001:db8::/60"}}""" }.lookup("x"))
    }

    private val signer = TestSigner()
    private val record = """{"status":"success","callsign":"K6BP","prefix60":"2001:db8:1230::/60","public_key":"KEYA","endpoint":"198.51.100.7:51820"}"""

    /** A portal that signs with `s` (kind `kind`, nonce echoed unless `nonce` is given); the flat fields are what an attacker could have changed. */
    private fun portal(s: TestSigner, kind: String = "directory", nonce: String? = null) = { _: String, body: String ->
        val asked = JSONObject(body).getJSONObject("params").optString("nonce")
        val doc = JSONObject(record).put("kind", kind).put("nonce", nonce ?: asked)
        val (payload, sig) = s.sign(doc)
        JSONObject().put("result", JSONObject(record).put("public_key", "FORGED").put("signed", JSONObject().put("payload", payload).put("signature", sig))).toString()
    }

    @Test fun withAPinnedKeyOnlyASignedRecordCountsAndItsNonceIsSent() {
        val pins = listOf(signer.publicKeyLine)
        var body = ""
        val ok = DirectoryClient({ p, b -> body = b; portal(signer)(p, b) }, pins, JvmCrypto).lookup("K6BP")
        assertEquals(PeerRecord("2001:db8:1230::/60", "KEYA", "198.51.100.7:51820"), ok, "the signed value, not the flat one")
        assertEquals(32, JSONObject(body).getJSONObject("params").getString("nonce").length)
        assertNull(DirectoryClient(portal(TestSigner()), pins, JvmCrypto).lookup("K6BP"), "wrong key")
        assertNull(DirectoryClient(portal(signer, nonce = "0".repeat(32)), pins, JvmCrypto).lookup("K6BP"), "replayed nonce")
        assertNull(DirectoryClient(portal(signer, kind = "status"), pins, JvmCrypto).lookup("K6BP"), "wrong kind")
        assertNull(DirectoryClient({ _, _ -> """{"result":$record}""" }, pins, JvmCrypto).lookup("K6BP"), "unsigned")
    }

    @Test fun withoutAPinnedListTheOldBehaviourStaysAndNoNonceIsSent() {
        var body = ""
        val r = DirectoryClient({ _, b -> body = b; """{"result":$record}""" }, emptyList(), JvmCrypto).lookup("K6BP")
        assertEquals("KEYA", r?.publicKey)
        assertTrue(!body.contains("nonce"))
    }
}
