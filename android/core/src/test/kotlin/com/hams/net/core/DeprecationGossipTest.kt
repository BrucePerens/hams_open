package com.hams.net.core

import org.json.JSONArray
import org.json.JSONObject
import uniffi.hams_net_ffi.DeprecationList
import uniffi.hams_net_ffi.PlatformCrypto
import java.net.Socket
import java.security.KeyFactory
import java.security.KeyPairGenerator
import java.security.Signature
import java.security.spec.X509EncodedKeySpec
import java.util.Base64
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** Two lists on loopback, no hams.com: the neighbour with the newer signed chain hands it over. */
class DeprecationGossipTest {
    private val pair = KeyPairGenerator.getInstance("Ed25519").generateKeyPair()
    private val keynum = ByteArray(8) { it.toByte() }
    private val b64 = Base64.getEncoder()
    private val keyLine = b64.encodeToString("Ed".toByteArray() + keynum + pair.public.encoded.copyOfRange(12, 44))

    private val crypto = object : PlatformCrypto {
        private val spki = byteArrayOf(0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00)
        override fun ed25519Verify(key: ByteArray, signature: ByteArray, message: ByteArray): Boolean = try {
            val pub = KeyFactory.getInstance("Ed25519").generatePublic(X509EncodedKeySpec(spki + key))
            Signature.getInstance("Ed25519").run { initVerify(pub); update(message); verify(signature) }
        } catch (e: java.security.GeneralSecurityException) {
            false
        }
        override fun base64Decode(text: String): ByteArray = try {
            Base64.getDecoder().decode(text)
        } catch (e: IllegalArgumentException) {
            ByteArray(0)
        }
    }

    private fun sign(body: JSONObject, signer: java.security.PrivateKey = pair.private): Pair<String, String> {
        val payload = body.toString().toByteArray()
        val raw = Signature.getInstance("Ed25519").run { initSign(signer); update(payload); sign() }
        return b64.encodeToString(payload) to b64.encodeToString("Ed".toByteArray() + keynum + raw)
    }

    private fun entry(key: String) = JSONObject().put("key", key).put("prefixes", JSONArray().put("2001:db8:5::/60"))

    private fun holder(): DeprecationList {
        val list = DeprecationList(crypto, keyLine)
        val full = sign(JSONObject().put("kind", "full").put("seq", 1).put("entries", JSONArray().put(entry("KEYA"))))
        val delta = sign(JSONObject().put("kind", "delta").put("from", 1).put("seq", 2).put("add", JSONArray().put(entry("KEYB"))))
        list.apply(full.first, full.second)
        list.apply(delta.first, delta.second)
        return list
    }

    // Tests [@ANCHOR: hams_net_ffi_deprecation_gossip_reply]
    // Tests [@ANCHOR: hams_net_ffi_deprecation_accept_gossip]
    @Test
    fun aNeighbourWithANewerListHandsOverTheSignedChain() {
        val server = GossipServer(holder(), 0, "127.0.0.1")
        server.start()
        try {
            val behind = DeprecationList(crypto, keyLine)
            val changes = DeprecationGossip.ask(behind, "127.0.0.1", server.port)
            assertEquals(2UL, behind.seq())
            assertEquals(listOf("KEYA"), changes[0].added.map { it.key })
            assertEquals(listOf("KEYB"), changes[1].added.map { it.key })
            // Up to date now: nothing more is sent, no change.
            assertTrue(DeprecationGossip.ask(behind, "127.0.0.1", server.port).isEmpty())
        } finally {
            server.stop()
        }
    }

    @Test
    fun aNeighbourSigningWithAnotherKeyIsRefused() {
        val rogue = KeyPairGenerator.getInstance("Ed25519").generateKeyPair()
        val forged = sign(JSONObject().put("kind", "full").put("seq", 9).put("entries", JSONArray().put(entry("GOOD"))), rogue.private)
        // The rogue neighbour serves a forged chain by hand.
        val rogueSocket = java.net.ServerSocket(0, 1, java.net.InetAddress.getByName("127.0.0.1"))
        val t = Thread { rogueSocket.accept().use { it.getOutputStream().write("full 0 ${forged.first} ${forged.second}\n".toByteArray()) } }
        t.start()
        val victim = DeprecationList(crypto, keyLine)
        assertTrue(DeprecationGossip.ask(victim, "127.0.0.1", rogueSocket.localPort).isEmpty())
        assertEquals(0UL, victim.seq())
        t.join(); rogueSocket.close()
    }

    @Test
    fun anUnreachableOrMuteNeighbourChangesNothing() {
        val list = DeprecationList(crypto, keyLine)
        assertTrue(DeprecationGossip.ask(list, "127.0.0.1", 1).isEmpty())
        val server = GossipServer(holder(), 0, "127.0.0.1")
        server.start()
        try {
            Socket("127.0.0.1", server.port).use { it.getOutputStream().write("garbage\n".toByteArray()); assertEquals(-1, it.getInputStream().read()) }
        } finally {
            server.stop()
        }
    }
}
